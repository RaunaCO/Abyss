//! Library: Tracks, Albums and Artists of the collection in one view.
//!
//! The three categories are the existing collection pages (`Page::MyTracks`,
//! `MyAlbums`, `MyArtists`, their data, loading and sorting); this module adds
//! the shared header ("Library" + the active category), the control bar
//! (category switch on the left; sort, list/grid and search on the right) and
//! the list rows for albums and artists. Switching category replaces the page
//! in place (no history entry, no reload) and each category keeps its own
//! search text for the session.

use gpui::{canvas, div, prelude::*, AnyElement, Context, Focusable, FontWeight, SharedString, Window};

use super::icons::Icon;
use super::widgets::{cover, fmt_long};
use super::{interact, sc, theme, AppView, MenuTarget, Page};
use crate::tidal::{Album, Artist};

/// Height shared by every control of the bar (logical px).
const CTRL_H: f32 = 32.0;

pub fn categories() -> [(&'static str, &'static str); 3] {
    [("tracks", crate::i18n::t("tracks")), ("albums", crate::i18n::t("albums")), ("artists", crate::i18n::t("artists"))]
}

/// The collection section shown by `page`, if it is a Library category.
pub fn section(page: &Page) -> Option<&'static str> {
    match page {
        Page::MyTracks => Some("tracks"),
        Page::MyAlbums => Some("albums"),
        Page::MyArtists => Some("artists"),
        _ => None,
    }
}

pub fn page_of(section: &str) -> Page {
    match section {
        "albums" => Page::MyAlbums,
        "artists" => Page::MyArtists,
        _ => Page::MyTracks,
    }
}

fn label_of(section: &str) -> &'static str {
    categories().iter().find(|(s, _)| *s == section).map(|(_, l)| *l).unwrap_or(crate::i18n::t("tracks"))
}

impl AppView {
    /// Library as last shown (category remembered between sessions).
    pub fn library_page(&self) -> Page {
        page_of(&self.settings.lock().lib_last)
    }

    /// Show `section` in Library. From inside Library the content is swapped in
    /// place; from elsewhere it is an ordinary navigation.
    pub fn set_library_cat(&mut self, section: &'static str, cx: &mut Context<Self>) {
        let target = page_of(section);
        if super::library::section(&self.page).is_none() {
            self.navigate(target, cx);
            return;
        }
        if self.page == target {
            return;
        }
        self.page = target;
        self.after_library_switch(cx);
    }

    /// Bookkeeping when a Library category becomes the page (navigation or switch).
    pub fn after_library_switch(&mut self, cx: &mut Context<Self>) {
        let Some(sec) = super::library::section(&self.page) else { return };
        {
            let mut s = self.settings.lock();
            if s.lib_last != sec {
                s.lib_last = sec.to_string();
                s.save();
            }
        }
        // Each category keeps its own search; an incompatible one never carries over.
        let q = self.lib_queries.get(sec).cloned().unwrap_or_default();
        self.filter = q.trim().to_lowercase();
        self.filter_input.update(cx, |i, cx| {
            i.set_text(&q, cx);
            i.set_placeholder(crate::i18n::t("search.placeholder"), cx);
        });
        self.menu = None;
        self.expanded_album = None;
        self.list.scroll_to(gpui::ListOffset { item_ix: 0, offset_in_item: sc(0.) });
        self.load_page_data(cx);
        self.content_changed_pub();
        cx.notify();
    }

    /// Grid (true) or list for a category; albums and artists start as grids.
    pub fn lib_grid(&self, section: &str) -> bool {
        self.settings.lock().lib_grid.get(section).copied().unwrap_or(section != "tracks")
    }

    fn set_lib_grid(&mut self, section: &'static str, grid: bool, cx: &mut Context<Self>) {
        if self.lib_grid(section) == grid {
            return;
        }
        {
            let mut s = self.settings.lock();
            s.lib_grid.insert(section.to_string(), grid);
            s.save();
        }
        self.expanded_album = None;
        self.content_changed_pub();
        cx.notify();
    }
}

