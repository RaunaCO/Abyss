//! Reusable pieces: covers, cards, track rows, buttons, badges.

use super::sc;
use super::OneLine;
use gpui::{
    div, img, prelude::*, px, relative, AnyElement, ClickEvent, Context, ElementId, FontWeight, MouseButton, MouseDownEvent, ObjectFit,
    Rgba, SharedString, Window,
};

use super::icons::Icon;
use super::interact;
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
        crate::i18n::text("h_min", &[format!("{}", h), format!("{}", m)])
    } else {
        crate::i18n::text("min", &[format!("{}", m)])
    }
}

/// Square (or round) cover art with a dark placeholder while loading.
pub fn cover(url: Option<String>, size: f32, round: bool, cx: &mut Context<AppView>) -> AnyElement {
    let radius = if round { sc(size / 2.0) } else { sc(4.0) };
    let image = url.as_deref().and_then(|u| images::sized(u, size, size, cx));
    match image {
        Some(i) => {
            // Fades in the first time it appears (paint-only, so it is smooth).
            let a = url.as_deref().map_or(1.0, images::fade_in);
            img(i).size(sc(size)).flex_none().rounded(radius).object_fit(ObjectFit::Cover).opacity(a).into_any_element()
        }
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

/// Text that navigates somewhere: slightly quieter at rest, it brightens to
/// full text colour (and is underlined) while hovered, fading in and out, so
/// it is clear it can be clicked. Only for interactive text.
pub fn nav_link(el: gpui::Stateful<gpui::Div>, key: String, cx: &mut Context<AppView>) -> gpui::Stateful<gpui::Div> {
    let h = interact::hover(&key);
    let rest = interact::mix(theme::text(), theme::text_secondary(), 0.35);
    interact::track(el.cursor_pointer().text_color(interact::mix(rest, theme::text(), h)).when(h > 0.5, |d| d.underline()), key, cx)
}

/// Same for secondary text (artist under a title): from the secondary colour to full text.
pub fn nav_link_secondary(el: gpui::Stateful<gpui::Div>, key: String, cx: &mut Context<AppView>) -> gpui::Stateful<gpui::Div> {
    let h = interact::hover(&key);
    interact::track(
        el.cursor_pointer().text_color(interact::mix(theme::text_secondary(), theme::text(), h)).when(h > 0.5, |d| d.underline()),
        key,
        cx,
    )
}

/// A cover drawn from cross-fade layers: the previous image under the new one
/// fading in (see `motion::Crossfade`), or a placeholder before any is ready.
pub fn cover_layers(prev: Option<gpui::ImageSource>, now: Option<(gpui::ImageSource, f32)>, size: f32) -> AnyElement {
    let layer = |src: gpui::ImageSource, a: f32| {
        img(src).absolute().top(sc(0.)).left(sc(0.)).size(sc(size)).rounded(sc(4.)).object_fit(ObjectFit::Cover).opacity(a)
    };
    div()
        .relative()
        .size(sc(size))
        .flex_none()
        .rounded(sc(4.))
        .when(prev.is_none() && now.is_none(), |d| d.bg(theme::surface()))
        .children(prev.map(|p| layer(p, 1.0)))
        .children(now.map(|(n, a)| layer(n, a)))
        .into_any_element()
}

/// Three bouncing bars shown next to the playing track.
pub fn eq_bars(playing: bool, color: Rgba) -> impl IntoElement {
    div().h(sc(14.)).flex().items_end().gap(sc(2.)).children((0..3usize).map(move |i| {
        let bar = div().w(sc(3.)).rounded(sc(1.)).bg(color);
        if playing {
            super::motion::looping(bar, std::time::Duration::from_millis(700 + i as u64 * 170), "eq-bars", move |b, t| {
                let phase = t * std::f32::consts::TAU + i as f32 * 1.9;
                b.h(sc(4.0 + 10.0 * (0.5 + 0.5 * phase.sin())))
            })
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
        .map(|u| match images::sized(u, half, half, cx) {
            Some(i) => img(i).size(sc(half)).flex_none().object_fit(ObjectFit::Cover).into_any_element(),
            None => div().size(sc(half)).flex_none().bg(theme::surface()).into_any_element(),
        })
        .collect();
    div().size(sc(size)).flex_none().rounded(sc(4.)).overflow_hidden().flex().flex_wrap().children(cells).into_any_element()
}

pub fn icon_button(
    id: impl Into<ElementId>,
    icon: Icon,
    size: f32,
    color: Rgba,
    on_click: impl Fn(&mut AppView, &ClickEvent, &mut Window, &mut Context<AppView>) + 'static,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let id: ElementId = id.into();
    let key = format!("ib-{id}");
    let (h, p) = (interact::hover(&key), interact::press(&key));
    // Hover: a soft disc fades in and the glyph grows; press: it sinks.
    let el = div()
        .id(id)
        .flex_none()
        .p(sc(6.))
        .rounded(sc(999.))
        .cursor_pointer()
        .bg(theme::ink(0.055 * h + 0.045 * p))
        .child(super::glass::sheen(SharedString::from(format!("glass-{key}")), 999., 0.62))
        .child(Icon::el(icon, size, color).opacity(0.78 + 0.22 * h - 0.2 * p))
        .on_click(cx.listener(move |this, ev, window, cx| {
            cx.stop_propagation();
            on_click(this, ev, window, cx)
        }));
    interact::track(el, key, cx)
}

pub const PANEL_GAP: f32 = 12.0;
/// Inner padding of every rounded panel (content, dialogs, info), so text and
/// artwork start at the same distance from the edge everywhere.
pub const PANEL_PAD: f32 = 20.0;
/// The same padding on narrow panels.
pub const PANEL_PAD_SM: f32 = 14.0;
/// Space between a panel's heading and its content.
pub const PANEL_HEAD_GAP: f32 = 14.0;

/// The matte finish shared by menus, search and dialogs.
pub fn floating_panel(radius: f32) -> gpui::Div {
    div()
        .relative()
        .rounded(sc(radius))
        .bg(theme::popover())
        .border_1()
        .border_color(theme::popover_border())
        .shadow(theme::popover_shadow())
}

pub fn panel_heading(icon: Icon, title: impl Into<SharedString>, detail: Option<SharedString>) -> gpui::Div {
    div()
        .flex_1()
        .min_w(sc(0.))
        .flex()
        .items_center()
        .gap(sc(10.))
        .child(
            div()
                .size(sc(32.))
                .flex_none()
                .rounded(sc(10.))
                .bg(theme::ink(0.06))
                .flex()
                .items_center()
                .justify_center()
                .child(icon.el(16., theme::text_secondary())),
        )
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .flex()
                .flex_col()
                .gap(sc(3.))
                .child(div().text_size(sc(theme::TYPE_SECTION)).font_weight(FontWeight::BOLD).truncate().child(title.into()))
                .when_some(detail, |d, detail| {
                    d.child(div().text_size(sc(11.5)).text_color(theme::text_secondary()).truncate().child(detail))
                }),
        )
}

/// Inner padding of a panel `width` px wide.
pub fn panel_pad(width: f32) -> f32 {
    if width < 520. {
        PANEL_PAD_SM
    } else {
        PANEL_PAD
    }
}

/// Shared surface for dashboard, feed and settings widgets.
pub fn widget_panel(id: impl Into<ElementId>) -> gpui::Div {
    div()
        .relative()
        .min_w(sc(0.))
        .rounded(sc(18.))
        .bg(theme::liquid_glass(0.0))
        .border_1()
        .border_color(theme::glass_rim(0.0))
        .shadow(theme::glass_shadow())
        .child(super::glass::sheen(id, 17., 0.78))
}

pub fn pill(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: Option<Icon>,
    primary: bool,
    on_click: impl Fn(&mut AppView, &ClickEvent, &mut Window, &mut Context<AppView>) + 'static,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let fg = if primary { theme::button_text() } else { theme::text() };
    let id: ElementId = id.into();
    let key = format!("pill-{id}");
    let (h, p) = (interact::hover(&key), interact::press(&key));
    let el = div()
        .id(id)
        .flex()
        .items_center()
        .gap(sc(8.))
        .h(sc(36.))
        .px(sc(18.))
        .rounded(sc(18.))
        // Hover: lighter and lifted (shadow); press: sinks a little and dims.
        .bg(if primary { interact::mix(theme::button(), theme::button_hover(), h).into() } else { theme::liquid_glass(h) })
        .when(!primary, |d| d.border_1().border_color(theme::glass_rim(h)))
        .opacity(1.0 - 0.18 * p)
        .shadow(vec![gpui::BoxShadow {
            color: gpui::Hsla::from(super::fx::with_alpha(gpui::rgb(0x000000), 0.32 * h * (1.0 - p))),
            offset: gpui::point(sc(0.), sc(4. * h)),
            blur_radius: sc(14.),
            spread_radius: sc(-2.),
        }])
        .text_color(fg)
        .text_size(sc(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .child(super::glass::sheen(SharedString::from(format!("glass-{key}")), 17., if primary { 0.58 } else { 1.15 }))
        .when_some(icon, |d, i| d.child(i.el(18., fg)))
        .child(label.into())
        .on_click(cx.listener(on_click));
    interact::track(el, key, cx)
}

/// Readable name of a TIDAL quality tag.
pub fn quality_name(tag: &str) -> String {
    match tag {
        "HIRES_LOSSLESS" => crate::i18n::t("max_hi_res_lossless").into(),
        "LOSSLESS" => crate::i18n::t("high_lossless").into(),
        "DOLBY_ATMOS" => crate::i18n::t("dolby_atmos").into(),
        "MQA" => crate::i18n::t("mqa").into(),
        "SONY_360RA" => crate::i18n::t("360_reality_audio").into(),
        other => other.to_string(),
    }
}

pub fn quality_badge(tags: &[String]) -> Option<AnyElement> {
    let (label, fg, bg) = if tags.iter().any(|t| t == "HIRES_LOSSLESS") {
        (crate::i18n::t("quality.max_badge"), theme::gold(), theme::gold_bg())
    } else if tags.iter().any(|t| t == "DOLBY_ATMOS") {
        (crate::i18n::t("atmos"), theme::text(), theme::active())
    } else if tags.iter().any(|t| t == "LOSSLESS") {
        (crate::i18n::t("quality.high_badge"), theme::high(), theme::high_bg())
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
        .child(crate::i18n::t("e"))
        .into_any_element()
}

/// Title of a card item.
pub fn item_title(item: &Item) -> String {
    item_text(item).0
}

pub(super) fn item_text(item: &Item) -> (String, String, Option<String>, bool) {
    match item {
        Item::Album(a) => (a.title.clone(), a.artist_names(), a.cover(320), false),
        Item::Playlist(p) => (
            p.title.clone(),
            if p.creator.is_empty() {
                crate::i18n::text("tracks.count", &[format!("{}", p.number_of_tracks)])
            } else {
                crate::i18n::text("by", &[p.creator.to_string()])
            },
            p.cover(320),
            false,
        ),
        Item::Artist(a) => (a.name.clone(), crate::i18n::t("artist").into(), a.picture(320), true),
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
    let key = format!("card-{group}");
    let (h, p) = (interact::hover(&key), interact::press(&key));
    let el = div()
        .id(id)
        .w(sc(width))
        .flex_none()
        .flex()
        .flex_col()
        .gap(sc(7.))
        .cursor_pointer()
        .group(group.clone())
        .on_click(cx.listener(move |this, _, _, cx| this.open_item(&open, cx)))
        .on_mouse_down(MouseButton::Right, {
            let target = match item {
                Item::Album(a) => Some(MenuTarget::Album(a.clone())),
                Item::Artist(a) => Some(MenuTarget::Artist(a.clone())),
                Item::Track(t) => Some(MenuTarget::Track(t.clone())),
                _ => None,
            };
            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                if let Some(t) = target.clone() {
                    this.menu = Some((ev.position, t));
                    cx.notify();
                }
            })
        })
        .child(
            div()
                .relative()
                .size(sc(width))
                .rounded(sc(if round { width / 2. } else { 4. }))
                // Hover: a soft shadow grows under the cover (paint only, so it glides).
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::Hsla::from(super::fx::with_alpha(gpui::rgb(0x000000), 0.26 * h)),
                    offset: gpui::point(sc(0.), sc(5. * h)),
                    blur_radius: sc(18.),
                    spread_radius: sc(-4.),
                }])
                .opacity(1.0 - 0.08 * p)
                .child(cover(url, width, round, cx))
                // Official card hover: gradient from clear to 70 % black, faded in.
                .child(div().absolute().top(sc(0.)).left(sc(0.)).size_full().rounded(sc(if round { width / 2. } else { 4. })).bg(
                    gpui::linear_gradient(
                        180.,
                        gpui::linear_color_stop(gpui::rgba(0x00000000), 0.),
                        gpui::linear_color_stop(super::fx::with_alpha(gpui::rgb(0x000000), 0.55 * h), 1.),
                    ),
                ))
                // The play button rises and fades in.
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
                        .opacity(h)
                        .when(h < 0.02, |d| d.invisible())
                        .shadow_lg()
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
                        .text_size(sc(theme::TYPE_LABEL))
                        .font_weight(FontWeight::BOLD)
                        .child(div().min_w(sc(0.)).one_line().group_hover(group.clone(), |s| s.underline()).child(title))
                        .when(explicit, |d| d.child(explicit_badge())),
                )
                .child(div().one_line().text_color(theme::text_secondary()).text_size(sc(11.5)).child(subtitle))
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
        );
    interact::track(el, key, cx).into_any_element()
}

#[derive(Clone, Copy)]
pub struct RowStyle {
    pub cover: bool,
    pub album: bool,
    pub number: bool,
    pub date: bool,
    pub bpm: bool,
    pub rating: bool,
    pub compact: bool,
    pub tiny: bool,
    /// Collection section whose sort the column headers control.
    pub sortable: Option<&'static str>,
}

impl RowStyle {
    pub const fn new(cover: bool, album: bool, number: bool) -> Self {
        Self { cover, album, number, date: false, bpm: false, rating: true, compact: false, tiny: false, sortable: None }
    }

    /// Drop optional columns (BPM, then date added, then rating) when the list
    /// is `width` logical px wide, so the title always keeps readable room
    /// (narrow windows, large zoom).
    pub fn fit(mut self, width: f32) -> Self {
        if width < 1110.0 {
            self.bpm = false;
        }
        if width < 1030.0 {
            self.date = false;
        }
        if width < 840.0 {
            self.rating = false;
        }
        if width < 700.0 {
            self.album = false;
        }
        self.compact = width < 510.0;
        self.tiny = width < 360.0;
        self
    }

    pub fn column_gap(self) -> f32 {
        if self.compact {
            8.0
        } else {
            12.0
        }
    }

    pub fn actions_width(self) -> f32 {
        if self.tiny {
            28.0
        } else {
            84.0
        }
    }
}

/// "2026-07-19T12:00:00" → "19 jul 2026".
pub fn fmt_date(iso: &str) -> String {
    let months: [&str; 12] = [
        crate::i18n::t("jan"),
        crate::i18n::t("feb"),
        crate::i18n::t("mar"),
        crate::i18n::t("apr"),
        crate::i18n::t("may"),
        crate::i18n::t("jun"),
        crate::i18n::t("jul"),
        crate::i18n::t("aug"),
        crate::i18n::t("sep"),
        crate::i18n::t("oct"),
        crate::i18n::t("nov"),
        crate::i18n::t("dec"),
    ];
    let mut it = iso.get(..10).unwrap_or("").split('-');
    let (Some(y), Some(m), Some(d)) = (it.next(), it.next(), it.next()) else { return String::new() };
    let (Ok(m), Ok(d)) = (m.parse::<usize>(), d.parse::<u32>()) else { return String::new() };
    crate::i18n::text("date.full", &[d.to_string(), months.get(m.wrapping_sub(1)).copied().unwrap_or("").into(), y.into()])
}

/// Column widths shared by the header and the rows so they line up.
pub struct Cols;

impl Cols {
    pub const PAD: f32 = 12.0;
    pub const COVER: f32 = 36.0;
    pub const COVER_GAP: f32 = 10.0;
    pub const NUM: f32 = 28.0;
    pub const DATE: f32 = 94.0;
    pub const TIME: f32 = 50.0;
    pub const BPM: f32 = 36.0;
    pub const RATING: f32 = 84.0;
    pub fn artist(style: RowStyle) -> f32 {
        if style.album {
            0.18
        } else {
            0.26
        }
    }
    pub const ALBUM: f32 = 0.18;
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
    let menu_target = match own_playlist {
        Some(uuid) => MenuTarget::PlaylistTrack { uuid, index: ix, track: track.clone() },
        None => MenuTarget::Track(track.clone()),
    };
    let more_target = menu_target.clone();
    let playing = view.now.as_ref().map(|n| n.track.id) == Some(track.id);
    let fav = view.fav_ids.contains(&track.id);
    let local = track.local_path.is_some();
    let group: SharedString = format!("track-{ix}").into();
    let row_key = format!("row-{ix}");
    let (h, p) = (interact::hover(&row_key), interact::press(&row_key));
    let title_color = if playing { theme::accent() } else { theme::text() };
    let t_fav = track.clone();
    let t_add = track.clone();
    let album_id = track.album.as_ref().map(|a| a.id).filter(|id| *id != 0);
    let artist_id = track.artists.first().map(|a| a.id).filter(|id| *id != 0);
    let on_play = std::rc::Rc::new(on_play);
    let p1 = on_play.clone();
    let p2 = on_play.clone();
    let audible = !view.snap.paused && !view.ended;

    let link = |id: (&'static str, usize), text: String, rel: Option<f32>, target: Option<Page>, cx: &mut Context<AppView>| {
        let el = div()
            .id(id)
            .min_w(sc(0.))
            .one_line()
            .text_size(sc(11.5))
            .when_some(rel, |d, rel| d.w(relative(rel)).flex_none().font_weight(FontWeight::SEMIBOLD));
        let el = match target.is_some() {
            true => nav_link(el, format!("lnk-{}-{}", id.0, id.1), cx),
            false => el,
        };
        el.child(text).on_click(cx.listener(move |this, _, _, cx| {
            if let Some(t) = target.clone() {
                cx.stop_propagation();
                this.navigate(t, cx);
            }
        }))
    };

    let action_box = |id, tip: &'static str| {
        div()
            .id(id)
            .size(sc(26.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(sc(6.))
            .cursor_pointer()
            .hover(|s| s.bg(theme::ink(0.12)))
            .active(|s| s.opacity(0.65))
            .tooltip(super::sidebar::tip(tip))
    };
    let row = div()
        .id(("track", ix))
        .h(sc(if style.compact { theme::ROW_H + 6. } else { theme::ROW_H }))
        .my(sc(2.))
        .px(sc(Cols::PAD))
        .flex()
        .items_center()
        .gap(sc(style.column_gap()))
        .rounded(sc(7.))
        .border_1()
        .border_color(if playing { super::fx::with_alpha(theme::accent(), 0.35) } else { theme::divider() })
        .relative()
        .group(group.clone())
        // Hover fades the row in; pressing deepens it.
        .bg(if playing { theme::accent_dim() } else { super::fx::with_alpha(theme::hover(), h * 0.8 + 0.1 * p) })
        .when(p > 0.0, |d| d.bg(interact::mix(super::fx::with_alpha(theme::hover(), h), theme::active(), p)))
        // An accent strip grows from the middle of the left edge.
        .child(
            div()
                .absolute()
                .left(sc(0.))
                .top(sc(9.))
                .bottom(sc(9.))
                .w(sc(2.))
                .rounded(sc(1.))
                .bg(super::fx::with_alpha(theme::accent(), 0.85 * h)),
        )
        .when(!track.streamable, |d| d.opacity(0.4))
        // Reorder by dragging (own playlists only).
        .when_some(drag, |d, dt| d.on_drag(dt, |dt, _, _, cx| cx.new(|_| dt.clone())))
        .when_some(drop_uuid, |d, uuid| {
            d.drag_over::<super::playlists::DraggedTrack>(|s, _, _, _| s.border_t_2().border_color(theme::accent())).on_drop(cx.listener(
                move |this, dt: &super::playlists::DraggedTrack, _, cx| {
                    if dt.uuid == uuid {
                        this.move_playlist_track(uuid.clone(), dt.index, ix, cx);
                    }
                },
            ))
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
            })
        })
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                this.menu = Some((ev.position, menu_target.clone()));
                cx.notify();
            }),
        )
        // Number, replaced by a play button on hover.
        .when(!style.tiny, |d| {
            d.child(
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
        })
        // Cover + title.
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .flex()
                .items_center()
                .gap(sc(Cols::COVER_GAP))
                .when(style.cover, |d| {
                    d.child(
                        div()
                            .flex_none()
                            .rounded(sc(4.))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::Hsla::from(super::fx::with_alpha(gpui::rgb(0x000000), 0.4 * h)),
                                offset: gpui::point(sc(0.), sc(2. * h)),
                                blur_radius: sc(8.),
                                spread_radius: sc(0.),
                            }])
                            .child(cover(track.cover(80), Cols::COVER, false, cx)),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w(sc(0.))
                        .flex()
                        .flex_col()
                        .gap(sc(2.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .min_w(sc(0.))
                                .gap(sc(6.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(sc(0.))
                                        .one_line()
                                        .text_size(sc(13.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(title_color)
                                        .child(track.display_title()),
                                )
                                .when(track.explicit, |d| d.child(explicit_badge())),
                        )
                        .when(style.compact, |d| {
                            d.child(link(("artist", ix), track.artist_names(), None, artist_id.map(Page::Artist), cx))
                        }),
                ),
        )
        .when(!style.compact, |d| {
            d.child(link(("artist", ix), track.artist_names(), Some(Cols::artist(style)), artist_id.map(Page::Artist), cx))
        })
        .when(style.album, |d| {
            d.child(link(
                ("album", ix),
                track.album.as_ref().map(|a| a.title.clone()).unwrap_or_default(),
                Some(Cols::ALBUM),
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
        .when(!style.tiny, |d| {
            d.child(
                div()
                    .w(sc(Cols::TIME))
                    .flex_none()
                    .text_size(sc(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(fmt_time(track.duration as f64)),
            )
        })
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
        .child(
            div()
                .w(sc(style.actions_width()))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .gap(sc(2.))
                .when(!style.tiny && !local, |d| {
                    d.child(
                        action_box(("add", ix), crate::i18n::t("add_to_playlist"))
                            .child(Icon::Plus.el(15., theme::text_secondary()))
                            .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.menu = Some((ev.position(), MenuTarget::AddToPlaylist(vec![t_add.clone()])));
                                cx.notify();
                            })),
                    )
                    .child(
                        action_box(
                            ("fav", ix),
                            if fav { crate::i18n::t("remove_from_favorites") } else { crate::i18n::t("add_to_favorites") },
                        )
                        .child(if fav {
                            Icon::HeartFilled.el_scaled(15., theme::text(), view.heart_scale(track.id))
                        } else {
                            Icon::Heart.el(15., theme::text_secondary())
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_favorite(&t_fav, cx)
                        })),
                    )
                })
                .child(
                    action_box(("more", ix), crate::i18n::t("more_options"))
                        .when(!style.tiny && !local, |d| d.invisible().group_hover(group, |s| s.visible()))
                        .child(Icon::More.el(15., theme::text_secondary()))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.menu = Some((ev.position, more_target.clone()));
                                cx.notify();
                            }),
                        ),
                ),
        );
    interact::track(row, row_key, cx).into_any_element()
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
    let id: ElementId = id.into();
    let key = format!("act-{id}");
    let (h, p) = (interact::hover(&key), interact::press(&key));
    let el = div()
        .id(id)
        .flex_none()
        .w(sc(56.))
        .py(sc(4.))
        .flex()
        .flex_col()
        .items_center()
        .gap(sc(5.))
        .cursor_pointer()
        .rounded(sc(8.))
        .bg(theme::ink(0.05 * h))
        .opacity(1.0 - 0.22 * p)
        .child(icon.el(18., if active { theme::accent() } else { theme::text() }))
        .child(
            div()
                .text_size(sc(10.))
                .font_weight(FontWeight::BOLD)
                .text_color(if active { theme::accent() } else { theme::text() })
                .child(label.into()),
        )
        .on_click(cx.listener(on_click));
    interact::track(el, key, cx).into_any_element()
}

pub fn section_title(title: impl Into<SharedString>) -> AnyElement {
    div().pt(sc(22.)).pb(sc(12.)).text_size(sc(18.)).font_weight(FontWeight::BOLD).child(title.into()).into_any_element()
}

pub fn message(text: impl Into<SharedString>) -> AnyElement {
    div().py(sc(48.)).text_color(theme::text_secondary()).child(text.into()).into_any_element()
}

/// A lyrics area with the floating "Sincronizar" button anchored to its
/// bottom edge. The button is not part of the layout (the lines never move or
/// get cut by it); it fades in while the user has scrolled away from the
/// current line and brings the view back to it, gliding, when pressed.
/// `grow`: the area fills a flex column (side panel) instead of its parent box.
pub fn with_sync_button(content: AnyElement, view: &mut AppView, key: &'static str, grow: bool, cx: &mut Context<AppView>) -> AnyElement {
    let follow = view.lyrics_follow;
    let vis_key = format!("sync-vis-{key}");
    // While syncing the button stays (its icon turning) until the lyrics are back.
    let (syncing, spin_at) = (view.lyrics_sync_at.is_some(), view.lyrics_spin_at);
    let visible = !follow || syncing;
    let shown = interact::follow(&vis_key, if visible { 1.0 } else { 0.0 }, super::motion::seconds(super::motion::CONTROL));
    if shown <= 0.01 && !syncing && spin_at.is_some() {
        view.lyrics_spin_at = None;
    }
    // Spin: eases in over 0.15 s, then a steady turn every 0.7 s.
    let angle = spin_at.map(|at| {
        if super::motion::reduced_motion() {
            return 0.0;
        }
        let t = at.elapsed().as_secs_f32();
        let speed = std::f32::consts::TAU / 0.7;
        let ramp = 0.15;
        if t < ramp {
            speed * t * t / (2.0 * ramp)
        } else {
            speed * (t - ramp / 2.0)
        }
    });
    if !super::motion::reduced_motion() && angle.is_some() && shown > 0.01 {
        super::motion::want_frame("lyrics-sync");
    }
    let area = div().relative().flex().flex_col();
    let area = if grow { area.flex_1().min_h(sc(0.)) } else { area.size_full() };
    area.child(content)
        .when(shown > 0.01, |d| {
            let hk = format!("sync-btn-{key}");
            let (h, p) = (interact::hover(&hk), interact::press(&hk));
            let button = div()
                .id(SharedString::from(hk.clone()))
                .h(sc(34.))
                .pl(sc(12.))
                .pr(sc(15.))
                .flex()
                .items_center()
                .gap(sc(7.))
                .rounded(sc(17.))
                .cursor_pointer()
                .bg(theme::glass(h))
                .border_1()
                .border_color(theme::ink(0.08 + 0.06 * h))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::Hsla::from(gpui::rgba(0x00000070)),
                    offset: gpui::point(sc(0.), sc(6.)),
                    blur_radius: sc(22.),
                    spread_radius: sc(-4.),
                }])
                .opacity(shown * (1.0 - 0.15 * p))
                .text_size(sc(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child(match angle {
                    Some(a) => super::icons::sync_turned(15., theme::text(), a),
                    None => Icon::Sync.el(15., theme::text()),
                })
                .child(crate::i18n::t("sync"))
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    if this.lyrics_sync_at.is_some() {
                        return;
                    }
                    let now = std::time::Instant::now();
                    this.lyrics_follow = true;
                    this.lyrics_scroll_at = now;
                    this.lyrics_sync_at = Some(now);
                    this.lyrics_spin_at = Some(now);
                    cx.notify();
                }));
            d.child(
                div().absolute().left(sc(0.)).right(sc(0.)).bottom(sc(26.)).flex().justify_center().child(interact::track(button, hk, cx)),
            )
        })
        .into_any_element()
}

