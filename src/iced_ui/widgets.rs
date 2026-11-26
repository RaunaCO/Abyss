//! Reusable pieces: covers, buttons, badges, track rows, cards, loading dots.

use std::time::Instant;

use iced::widget::{button, column, container, image, mouse_area, row, text, Space};
use iced::{alignment, Alignment, Background, Border, Color, Element, Length, Shadow, Vector};

use super::icons::{icon, Icon};
use super::{font, images, theme, App, MenuTarget, Message};
use crate::tidal::Item;

pub const SEARCH_ID: &str = "abyss-search";

pub fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn fmt_long(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        crate::i18n::text("h_min", &[format!("{h}"), format!("{m}")])
    } else {
        crate::i18n::text("min", &[format!("{m}")])
    }
}

/// "2026-11-22..." as a full date in the interface's language.
pub fn fmt_date(iso: &str) -> String {
    let months = crate::shared::replay::months();
    let mut it = iso.get(..10).unwrap_or("").split('-');
    let (Some(y), Some(m), Some(d)) = (it.next(), it.next(), it.next()) else { return String::new() };
    let (Ok(m), Ok(d)) = (m.parse::<usize>(), d.parse::<u32>()) else { return String::new() };
    crate::i18n::text("date.full", &[d.to_string(), months.get(m.wrapping_sub(1)).copied().unwrap_or("").into(), y.into()])
}

pub fn semibold() -> iced::Font {
    font(iced::font::Weight::Semibold)
}
pub fn bold() -> iced::Font {
    font(iced::font::Weight::Bold)
}

/// One line of text, never wrapped (the row around it clips what does not fit).
/// One line of text that ends in "…" when it does not fit (GPUI's `truncate()`).
pub fn line<'a>(s: impl Into<std::borrow::Cow<'a, str>>, size: f32, color: Color) -> super::ellipsis::Line<'a> {
    super::ellipsis::line(s, size, color)
}

pub fn text_link<'a>(label: super::ellipsis::Line<'a>, msg: Message) -> Element<'a, Message> {
    mouse_area(super::wash::track(label.brighten_on_hover(theme::text())))
        .on_press(msg)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

/// Square (or round) cover art with a placeholder while loading; it fades
/// in the first time it appears.
pub fn cover<'a>(url: Option<String>, size: f32, round: bool) -> Element<'a, Message> {
    cover_at_opacity(url, size, round, 1.0)
}

pub fn cover_at_opacity<'a>(url: Option<String>, size: f32, round: bool, opacity: f32) -> Element<'a, Message> {
    let radius = if round { images::snap(size) / 2.0 } else { 4.0 };
    cover_with(url, size, radius, opacity)
}

/// A cover with `radius` corners (GPUI clips the catalog header's to 12 px).
pub fn cover_rounded<'a>(url: Option<String>, size: f32, radius: f32) -> Element<'a, Message> {
    cover_with(url, size, radius, 1.0)
}

fn cover_with<'a>(url: Option<String>, size: f32, radius: f32, opacity: f32) -> Element<'a, Message> {
    let size = images::snap(size);
    let handle = url.as_deref().and_then(|u| images::sized(u, size, size));
    match handle {
        Some(h) => {
            let a = opacity * url.as_deref().map_or(1.0, images::fade_in);
            container(image(h).width(size).height(size).content_fit(iced::ContentFit::Cover).border_radius(radius).opacity(a))
                .width(size)
                .height(size)
                .style(move |_| container::Style {
                    border: Border { color: theme::ink(0.09), width: 1.0, radius: radius.into() },
                    ..Default::default()
                })
                .into()
        }
        // The placeholder: a note big enough to read even in a 34 px tile, on
        // an ink wash so the tile shows on the page, the rail and glass alike.
        None => container(icon(Icon::Track, (size * 0.3).max(14.0).min(size * 0.5), theme::with_alpha(theme::text_dim(), opacity)))
            .width(size)
            .height(size)
            .center(size)
            .style(move |_| container::Style {
                background: Some(theme::ink(0.07 * opacity).into()),
                border: Border { color: theme::ink(0.09), width: 1.0, radius: radius.into() },
                ..Default::default()
            })
            .into(),
    }
}

pub fn playing_indicator<'a>(active: bool, size: f32) -> Element<'a, Message> {
    let moving = active && !crate::shared::lyrics::reduced_motion();
    iced::widget::canvas(PlayingBars { moving, color: theme::accent() }).width(size).height(size).into()
}

struct PlayingBars {
    moving: bool,
    color: Color,
}

impl iced::widget::canvas::Program<Message> for PlayingBars {
    type State = Option<Instant>;