/// A segmented control: one surface, the active option lifted inside it.
fn segmented<T: Clone + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, AnyElement, f32)>,
    active: T,
    on_pick: impl Fn(&mut AppView, T, &mut Context<AppView>) + 'static,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let on_pick = std::rc::Rc::new(on_pick);
    div()
        .id(id)
        .h(sc(CTRL_H))
        .p(sc(3.))
        .flex()
        .flex_none()
        .items_center()
        .gap(sc(2.))
        .rounded(sc(9.))
        .relative()
        .child(super::glass::sheen(SharedString::from(format!("glass-{id}")), 9., 0.8))
        .bg(theme::liquid_glass(0.))
        .border_1()
        .border_color(theme::glass_rim(0.))
        .children(options.into_iter().enumerate().map(|(i, (value, content, w))| {
            let key = format!("{id}-{i}");
            let on = value == active;
            // The lifted surface fades between options (no jump).
            let a = interact::follow(&format!("seg-{key}"), if on { 1.0 } else { 0.0 }, super::motion::seconds(super::motion::CONTROL));
            let h = interact::hover(&key);
            let pick = on_pick.clone();
            let v = value.clone();
            let el = div()
                .id(SharedString::from(key.clone()))
                .h_full()
                .min_w(sc(w))
                .px(sc(10.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(sc(6.))
                .cursor_pointer()
                .bg(interact::mix(theme::ink(0.05 * h * (1.0 - a)), theme::control_active(), a))
                .when(a > 0.5, |d| d.shadow_sm())
                .child(content)
                .on_click(cx.listener(move |this, _, _, cx| pick(this, v.clone(), cx)));
            interact::track(el, key, cx).into_any_element()
        }))
        .into_any_element()
}

/// "Library" with the active category beside it, then the control bar.
pub fn bar(view: &mut AppView, section: &'static str, window: &Window, cx: &mut Context<AppView>) -> AnyElement {
    let col_w = view.column_width(window);
    // Keep search and sorting accessible as the available width changes.
    let search_w = if col_w < 640.0 {
        110.0
    } else if col_w < 820.0 {
        150.0
    } else {
        190.0
    };
    let compact = col_w < 560.0;

    let header = div()
        .pt(sc(24.))
        .pb(sc(16.))
        .flex()
        .flex_col()
        .gap(sc(4.))
        .child(div().text_size(sc(34.)).font_weight(FontWeight::BOLD).child(label_of(section)))
        .child(div().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_secondary()).child(crate::i18n::t("your_library")));

    // Left: the category switch.
    let cat_text = |label: &'static str, on: bool| -> AnyElement {
        div()
            .text_size(sc(12.5))
            .font_weight(if on { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
            .text_color(if on { theme::text() } else { theme::text_secondary() })
            .child(label)
            .into_any_element()
    };
    let cats = segmented(
        "lib-cat",
        categories().iter().map(|(s, l)| (*s, cat_text(l, *s == section), if compact { 52.0 } else { 70.0 })).collect(),
        section,
        |this, s, cx| this.set_library_cat(s, cx),
        cx,
    );

    // Right: sort criterion (readable text + chevron), list/grid, search.
    let (key, _) = view.sort_of(section);
    let sk = format!("lib-sort-{section}");
    let h = interact::hover(&sk);
    let sort = div()
        .id("lib-sort")
        .h(sc(CTRL_H))
        .px(sc(12.))
        .flex()
        .flex_none()
        .items_center()
        .gap(sc(6.))
        .rounded(sc(9.))
        .cursor_pointer()
        .relative()
        .child(super::glass::sheen("glass-lib-sort", 9., 0.9))
        .bg(theme::liquid_glass(h))
        .border_1()
        .border_color(theme::glass_rim(h))
        .text_size(sc(12.5))
        .font_weight(FontWeight::MEDIUM)
        .child(div().whitespace_nowrap().child(if col_w < 760.0 { key.short_label() } else { key.label() }))
        .child(Icon::ChevDown.el(12., theme::text_secondary()))
        // Remember where the button is, so the options open right under it.
        .child(
            canvas(
                {
                    let e = cx.entity();
                    move |b, _, cx| e.update(cx, |v, _| v.lib_sort_bounds = b)
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .relative()
        .on_click(cx.listener(move |this, _, _, cx| {
            let b = this.lib_sort_bounds;
            let at = gpui::point(b.origin.x, b.origin.y + b.size.height + sc(4.));
            this.menu = Some((at, MenuTarget::Sort(section)));
            cx.notify();
        }));
    let sort = interact::track(sort, sk, cx);

    let grid = view.lib_grid(section);
    let view_icon =
        |icon: Icon, on: bool| -> AnyElement { icon.el(15., if on { theme::text() } else { theme::text_secondary() }).into_any_element() };
    let views = segmented(
        "lib-view",
        vec![(false, view_icon(Icon::List, !grid), 30.0), (true, view_icon(Icon::Grid, grid), 30.0)],
        grid,
        move |this, g, cx| this.set_lib_grid(section, g, cx),
        cx,
    );

    let search = div()
        .id("lib-search")
        .w(sc(search_w))
        .h(sc(CTRL_H))
        .px(sc(10.))
        .flex()
        .flex_none()
        .items_center()
        .gap(sc(7.))
        .rounded(sc(9.))
        .relative()
        .child(super::glass::sheen("glass-lib-search", 9., 0.9))
        .bg(theme::liquid_glass(0.))
        .border_1()
        .border_color(theme::glass_rim(0.))
        .cursor_text()
        .child(Icon::Search.el(13., theme::text_secondary()))
        .child(div().flex_1().min_w(sc(0.)).overflow_hidden().child(view.filter_input.clone()))
        .on_click(cx.listener(|this, _, window, cx| {
            let fh = this.filter_input.focus_handle(cx);
            window.focus(&fh);
        }));

    let controls = div()
        .rounded(sc(12.))
        .bg(theme::ink(0.035))
        .border_1()
        .border_color(theme::panel_border())
        .p(sc(8.))
        .flex()
        .items_center()
        .justify_between()
        .flex_wrap()
        .gap(sc(10.))
        .child(cats)
        .child(div().flex().min_w(sc(0.)).max_w_full().flex_wrap().items_center().gap(sc(8.)).child(sort).child(views).child(search));

    div().pb(sc(16.)).child(header).child(controls).into_any_element()
}

/// Album in the list view: cover, title and artist, year, tracks, length.
pub fn album_row(i: usize, album: &Album, width: f32, cx: &mut Context<AppView>) -> AnyElement {
    let key = format!("lib-al-{i}");
    let (h, p) = (interact::hover(&key), interact::press(&key));
    let id = album.id;
    let meta = |t: String, w: f32| div().w(sc(w)).flex_none().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_secondary()).child(t);
    let el = div()
        .id(("lib-album", i))
        .h(sc(62.))
        .border_b_1()
        .border_color(theme::divider())
        .px(sc(12.))
        .flex()
        .items_center()
        .gap(sc(14.))
        .rounded(sc(6.))
        .cursor_pointer()
        .bg(interact::mix(super::fx::with_alpha(theme::hover(), h), theme::active(), p))
        .child(cover(album.cover(160), 44., false, cx))
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .flex()
                .flex_col()
                .gap(sc(2.))
                .child(div().truncate().text_size(sc(13.)).font_weight(FontWeight::SEMIBOLD).child(album.title.clone()))
                .child(div().truncate().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_secondary()).child(album.artist_names())),
        )
        .when(width >= 500., |d| d.child(meta(album.year().to_string(), 52.)))
        .when(width >= 420., |d| {
            d.child(meta(
                if album.number_of_tracks == 1 {
                    crate::i18n::t("1_track").to_string()
                } else {
                    crate::i18n::text("tracks.count", &[format!("{}", album.number_of_tracks)])
                },
                76.,
            ))
        })
        .when(width >= 320., |d| d.child(meta(fmt_long(album.duration as u64), 70.)))
        .on_click(cx.listener(move |this, _, _, cx| this.navigate(Page::Album(id), cx)))
        .on_mouse_down(gpui::MouseButton::Right, {
            let al = album.clone();
            cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                this.menu = Some((ev.position, MenuTarget::Album(al.clone())));
                cx.notify();
            })
        });
    interact::track(el, key, cx).into_any_element()
}

