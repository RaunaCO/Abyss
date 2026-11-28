//! Fullscreen playback: artwork and controls beside a tabbed detail panel
//! (queue, similar, credits, lyrics), over the turning blurred cover.

use std::time::Instant;

use iced::widget::{button, column, container, mouse_area, row, scrollable, shader, stack, text};
use iced::{alignment, Alignment, Border, Color, Element, Length, Shadow, Vector};

use super::gpu::backdrop::Backdrop;
use super::icons::{turned, Icon};
use super::lyrics::{Colors, Kind, Lyrics, Style};
use super::widgets::{cover, fmt_time, line, loading, pill};
use super::{theme, App, Message};
use crate::shared::Load;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NpTab {
    #[default]
    Lyrics,
    Credits,
    Similar,
    Queue,
}

impl NpTab {
    fn label(self) -> &'static str {
        match self {
            NpTab::Queue => crate::i18n::t("queue"),
            NpTab::Similar => crate::i18n::t("similar"),
            NpTab::Credits => crate::i18n::t("credits"),
            NpTab::Lyrics => crate::i18n::t("lyrics"),
        }
    }
}

// The official app's background rotation: the blurred cover turns about its
// centre at 360 degrees per minute while playing; on pause it coasts 30
// degrees further over 1.5 s (ease-out cubic) and stops; while the progress
// bar is dragged the angle follows the bar (180 degrees across the track).
const SPIN_PERIOD_S: f32 = 60.0;
const COAST_DEG: f32 = 30.0;
const COAST_S: f32 = 1.5;

#[derive(Clone, Copy, Debug)]
pub enum BgSpin {
    Stopped(f32),
    Spinning { start: f32, at: Instant },
    Coasting { start: f32, at: Instant },
    Scrub { base: f32, frac0: f32 },
}

impl BgSpin {
    pub fn angle(self, now: Instant, frac: f32) -> f32 {
        match self {
            BgSpin::Stopped(a) => a,
            BgSpin::Spinning { start, at } => start + now.saturating_duration_since(at).as_secs_f32() / SPIN_PERIOD_S * 360.0,
            BgSpin::Coasting { start, at } => {
                let t = (now.saturating_duration_since(at).as_secs_f32() / COAST_S).clamp(0.0, 1.0);
                start + COAST_DEG * (1.0 - (1.0 - t).powi(3))
            }
            BgSpin::Scrub { base, frac0 } => base + (frac - frac0) * 180.0,
        }
    }
}

impl App {
    /// Advance the background's state machine (called on every tick and drag).
    pub(super) fn step_spin(&mut self) {
        let now = Instant::now();
        let playing = self.is_playing();
        let scrubbing = self.drag == Some(super::Drag::Progress);
        let frac = self.drag_value;
        let cur = self.bg_spin.angle(now, frac);
        self.bg_spin = match (scrubbing, playing, self.bg_spin) {
            (true, _, s @ BgSpin::Scrub { .. }) => s,
            (true, _, _) => BgSpin::Scrub { base: cur, frac0: frac },
            (false, true, s @ BgSpin::Spinning { .. }) => s,
            (false, true, _) => BgSpin::Spinning { start: cur, at: now },
            (false, false, BgSpin::Spinning { .. } | BgSpin::Scrub { .. }) => BgSpin::Coasting { start: cur, at: now },
            (false, false, BgSpin::Coasting { start, at }) => {
                if now.duration_since(at).as_secs_f32() >= COAST_S {
                    BgSpin::Stopped(start + COAST_DEG)
                } else {
                    BgSpin::Coasting { start, at }
                }
            }
            (false, false, s) => s,
        };
    }
}

/// Cover over details (one column) when the window is narrow, or tall enough
/// that two columns would leave the cover small in empty height.
pub fn np_stacked(vw: f32, vh: f32) -> bool {
    vw < 820.0 || vh > vw * 1.15
}