/// Loading indicator, centred in `height` logical px: three dots that breathe
/// one after another and a quiet label. It waits 120 ms before fading in, so
/// quick loads never flash it; frames are asked for only while it is shown.
pub fn loading(height: f32) -> AnyElement {
    use std::cell::Cell;
    thread_local! {
        // When the indicator started being shown, and the last time it was drawn.
        static SHOWN: Cell<Option<(std::time::Instant, std::time::Instant)>> = const { Cell::new(None) };
    }
    let now = std::time::Instant::now();
    let since = SHOWN.with(|s| {
        let start = match s.get() {
            Some((start, last)) if now.duration_since(last).as_millis() < 250 => start,
            _ => now,
        };
        s.set(Some((start, now)));
        start
    });
    let t = ((now.duration_since(since).as_secs_f32() - 0.12) / 0.25).clamp(0.0, 1.0);
    let appear = if super::motion::reduced_motion() { 1.0 } else { super::motion::ease_out(t) };
    let row = super::motion::looping(div().flex().gap(sc(9.)), std::time::Duration::from_millis(1200), "loading", move |d, phase| {
        d.children((0..3).map(move |i| {
            // Each dot rises and fades in turn (a soft wave, no hard blinking).
            let p = (phase - i as f32 * 0.16).rem_euclid(1.0);
            let wave = (p * std::f32::consts::TAU).sin().max(0.0);
            div().size(sc(7.)).rounded(sc(3.5)).bg(theme::text()).opacity(0.22 + 0.68 * wave)
        }))
    });
    div()
        .w_full()
        .h(sc(height.max(160.0)))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(sc(14.))
        .opacity(appear)
        .child(row)
        .child(div().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_dim()).child(crate::i18n::t("loading.label")))
        .into_any_element()
}

