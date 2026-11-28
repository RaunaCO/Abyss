//! The bottom bar, laid out like the official app: track info with like and
//! menu on the left; shuffle / previous / play / next / repeat with the time
//! and progress line below them in the centre; queue, lyrics, volume and the
//! full-screen button on the right. `controls` is shared with Now Playing.

use iced::widget::{button, column, container, mouse_area, row, slider, text};
use iced::{alignment, Alignment, Border, Color, Element, Length, Shadow, Vector};

use super::icons::Icon;
use super::widgets::{cover, explicit_badge, fmt_time, icon_button, line, stars};
use super::{theme, App, Drag, MenuTarget, Message, NpTab};
use crate::config::Repeat;
use crate::shared::Page;

pub fn view(app: &App) -> Element<'_, Message> {
    let width = app.viewport.width - 2.0 * theme::FRAME_GAP;
    let compact = width < 1150.0;
    let wing = (width * 0.28).min(400.0);
    let now = app.now.as_ref().map(|n| n.track.clone());
    let left: Element<'_, Message> = match now {
        None => super::widgets::panel_heading(Icon::Track, crate::i18n::t("nothing_playing"), None),
        Some(track) => {
            let fav = app.fav_ids.contains(&track.id);
            let local = track.local_path.is_some();
            let album_id = track.album.as_ref().map(|a| a.id).filter(|id| *id != 0);
            let artist_id = track.artists.first().map(|a| a.id).filter(|id| *id != 0);
            let link = |s: String, size: f32, color: Color, bold: bool, target: Option<Page>| -> Element<'_, Message> {
                let t = line(s, size, color);
                let t = if bold { t.font(super::widgets::bold()) } else { t };
                match target {
                    Some(p) => mouse_area(super::wash::track(t.brighten_on_hover(theme::text())))
                        .on_press(Message::Navigate(p))
                        .interaction(iced::mouse::Interaction::Pointer)
                        .into(),
                    None => t.into(),
                }
            };
            let album = track.album.as_ref().map(|a| a.title.clone()).filter(|t| !t.trim().is_empty());
            let artists = track.artist_names();
            let sub = row![]
                .push(album.map(|a| link(a, 11.0, theme::text_secondary(), false, album_id.map(Page::Album))))
                .push((track.album.is_some() && !artists.is_empty()).then(|| text("·").size(11).color(theme::text_secondary())))
                .push(link(artists, 11.0, theme::text_secondary(), false, artist_id.map(Page::Artist)))
                .spacing(5)
                .align_y(Alignment::Center);
            let title = row![link(track.display_title(), 12.0, theme::text(), true, album_id.map(Page::Album))]
                .push(track.explicit.then(explicit_badge))
                .spacing(6)
                .align_y(Alignment::Center);
            // GPUI `heart_scale`: the heart pops after a like (paint scale, drawn per frame).
            let pop = app.heart_pop.filter(|(id, _)| *id == track.id).map(|(_, at)| at);
            // GPUI: a new track slides its title in, once per track
            // (`opacity(0.7 + 0.3 t).left((1 - t) * 3)` over `FAST`).
            let names = super::wash::appear(column![title, sub].spacing(3).width(Length::Fill), super::motion::FAST, track.id, 0.7, 3.0);
            row![cover(track.cover(160), 42.0, false), names]
                .push((!local).then(|| {
                    super::widgets::glyph_button(
                        super::widgets::soft_glyph(
                            if fav { Icon::HeartFilled } else { Icon::Heart },
                            15.0,
                            if fav { theme::text() } else { theme::text_secondary() },
                        )
                        .pop(pop),
                        15.0,
                        Message::ToggleFavorite(track.clone()),
                    )
                }))
                .push(icon_button(Icon::More, 15.0, theme::text_secondary(), Message::OpenMenu(MenuTarget::Track(track.clone()))))
                .push((!compact && !local && app.settings.lock().show_ratings).then(|| stars(app, track.id, false)))
                .spacing(10)
                .align_y(Alignment::Center)
                .into()
        }
    };
    let bar = row![container(left).width(wing), controls(app, false, width)]
        .spacing(if compact { 10 } else { 18 })
        .align_y(Alignment::Center)
        .padding([0, 16])
        .height(theme::PLAYER_H);
    let palette = app.palette.now();
    let lights = app.now.as_ref().and_then(|n| n.track.cover(640)).and_then(|_| palette.try_into().ok());
    super::liquid_glass::glass(bar.width(Length::Fill), theme::PANEL_R - 1.0).player_bar(lights).into()
}

