//! Quick search, like the official app: typing in the header box opens a small
//! menu right under it with the best matches (top result, tracks, albums,
//! artists, playlists) and recent searches. The full results page only opens
//! on Enter or with "Ver todos los resultados".

use super::OneLine;
use std::time::Duration;

use super::sc;
use gpui::AnimationExt;
use gpui::{deferred, div, prelude::*, AnyElement, Context, FontWeight, SharedString, Window};

use super::icons::Icon;
use super::widgets::cover;
use super::{theme, AppView, Load, Page};
use crate::tidal::{Item, SearchResults};

const DEBOUNCE: Duration = Duration::from_millis(260);

impl AppView {
    pub fn with_local_search(&self, query: &str, mut results: SearchResults) -> SearchResults {
        // Accent- and case-insensitive; track keys are folded once per library change.
        let query = crate::local::fold(query);
        let matches = |fields: &[&str]| fields.iter().any(|s| crate::local::fold(s).contains(&query));
        results.tracks.retain(|t| t.local_path.is_none());
        results.albums.retain(|a| !crate::local::is_local_id(a.id));
        results.artists.retain(|a| !crate::local::is_local_id(a.id));
        if results.top.as_ref().is_some_and(|item| match item {
            Item::Track(t) => t.local_path.is_some(),
            Item::Album(a) => crate::local::is_local_id(a.id),
            Item::Artist(a) => crate::local::is_local_id(a.id),
            _ => false,
        }) {
            results.top = None;
        }
        let mut tracks: Vec<_> = self
            .lib_tracks
            .iter()
            .filter(|t| {
                t.local_path.is_some()
                    && match self.local_search_keys.get(&t.id) {
                        Some(key) => key.contains(&query),
                        None => crate::local::search_key(t).contains(&query),
                    }
            })
            .cloned()
            .collect();
        tracks.extend(results.tracks);
        results.tracks = tracks;
        let mut albums: Vec<_> = self
            .lib_albums
            .iter()
            .filter(|a| crate::local::is_local_id(a.id) && matches(&[&a.title, &a.artist_names()]))
            .cloned()
            .collect();
        albums.extend(results.albums);
        results.albums = albums;
        let mut artists: Vec<_> =
            self.lib_artists.iter().filter(|a| crate::local::is_local_id(a.id) && matches(&[&a.name])).cloned().collect();
        artists.extend(results.artists);
        results.artists = artists;
        if results.top.is_none() {
            results.top = results
                .tracks
                .first()
                .cloned()
                .map(Item::Track)
                .or_else(|| results.albums.first().cloned().map(Item::Album))
                .or_else(|| results.artists.first().cloned().map(Item::Artist));
        }
        results
    }

    pub fn load_search(&mut self, query: String, cx: &mut Context<Self>) {
        let local = self.with_local_search(&query, SearchResults::default());
        let has_local = local.top.is_some();
        self.search.insert(query.clone(), if has_local || !self.signed_in { Load::Ready(local) } else { Load::Loading });
        if !self.signed_in {
            return;
        }
        let q = query.clone();
        self.fetch(
            cx,
            move |t| t.search(&q),
            move |view, result, _| {
                let result = match result {
                    Ok(remote) => Load::Ready(view.with_local_search(&query, remote)),
                    Err(error) => {
                        let local = view.with_local_search(&query, SearchResults::default());
                        if local.top.is_some() {
                            Load::Ready(local)
                        } else {
                            Load::Failed(format!("{error:#}"))
                        }
                    }
                };
                view.search.insert(query, result);
            },
        );
    }

