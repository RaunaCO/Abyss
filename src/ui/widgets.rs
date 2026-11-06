//! Reusable pieces: covers, cards, track rows, buttons, badges.

use super::sc;
use gpui::{
    div, img, prelude::*, px, relative, AnyElement, ClickEvent, Context, ElementId, FontWeight, MouseButton, MouseDownEvent,
    ObjectFit, Rgba, SharedString, Window,
};

use super::icons::Icon;
use super::{images, theme, AppView, MenuTarget, Page};
use crate::tidal::{Item, Track};

pub fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// 7728 → "2:08:48"; 968 → "16:08".
pub fn fmt_hms(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn fmt_long(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{m} min")
    }
}

/// Square (or round) cover art with a dark placeholder while loading.
pub fn cover(url: Option<String>, size: f32, round: bool, cx: &mut Context<AppView>) -> AnyElement {
    let radius = if round { sc(size / 2.0) } else { sc(4.0) };
    let image = url.as_deref().and_then(|u| images::get(u, cx));
    match image {
        Some(i) => img(i).size(sc(size)).flex_none().rounded(radius).object_fit(ObjectFit::Cover).into_any_element(),
        None => div()
            .size(sc(size))
            .flex_none()
            .rounded(radius)
            .bg(theme::surface())
            .flex()
            .items_center()
            .justify_center()
            .child(Icon::Track.el(size * 0.3, theme::text_dim()))
            .into_any_element(),
    }
}

/// Three bouncing bars shown next to the playing track.
pub fn eq_bars(playing: bool, color: Rgba) -> impl IntoElement {
    use gpui::{Animation, AnimationExt};
    div()
        .h(sc(14.))
        .flex()
        .items_end()
        .gap(sc(2.))
        .children((0..3usize).map(move |i| {
            let bar = div().w(sc(3.)).rounded(sc(1.)).bg(color);
            if playing {
                bar.with_animation(
                    ("eq-bar", i as usize),
                    Animation::new(std::time::Duration::from_millis(700 + i as u64 * 170)).repeat(),
                    move |b, t| {
                        let phase = t * std::f32::consts::TAU + i as f32 * 1.9;
                        b.h(sc(4.0 + 10.0 * (0.5 + 0.5 * phase.sin())))
                    },
                )
                .into_any_element()
            } else {
                bar.h(sc(4.0 + i as f32 * 3.0)).into_any_element()
            }
        }))
}

/// 2x2 grid of covers (playlists without an image of their own).
pub fn mosaic(urls: &[String], size: f32, cx: &mut Context<AppView>) -> AnyElement {
    let half = size / 2.0;
    let cells: Vec<AnyElement> = urls
        .iter()
        .take(4)
        .map(|u| match images::get(u, cx) {
            Some(i) => img(i).size(sc(half)).flex_none().object_fit(ObjectFit::Cover).into_any_element(),
            None => div().size(sc(half)).flex_none().bg(theme::surface()).into_any_element(),
        })
        .collect();
    div()
        .size(sc(size))
        .flex_none()
        .rounded(sc(4.))
        .overflow_hidden()
        .flex()
        .flex_wrap()
        .children(cells)
        .into_any_element()
}