/// How long the volume's percentage stays up after it changed.
pub const VOLUME_TIP: std::time::Duration = std::time::Duration::from_millis(1100);

/// A thin slider: 4 px line, a knob `knob` px wide shown while hovered or
/// dragged, in a hit area `height` px tall (GPUI: progress 20 / 12, volume 14 / 10).
fn line_slider<'a>(
    value: f32,
    on_change: impl Fn(f32) -> Message + 'a,
    on_release: Message,
    dragging: bool,
    height: f32,
    knob: f32,
) -> iced::widget::Slider<'a, f32, Message> {
    slider(0.0..=1.0, value.clamp(0.0, 1.0), on_change).step(0.0005_f32).on_release(on_release).height(height).style(move |_, status| {
        let active = dragging || !matches!(status, slider::Status::Active);
        slider::Style {
            rail: slider::Rail {
                // Over the glass bar GPUI's opaque `active` greys vanish: an
                // ink of the text colour keeps the empty rail readable on any backdrop.
                backgrounds: (theme::text().into(), theme::ink(if active { 0.26 } else { 0.18 }).into()),
                width: 4.0,
                border: Border { radius: 2.0.into(), ..Default::default() },
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: if active { knob / 2.0 } else { 0.0 } },
                background: theme::text().into(),
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
        }
    })
}