/// Artwork fits the available column and leaves room for metadata and controls.
pub fn cover_size(vw: f32, vh: f32) -> f32 {
    if np_stacked(vw, vh) {
        let margin = (vw * 0.035).clamp(20.0, 64.0);
        return (vw - 2.0 * margin).min(vh * 0.34).clamp(80.0, 900.0);
    }
    let width = (vw * 0.46 - 80.0).min(640.0);
    width.min((vh - 400.0).max(120.0)).max(80.0)
}

/// Height left to the queue tab under the tabs and inside the margins.
pub fn queue_height(vw: f32, vh: f32) -> f32 {
    let margin = (vw * 0.035).clamp(20.0, 64.0);
    let details = if np_stacked(vw, vh) { (vh * 0.36).clamp(320.0, 900.0) } else { vh - 2.0 * margin };
    // The tabs (a line of 12 or 13 px text in 6 or 7 px of padding, 20 px
    // under; see `tab_button`) and the tab's own inset.
    let tabs = if vh >= 1000.0 { 13.0 * 1.3 + 14.0 } else { 12.0 * 1.3 + 12.0 };
    (details - tabs - 20.0 - 2.0 * margin).max(0.0)
}

// TODO(theme): the shared spacing scale (`theme::SPACE_*`) once arte's tokens are merged.
const SPACE_MD: f32 = 12.0;
const SPACE_LG: f32 = 16.0;
const SPACE_XL: f32 = 24.0;
const SPACE_XXL: f32 = 32.0;

/// Width of the column under the cover (titles), as laid out by `view`.
fn titles_width(vw: f32, vh: f32) -> f32 {
    let margin = (vw * 0.035).clamp(20.0, 64.0);
    let column = if np_stacked(vw, vh) { vw - 2.0 * margin } else { vw * 0.46 - margin };
    // Never narrower than a big cover above it.
    column.clamp(120.0, cover_size(vw, vh).max(560.0)).floor()
}

/// Sizes of the title and the artists: they grow with the cover, so the
/// block under a 640 px cover is not a caption under a poster.
fn title_sizes(vw: f32, vh: f32) -> (f32, f32) {
    let title = if np_stacked(vw, vh) { (vw * 0.028).clamp(22.0, 34.0) } else { (cover_size(vw, vh) * 0.075).clamp(26.0, 36.0) };
    (title.round(), (title * 0.56).clamp(15.0, 21.0).round())
}