pub fn icon_button(
    id: impl Into<ElementId>,
    icon: Icon,
    size: f32,
    color: Rgba,
    on_click: impl Fn(&mut AppView, &ClickEvent, &mut Window, &mut Context<AppView>) + 'static,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_none()
        .p(sc(6.))
        .rounded(sc(999.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .active(|s| s.bg(theme::active()).opacity(0.6))
        .child(Icon::el(icon, size, color))
        .on_click(cx.listener(move |this, ev, window, cx| {
            cx.stop_propagation();
            on_click(this, ev, window, cx)
        }))
}

pub fn pill(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: Option<Icon>,
    primary: bool,
    on_click: impl Fn(&mut AppView, &ClickEvent, &mut Window, &mut Context<AppView>) + 'static,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let (bg, fg) = if primary { (theme::button(), theme::button_text()) } else { (theme::elevated(), theme::text()) };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(sc(8.))
        .h(sc(36.))
        .px(sc(18.))
        .rounded(sc(18.))
        .bg(bg)
        .text_color(fg)
        .text_size(sc(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|s| s.opacity(0.88).shadow_md())
        .active(|s| s.opacity(0.6))
        .when_some(icon, |d, i| d.child(i.el(18., fg)))
        .child(label.into())
        .on_click(cx.listener(on_click))
}

pub fn quality_badge(tags: &[String]) -> Option<AnyElement> {
    let (label, fg, bg) = if tags.iter().any(|t| t == "HIRES_LOSSLESS") {
        ("MAX", theme::gold(), theme::gold_bg())
    } else if tags.iter().any(|t| t == "DOLBY_ATMOS") {
        ("ATMOS", theme::text(), theme::active())
    } else {
        return None;
    };
    Some(
        div()
            .flex_none()
            .px(sc(5.))
            .py(sc(1.))
            .rounded(sc(3.))
            .bg(bg)
            .text_color(fg)
            .text_size(sc(10.))
            .font_weight(FontWeight::BOLD)
            .child(label)
            .into_any_element(),
    )
}

pub fn explicit_badge() -> AnyElement {
    div()
        .flex_none()
        .size(sc(14.))
        .rounded(sc(2.))
        .bg(theme::text_dim())
        .text_color(theme::bg())
        .text_size(sc(9.))
        .font_weight(FontWeight::BOLD)
        .flex()
        .items_center()
        .justify_center()
        .child("E")
        .into_any_element()
}

/// Title of a card item.
pub fn item_title(item: &Item) -> String {
    item_text(item).0
}

fn item_text(item: &Item) -> (String, String, Option<String>, bool) {
    match item {
        Item::Album(a) => (a.title.clone(), a.artist_names(), a.cover(320), false),
        Item::Playlist(p) => (
            p.title.clone(),
            if p.creator.is_empty() { format!("{} pistas", p.number_of_tracks) } else { format!("de {}", p.creator) },
            p.cover(320),
            false,
        ),
        Item::Artist(a) => (a.name.clone(), "Artista".into(), a.picture(320), true),
        Item::Mix(m) => (m.title.clone(), m.subtitle.clone(), m.image.clone(), false),
        Item::Track(t) => (t.display_title(), t.artist_names(), t.cover(320), false),
        Item::Link(l) => (l.title.clone(), String::new(), None, false),
        Item::Promo(p) => (p.title.clone(), p.subtitle.clone(), p.image_url(), false),
    }
}

/// A cover card: cover edge to edge (no panel), bold title, grey artist and,
/// in grids, the release year.
pub fn card(key: SharedString, item: &Item, width: f32, detail: bool, view: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let (title, subtitle, url, round) = item_text(item);
    let group = key.clone();
    let id = ElementId::Name(key);
    let open = item.clone();
    let play = item.clone();
    let group_for_stars = group.clone();
    let album_id = if let Item::Album(a) = item { Some(a.id) } else { None };
    let (explicit, year) = match item {
        Item::Album(a) => (a.explicit, a.year().to_string()),
        Item::Track(t) => (t.explicit, String::new()),
        _ => (false, String::new()),
    };
    div()
        .id(id)
        .w(sc(width))
        .flex_none()
        .flex()
        .flex_col()
        .gap(sc(7.))
        .cursor_pointer()
        .group(group.clone())
        .on_click(cx.listener(move |this, _, _, cx| this.open_item(&open, cx)))
        .child(
            div()
                .relative()
                .size(sc(width))
                .rounded(sc(if round { width / 2. } else { 4. }))
                // Hover: the cover lifts off the page.
                .group_hover(group.clone(), |s| s.shadow_lg())
                .child(cover(url, width, round, cx))
                // Hover: the cover dims slightly and a play button appears.
                .child(
                    div()
                        .absolute()
                        .top(sc(0.))
                        .left(sc(0.))
                        .size_full()
                        .rounded(sc(if round { width / 2. } else { 4. }))
                        .bg(gpui::rgba(0x00000000))
                        .group_hover(group.clone(), |s| {
                            // Official card hover: gradient from clear to 70 % black.
                            s.bg(gpui::linear_gradient(
                                180.,
                                gpui::linear_color_stop(gpui::rgba(0x00000000), 0.),
                                gpui::linear_color_stop(gpui::rgba(0x000000b3), 1.),
                            ))
                        }),
                )
                .child(
                    div()
                        .id("play")
                        .absolute()
                        .right(sc(8.))
                        .bottom(sc(8.))
                        .size(sc(34.))
                        .rounded(sc(17.))
                        .bg(theme::button())
                        .flex()
                        .items_center()
                        .justify_center()
                        .invisible()
                        .group_hover(group, |s| s.visible())
                        .hover(|s| s.bg(theme::accent()))
                        .active(|s| s.opacity(0.7))
                        .child(Icon::Play.el(18., theme::button_text()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.play_item(&play, cx)
                        })),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(sc(3.))
                .min_w(sc(0.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(sc(6.))
                        .min_w(sc(0.))
                        .text_size(sc(12.))
                        .font_weight(FontWeight::BOLD)
                        .child(div().min_w(sc(0.)).truncate().child(title))
                        .when(explicit, |d| d.child(explicit_badge())),
                )
                .child(div().truncate().text_color(theme::text_secondary()).text_size(sc(11.5)).child(subtitle))
                .when(detail && !year.is_empty(), |d| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_size(sc(10.)).font_weight(FontWeight::BOLD).text_color(theme::text_dim()).child(year))
                            .when_some(album_id, |d, aid| {
                                // Stars: always visible once rated, otherwise on hover.
                                let rated = view.ratings.get_album(aid) > 0;
                                d.child(
                                    div()
                                        .when(!rated, |d| d.invisible().group_hover(group_for_stars.clone(), |s| s.visible()))
                                        .child(album_stars("card", aid, 11., view, cx)),
                                )
                            }),
                    )
                }),
        )
        .into_any_element()
}

#[derive(Clone, Copy)]
pub struct RowStyle {
    pub cover: bool,
    pub album: bool,
    pub number: bool,
    pub date: bool,
    pub bpm: bool,
    pub rating: bool,
    /// Collection section whose sort the column headers control.
    pub sortable: Option<&'static str>,
}

impl RowStyle {
    pub const fn new(cover: bool, album: bool, number: bool) -> Self {
        Self { cover, album, number, date: false, bpm: false, rating: true, sortable: None }
    }
}

/// "2026-07-19T12:00:00" → "19 jul 2026".
pub fn fmt_date(iso: &str) -> String {
    const MONTHS: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"];
    let mut it = iso.get(..10).unwrap_or("").split('-');
    let (Some(y), Some(m), Some(d)) = (it.next(), it.next(), it.next()) else { return String::new() };
    let (Ok(m), Ok(d)) = (m.parse::<usize>(), d.parse::<u32>()) else { return String::new() };
    format!("{d} {} {y}", MONTHS.get(m.wrapping_sub(1)).copied().unwrap_or(""))
}

/// Column widths shared by the header and the rows so they line up.
pub struct Cols;

impl Cols {
    pub const NUM: f32 = 30.0;
    pub const DATE: f32 = 100.0;
    pub const TIME: f32 = 46.0;
    pub const BPM: f32 = 38.0;
    pub const RATING: f32 = 84.0;
    pub const ACTIONS: f32 = 84.0;
    pub fn artist(style: RowStyle) -> f32 {
        if style.album { 0.22 } else { 0.30 }
    }
    pub const ALBUM: f32 = 0.20;
}

/// A track line in a list, laid out in the original's columns:
/// number, cover + title, artist, album, date added, time, BPM, "+" and like.
pub fn track_row(
    ix: usize,
    track: &Track,
    style: RowStyle,
    on_play: impl Fn(&mut AppView, usize, &mut Context<AppView>) + 'static,
    own_playlist: Option<String>,
    view: &AppView,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let drag = own_playlist.clone().map(|uuid| super::playlists::DraggedTrack { uuid, index: ix, title: track.display_title() });
    let drop_uuid = own_playlist.clone();
    let menu_uuid = own_playlist.clone();
    let playing = view.now.as_ref().map(|n| n.track.id) == Some(track.id);
    let fav = view.fav_ids.contains(&track.id);
    let group: SharedString = format!("track-{ix}").into();
    let title_color = if playing { theme::accent() } else { theme::text() };
    let t_menu = track.clone();
    let t_fav = track.clone();
    let t_more = track.clone();
    let t_add = track.clone();
    let album_id = track.album.as_ref().map(|a| a.id);
    let artist_id = track.artists.first().map(|a| a.id);
    let on_play = std::rc::Rc::new(on_play);
    let p1 = on_play.clone();
    let p2 = on_play.clone();
    let audible = !view.snap.paused && !view.ended;

    let link = |id: (&'static str, usize), text: String, rel: f32, target: Option<Page>, cx: &mut Context<AppView>| {
        div()
            .id(id)
            .w(relative(rel))
            .flex_none()
            .truncate()
            .text_size(sc(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .when(target.is_some(), |d| d.cursor_pointer().hover(|s| s.underline()))
            .child(text)
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(t) = target.clone() {
                    cx.stop_propagation();
                    this.navigate(t, cx);
                }
            }))
    };

    div()
        .id(("track", ix))
        .h(sc(theme::ROW_H))
        .px(sc(8.))
        .flex()
        .items_center()
        .gap(sc(12.))
        .rounded(sc(4.))
        .group(group.clone())
        .hover(|s| s.bg(theme::hover()))
        .active(|s| s.bg(theme::active()))
        .when(playing, |d| d.bg(theme::accent_dim()))
        .when(!track.streamable, |d| d.opacity(0.4))
        // Reorder by dragging (own playlists only).
        .when_some(drag, |d, dt| d.on_drag(dt, |dt, _, _, cx| cx.new(|_| dt.clone())))
        .when_some(drop_uuid, |d, uuid| {
            d.drag_over::<super::playlists::DraggedTrack>(|s, _, _, _| s.border_t_2().border_color(theme::accent()))
                .on_drop(cx.listener(move |this, dt: &super::playlists::DraggedTrack, _, cx| {
                    if dt.uuid == uuid {
                        this.move_playlist_track(uuid.clone(), dt.index, ix, cx);
                    }
                }))
        })
        .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
            if ev.click_count() >= 2 {
                p1(this, ix, cx)
            }
        }))
        // Gesture: middle click adds the track to the end of the queue.
        .on_mouse_down(MouseButton::Middle, {
            let t = track.clone();
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.add_to_queue(t.clone(), false, cx);
                this.pulse("queue");
            })
        })
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                this.menu = Some((
                    ev.position,
                    match &menu_uuid {
                        Some(uuid) => MenuTarget::PlaylistTrack { uuid: uuid.clone(), index: ix, track: t_menu.clone() },
                        None => MenuTarget::Track(t_menu.clone()),
                    },
                ));
                cx.notify();
            }),
        )
        // Number, replaced by a play button on hover.
        .child(
            div()
                .relative()
                .w(sc(Cols::NUM))
                .flex_none()
                .flex()
                .justify_center()
                .text_size(sc(11.))
                .text_color(theme::text_dim())
                .child(
                    div()
                        .group_hover(group.clone(), |s| s.invisible())
                        .when(playing, |d| d.child(eq_bars(audible, theme::accent())))
                        .when(!playing, |d| {
                            d.child(if style.number && track.track_number > 0 {
                                track.track_number.to_string()
                            } else {
                                (ix + 1).to_string()
                            })
                        }),
                )
                .child(
                    div()
                        .id("rowplay")
                        .absolute()
                        .invisible()
                        .group_hover(group.clone(), |s| s.visible())
                        .cursor_pointer()
                        .child(Icon::Play.el(15., theme::text()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            p2(this, ix, cx)
                        })),
                ),
        )
        // Cover + title.
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .flex()
                .items_center()
                .gap(sc(10.))
                .when(style.cover, |d| d.child(cover(track.cover(80), 32., false, cx)))
                .child(
                    div()
                        .flex_1()
                        .min_w(sc(0.))
                        .flex()
                        .items_center()
                        .gap(sc(6.))
                        .child(
                            div()
                                .min_w(sc(0.))
                                .truncate()
                                .text_size(sc(12.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(title_color)
                                .child(track.display_title()),
                        )
                        .when(track.explicit, |d| d.child(explicit_badge())),
                ),
        )
        .child(link(("artist", ix), track.artist_names(), Cols::artist(style), artist_id.map(Page::Artist), cx))
        .when(style.album, |d| {
            d.child(link(
                ("album", ix),
                track.album.as_ref().map(|a| a.title.clone()).unwrap_or_default(),
                Cols::ALBUM,
                album_id.map(Page::Album),
                cx,
            ))
        })
        .when(style.rating, |d| d.child(div().w(sc(Cols::RATING)).flex_none().child(stars("row", track.id, 13., view, cx))))
        .when(style.date, |d| {
            d.child(
                div()
                    .w(sc(Cols::DATE))
                    .flex_none()
                    .text_size(sc(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(track.added.as_deref().map(fmt_date).unwrap_or_default()),
            )
        })
        .child(
            div()
                .w(sc(Cols::TIME))
                .flex_none()
                .text_size(sc(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .child(fmt_time(track.duration as f64)),
        )
        .when(style.bpm, |d| {
            d.child(
                div()
                    .w(sc(Cols::BPM))
                    .flex_none()
                    .text_size(sc(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(track.bpm.map(|b| b.to_string()).unwrap_or_else(|| "-".into())),
            )
        })
        // "+" (add to playlist), like, and "…" on hover.
        .child(
            div()
                .w(sc(Cols::ACTIONS))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .gap(sc(8.))
                .child(
                    div()
                        .id(("add", ix))
                        .cursor_pointer()
                        .active(|s| s.opacity(0.5))
                        .child(Icon::Plus.el(15., theme::text_secondary()))
                        .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.menu = Some((ev.position(), MenuTarget::AddToPlaylist(vec![t_add.clone()])));
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .id(("fav", ix))
                        .cursor_pointer()
                        .active(|s| s.opacity(0.5))
                        .child(if fav {
                            Icon::HeartFilled.el(15. * view.heart_scale(track.id), theme::text())
                        } else {
                            Icon::Heart.el(15., theme::text_secondary())
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_favorite(&t_fav, cx)
                        })),
                )
                .child(
                    div()
                        .id(("more", ix))
                        .cursor_pointer()
                        .invisible()
                        .group_hover(group, |s| s.visible())
                        .child(Icon::More.el(15., theme::text_secondary()))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.menu = Some((ev.position, MenuTarget::Track(t_more.clone())));
                                cx.notify();
                            }),
                        ),
                ),
        )
        .into_any_element()
}

/// A labelled round action under a page header: icon with a small caption.
pub fn action(
    id: impl Into<ElementId>,
    icon: Icon,
    label: impl Into<SharedString>,
    active: bool,
    on_click: impl Fn(&mut AppView, &ClickEvent, &mut Window, &mut Context<AppView>) + 'static,
    cx: &mut Context<AppView>,
) -> AnyElement {
    div()
        .id(id)
        .flex_none()
        .w(sc(56.))
        .py(sc(4.))
        .flex()
        .flex_col()
        .items_center()
        .gap(sc(5.))
        .cursor_pointer()
        .rounded(sc(6.))
        .hover(|s| s.bg(theme::hover()))
        .active(|s| s.opacity(0.6))
        .child(icon.el(18., if active { theme::accent() } else { theme::text() }))
        .child(
            div()
                .text_size(sc(10.))
                .font_weight(FontWeight::BOLD)
                .text_color(if active { theme::accent() } else { theme::text() })
                .child(label.into()),
        )
        .on_click(cx.listener(on_click))
        .into_any_element()
}

pub fn section_title(title: impl Into<SharedString>) -> AnyElement {
    div()
        .pt(sc(24.))
        .pb(sc(12.))
        .text_size(sc(15.))
        .font_weight(FontWeight::BOLD)
        .child(title.into())
        .into_any_element()
}

pub fn message(text: impl Into<SharedString>) -> AnyElement {
    div().py(sc(48.)).text_color(theme::text_secondary()).child(text.into()).into_any_element()
}

/// Star rating of a track, 0 to 5 in half-star steps. Each star has a left and
/// a right half you can click; the stars preview the value under the pointer,
/// and clicking the current value clears the rating.
pub fn stars(scope: &'static str, track_id: u64, size: f32, view: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    stars_for(false, scope, track_id, size, view, cx)
}

/// The same control for a whole album.
pub fn album_stars(scope: &'static str, album_id: u64, size: f32, view: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    stars_for(true, scope, album_id, size, view, cx)
}

fn stars_for(album: bool, scope: &'static str, id: u64, size: f32, view: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let actual = if album { view.ratings.get_album(id) } else { view.ratings.get(id) };
    let shown = match view.rating_hover {
        Some((a, i, v)) if a == album && i == id => v,
        _ => actual,
    };
    let kind = if album { "album" } else { "track" };
    let pop_key = format!("stars-{scope}-{kind}-{id}");
    let pop = view.pulse_scale(&pop_key);
    div()
        .flex()
        .items_center()
        .children((0..5u8).map(|k| {
            let (half, full) = (2 * k + 1, 2 * k + 2);
            let icon = if shown >= full {
                Icon::Star
            } else if shown == half {
                Icon::StarHalf
            } else {
                Icon::StarOutline
            };
            let color = if shown >= half { theme::gold() } else { theme::text_dim() };
            let hit = |v: u8, left: bool, cx: &mut Context<AppView>| {
                div()
                    .id(ElementId::Name(format!("{scope}-{kind}-star-{id}-{v}").into()))
                    .absolute()
                    .top(sc(0.))
                    .h_full()
                    .w(gpui::relative(0.5))
                    .when(left, |d| d.left(sc(0.)))
                    .when(!left, |d| d.right(sc(0.)))
                    .cursor_pointer()
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.rating_hover = Some((album, id, v));
                        } else if this.rating_hover == Some((album, id, v)) {
                            this.rating_hover = None;
                        }
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.pulse(format!("stars-{scope}-{kind}-{id}"));
                        let now = if album { this.ratings.get_album(id) } else { this.ratings.get(id) };
                        let next = if now == v { 0 } else { v };
                        if album {
                            this.set_album_rating(id, next, cx);
                        } else {
                            this.set_rating(id, next, cx);
                        }
                    }))
            };
            div()
                .relative()
                .size(sc(size))
                .flex_none()
                .child(icon.el(size * pop, color))
                .child(hit(half, true, cx))
                .child(hit(full, false, cx))
        }))
        .into_any_element()
}

