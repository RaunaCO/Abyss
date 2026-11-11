//! A scrollbar for long pages, on the right edge: shows how far down you are,
//! can be dragged or clicked to jump, and while dragging (or gliding) shows a
//! bubble with the letter you are at when the list is sorted by name, artist
//! or album (or the year when sorted by year), like a phone's contact list.
//!
//! Position is by item index (cheap and stable even before every row has been
//! measured); long lists have rows of equal height, so it reads true.

use super::sc;
use gpui::{div, prelude::*, AnyElement, Context, MouseButton, MouseDownEvent, Pixels, Point};

use super::pages::Block;
use super::sort::SortKey;
use super::{theme, AppView};

const PAD: f32 = 8.0;
const THUMB: f32 = 46.0;
const MIN_ITEMS: usize = 14;

impl AppView {
    fn scroll_fraction(&self) -> f32 {
        let n = self.list.item_count();
        if n < 2 {
            return 0.0;
        }
        let top = self.list.logical_scroll_top();
        let within = self
            .list
            .bounds_for_item(top.item_ix)
            .map(|b| (f32::from(top.offset_in_item) / f32::from(b.size.height).max(1.0)).clamp(0.0, 1.0))
            .unwrap_or(0.0);
        ((top.item_ix as f32 + within) / (n - 1) as f32).clamp(0.0, 1.0)
    }

    /// Jump the list to the pointer's height on the scrollbar.
    pub fn scrollbar_to(&mut self, pos: Point<Pixels>) {
        let vp = self.list.viewport_bounds();
        let h = f32::from(vp.size.height);
        let z = super::zoom();
        let track = h - (2.0 * PAD + THUMB) * z;
        if track <= 1.0 {
            return;
        }
        let y = f32::from(pos.y) - f32::from(vp.origin.y) - (PAD + THUMB / 2.0) * z;
        let frac = (y / track).clamp(0.0, 1.0);
        let n = self.list.item_count();
        let ix = ((n.saturating_sub(1)) as f32 * frac).round() as usize;
        self.stop_smooth_scroll();
        self.list.scroll_to(gpui::ListOffset { item_ix: ix, offset_in_item: sc(0.) });
    }

    /// Letter (or year) of the row at the top, following the page's sort.
    fn scroll_label(&self) -> Option<String> {
        let section = super::sort::section(&self.page)?;
        let key = self.sort_of(section).0;
        let top = self.list.logical_scroll_top().item_ix;
        let initial = |s: &str| {
            let s = s.trim();
            let s = s.strip_prefix("The ").or_else(|| s.strip_prefix("the ")).unwrap_or(s);
            s.chars().find(|c| c.is_alphanumeric()).map(|c| if c.is_ascii_digit() { "#".to_string() } else { c.to_uppercase().to_string() })
        };
        for block in self.current_blocks.iter().skip(top).take(6) {
            match block {
                Block::Track(src, i, _) => {
                    let t = self.tracks_of(src)?.get(*i)?;
                    return match key {
                        SortKey::Name => initial(&t.title),
                        SortKey::Artist => initial(&t.artist_names()),
                        SortKey::Album => t.album.as_ref().and_then(|a| initial(&a.title)),
                        _ => None,
                    };
                }
                Block::Cards(items) => {
                    let first = items.first()?;
                    return match (key, first) {
                        (SortKey::Name, _) => initial(&super::widgets::item_title(first)),
                        (SortKey::Artist, crate::tidal::Item::Album(a)) => initial(&a.artist_names()),
                        (SortKey::Year, crate::tidal::Item::Album(a)) => Some(a.year().to_string()).filter(|y| !y.is_empty()),
                        _ => None,
                    };
                }
                _ => {}
            }
        }
        None
    }
}

pub fn render(view: &AppView, cx: &mut Context<AppView>) -> Option<AnyElement> {
    let n = view.list.item_count();
    let vh = f32::from(view.list.viewport_bounds().size.height) / super::zoom();
    if n < MIN_ITEMS || vh < 160.0 {
        return None;
    }
    let frac = view.scroll_fraction();
    let track = vh - 2.0 * PAD - THUMB;
    let y = PAD + frac * track;
    let dragging = view.scrollbar_drag;
    let label = if dragging || view.scroll_animating { view.scroll_label() } else { None };
    Some(
        div()
            .id("page-scrollbar")
            .absolute()
            .top(sc(0.))
            .bottom(sc(0.))
            .right(sc(2.))
            .w(sc(16.))
            .group("sb")
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.scrollbar_drag = true;
                    this.scrollbar_to(ev.position);
                    cx.notify();
                }),
            )
            // The thumb: thin at rest, wider when hovered or held.
            .child(
                div()
                    .absolute()
                    .top(sc(y))
                    .right(sc(4.))
                    .h(sc(THUMB))
                    .w(sc(if dragging { 8. } else { 4. }))
                    .rounded(sc(4.))
                    .bg(gpui::rgba(if dragging { 0xffffffaa } else { 0xffffff40 }))
                    .group_hover("sb", |s| s.w(sc(8.)).bg(theme::ink(0.5))),
            )
            // The letter bubble beside the thumb.
            .when_some(label, |d, l| {
                d.child(
                    div()
                        .absolute()
                        .top(sc(y + THUMB / 2.0 - 26.0))
                        .right(sc(22.))
                        .size(sc(52.))
                        .rounded(sc(14.))
                        .bg(theme::liquid_glass(0.2))
                        .border_1()
                        .border_color(theme::popover_border())
                        .shadow(theme::popover_shadow())
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(sc(if l.chars().count() > 2 { 15. } else { 24. }))
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(l),
                )
            })
            .into_any_element(),
    )
}