    /// The text in the search box changed: show the menu and search after a short pause.
    pub fn search_typed(&mut self, text: &str, cx: &mut Context<Self>) {
        self.search_query = text.trim().to_string();
        self.search_open = true;
        self.search_gen += 1;
        let gen = self.search_gen;
        let q = self.search_query.clone();
        if !q.is_empty() && !self.search.contains_key(&q) {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(DEBOUNCE).await;
                let _ = this.update(cx, |this, cx| {
                    if this.search_gen == gen && !this.search.contains_key(&q) {
                        this.load_search(q, cx);
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// Open the full results page for `q` (Enter or "see all").
    pub fn search_all(&mut self, q: String, window: &mut Window, cx: &mut Context<Self>) {
        let q = q.trim().to_string();
        self.search_open = false;
        if q.is_empty() {
            return;
        }
        {
            let mut s = self.settings.lock();
            s.recent_searches.retain(|x| x != &q);
            s.recent_searches.insert(0, q.clone());
            s.recent_searches.truncate(12);
            s.save();
        }
        if let Page::Search(_) = self.page {
            self.page = Page::Search(q);
            self.page_changed(cx);
        } else {
            self.navigate(Page::Search(q), cx);
        }
        window.focus(&self.focus);
    }

    fn use_recent(&mut self, q: String, cx: &mut Context<Self>) {
        self.search_input.update(cx, |i, cx| i.set_text(&q, cx));
        self.search_typed(&q, cx);
    }
}

fn row(
    id: SharedString,
    cover_url: Option<String>,
    round: bool,
    title: String,
    sub: String,
    cx: &mut Context<AppView>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .h(sc(54.))
        .px(sc(10.))
        .flex()
        .items_center()
        .gap(sc(10.))
        .rounded(sc(9.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .active(|s| s.bg(theme::active()))
        .child(cover(cover_url, 36., round, cx))
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .flex()
                .flex_col()
                .gap(sc(2.))
                .child(div().one_line().text_size(sc(12.)).font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().one_line().text_size(sc(10.5)).text_color(theme::text_secondary()).child(sub)),
        )
}

/// A section title; the first one needs no rule above it (the panel has one).
fn heading(first: bool, t: &'static str) -> AnyElement {
    div()
        .flex_none()
        .px(sc(8.))
        .when(!first, |d| d.mt(sc(6.)).pt(sc(9.)).border_t_1().border_color(theme::divider()))
        .when(first, |d| d.pt(sc(4.)))
        .pb(sc(6.))
        .text_size(sc(12.))
        .font_weight(FontWeight::BOLD)
        .text_color(theme::text_dim())
        .child(t)
        .into_any_element()
}

pub fn overlay(view: &mut AppView, window: &mut Window, cx: &mut Context<AppView>) -> Option<AnyElement> {
    if !view.search_open {
        return None;
    }
    let q = view.search_query.clone();
    let mut body: Vec<AnyElement> = Vec::new();
    if q.is_empty() {
        let recent = view.settings.lock().recent_searches.clone();
        if recent.is_empty() {
            body.push(
                div()
                    .p(sc(14.))
                    .text_size(sc(12.))
                    .text_color(theme::text_dim())
                    .child(crate::i18n::t("type_to_search_songs_albums_artists_or_playlists"))
                    .into_any_element(),
            );
        } else {
            body.push(heading(body.is_empty(), crate::i18n::t("recent_searches")));
            // The new glass pills, wrapped: a recent search is one click.
            body.push(
                div()
                    .flex_none()
                    .px(sc(8.))
                    .pb(sc(6.))
                    .flex()
                    .flex_wrap()
                    .gap(sc(8.))
                    .children(recent.into_iter().take(8).enumerate().map(|(i, r)| {
                        let rr = r.clone();
                        super::widgets::pill(
                            ("recent-q", i),
                            r,
                            Some(Icon::Clock),
                            false,
                            move |this, _, _, cx| this.use_recent(rr.clone(), cx),
                            cx,
                        )
                        .into_any_element()
                    }))
                    .into_any_element(),
            );
        }
    } else {
        match view.search.get(&q) {
            Some(Load::Ready(r)) => {
                let r = r.clone();
                if let Some(top) = r.top.clone() {
                    body.push(heading(body.is_empty(), crate::i18n::t("top_result")));
                    let (title, sub, url, round) = match &top {
                        Item::Album(a) => {
                            (a.title.clone(), crate::i18n::text("album.detail", &[a.artist_names().to_string()]), a.cover(160), false)
                        }
                        Item::Artist(a) => (a.name.clone(), crate::i18n::t("artist").into(), a.picture(160), true),
                        Item::Track(t) => {
                            (t.display_title(), crate::i18n::text("song", &[t.artist_names().to_string()]), t.cover(160), false)
                        }
                        Item::Playlist(p) => (p.title.clone(), crate::i18n::t("playlist").into(), p.cover(160), false),
                        _ => (String::new(), String::new(), None, false),
                    };
                    if !title.is_empty() {
                        body.push(
                            row("sq-top".into(), url, round, title, sub, cx)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.search_open = false;
                                    this.open_item(&top, cx);
                                }))
                                .into_any_element(),
                        );
                    }
                }
                if !r.tracks.is_empty() {
                    body.push(heading(body.is_empty(), crate::i18n::t("songs")));
                    let list: Vec<_> = r.tracks.iter().take(8).cloned().collect();
                    for (i, t) in list.iter().enumerate() {
                        let all = list.clone();
                        body.push(
                            row(SharedString::from(format!("sq-t-{i}")), t.cover(160), false, t.display_title(), t.artist_names(), cx)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.search_open = false;
                                    this.play_tracks(all.clone(), i, cx);
                                }))
                                .into_any_element(),
                        );
                    }
                }
                let mut others: Vec<(Item, String, String, Option<String>, bool)> = Vec::new();
                for a in r.albums.iter().take(5) {
                    others.push((
                        Item::Album(a.clone()),
                        a.title.clone(),
                        crate::i18n::text("album.detail", &[a.artist_names().to_string()]),
                        a.cover(160),
                        false,
                    ));
                }
                for a in r.artists.iter().take(4) {
                    others.push((Item::Artist(a.clone()), a.name.clone(), crate::i18n::t("artist").into(), a.picture(160), true));
                }
                for p in r.playlists.iter().take(3) {
                    others.push((Item::Playlist(p.clone()), p.title.clone(), crate::i18n::t("playlist").into(), p.cover(160), false));
                }
                if !others.is_empty() {
                    body.push(heading(body.is_empty(), crate::i18n::t("albums_artists_and_playlists")));
                    for (i, (item, title, sub, url, round)) in others.into_iter().enumerate() {
                        body.push(
                            row(SharedString::from(format!("sq-o-{i}")), url, round, title, sub, cx)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.search_open = false;
                                    this.open_item(&item, cx);
                                }))
                                .into_any_element(),
                        );
                    }
                }
                if body.is_empty() {
                    body.push(
                        div()
                            .p(sc(14.))
                            .text_size(sc(12.))
                            .text_color(theme::text_dim())
                            .child(crate::i18n::t("no_results"))
                            .into_any_element(),
                    );
                }
            }
            Some(Load::Failed(e)) => {
                body.push(div().p(sc(14.)).text_size(sc(12.)).text_color(theme::danger()).child(e.clone()).into_any_element())
            }
            _ => body.push(
                div().p(sc(14.)).text_size(sc(12.)).text_color(theme::text_dim()).child(crate::i18n::t("searching")).into_any_element(),
            ),
        }
    }
    // "See all results" lives in a bar under the list, always in view.
    let footer = (!q.is_empty()).then(|| {
        let qq = q.clone();
        let shown = if q.chars().count() > 26 { format!("{}…", q.chars().take(25).collect::<String>()) } else { q.clone() };
        div()
            .flex_none()
            .px(sc(14.))
            .py(sc(10.))
            .border_t_1()
            .border_color(theme::divider())
            .flex()
            .items_center()
            .justify_end()
            .child(super::widgets::pill(
                "sq-all",
                crate::i18n::text("see_all_results_for", &[shown]),
                Some(Icon::Search),
                false,
                move |this, _, window, cx| this.search_all(qq.clone(), window, cx),
                cx,
            ))
            .into_any_element()
    });
    let layout = super::titlebar::layout(view, window);
    let zoom = super::zoom();
    let (vw, vh) = (f32::from(window.viewport_size().width) / zoom, f32::from(window.viewport_size().height) / zoom);
    // Everything below the top bar sits under the veil; the bar (and the window
    // controls in it) stay usable.
    let veil_top = theme::FRAME_GAP + layout.height;
    // The panel scales with the window: roomy on a large screen, full width on a narrow one.
    let panel_w = (vw * 0.5).clamp(460., 760.).min(vw - 24.);
    let panel_top = (vh * 0.09).clamp(16., 120.);
    let list_h = (vh - veil_top - panel_top - 150.).clamp(160., (vh * 0.62).max(160.));
    let veil = {
        // GPUI cannot blur what is behind a view, so the background is frosted
        // the other way: a veil of the page colour that thickens toward the
        // bottom, over which the panel floats.
        let base = theme::bg();
        let (top, bottom) = if theme::is_light() { (0.62, 0.78) } else { (0.58, 0.8) };
        div()
            .id("search-veil")
            .absolute()
            .top(sc(veil_top))
            .left(sc(0.))
            .right(sc(0.))
            .bottom(sc(0.))
            .occlude()
            .bg(theme::gradient(
                180.,
                gpui::linear_color_stop(super::fx::with_alpha(base, top), 0.),
                gpui::linear_color_stop(super::fx::with_alpha(base, bottom), 1.),
            ))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if !this.search_sb_drag {
                        this.search_open = false;
                        window.focus(&this.focus);
                        cx.notify();
                    }
                }),
            )
            .with_animation("search-veil-fade", super::motion::animation(super::motion::CONTROL), super::motion::enter)
    };
    let input_row = div()
        .flex_none()
        .h(sc(54.))
        .px(sc(16.))
        .flex()
        .items_center()
        .gap(sc(12.))
        .child(Icon::Search.el(17., theme::text_secondary()))
        .child(div().flex_1().min_w(sc(0.)).overflow_hidden().child(view.search_input.clone()))
        .child(
            div()
                .flex_none()
                .h(sc(22.))
                .px(sc(8.))
                .flex()
                .items_center()
                .rounded(sc(7.))
                .bg(theme::ink(0.07))
                .border_1()
                .border_color(theme::glass_rim(0.))
                .text_size(sc(10.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_dim())
                .child(crate::i18n::t("search.close_key")),
        );
    let list = div().relative().flex_none().child(
        div()
            .id("search-menu-list")
            .max_h(sc(list_h))
            .overflow_y_scroll()
            .track_scroll(&view.search_scroll)
            .child({
                super::smooth::register("search", &view.search_scroll);
                super::smooth::glide("search")
            })
            .p(sc(8.))
            .pr(sc(16.))
            .flex()
            .flex_col()
            .children(body),
    );
    let list = list.children(scrollbar(view, cx));
    Some(
        deferred(
            div().absolute().top(sc(0.)).left(sc(0.)).size_full().child(veil).child(
                div().absolute().top(sc(panel_top)).left(sc(0.)).right(sc(0.)).flex().justify_center().child(
                    super::widgets::floating_panel(18.)
                        .id("search-menu")
                        .occlude()
                        .w(sc(panel_w))
                        .flex()
                        .flex_col()
                        .child(super::glass::sheen_rim("quick-search-glass", 18., 0.5))
                        .child(input_row)
                        .child(div().h(sc(1.)).flex_none().bg(theme::divider()))
                        .child(list)
                        .children(footer)
                        .with_animation("search_menu-entrance", super::motion::animation(super::motion::PANEL), |d, t| {
                            d.opacity(if super::motion::reduced_motion() { 1.0 } else { t })
                        }),
                ),
            ),
        )
        .with_priority(3)
        .into_any_element(),
    )
}