    fn update(
        &self,
        phase: &mut Self::State,
        event: &iced::Event,
        _: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Option<iced::widget::Action<Message>> {
        if self.moving {
            if let iced::Event::Window(iced::window::Event::RedrawRequested(at)) = event {
                phase.get_or_insert(*at);
                return Some(iced::widget::Action::request_redraw_at(*at + super::motion::ambient_interval()));
            }
        }
        None
    }

    fn draw(
        &self,
        phase: &Self::State,
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        let time = phase.filter(|_| self.moving).map(|at| at.elapsed().as_secs_f32());
        let mut frame = iced::widget::canvas::Frame::new(renderer, bounds.size());
        for i in 0..3 {
            let bar = time.map_or([0.44, 0.84, 0.60][i], |seconds| {
                let phase = seconds * (7.0 + i as f32) + i as f32 * 1.9;
                0.32 + (phase.sin() * 0.5 + 0.5) * 0.64
            });
            let start = iced::Point::new(bounds.width * (0.1 + i as f32 * 0.30), bounds.height * (1.0 - bar));
            frame.fill_rectangle(start, iced::Size::new(bounds.width * 0.18, bounds.height * bar), self.color);
        }
        vec![frame.into_geometry()]
    }
}

/// A round icon button (GPUI `widgets::icon_button`): the glyph with 6 px
/// around it; a soft disc fades in under the pointer (ink 5.5 %, plus 4.5 %
/// pressed), the glyph brightens from 78 % and dims while held; a new glyph
/// (heart, mute...) dissolves in.
pub fn icon_button<'a>(i: Icon, size: f32, color: Color, msg: Message) -> Element<'a, Message> {
    glyph_button(soft_glyph(i, size, color), size, msg)
}

/// An icon in a square box `side` px wide with 6 px corners (GPUI's row
/// `action_box`, 26 px around a 15 px glyph), with the fading wash and the
/// glyph of [`icon_button`].
pub fn icon_box<'a>(i: Icon, size: f32, side: f32, color: Color, msg: Message) -> Element<'a, Message> {
    glyph_box(soft_glyph(i, size, color), side, msg)
}

/// [`icon_box`] around any glyph (the row's heart, which pops).
pub fn glyph_box<'a>(g: super::icons::Glyph<'a>, side: f32, msg: Message) -> Element<'a, Message> {
    fading(button(container(g).center(side)).padding(0).style(still(6.0)).on_press(msg), 6.0)
}

/// The glyph of [`icon_button`]: GPUI's `color.a * (0.78 + 0.22 h - 0.2 p)`.
pub fn soft_glyph<'a>(i: Icon, size: f32, color: Color) -> super::icons::Glyph<'a> {
    super::icons::glyph(i, size, color).swap().live(|p, c| Color { a: c.a * (0.78 + 0.22 * p.hover - 0.2 * p.press), ..c })
}

/// A round button around any glyph, `size` the glyph's side: 6 px around it
/// (GPUI's `icon_button` metrics).
pub fn glyph_button<'a>(g: super::icons::Glyph<'a>, size: f32, msg: Message) -> Element<'a, Message> {
    let side = size + 12.0;
    super::wash::wash(button(container(g).center(side)).padding(0).style(still(side / 2.0)).on_press(msg), theme::ink(0.055), side / 2.0)
        .press(theme::ink(0.045))
        .into()
}

/// A transport control (GPUI `haptics::button`): the disc fades in, the glyph
/// brightens from 80 % and sinks 7 % under the finger, easing back on release.
pub fn transport_button<'a>(i: Icon, size: f32, color: Color, msg: Message) -> Element<'a, Message> {
    let g = super::icons::glyph(i, size, color)
        .swap()
        .live(|p, c| Color { a: c.a * (0.8 + 0.2 * p.hover - 0.1 * p.press), ..c })
        .scale(|p| 1.0 - 0.07 * p.press);
    glyph_button(g, size, msg)
}

/// A quiet button whose hover and press fade like GPUI's.
pub fn soft<'a>(b: button::Button<'a, Message>, radius: f32) -> Element<'a, Message> {
    fading(b.style(still(radius)), radius)
}

/// A ghost button's look without its instant hover: [`fading`] draws it.
pub fn still(radius: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_, _| button::Style {
        text_color: theme::text(),
        border: Border { radius: radius.into(), ..Default::default() },
        ..Default::default()
    }
}

/// A ghost button's hover and press, fading in like GPUI's.
pub fn fading<'a>(content: impl Into<Element<'a, Message>>, radius: f32) -> Element<'a, Message> {
    super::wash::wash(content, theme::ink(0.12), radius).press(theme::ink(0.06)).into()
}