/// `text` as it shows in at most `lines` lines `width` px wide: whole when it
/// fits, else cut at the last line with "…" (Iced only clips). Cached: the
/// view is built far more often than the track or the window changes.
fn clamp_lines(text: &str, size: f32, weight: u16, width: f32, lines: usize) -> String {
    type Key = (String, u32, u16, u32, usize);
    thread_local! {
        static CACHE: std::cell::RefCell<Vec<(Key, String)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    let key: Key = (text.to_string(), (size * 10.0) as u32, weight, width as u32, lines);
    if let Some(hit) = CACHE.with_borrow(|c| c.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
        return hit;
    }
    let out = super::lyrics::clamp(text, size, weight, width, lines);
    CACHE.with_borrow_mut(|c| {
        if c.len() >= 8 {
            c.remove(0);
        }
        c.push((key, out.clone()));
    });
    out
}

/// Human labels for TIDAL credit roles.
fn role_label(role: &str) -> String {
    match role {
        "Main Artist" | "MAIN_ARTIST" => crate::i18n::t("main_artist").into(),
        "Featured Artist" | "FEATURED_ARTIST" => crate::i18n::t("featured_artist").into(),
        "Producer" => crate::i18n::t("production").into(),
        "Composer" => crate::i18n::t("composition").into(),
        "Lyricist" => crate::i18n::t("lyrics").into(),
        "Songwriter" | "Writer" => crate::i18n::t("songwriting").into(),
        "Mixing Engineer" | "Mixer" => crate::i18n::t("mixing").into(),
        "Mastering Engineer" => crate::i18n::t("mastering").into(),
        "Engineer" | "Recording Engineer" => crate::i18n::t("sound_engineering").into(),
        "Record Label" => crate::i18n::t("record_label").into(),
        "Vocals" | "Vocal" => crate::i18n::t("vocals").into(),
        "Guitar" => crate::i18n::t("guitar").into(),
        "Bass" | "Bass Guitar" => crate::i18n::t("bass").into(),
        "Drums" => crate::i18n::t("drums").into(),
        "Keyboards" => crate::i18n::t("keyboards").into(),
        other => other.to_string(),
    }
}

/// The lyrics of the playing track for either presentation: the GPU view
/// for synchronised lines, plain text otherwise, the dots while loading.
pub fn lyrics_view(app: &App, kind: Kind, style: Style) -> Element<'_, Message> {
    let Some(now) = &app.now else { return super::widgets::message(crate::i18n::t("nothing_is_playing")) };
    let id = now.track.id;
    match app.lyrics.get(&id) {
        Some(Load::Ready(l)) if !l.synced.is_empty() => {
            let Some(doc) = app.lyrics_doc(id) else { return loading() };
            let colors = match kind {
                Kind::NowPlaying => {
                    let c = app.ambient.now();
                    Colors {
                        text: Color::WHITE,
                        dim: Color { a: 0.55, ..Color::WHITE },
                        past: Color { a: 0.55, ..Color::WHITE },
                        hover: theme::ink(0.1),
                        // Lift the cover colour towards white so the halo reads on any cover.
                        glow: Color { r: c.r * 0.6 + 0.4, g: c.g * 0.6 + 0.4, b: c.b * 0.6 + 0.4, a: 1.0 },
                        provider: theme::text_secondary(),
                    }
                }
                Kind::Side => Colors {
                    text: theme::text(),
                    dim: theme::text_dim(),
                    past: theme::text_secondary(),
                    hover: theme::hover(),
                    glow: Color::TRANSPARENT,
                    provider: theme::text_dim(),
                },
            };
            let program = Lyrics {
                provider_label: crate::i18n::text("lyrics.provider", std::slice::from_ref(&doc.provider)),
                doc,
                style,
                colors,
                position: app.snap.position,
                at: app.snap_at,
                playing: app.is_playing() && app.drag != Some(super::Drag::Progress),
                duration: app.duration(),
                follow: app.lyrics_follow,
                reading_first: kind == Kind::NowPlaying,
            };
            let view = super::glide::glide(shader(program).width(Length::Fill).height(Length::Fill));
            with_sync_button(app, view)
        }
        Some(Load::Ready(l)) if !l.text.is_empty() => {
            let show_roman = app.settings.lock().lyrics_romanize;
            let (size, lh) = match kind {
                Kind::NowPlaying => (36.0 * style.font / 44.0, 48.0 * style.font / 44.0),
                Kind::Side => (theme::TYPE_SECTION, 26.0),
            };
            let lines = l.text.lines().enumerate().map(|(i, line)| {
                let reading = l.roman.get(i).and_then(|r| r.as_ref()).filter(|_| show_roman);
                let primary = text(if line.is_empty() { "\u{a0}".to_string() } else { line.to_owned() })
                    .size(size)
                    .line_height(iced::widget::text::LineHeight::Absolute(lh.into()))
                    .font(super::widgets::semibold())
                    .color(if kind == Kind::NowPlaying { Color { a: 0.85, ..Color::WHITE } } else { theme::text() });
                column![primary].push(reading.map(|r| text(r.clone()).size(size * 0.45).color(theme::text_secondary()))).into()
            });
            let provider = text(crate::i18n::text("unsynced", std::slice::from_ref(&l.provider))).size(11).color(theme::text_dim());
            super::glide::glide(
                scrollable(
                    column(lines)
                        .push(container(provider).padding(iced::Padding { top: 28.0, ..Default::default() }))
                        .padding(iced::Padding { left: style.left, right: style.right, bottom: 64.0, top: 0.0 }),
                )
                .height(Length::Fill)
                .style(super::pages::scroll_style),
            )
        }
        Some(Load::Loading) => loading(),
        _ => missing_lyrics(app),
    }
}

