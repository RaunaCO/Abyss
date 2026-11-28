//! The play queue, in the side panel and in Now Playing. It starts at the top
//! while it fits and scrolls once its rows are taller than the space (1.6.0),
//! keeping the current track in the middle where the list allows. Full screen
//! used to pad the list so even the first track sat in the middle: the glass
//! pane then showed an empty, refracting top half (1.8). The rows are
//! virtualised: a whole library can be queued.

use iced::widget::{button, column, container, mouse_area, row, scrollable, text, Space};
use iced::{Alignment, Border, Element, Length, Task};

use super::icons::Icon;
use super::widgets::{self, fmt_time, icon_button, line};
use super::{theme, App, MenuTarget, Message, NpTab};

pub(super) const SCROLL_ID: &str = "abyss-queue";
const ROW_H: f32 = 60.0;
/// Extra height of the current row (its margins).
const CURRENT_EXTRA: f32 = 18.0;
/// Farthest the queue glides to the next track (beyond, it jumps).
const GLIDE_MAX: f32 = 4.0 * ROW_H;

/// Where the rows are for a panel `height` tall.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Geometry {
    /// Space before the first row.
    lead: f32,
    /// Space after the last one.
    tail: f32,
    /// Offset that centres the current track, clamped to the range.
    centred: f32,
}

fn geometry(count: usize, current: Option<usize>, height: f32) -> Geometry {
    let content = count as f32 * ROW_H + if current.is_some() { CURRENT_EXTRA } else { 0.0 };
    let Some(i) = current.filter(|_| content > height) else { return Geometry { lead: 0.0, tail: 0.0, centred: 0.0 } };
    let middle = i as f32 * ROW_H + (ROW_H + CURRENT_EXTRA) / 2.0;
    let centred = (middle - height / 2.0).clamp(0.0, content - height);
    Geometry { lead: 0.0, tail: 0.0, centred }
}

/// What the centring last answered to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Centred {
    fullscreen: bool,
    uid: u64,
    index: usize,
    height: u32,
}

impl App {
    /// The presentation showing the queue and its height.
    fn queue_view(&self) -> Option<(bool, f32)> {
        if self.now_playing_open {
            return (self.np_tab == NpTab::Queue)
                .then(|| (true, super::now_playing::queue_height(self.viewport.width, self.viewport.height)));
        }
        // The side panel's heading with its rule, and the 8 px above and below the list.
        self.show_queue.then(|| (false, (super::pages::page_area(self).height - 66.0 - 16.0).max(0.0)))
    }

    /// Bring the current track to the middle when the queue opens, the track
    /// changes or the panel changes height.
    pub(super) fn centre_queue(&mut self) -> Task<Message> {
        let Some((fullscreen, height)) = self.queue_view() else {
            self.queue_centred = None;
            return Task::none();
        };
        let current = self.now.as_ref().and_then(|n| self.queue.get(n.index).map(|q| (q.uid, n.index)));
        let (uid, index) = current.unwrap_or((0, 0));
        let key = Centred { fullscreen, uid, index, height: height as u32 };
        if self.queue_centred == Some(key) {
            return Task::none();
        }
        let before = self.queue_centred.replace(key);
        let centred = geometry(self.queue.len(), current.map(|(_, i)| i), height).centred;
        // The track moved on in a queue already showing: it glides the row or
        // two, as a wheel notch, so the eye can follow where the song went.
        if let Some(by) = glide_by(before, key, self.queue_scroll, centred) {
            self.queue_nudge = (self.queue_nudge.0.wrapping_add(1), by);
            return Task::none();
        }
        self.queue_scroll = centred;
        iced::widget::operation::scroll_to(SCROLL_ID, scrollable::AbsoluteOffset { x: 0.0, y: self.queue_scroll })
    }
}

/// How far to glide (instead of jumping) from offset `at` to `centred` once
/// the centring key goes from `before` to `key`: only when just the track
/// changed in the same presentation, and not far.
fn glide_by(before: Option<Centred>, key: Centred, at: f32, centred: f32) -> Option<f32> {
    let before = before?;
    let same_view = before.fullscreen == key.fullscreen && before.height == key.height;
    let by = centred - at;
    (same_view && before.uid != key.uid && by.abs() > 0.5 && by.abs() <= GLIDE_MAX).then_some(by)
}

/// The queue's list (`_fullscreen`: both presentations now lay it out alike).
pub fn panel(app: &App, _fullscreen: bool) -> Element<'_, Message> {
    if app.queue.is_empty() {
        return super::now_playing::empty_state(Icon::Queue, crate::i18n::t("add_music_to_the_queue_to_see_it_here"));
    }
    let height = app.queue_view().map_or(app.viewport.height, |(_, h)| h);
    let count = app.queue.len();
    let current = app.now.as_ref().map(|n| n.index).filter(|&i| i < count);
    let geo = geometry(count, current, height);

    // Whole rows around what shows; the current one may be taller.
    let first = (((app.queue_scroll - geo.lead) / ROW_H).floor().max(0.0) as usize).saturating_sub(6).min(count);
    let last = (first + (height.max(app.viewport.height) / ROW_H) as usize + 12).min(count);
    let height_of = |from: usize, to: usize| {
        (to - from) as f32 * ROW_H + if current.is_some_and(|c| (from..to).contains(&c)) { CURRENT_EXTRA } else { 0.0 }
    };
    let mut list = column![Space::new().height(geo.lead + height_of(0, first))].width(Length::Fill);
    for i in first..last {
        list = list.push(queue_row(app, i, current));
    }
    list = list.push(Space::new().height(height_of(last, count) + geo.tail));
    let scrolling = scrollable(list.padding([0, 12]))
        .id(SCROLL_ID)
        .on_scroll(|v| Message::QueueScrolled(v.absolute_offset().y))
        .direction(scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(6).scroller_width(6).margin(2)))
        .style(super::pages::scroll_style)
        .height(Length::Fill);
    super::glide::glide_nudged(scrolling, app.queue_nudge)
}