/// A pill-shaped button: primary (white on dark) or glass.
pub fn pill<'a>(label: impl text::IntoFragment<'a>, i: Option<Icon>, primary: bool, msg: Option<Message>) -> Element<'a, Message> {
    let fg = if primary { theme::button_text() } else { theme::text() };
    let content = row![]
        .push(i.map(|i| icon(i, 18.0, fg)))
        .push(text(label).size(12.5).font(semibold()).color(fg))
        .spacing(8)
        .align_y(Alignment::Center);
    let b = button(container(content).height(36).padding([0, 18]).align_y(alignment::Vertical::Center))
        .padding(0)
        .style(move |_, _| button::Style { text_color: fg, ..Default::default() })
        .on_press_maybe(msg);
    // GPUI `pill`: lighter and lifted on hover (shadow), sinking while held,
    // all eased through `interact` instead of switching with the status.
    super::wash::surface(b, move |super::wash::Phase { hover: h, press: p, .. }| {
        let bg = if primary {
            theme::lerp(theme::lerp(theme::button(), theme::button_hover(), h), theme::button_pressed(), p)
        } else {
            theme::lerp(theme::glass_top(), theme::active_strong(), 0.35 * h + 0.2 * p)
        };
        super::wash::Look {
            background: Some(bg.into()),
            border: Border { color: if primary { Color::TRANSPARENT } else { theme::glass_rim(h) }, width: 1.0, radius: 18.0.into() },
            shadow: Shadow {
                color: Color { a: 0.32 * h * (1.0 - p), ..Color::BLACK },
                offset: Vector::new(0.0, 4.0 * h),
                blur_radius: 14.0,
            },
        }
    })
    .into()
}

/// A page's primary action on prominent glass: a capsule tinted with the
/// cover's colour (`tint` and label colour from `liquid_glass::prominent`),
/// following the pointer. Same metrics as [`pill`]; dimmed while disabled.
pub fn prominent_pill<'a>(
    label: impl text::IntoFragment<'a>,
    i: Option<Icon>,
    (tint, ink): (Color, Color),
    msg: Option<Message>,
) -> Element<'a, Message> {
    let fg = if msg.is_some() { ink } else { Color { a: 0.55, ..ink } };
    let content = row![]
        .push(i.map(|i| icon(i, 18.0, fg)))
        .push(text(label).size(12.5).font(semibold()).color(fg))
        .spacing(8)
        .align_y(Alignment::Center);
    let b = button(container(content).height(36).padding([0, 18]).align_y(alignment::Vertical::Center))
        .padding(0)
        .style(move |_, _| button::Style { text_color: fg, ..Default::default() })
        .on_press_maybe(msg);
    super::liquid_glass::glass(b, super::liquid_glass::CAPSULE).prominent(tint).interactive().shadow(theme::control_shadow()).into()
}

/// A text style in `color` that follows the fade of the floating surface it
/// is drawn in (see [`super::wash::enter`]).
pub fn faded_text(color: Color) -> impl Fn(&iced::Theme) -> text::Style {
    move |_| text::Style { color: Some(super::wash::faded(color)) }
}

/// `content` with a label shown while the pointer rests on it.
pub fn tip<'a>(content: impl Into<Element<'a, Message>>, label: &'a str) -> Element<'a, Message> {
    // After a short rest, so sweeping the pointer along a bar does not flash
    // a bubble on every icon it crosses.
    iced::widget::tooltip(content, tip_bubble(label), iced::widget::tooltip::Position::Bottom)
        .padding(0)
        .delay(std::time::Duration::from_millis(450))
        .into()
}

/// The tooltips' bubble: GPUI's `sidebar::Tip` metrics (12 px text, 10 x 6
/// padding, 7 px corners) on a small pane of glass.
pub fn tip_bubble<'a>(label: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    super::liquid_glass::glass(container(text(label).size(theme::TYPE_LABEL).color(theme::text())).padding([6, 10]), 7.0)
        .optics(super::liquid_glass::Optics::POPOVER)
        .shadow(theme::popover_shadow())
        .into()
}