/// "Sincronizar" floating over the lyrics while they do not follow the song.
fn with_sync_button<'a>(app: &'a App, content: Element<'a, Message>) -> Element<'a, Message> {
    let syncing = app.lyrics_sync_at;
    if app.lyrics_follow && syncing.is_none() {
        return super::widgets::layered(content, vec![None]);
    }
    // Spin: eases in over 0.15 s, then a steady turn every 0.7 s.
    let deg = syncing.map_or(0.0, |at| {
        if crate::shared::lyrics::reduced_motion() {
            return 0.0;
        }
        let t = at.elapsed().as_secs_f32();
        let speed = 360.0 / 0.7;
        let ramp = 0.15;
        if t < ramp {
            speed * t * t / (2.0 * ramp)
        } else {
            speed * (t - ramp / 2.0)
        }
    });
    let btn = button(
        container(
            row![
                turned(Icon::Sync, 15.0, theme::text(), deg),
                text(crate::i18n::t("sync")).size(13).font(super::widgets::semibold()).color(theme::text())
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        )
        .center_y(Length::Fill),
    )
    .padding(iced::Padding { top: 0.0, bottom: 0.0, left: 12.0, right: 15.0 })
    .height(34)
    .style(|_, _| button::Style { text_color: theme::text(), snap: true, ..Default::default() })
    .on_press(Message::LyricsSync);
    // A capsule of liquid glass over the lyrics, as the other floating controls.
    // A control over text: popover frost for legibility, and it follows the pointer.
    let btn = super::liquid_glass::glass(super::wash::track(btn), 17.0)
        .optics(super::liquid_glass::Optics::POPOVER)
        .interactive()
        .shadow(Shadow { color: Color { a: 0.44, ..Color::BLACK }, offset: Vector::new(0.0, 6.0), blur_radius: 22.0 });
    let overlay = container(btn)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Bottom)
        .padding(iced::Padding { bottom: 26.0, ..Default::default() });
    super::widgets::layered(content, vec![Some(overlay.into())])
}

/// An empty state centred in the space given, a little above the middle
/// (the optical centre), so it does not cling to the tabs or the heading.
fn centred_state<'a>(block: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(block)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(iced::Padding { bottom: SPACE_XXL, left: SPACE_LG, right: SPACE_LG, top: 0.0 })
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Center)
        .into()
}

/// The icon tile of the empty states (`widgets::notice`'s, without its 48 px
/// padding, so the actions under it keep one rhythm).
fn state_tile<'a>(i: Icon) -> Element<'a, Message> {
    container(super::icons::icon(i, 20.0, theme::text_secondary()))
        .center(48)
        .style(|_| container::Style {
            background: Some(theme::ink(0.06).into()),
            border: Border { color: theme::ink(0.08), width: 1.0, radius: 24.0.into() },
            ..Default::default()
        })
        .into()
}

/// A note under its tile, as every empty state: `widgets::notice` centred in the space.
pub fn empty_state<'a>(i: Icon, note: &'a str) -> Element<'a, Message> {
    centred_state(
        column![state_tile(i), text(note).size(theme::TYPE_BODY).color(theme::text_secondary()).center()]
            .spacing(14)
            .align_x(Alignment::Center)
            .max_width(420),
    )
}

/// No lyrics: retry, and offer third-party providers when they are off.
fn missing_lyrics(app: &App) -> Element<'_, Message> {
    let external = app.settings.lock().external_lyrics;
    // The shape every empty state shares (icon tile, centred note), then the
    // actions: the retry, and the offer as a second, quieter paragraph.
    let note = |s: &'static str, color: Color| {
        text(s).size(theme::TYPE_BODY).line_height(iced::widget::text::LineHeight::Absolute(20.0.into())).color(color).center()
    };
    let head = column![state_tile(Icon::Lyrics), note(crate::i18n::t("no_provider_has_lyrics_for_this_track"), theme::text_secondary())]
        .spacing(14)
        .align_x(Alignment::Center);
    let retry = pill(crate::i18n::t("lyrics.retry"), Some(Icon::Sync), false, Some(Message::RetryLyrics));
    let offer = (!external).then(|| {
        column![
            note(crate::i18n::t("lyrics.external_offer"), theme::text_dim()),
            pill(crate::i18n::t("lyrics.external_enable"), None, false, Some(Message::ToggleSetting("external_lyrics"))),
        ]
        .spacing(SPACE_MD)
        .align_x(Alignment::Center)
    });
    let block = column![head, retry].push(offer).spacing(SPACE_XL).align_x(Alignment::Center).max_width(400);
    centred_state(block)
}