/// Artist in the list view: picture and name.
pub fn artist_row(i: usize, artist: &Artist, cx: &mut Context<AppView>) -> AnyElement {
    let key = format!("lib-ar-{i}");
    let (h, p) = (interact::hover(&key), interact::press(&key));
    let id = artist.id;
    let el = div()
        .id(("lib-artist", i))
        .h(sc(62.))
        .border_b_1()
        .border_color(theme::divider())
        .px(sc(12.))
        .flex()
        .items_center()
        .gap(sc(14.))
        .rounded(sc(6.))
        .cursor_pointer()
        .bg(interact::mix(super::fx::with_alpha(theme::hover(), h), theme::active(), p))
        .child(cover(artist.picture(320), 44., true, cx))
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .flex()
                .flex_col()
                .gap(sc(2.))
                .child(div().truncate().text_size(sc(13.)).font_weight(FontWeight::SEMIBOLD).child(artist.name.clone()))
                .child(div().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_secondary()).child(crate::i18n::t("artist"))),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.navigate(Page::Artist(id), cx)))
        .on_mouse_down(gpui::MouseButton::Right, {
            let ar = artist.clone();
            cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                this.menu = Some((ev.position, MenuTarget::Artist(ar.clone())));
                cx.notify();
            })
        });
    interact::track(el, key, cx).into_any_element()
}