/// 7728 -> "2:08:48"; 968 -> "16:08".
pub fn fmt_hms(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
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

pub fn quality_badge<'a>(tags: &[String]) -> Option<Element<'a, Message>> {
    let (label, fg, bg) = if tags.iter().any(|t| t == "HIRES_LOSSLESS") {
        (crate::i18n::t("quality.max_badge"), theme::gold(), theme::gold_bg())
    } else if tags.iter().any(|t| t == "DOLBY_ATMOS") {
        (crate::i18n::t("atmos"), theme::text(), theme::active())
    } else if tags.iter().any(|t| t == "LOSSLESS") {
        (crate::i18n::t("quality.high_badge"), theme::high(), theme::high_bg())
    } else {
        return None;
    };
    Some(badge(label, fg, bg))
}

pub fn badge<'a>(label: &'a str, fg: Color, bg: Color) -> Element<'a, Message> {
    container(text(label).size(10).font(bold()).color(fg))
        .padding([1, 5])
        .style(move |_| container::Style {
            background: Some(bg.into()),
            border: Border { radius: 3.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}

pub fn explicit_badge<'a>() -> Element<'a, Message> {
    container(text(crate::i18n::t("e")).size(9).font(bold()).color(theme::bg()))
        .center(14)
        .style(|_| container::Style {
            background: Some(theme::text_dim().into()),
            border: Border { radius: 2.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}

/// Height of [`section_title`]: 22 px above, an 18 px line, 12 px below.
pub const SECTION_H: f32 = 22.0 + 18.0 * 1.3 + 12.0;

/// A section's heading in a page (GPUI's `section_title`: 18 px bold, 22 px above, 12 below).
pub fn section_title<'a>(title: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    container(text(title).size(18).font(bold()).color(theme::text()))
        .padding(iced::Padding { top: 22.0, bottom: 12.0, ..Default::default() })
        .height(SECTION_H)
        .into()
}

/// A quiet notice in a page (GPUI's `message`, centred): body text, 48 px
/// above and below.
pub fn message<'a>(s: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    state_block(None, s, None)
}

/// An empty state: `i` in a soft round tile over a centred note.
pub fn notice<'a>(i: Icon, s: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    state_block(Some(i), s, None)
}

/// The shape every empty and failed state shares: an optional icon tile, the
/// note (centred, never wider than a comfortable line) and an optional action.
fn state_block<'a>(i: Option<Icon>, s: impl text::IntoFragment<'a>, action: Option<Element<'a, Message>>) -> Element<'a, Message> {
    let tile = i.map(|i| {
        container(icon(i, 20.0, theme::text_secondary())).center(48).style(|_| container::Style {
            background: Some(theme::ink(0.06).into()),
            border: Border { color: theme::ink(0.08), width: 1.0, radius: 24.0.into() },
            ..Default::default()
        })
    });
    let note = text(s)
        .size(theme::TYPE_BODY)
        .line_height(text::LineHeight::Absolute(21.0.into()))
        .color(theme::text_secondary())
        .align_x(alignment::Horizontal::Center)
        .width(Length::Fill);
    let body = column![].push(tile).push(container(note).max_width(460)).push(action).spacing(14).align_x(Alignment::Center);
    container(body).width(Length::Fill).padding([48, 16]).center_x(Length::Fill).into()
}

/// Three dots that rise and fall while something loads, centred in the space given.
pub fn loading<'a>() -> Element<'a, Message> {
    container(loading_dots()).width(Length::Fill).height(Length::Fill).center(Length::Fill).into()
}

/// The three dots alone (5 px, a wave every 1.2 s; still with reduced motion).
pub fn loading_dots<'a>() -> Element<'a, Message> {
    iced::widget::canvas(LoadingDots { color: theme::text(), reduced: crate::shared::lyrics::reduced_motion() }).width(25).height(5).into()
}

struct LoadingDots {
    color: Color,
    reduced: bool,
}

impl iced::widget::canvas::Program<Message> for LoadingDots {
    type State = Option<Instant>;

    fn update(
        &self,
        start: &mut Self::State,
        event: &iced::Event,
        _: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Option<iced::widget::Action<Message>> {
        if !self.reduced {
            if let iced::Event::Window(iced::window::Event::RedrawRequested(at)) = event {
                start.get_or_insert(*at);
                return Some(iced::widget::Action::request_redraw_at(*at + std::time::Duration::from_millis(33)));
            }
        }
        None
    }

    fn draw(
        &self,
        start: &Self::State,
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, Path};
        let phase = start.map_or(0.0, |at| (at.elapsed().as_secs_f32() / 1.2).fract());
        let mut frame = Frame::new(renderer, bounds.size());
        for i in 0..3 {
            let offset = (phase - i as f32 * 0.16) * std::f32::consts::TAU;
            let wave = if self.reduced { 0.6 } else { offset.sin().max(0.0) };
            let dot = Path::circle(iced::Point::new(2.5 + i as f32 * 10.0, 2.5), 2.5);
            frame.fill(&dot, theme::with_alpha(self.color, 0.30 + 0.60 * wave));
        }
        vec![frame.into_geometry()]
    }
}

/// A failed load: the reason and a button that loads the page again, in the
/// shape of the empty states.
pub fn error_message<'a>(reason: String, retry: Message) -> Element<'a, Message> {
    state_block(None, reason, Some(pill(crate::i18n::t("retry"), Some(Icon::Sync), false, Some(retry))))
}

/// The notice at the bottom of the window.
pub fn toast<'a>(s: &'a str) -> Element<'a, Message> {
    // As GPUI: inverted colours (text on a text-coloured card), semibold body text.
    let card =
        container(text(s).size(theme::TYPE_BODY).font(semibold()).color(theme::bg())).padding([10, 18]).style(|_| container::Style {
            background: Some(theme::text().into()),
            border: Border { radius: 8.0.into(), ..Default::default() },
            shadow: theme::popover_shadow(),
            ..Default::default()
        });
    container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Bottom)
        .padding(iced::Padding { bottom: theme::PLAYER_H + theme::FRAME_GAP + 16.0, ..Default::default() })
        .into()
}