fn credits_view(app: &App, id: u64) -> Element<'_, Message> {
    match app.credits.get(&id) {
        Some(Load::Ready(list)) if !list.is_empty() => scrollable(
            column(list.iter().map(|(role, names)| {
                column![
                    text(role_label(role).to_uppercase())
                        .size(11)
                        .font(super::widgets::semibold())
                        .color(Color { a: 0.55, ..Color::WHITE }),
                    text(names.join(", ")).size(18).font(super::widgets::semibold()).color(theme::text()),
                ]
                .spacing(4)
                .into()
            }))
            .spacing(20)
            // The names start where the queue's covers do (12 + 8 px in the pane).
            .padding(iced::Padding { left: 20.0, right: 24.0, top: 6.0, bottom: 64.0 }),
        )
        .height(Length::Fill)
        .style(super::pages::scroll_style)
        .into(),
        Some(Load::Loading) => loading(),
        _ => empty_state(Icon::List, crate::i18n::t("no_credits_for_this_track")),
    }
}

fn similar_view(app: &App, id: u64) -> Element<'_, Message> {
    let tracks = match app.similar.get(&id) {
        Some(Load::Ready(t)) => std::sync::Arc::new(t.clone()),
        Some(Load::Loading) => return loading(),
        _ => return empty_state(Icon::Mix, crate::i18n::t("no_similar_tracks")),
    };
    let playing = app.now.as_ref().map(|n| n.track.id);
    let rows = tracks.iter().enumerate().map(|(i, t)| {
        let current = playing == Some(t.id);
        let r = row![
            cover(t.cover(80), 38.0, false),
            column![
                line(t.display_title(), 13.0, if current { theme::accent() } else { theme::text() }).font(super::widgets::bold()),
                line(t.artist_names(), 12.0, Color { a: 0.6, ..Color::WHITE }),
            ]
            .spacing(2)
            .width(Length::Fill),
            text(fmt_time(t.duration as f64)).size(11).color(Color { a: 0.5, ..Color::WHITE }),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .height(52)
        .padding([0, 8]);
        mouse_area(
            super::wash::wash(r.width(Length::Fill), Color { a: 0.08, ..Color::WHITE }, 8.0).press(Color { a: 0.06, ..Color::WHITE }),
        )
        .on_double_click(Message::PlayTracks(tracks.clone(), i))
        .on_right_press(Message::OpenMenu(super::MenuTarget::Track(t.clone())))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
    });
    super::glide::glide(
        scrollable(column(rows).padding(iced::Padding { left: 12.0, right: 12.0, bottom: 64.0, top: 0.0 }))
            .height(Length::Fill)
            .style(super::pages::scroll_style),
    )
}