/// One of the quick links at the top of "For you": a small cover with the name beside it.
pub fn shortcut(id: (&'static str, usize), item: &Item, cx: &mut Context<AppView>) -> AnyElement {
    let (title, subtitle, url, _) = item_text(item);
    let open = item.clone();
    div()
        .id(id)
        .w(gpui::relative(0.328))
        .h(sc(46.))
        .flex_none()
        .flex()
        .items_center()
        .gap(sc(10.))
        .pr(sc(10.))
        .rounded(sc(6.))
        .overflow_hidden()
        .bg(theme::surface())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .active(|s| s.bg(theme::active()))
        .child(match &url {
            Some(_) => cover(url.clone(), 46., false, cx),
            None => div()
                .size(sc(46.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::linear_gradient(
                    135.,
                    gpui::linear_color_stop(gpui::rgb(0x6a5cff), 0.),
                    gpui::linear_color_stop(gpui::rgb(0x2bb8ff), 1.),
                ))
                .child(Icon::Heart.el(18., theme::text()))
                .into_any_element(),
        })
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .flex()
                .flex_col()
                .gap(sc(2.))
                .child(div().truncate().text_size(sc(11.)).font_weight(FontWeight::BOLD).child(title))
                .when(!subtitle.is_empty(), |d| d.child(div().truncate().text_size(sc(10.)).text_color(theme::text_dim()).child(subtitle))),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.open_item(&open, cx)))
        .into_any_element()
}