/// The arrow at the window edge that grows with an unfinished swipe
/// (GPUI `haptics::swipe_indicator`): 150 px of swipe fills it.
pub fn swipe_indicator<'a>(swipe: f32) -> Option<Element<'a, Message>> {
    if swipe == 0.0 {
        return None;
    }
    let k = (swipe.abs() / 150.0).min(1.0);
    let back = swipe > 0.0;
    let d = 34.0 + 12.0 * k;
    let arrow = super::icons::icon(if back { Icon::Back } else { Icon::Forward }, 18.0 + 4.0 * k, Color { a: k, ..theme::text() });
    let disc = container(arrow).center(d).style(move |_| container::Style {
        background: Some(Color { a: (0.6 + 0.4 * k) * k, ..theme::hover() }.into()),
        border: Border { radius: (d / 2.0).into(), width: 1.0, color: Color { a: 0.2 * k * k, ..theme::text() } },
        ..Default::default()
    });
    // It slides in from beyond the edge to 18 px inside it.
    let off = -d + (d + 18.0) * k;
    let pad =
        iced::Padding { left: if back { off.max(0.0) } else { 0.0 }, right: if back { 0.0 } else { off.max(0.0) }, ..Default::default() };
    Some(
        container(disc)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(if back { alignment::Horizontal::Left } else { alignment::Horizontal::Right })
            .align_y(alignment::Vertical::Center)
            .padding(pad)
            .into(),
    )
}

/// Title, subtitle, cover URL and round (artists) for a card.
pub fn item_text(item: &Item) -> (String, String, Option<String>, bool) {
    match item {
        Item::Album(a) => (a.title.clone(), a.artist_names(), a.cover(320), false),
        Item::Playlist(p) => (
            p.title.clone(),
            if p.creator.is_empty() {
                crate::i18n::text("tracks.count", &[format!("{}", p.number_of_tracks)])
            } else {
                crate::i18n::text("by", std::slice::from_ref(&p.creator))
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

fn menu_of(item: &Item) -> Option<MenuTarget> {
    match item {
        Item::Album(a) => Some(MenuTarget::Album(a.clone())),
        Item::Artist(a) => Some(MenuTarget::Artist(a.clone())),
        Item::Track(t) => Some(MenuTarget::Track(t.clone())),
        _ => None,
    }
}

/// A cover card: cover edge to edge, bold title, grey subtitle (and the
/// year for albums). Click opens, the play button plays, right click menus.
pub fn card<'a>(item: &Item, width: f32) -> Element<'a, Message> {
    catalogue_card(item, width, None)
}

pub fn grid_card<'a>(app: &App, item: &Item, width: f32) -> Element<'a, Message> {
    let rating = match item {
        Item::Album(album) => Some((stars_sized(app, album.id, true, 10.0), app.ratings.get_album(album.id) > 0)),
        _ => None,
    };
    catalogue_card(item, width, rating)
}

fn catalogue_card<'a>(item: &Item, width: f32, rating: Option<(Element<'a, Message>, bool)>) -> Element<'a, Message> {
    let (title, subtitle, url, round) = item_text(item);
    let year = match item {
        Item::Album(a) => a.year().to_string(),
        _ => String::new(),
    };
    let explicit = matches!(item, Item::Album(a) if a.explicit) || matches!(item, Item::Track(t) if t.explicit);
    let playable = !matches!(item, Item::Link(_) | Item::Promo(_));
    let art: Element<'a, Message> = if let Item::Link(_) = item {
        container(text(title.clone()).size(15).font(bold()).color(theme::text()))
            .width(width)
            .height(width * 0.56)
            .padding(14)
            .align_y(alignment::Vertical::Bottom)
            .style(|_| container::Style {
                background: Some(theme::surface().into()),
                border: Border { radius: 8.0.into(), ..Default::default() },
                ..Default::default()
            })
            .into()
    } else {
        // GPUI `card`: under the pointer a soft shadow grows under the cover,
        // a gradient to 55 % black fades over it and the play button fades in,
        // all with the card's eased hover (read at draw time).
        let r = if round { images::snap(width) / 2.0 } else { 4.0 };
        let shade = move |_: &iced::Theme| {
            let phase = super::wash::phase();
            let h = phase.hover;
            container::Style {
                background: (h > 0.0 || phase.press > 0.0).then(|| {
                    let dim = 0.08 * phase.press;
                    iced::Background::Gradient(iced::Gradient::Linear(
                        iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
                            .add_stop(0.0, Color { a: dim, ..Color::BLACK })
                            .add_stop(1.0, Color { a: dim + (1.0 - dim) * 0.55 * h, ..Color::BLACK }),
                    ))
                }),
                border: Border { radius: r.into(), ..Default::default() },
                shadow: Shadow { color: Color { a: 0.26 * h, ..Color::BLACK }, offset: Vector::new(0.0, 5.0 * h), blur_radius: 18.0 },
                ..Default::default()
            }
        };
        iced::widget::stack![
            container(Space::new().width(width).height(width)).style(move |t| container::Style { background: None, ..shade(t) }),
            cover(url, width, round),
            container(Space::new().width(width).height(width)).style(move |t| container::Style { shadow: Shadow::default(), ..shade(t) }),
        ]
        .into()
    };
    let name = line(title.clone(), theme::TYPE_LABEL, theme::text()).font(bold()).underline_on_hover();
    let title_row = row![]
        .push(if explicit { leave_room(name, 20.0) } else { name.into() })
        .push(explicit.then(explicit_badge))
        .spacing(6)
        .align_y(Alignment::Center);
    let (visible_stars, hover_stars) = match rating {
        Some((stars, true)) => (Some(stars), None),
        Some((stars, false)) => (None, Some(stars)),
        None => (None, None),
    };
    let year_row = (!year.is_empty() || visible_stars.is_some() || hover_stars.is_some()).then(|| {
        row![line(year, 10.0, theme::text_dim()).font(bold()).width(Length::Fill)].push(visible_stars).height(13).align_y(Alignment::Center)
    });
    let body = column![
        art,
        // GPUI's caption: 12 px bold title, 11.5 px subtitle, 10 px bold year.
        column![title_row, line(subtitle, 11.5, theme::text_secondary())].push(year_row).spacing(3)
    ]
    .spacing(7)
    .width(width);
    let body = if playable {
        let play = button(container(icon(Icon::Play, 18.0, theme::button_text())).center(34))
            .padding(0)
            .style(|_, status| {
                let bg = match status {
                    button::Status::Hovered => theme::accent(),
                    button::Status::Pressed => theme::with_alpha(theme::accent(), 0.7),
                    _ => theme::button(),
                };
                button::Style {
                    background: Some(bg.into()),
                    border: Border { radius: 17.0.into(), ..Default::default() },
                    shadow: Shadow { color: Color { a: 0.4, ..Color::BLACK }, offset: Vector::new(0.0, 4.0), blur_radius: 12.0 },
                    ..Default::default()
                }
            })
            .on_press(Message::PlayItem(item.clone()));
        let corner = container(play)
            .width(width)
            .height(width)
            .padding(8)
            .align_x(alignment::Horizontal::Right)
            .align_y(alignment::Vertical::Bottom);
        if let Some(stars) = hover_stars {
            let stars = container(stars).width(width).height(13).align_x(alignment::Horizontal::Right).center_y(13);
            iced::widget::hover(body, column![corner, Space::new().height(Length::Fill), stars].height(Length::Fill))
        } else {
            iced::widget::hover(body, corner)
        }
    } else {
        body.into()
    };
    let open = button(body).padding(0).style(still(0.0)).on_press(Message::OpenItem(item.clone()));
    let open = mouse_area(super::wash::track(open));
    match menu_of(item) {
        Some(target) => open.on_right_press(Message::OpenMenu(target)).into(),
        None => open.into(),
    }
}