fn queue_row(app: &App, i: usize, current: Option<usize>) -> Element<'_, Message> {
    let item = &app.queue[i];
    let (track, uid) = (&item.track, item.uid);
    let is_current = Some(i) == current;
    let previous = current.is_some_and(|c| i < c);
    let color = if is_current {
        theme::accent()
    } else if previous {
        theme::text_dim()
    } else {
        theme::text()
    };
    let names = column![
        line(track.display_title(), theme::TYPE_BODY, color).font(widgets::semibold()),
        line(track.artist_names(), theme::TYPE_LABEL, theme::text_secondary()),
    ];
    // GPUI: a 19 x 16 meter beside the current track, an 18 px remove button on the others.
    let end: Element<'_, Message> = if is_current {
        container(widgets::playing_indicator(app.is_playing(), 16.0)).center_x(19).into()
    } else {
        icon_button(Icon::Close, 18.0, theme::text_secondary(), Message::RemoveQueued(uid))
    };
    let content = row![
        widgets::cover_at_opacity(track.cover(80), 40.0, false, if previous { 0.55 } else { 1.0 }),
        container(names).width(Length::Fill).clip(true),
        text(fmt_time(track.duration as f64)).size(theme::TYPE_LABEL).color(theme::text_dim()),
        end,
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .padding([0, 8])
    .height(ROW_H - 1.0);
    // Each row ends in a rule, as GPUI's `border_b_1`.
    let rule = container(Space::new().width(Length::Fill).height(1))
        .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() });
    let content = column![content, rule];
    let press = (!is_current).then_some(Message::PlayQueued(uid));
    // GPUI's rows: the hover colour fades in, the press deepens it.
    let boxed = button(content).padding(0).width(Length::Fill).on_press_maybe(press).style(move |_, _| {
        let p = super::wash::phase();
        let hover = theme::hover();
        button::Style {
            background: if is_current {
                Some(super::wash::mix(theme::active(), hover, p.hover).into())
            } else {
                let c = super::wash::mix(iced::Color { a: hover.a * p.hover, ..hover }, theme::active(), p.press);
                (c.a > 0.0).then(|| c.into())
            },
            text_color: theme::text(),
            border: Border { radius: 9.0.into(), ..Default::default() },
            ..Default::default()
        }
    });
    let boxed = super::wash::track(boxed);
    let margin = if is_current { CURRENT_EXTRA / 2.0 } else { 0.0 };
    mouse_area(container(boxed).padding([margin, 0.0])).on_right_press(Message::OpenMenu(MenuTarget::QueueItem(uid))).into()
}

#[cfg(test)]
mod tests {
    use super::{geometry, glide_by, Centred, Geometry, GLIDE_MAX, ROW_H};

    #[test]
    fn the_queue_glides_to_the_next_track_and_jumps_otherwise() {
        let side = |uid: u64, index: usize| Centred { fullscreen: false, uid, index, height: 600 };
        // Next track, one row on: glide the row.
        assert_eq!(glide_by(Some(side(1, 5)), side(2, 6), 300.0, 360.0), Some(ROW_H));
        // Opening the queue, resizing it or moving to full screen: jump.
        assert_eq!(glide_by(None, side(2, 6), 300.0, 360.0), None);
        let taller = Centred { height: 700, ..side(2, 6) };
        assert_eq!(glide_by(Some(side(1, 5)), taller, 300.0, 360.0), None);
        // Far away (scrolled off, or a jump across the queue): jump.
        assert_eq!(glide_by(Some(side(1, 5)), side(2, 50), 300.0, 300.0 + GLIDE_MAX + 1.0), None);
        // Already there: nothing to do.
        assert_eq!(glide_by(Some(side(1, 5)), side(2, 6), 360.0, 360.0), None);
    }

    #[test]
    fn a_short_queue_starts_at_the_top_in_either_presentation() {
        assert_eq!(geometry(3, Some(1), 600.0), Geometry { lead: 0.0, tail: 0.0, centred: 0.0 });
    }

    #[test]
    fn a_long_queue_centres_the_current_track_within_its_range() {
        assert_eq!(geometry(100, Some(0), 600.0).centred, 0.0);
        assert_eq!(geometry(100, Some(50), 600.0).centred, 3000.0 + 39.0 - 300.0);
        assert_eq!(geometry(100, Some(99), 600.0).centred, 6018.0 - 600.0);
        // Nothing playing: the top.
        assert_eq!(geometry(100, None, 600.0).centred, 0.0);
    }
}