/// A failed load: the reason and a button that loads the page again.
pub fn error_message(text: impl Into<SharedString>, cx: &mut Context<AppView>) -> AnyElement {
    div()
        .py(sc(48.))
        .flex()
        .flex_col()
        .items_start()
        .gap(sc(14.))
        .child(div().text_color(theme::text_secondary()).child(text.into()))
        .child(pill(
            "retry-load",
            crate::i18n::t("retry"),
            None,
            false,
            |this, _, _, cx| {
                this.reset_data();
                this.load_page_data(cx);
                cx.notify();
            },
            cx,
        ))
        .into_any_element()
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
                .child(icon.el_scaled(size, color, pop))
                .child(hit(half, true, cx))
                .child(hit(full, false, cx))
        }))
        .into_any_element()
}

/// One of the quick links at the top of "For you": a small cover with the name beside it.
pub fn shortcut(id: (&'static str, usize), item: &Item, width: f32, cx: &mut Context<AppView>) -> AnyElement {
    let (title, subtitle, url, _) = item_text(item);
    let open = item.clone();
    let sk = format!("sc-{}-{}", id.0, id.1);
    let (sh, sp) = (interact::hover(&sk), interact::press(&sk));
    let el = div()
        .id(id)
        .group(SharedString::from(format!("sc-{}-{}", id.0, id.1)))
        .w(sc(width))
        .h(sc(48.))
        .flex_none()
        .flex()
        .items_center()
        .gap(sc(10.))
        .pr(sc(12.))
        .rounded(sc(16.))
        .overflow_hidden()
        .relative()
        .border_1()
        .border_color(theme::glass_rim(sh))
        .cursor_pointer()
        .bg(theme::liquid_glass(sh))
        .child(super::glass::sheen(SharedString::from(format!("glass-{sk}")), 16., 0.9))
        .shadow(vec![gpui::BoxShadow {
            color: gpui::Hsla::from(super::fx::with_alpha(gpui::rgb(0x000000), 0.3 * sh)),
            offset: gpui::point(sc(0.), sc(4. * sh)),
            blur_radius: sc(16.),
            spread_radius: sc(-2.),
        }])
        .opacity(1.0 - 0.15 * sp)
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
                    gpui::linear_color_stop(theme::accent(), 0.),
                    gpui::linear_color_stop(interact::mix(theme::accent(), gpui::rgb(0x000000), 0.45), 1.),
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
                .child(div().one_line().text_size(sc(11.)).font_weight(FontWeight::BOLD).child(title))
                .when(!subtitle.is_empty(), |d| d.child(div().one_line().text_size(sc(10.)).text_color(theme::text_dim()).child(subtitle))),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.open_item(&open, cx)));
    interact::track(el, sk, cx).into_any_element()
}

/// The grayscale Abyss mark, with identical presentation on every surface.
pub fn brand_mark(size: f32) -> AnyElement {
    div()
        .size(sc(size))
        .flex_none()
        .bg(gpui::rgb(0xffffff))
        .rounded(sc(size * 0.22))
        .child(gpui::img(super::images::logo()).size_full())
        .into_any_element()
}