/// Star rating in half-star steps: each star has a left and a right half to
/// click, and clicking the current value clears it.
pub fn stars<'a>(app: &App, id: u64, album: bool) -> Element<'a, Message> {
    stars_sized(app, id, album, 13.0)
}

/// [`stars`] with each star `size` px square, as GPUI sizes them per place
/// (13 in rows, 17 in headers, 15 and 12 in the unfolded album).
pub fn stars_sized<'a>(app: &App, id: u64, album: bool, size: f32) -> Element<'a, Message> {
    let value = if album { app.ratings.get_album(id) } else { app.ratings.get(id) };
    Element::new(Stars { id, album, value, size })
}

struct Stars {
    id: u64,
    album: bool,
    value: u8,
    size: f32,
}

#[derive(Default)]
struct StarInteraction {
    preview: Option<u8>,
}

/// Extra room around the stars that still counts as aiming at them (they are
/// only 10-17 px tall): above and below, and on each side.
const STAR_SLOP: (f32, f32) = (5.0, 3.0);

impl Stars {
    /// The half-star value under the pointer, within the stars and a little
    /// slop around them (the slop clamps to the first and last half).
    fn target(&self, cursor: iced::mouse::Cursor, bounds: iced::Rectangle, viewport: &iced::Rectangle) -> Option<u8> {
        let area = iced::Rectangle {
            x: bounds.x - STAR_SLOP.1,
            y: bounds.y - STAR_SLOP.0,
            width: bounds.width + 2.0 * STAR_SLOP.1,
            height: bounds.height + 2.0 * STAR_SLOP.0,
        };
        let point = cursor.position_over(area).filter(|p| viewport.contains(*p))?;
        Some(half_at(point.x - bounds.x, self.size))
    }
}