fn tab_button(app: &App, tab: NpTab) -> Element<'_, Message> {
    let active = app.np_tab == tab;
    // A step larger on tall windows, where the lyrics beside them are 50 px.
    let large = app.viewport.height >= 1000.0;
    let b = button(text(tab.label()).size(if large { 13 } else { 12 }).font(super::widgets::semibold()))
        .padding(if large { [7, 16] } else { [6, 14] })
        .style(move |_, _| button::Style {
            background: active.then(|| theme::with_alpha(theme::text(), 1.0 - 0.1 * super::wash::phase().hover).into()),
            text_color: if active {
                theme::bg()
            } else {
                super::wash::mix(Color { a: 0.7, ..Color::WHITE }, theme::text(), super::wash::phase().hover)
            },
            border: Border { radius: 13.0.into(), ..Default::default() },
            ..Default::default()
        })
        .on_press(Message::NpTab(tab));
    super::wash::track(b).into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let (vw, vh) = (app.viewport.width, app.viewport.height);
    let stacked = np_stacked(vw, vh);
    let margin = (vw * 0.035).clamp(20.0, 64.0);
    let Some(track) = app.now.as_ref().map(|n| n.track.clone()) else {
        return container(
            column![
                text(crate::i18n::t("play_a_track_to_view_it_in_full_screen")).color(theme::text_secondary()),
                container(super::player_bar::controls(app, true, vw)).max_width(360),
            ]
            .spacing(28)
            .align_x(Alignment::Center),
        )
        .center(Length::Fill)
        .padding(margin)
        .style(|_| container::Style { background: Some(theme::bg().into()), ..Default::default() })
        .into();
    };
    let ambient = app.ambient.now();
    let palette = app.palette.now();
    let blur = app.settings.lock().np_blur;
    let cover_url = track.cover(640);
    let backdrop = Backdrop {
        cover: if blur { cover_url.as_deref().and_then(|u| super::images::backdrop(u).map(|img| (u.to_string(), img))) } else { None },
        blur,
        spin: app.bg_spin,
        drag_frac: app.drag_value,
        ambient,
        palette: [palette.first().copied().unwrap_or(ambient), palette.get(1).copied().unwrap_or(ambient)],
        shade: theme::multiply(theme::with_lightness(ambient, 0.42), theme::rgb(0x2a2a2a)),
    };
    let size = cover_size(vw, vh);
    let local = track.local_path.is_some();
    let id = track.id;
    let tab = if local && matches!(app.np_tab, NpTab::Credits | NpTab::Similar) { NpTab::Lyrics } else { app.np_tab };
    // The text follows the height of the window, but never outgrows its width.
    let lyric_scale = (vh / 900.0).min(vw / 1500.0).clamp(0.55, 2.5);
    let body: Element<'_, Message> = match tab {
        NpTab::Lyrics => lyrics_view(app, Kind::NowPlaying, Style::now_playing(lyric_scale, margin)),
        NpTab::Queue => super::queue::panel(app, true),
        NpTab::Credits => credits_view(app, id),
        NpTab::Similar => similar_view(app, id),
    };
    let tabs = row![tab_button(app, NpTab::Queue)]
        .push((!local).then(|| tab_button(app, NpTab::Similar)))
        .push((!local).then(|| tab_button(app, NpTab::Credits)))
        .push(tab_button(app, NpTab::Lyrics))
        .spacing(6)
        .align_y(Alignment::Center);
    // Double-click the cover to like the track.
    let big_cover = cover_url.clone().map(|_| track.cover(1280)).unwrap_or(None);
    let mut art_layers: Vec<Element<'_, Message>> = vec![big_art(big_cover, size)];
    // GPUI `haptics::heart_overlay`, drawn per frame by the glyph itself.
    if let Some(at) = app.heart_burst.filter(|at| at.elapsed() < super::icons::BURST) {
        let heart = super::icons::glyph(Icon::HeartFilled, size * 0.3, Color { a: 0.92, ..theme::text() }).burst(at);
        art_layers.push(container(heart).center(size).into());
    }
    let artwork = mouse_area(stack(art_layers).width(size).height(size)).on_double_click(Message::CoverDoubleClick);
    // The title and the artists, two lines each at most, ending in "…" when
    // longer; the lines are 1.2 em apart whatever the size.
    let (title_size, artist_size) = title_sizes(vw, vh);
    let title_w = titles_width(vw, vh);
    let title = clamp_lines(&track.display_title(), title_size, 700, title_w, 2);
    let artists = clamp_lines(&track.artist_names(), artist_size, 400, title_w, 2);
    let titles = column![
        text(title)
            .size(title_size)
            .line_height(iced::widget::text::LineHeight::Relative(1.2))
            .font(super::widgets::bold())
            .color(theme::text())
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
            .align_x(alignment::Horizontal::Center)
            .width(Length::Fill),
        text(artists)
            .size(artist_size)
            .line_height(iced::widget::text::LineHeight::Relative(1.25))
            .color(Color { a: 0.72, ..Color::WHITE })
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
            .align_x(alignment::Horizontal::Center)
            .width(Length::Fill),
    ]
    .spacing(6)
    .width(title_w);
    let controls = container(super::player_bar::controls(app, true, vw))
        // As wide as a big cover (the bar under a 640 px cover was 500 px).
        .max_width(if stacked { (vw * 0.45).clamp(500.0, 760.0) } else { size.clamp(500.0, 660.0) })
        .width(Length::Fill);
    let left = column![artwork, titles, controls].spacing(24).align_x(Alignment::Center);
    let left: Element<'_, Message> = if stacked {
        container(left).width(Length::Fill).padding([0.0, margin]).align_x(alignment::Horizontal::Center).into()
    } else {
        container(left)
            .width(Length::FillPortion(46))
            .height(Length::Fill)
            .padding(iced::Padding { left: margin, ..Default::default() })
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center)
            .into()
    };
    // GPUI insets the other tabs by the margin on the sides only.
    // Inside the glass pane the other tabs keep a compact inset (the pane
    // itself takes the margin outside).
    let details_body =
        container(body).width(Length::Fill).height(Length::Fill).padding([0.0, if tab == NpTab::Lyrics { 0.0 } else { 14.0 }]);
    // GPUI: each tab's content fades in over `PANEL` (`motion::enter`).
    let details_body = super::wash::appear(details_body, super::motion::PANEL, tab as u64, 0.0, 0.0);
    // Queue, similar tracks and credits sit on a pane of glass over the
    // flowing background (readable whatever the cover); the lyrics float free.
    let details_body: Element<'_, Message> = if tab == NpTab::Lyrics {
        details_body
    } else {
        container(
            super::liquid_glass::glass(container(details_body).padding([14, 0]).width(Length::Fill).height(Length::Fill), theme::PANEL_R)
                .optics(super::liquid_glass::Optics::OVER_ART)
                .shadow(theme::popover_shadow()),
        )
        .padding(iced::Padding { left: margin * 0.5, right: margin, bottom: margin * 0.5, top: 0.0 })
        .into()
    };
    // The tabs on a capsule of glass: a control (one pane for the group).
    let tabs = super::liquid_glass::control(container(tabs).padding(4));
    // Centred over the pane under them (which sits half a margin in on the
    // left, a whole one on the right).
    let details = column![
        container(tabs).width(Length::Fill).align_x(alignment::Horizontal::Center).padding(iced::Padding {
            bottom: 20.0,
            left: margin * 0.5,
            right: margin,
            top: 0.0
        }),
        details_body
    ];
    let layout: Element<'_, Message> = if stacked {
        scrollable(
            column![left, container(details).width(Length::Fill).height((vh * 0.36).clamp(320.0, 900.0))]
                .spacing(margin)
                .padding([margin, 0.0]),
        )
        .height(Length::Fill)
        .style(super::pages::scroll_style)
        .into()
    } else {
        row![left, container(details).width(Length::FillPortion(54)).height(Length::Fill)]
            .padding([margin, 0.0])
            .height(Length::Fill)
            .into()
    };
    stack![shader(backdrop).width(Length::Fill).height(Length::Fill), layout].width(Length::Fill).height(Length::Fill).into()
}

/// The big cover, with a soft shadow.
fn big_art<'a>(url: Option<String>, size: f32) -> Element<'a, Message> {
    let art = match url.as_deref().and_then(|u| super::images::sized(u, size, size)) {
        Some(h) => iced::widget::image(h).width(size).height(size).content_fit(iced::ContentFit::Cover).border_radius(8.0).into(),
        None => cover(url, size, false),
    };
    // GPUI `tilt`: a light follows the pointer over the still picture.
    let art = super::sheen::sheen(art, 8.0);
    container(art)
        .width(size)
        .height(size)
        .style(|_| container::Style {
            shadow: Shadow { color: Color { a: 0.45, ..Color::BLACK }, offset: Vector::new(0.0, 18.0), blur_radius: 48.0 },
            border: Border { radius: 8.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
#[path = "panel_shots.rs"]
mod panel_shots;