/// Shared playback controls.
pub fn controls(app: &App, over_fullscreen: bool, width: f32) -> Element<'_, Message> {
    let compact = width < 1150.0;
    let wing = (width * 0.28).min(400.0);
    let duration = app.duration().max(0.0);
    let position =
        if app.drag == Some(Drag::Progress) { app.drag_value as f64 * duration } else { app.display_position().clamp(0.0, duration) };
    let frac = if duration > 0.0 { (position / duration) as f32 } else { 0.0 };
    let playing = app.now.is_some() && !app.snap.paused && !app.ended;
    // While music plays the ambient drift never comes to rest (`motion::AMBIENT_IDLE`).
    super::motion::note_playing(app.is_playing());
    let shuffle_color = if app.shuffle { theme::accent() } else { theme::text_secondary() };
    let repeat_icon = if app.repeat == Repeat::One { Icon::RepeatOne } else { Icon::Repeat };
    let repeat_color = if app.repeat == Repeat::Off { theme::text_secondary() } else { theme::accent() };
    let big = over_fullscreen;
    let pp = if big { 52.0 } else { 38.0 };
    // GPUI: play and pause dissolve into each other and the glyph sinks and
    // springs back on click (`haptics::press_spring`).
    let glyph =
        super::icons::glyph(if playing { Icon::PauseRound } else { Icon::PlayRound }, if big { 28.0 } else { 24.0 }, theme::button_text())
            .swap()
            .scale(|p| p.pulse);
    let play = button(container(glyph).center(pp))
        .padding(0)
        .style(move |_, _| {
            let phase = super::wash::phase();
            let fill = theme::lerp(theme::button(), theme::button_hover(), phase.hover);
            button::Style {
                background: Some(theme::lerp(fill, theme::button_pressed(), phase.press).into()),
                border: Border { radius: (pp / 2.0).into(), ..Default::default() },
                shadow: Shadow { color: Color { a: 0.25, ..Color::BLACK }, offset: Vector::new(0.0, 2.0), blur_radius: 8.0 },
                ..Default::default()
            }
        })
        .on_press(Message::TogglePlay);
    let play = super::wash::track(play).pulse();
    use super::widgets::transport_button;
    let transport = row![
        transport_button(Icon::Shuffle, 15.0, shuffle_color, Message::ToggleShuffle),
        transport_button(Icon::Prev, 18.0, theme::text(), Message::Prev),
        play,
        transport_button(Icon::Next, 18.0, theme::text(), Message::Next),
        transport_button(repeat_icon, 15.0, repeat_color, Message::CycleRepeat),
    ]
    .spacing(if big {
        18
    } else if compact {
        5
    } else {
        9
    })
    .align_y(Alignment::Center);
    let progress =
        mouse_area(line_slider(frac, Message::ProgressDrag, Message::ProgressRelease, app.drag == Some(Drag::Progress), 20.0, 12.0))
            .on_scroll(|delta| {
                let dy = match delta {
                    iced::mouse::ScrollDelta::Lines { y, .. } => y,
                    iced::mouse::ScrollDelta::Pixels { y, .. } => y / 40.0,
                };
                Message::SeekBy(5.0 * dy.signum() as f64)
            });
    let time = row![
        container(text(fmt_time(position)).size(10.5).color(theme::text_secondary())).width(34).align_x(alignment::Horizontal::Right),
        container(progress).width(Length::Fill),
        container(text(fmt_time(duration)).size(10.5).color(theme::text_secondary())).width(34),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let time = container(time).max_width(580);
    let center = if big {
        column![time, transport].spacing(12).align_x(Alignment::Center).width(Length::Fill)
    } else {
        column![transport, time].spacing(1).align_x(Alignment::Center).width(Length::Fill)
    };

    let vol = if app.muted { 0.0 } else { app.volume };
    let volume = mouse_area(
        container(line_slider(vol, Message::VolumeDrag, Message::VolumeRelease, app.drag == Some(Drag::Volume), 14.0, 10.0)).width(84),
    )
    .on_scroll(|delta| {
        let dy = match delta {
            iced::mouse::ScrollDelta::Lines { y, .. } => y,
            iced::mouse::ScrollDelta::Pixels { y, .. } => y / 12.0,
        };
        Message::VolumeStep(dy.signum() * 0.01)
    });
    // GPUI's bar toggles: the hover colour fades over the rest one (85 %),
    // the glyph grows 6 % under the pointer and sinks 9 % while held.
    let toggle = |i: Icon, on: bool, msg: Message| -> Element<'_, Message> {
        let g = super::icons::glyph(i, 16.0, if on { theme::accent() } else { theme::text_secondary() })
            .swap()
            .scale(|p| 1.0 + 0.06 * p.hover - 0.09 * p.press);
        let b = button(container(g).center(30)).padding(0).style(|_, _| button::Style::default()).on_press(msg);
        let rest = if on { theme::elevated() } else { theme::with_alpha(theme::elevated(), 0.0) };
        super::wash::surface(b, move |p| super::wash::Look {
            background: Some(super::wash::mix(rest, theme::hover(), p.hover * 0.85).into()),
            border: Border { radius: 8.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
    };
    // While the volume changes (dragged or scrolled) its percentage floats
    // over the slider in a tooltip's bubble, for `VOLUME_TIP` after the change.
    let show_pct = app.drag == Some(Drag::Volume) || app.volume_tip.is_some_and(|at| at.elapsed() < VOLUME_TIP);
    let volume: Element<'_, Message> = if show_pct {
        let pct = crate::i18n::text("format.percent", &[format!("{}", (vol * 100.0).round() as i32)]);
        // Over the slider's knob, kept inside the slider's width.
        let bubble = container(super::widgets::tip_bubble(pct)).center_x(Length::Fill).height(30).align_y(alignment::Vertical::Center);
        let bubble = iced::widget::float(bubble).translate(|_, _| Vector::new(0.0, -32.0));
        iced::widget::stack![container(volume).height(30).align_y(alignment::Vertical::Center), bubble].width(84).into()
    } else {
        container(volume).height(30).align_y(alignment::Vertical::Center).into()
    };
    // As GPUI, every toggle names itself in a tooltip.
    let right = row![
        super::widgets::tip(
            toggle(Icon::Queue, if big { app.np_tab == NpTab::Queue } else { app.show_queue }, Message::ToggleQueue),
            crate::i18n::t("queue"),
        ),
        super::widgets::tip(
            toggle(Icon::Lyrics, if big { app.np_tab == NpTab::Lyrics } else { app.show_lyrics }, Message::ToggleLyrics),
            crate::i18n::t("lyrics"),
        ),
        mouse_area(icon_button(
            if vol <= 0.001 { Icon::VolumeOff } else { Icon::VolumeUp },
            16.0,
            theme::text_secondary(),
            Message::ToggleMute
        ))
        .on_right_press(Message::OpenMenu(MenuTarget::Output)),
        volume,
        super::widgets::tip(
            toggle(if big { Icon::ExitFullscreen } else { Icon::Fullscreen }, big, Message::ToggleNowPlaying),
            if big { crate::i18n::t("exit_full_screen_esc") } else { crate::i18n::t("full_screen") },
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    if big {
        column![center, container(right).width(Length::Fill).align_x(alignment::Horizontal::Center)].spacing(16).width(Length::Fill).into()
    } else {
        row![container(center).width(Length::Fill), container(right).width(wing).align_x(alignment::Horizontal::Right)]
            .spacing(if compact { 10 } else { 18 })
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
    }
}