/// Half stars (1..=10) at `x` px from the left edge of stars `size` px wide.
fn half_at(x: f32, size: f32) -> u8 {
    ((x.max(0.0) / (size * 0.5)).floor() as u8 + 1).clamp(1, 10)
}

/// The new rating for a click on `target` while `current` is set: the same
/// value clears it, anything else sets it.
fn rating_after_click(current: u8, target: u8) -> u8 {
    if target == current {
        0
    } else {
        target
    }
}

impl iced::advanced::Widget<Message, iced::Theme, iced::Renderer> for Stars {
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<StarInteraction>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(StarInteraction::default())
    }

    fn size(&self) -> iced::Size<Length> {
        iced::Size::new(Length::Fixed(self.size * 5.0), Length::Fixed(self.size))
    }

    fn layout(
        &mut self,
        _: &mut iced::advanced::widget::Tree,
        _: &iced::Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        iced::advanced::layout::atomic(limits, self.size().width, self.size().height)
    }

    fn update(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::mouse::Cursor,
        _: &iced::Renderer,
        _: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) {
        use iced::mouse::{Button, Event};
        let state = tree.state.downcast_mut::<StarInteraction>();
        let target = self.target(cursor, layout.bounds(), viewport);
        if target != state.preview {
            state.preview = target;
            shell.request_redraw();
        }
        // Rated on the press itself: no need to release over the same half
        // (a 5 px target), and the row under it never sees the click.
        if let (iced::Event::Mouse(Event::ButtonPressed(Button::Left)), Some(target)) = (event, target) {
            let value = rating_after_click(self.value, target);
            shell.publish(if self.album { Message::SetAlbumRating(self.id, value) } else { Message::SetRating(self.id, value) });
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        _: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::mouse::Cursor,
        viewport: &iced::Rectangle,
        _: &iced::Renderer,
    ) -> iced::mouse::Interaction {
        if self.target(cursor, layout.bounds(), viewport).is_some() {
            iced::mouse::Interaction::Pointer
        } else {
            iced::mouse::Interaction::None
        }
    }

    fn draw(
        &self,
        _: &iced::advanced::widget::Tree,
        renderer: &mut iced::Renderer,
        _: &iced::Theme,
        _: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        use iced::advanced::svg::{Renderer as _, Svg};
        let bounds = layout.bounds();
        let value = self.target(cursor, bounds, viewport).unwrap_or(self.value);
        for i in 0..5 {
            let (glyph, color) = if value >= (i + 1) * 2 {
                (Icon::Star, theme::gold())
            } else if value == i * 2 + 1 {
                (Icon::StarHalf, theme::gold())
            } else {
                (Icon::StarOutline, theme::text_dim())
            };
            let color = super::wash::faded(color);
            if color.a > 0.002 {
                let rect = iced::Rectangle { x: bounds.x + f32::from(i) * self.size, y: bounds.y, width: self.size, height: self.size };
                renderer.draw_svg(Svg::new(super::icons::handle(glyph, self.size, 0.0)).color(color), rect, *viewport);
            }
        }
    }
}

/// A rounded panel surface.
pub fn panel_style(_: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::surface())),
        border: Border { color: theme::divider(), width: 1.0, radius: 12.0.into() },
        ..Default::default()
    }
}

/// A heading of a panel: icon, title and a quieter detail.
/// As GPUI: the icon in a 32 px tile, the title at section size.
pub fn panel_heading<'a>(i: Icon, title: impl Into<std::borrow::Cow<'a, str>>, detail: Option<String>) -> Element<'a, Message> {
    let tile = container(icon(i, 16.0, theme::text_secondary())).center(32).style(|_| container::Style {
        background: Some(theme::ink(0.06).into()),
        border: Border { radius: 10.0.into(), ..Default::default() },
        ..Default::default()
    });
    row![
        tile,
        column![line(title, theme::TYPE_SECTION, theme::text()).font(bold())]
            .push(detail.map(|d| line(d, 11.5, theme::text_secondary())))
            .spacing(3)
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

/// `content` laid out `room` px narrower than the space it is offered, so
/// what follows it in a row keeps its size: the explicit badge after a title
/// that ends in "…" (GPUI's `flex_none` beside a truncating title).
pub fn leave_room<'a>(content: impl Into<Element<'a, Message>>, room: f32) -> Element<'a, Message> {
    Element::new(Room { content: content.into(), room })
}

struct Room<'a> {
    content: Element<'a, Message>,
    room: f32,
}

mod room {
    use iced::advanced::widget::{Operation, Tree, Widget};
    use iced::advanced::{layout, mouse, renderer, Clipboard, Layout, Shell};
    use iced::{Event, Length, Rectangle, Renderer, Size, Theme};

    use super::{Message, Room};