/// Draggable scrollbar on the right of the quick-search menu.
fn scrollbar(view: &AppView, cx: &mut Context<AppView>) -> Option<AnyElement> {
    let h = &view.search_scroll;
    let max = f32::from(h.max_offset().height);
    let vh = f32::from(h.bounds().size.height);
    if max < 1.0 || vh < 1.0 {
        return None;
    }
    let z = super::zoom();
    let (max, vh) = (max / z, vh / z);
    let thumb = (vh * vh / (vh + max)).max(36.0);
    let frac = (-f32::from(h.offset().y) / z / max).clamp(0.0, 1.0);
    let top = 6.0 + frac * (vh - 12.0 - thumb);
    let dragging = view.search_sb_drag;
    Some(
        div()
            .id("search-sb")
            .absolute()
            .top(sc(0.))
            .bottom(sc(0.))
            .right(sc(2.))
            .w(sc(12.))
            .cursor_pointer()
            .group("ssb")
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, ev: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.search_sb_drag = true;
                    this.search_scrollbar_to(ev.position);
                    cx.notify();
                }),
            )
            .child(
                div()
                    .absolute()
                    .top(sc(top))
                    .right(sc(3.))
                    .h(sc(thumb))
                    .w(sc(if dragging { 6. } else { 4. }))
                    .rounded(sc(3.))
                    .bg(gpui::rgba(if dragging { 0xffffffaa } else { 0xffffff45 }))
                    .group_hover("ssb", |s| s.w(sc(6.)).bg(theme::ink(0.5))),
            )
            .into_any_element(),
    )
}

impl AppView {
    /// Scroll the quick-search menu to the pointer's height on its scrollbar.
    pub fn search_scrollbar_to(&mut self, pos: gpui::Point<gpui::Pixels>) {
        let h = &self.search_scroll;
        let b = h.bounds();
        let max = f32::from(h.max_offset().height);
        let vh = f32::from(b.size.height);
        if max < 1.0 || vh < 1.0 {
            return;
        }
        let frac = ((f32::from(pos.y) - f32::from(b.origin.y)) / vh).clamp(0.0, 1.0);
        h.set_offset(gpui::point(gpui::px(0.), gpui::px(-max * frac)));
    }
}