    impl Widget<Message, Theme, Renderer> for Room<'_> {
        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.content)]
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_ref(&self.content));
        }

        fn size(&self) -> Size<Length> {
            self.content.as_widget().size()
        }

        fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
            let max = limits.max();
            let narrower = layout::Limits::new(limits.min(), Size::new((max.width - self.room).max(0.0), max.height));
            self.content.as_widget_mut().layout(&mut tree.children[0], renderer, &narrower)
        }

        fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
            self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
        }

        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) {
            self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
        }

        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
        }
    }
}

/// How far the switch's knob slides: the track less its padding and the knob.
const SWITCH_TRAVEL: f32 = 40.0 - 2.0 * 2.0 - 18.0;

/// A pill-shaped toggle switch.
pub fn switch<'a>(on: bool, msg: Message) -> Element<'a, Message> {
    button(switch_face(on)).padding(0).style(|_, _| button::Style::default()).on_press(msg).into()
}

/// The switch as drawn (knob sliding, track fading), without a button.
pub fn switch_face<'a>(on: bool) -> Element<'a, Message> {
    let (off_track, on_track, off_knob, on_knob) = theme::switch_colors();
    // GPUI `settings::switch`: 40 x 22, an 18 px knob 2 px from the edge.
    // The knob slides and the track fades between its colours over
    // `CONTROL`, following `on` (paint only).
    let knob = container(Space::new().width(18).height(18)).style(move |_| container::Style {
        background: Some(super::wash::faded(theme::lerp(off_knob, on_knob, super::wash::phase().active)).into()),
        border: Border { radius: 9.0.into(), ..Default::default() },
        ..Default::default()
    });
    let knob = super::wash::shifted(knob, |p| Vector::new(SWITCH_TRAVEL * p.active, 0.0));
    let track = container(knob).width(40).height(22).padding(2).align_x(alignment::Horizontal::Left);
    let track = super::wash::surface(track, move |p| super::wash::Look {
        background: Some(theme::lerp(off_track, on_track, p.active).into()),
        border: Border { radius: 11.0.into(), ..Default::default() },
        ..Default::default()
    })
    .active(on);
    track.into()
}

/// `base` with optional layers over it. The tree keeps one shape whichever
/// layers show: Iced drops a widget's state (focus, scroll, drag) as soon as
/// the widget type at its position changes, so an absent layer leaves an empty
/// `Space` in its place and the base is never returned bare.
pub fn layered<'a>(base: Element<'a, Message>, over: Vec<Option<Element<'a, Message>>>) -> Element<'a, Message> {
    let mut children = vec![base];
    children.extend(over.into_iter().map(|layer| layer.unwrap_or_else(|| Space::new().into())));
    iced::widget::stack(children).width(Length::Fill).height(Length::Fill).into()
}

#[cfg(test)]
mod layer_tests {
    use super::*;
    use iced::advanced::widget::Tree;

    type Field = iced::widget::text_input::State<<iced::Renderer as iced::advanced::text::Renderer>::Paragraph>;

    /// The text field's state, wherever it sits in the tree.
    fn field(tree: &mut Tree) -> Option<&mut Field> {
        if tree.tag == iced::advanced::widget::tree::Tag::of::<Field>() {
            return Some(tree.state.downcast_mut::<Field>());
        }
        tree.children.iter_mut().find_map(field)
    }

    fn view<'a>(shown: [bool; 2]) -> Element<'a, Message> {
        let base = iced::widget::text_input("", "").on_input(Message::SearchInput).into();
        layered(base, shown.into_iter().map(|on| on.then(|| text("over").into())).collect())
    }

    #[test]
    fn layers_coming_and_going_keep_the_focus_of_what_is_under_them() {
        let first = view([false, false]);
        let mut tree = Tree::new(first.as_widget());
        field(&mut tree).unwrap().focus();
        for shown in [[true, false], [true, true], [false, true], [false, false]] {
            let next = view(shown);
            tree.diff(next.as_widget());
            assert!(field(&mut tree).unwrap().is_focused(), "focus lost with layers {shown:?}");
        }
    }
}

#[cfg(test)]
mod star_tests {
    use super::{half_at, rating_after_click};

    #[test]
    fn each_star_has_two_halves_and_the_edges_clamp() {
        assert_eq!(half_at(0.0, 13.0), 1);
        assert_eq!(half_at(6.4, 13.0), 1);
        assert_eq!(half_at(6.6, 13.0), 2);
        assert_eq!(half_at(64.9, 13.0), 10);
        // In the slop beyond either end: the first or the last half.
        assert_eq!(half_at(-3.0, 13.0), 1);
        assert_eq!(half_at(70.0, 13.0), 10);
    }

    #[test]
    fn clicking_the_current_value_clears_it_and_any_other_sets_it() {
        assert_eq!(rating_after_click(0, 7), 7);
        assert_eq!(rating_after_click(7, 7), 0);
        assert_eq!(rating_after_click(7, 4), 4);
    }
}
