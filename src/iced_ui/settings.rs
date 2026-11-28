//! Settings: audio, appearance, library and lyrics, connections, account,
//! about, local files and the equaliser. Same sections and settings as the
//! GPUI reference (`src/ui/settings.rs`, `equalizer.rs`).

use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{alignment, Alignment, Border, Color, Element, Length, Padding};

use super::icons::{icon, Icon};
use super::widgets::{self, switch};
use super::{theme, App, Message};
use crate::config::Quality;

pub const ABOUT_TAB: usize = 5;
/// Between the groups of a section: twice the 12 px between a group's
/// heading and its card, so each heading reads with the card under it.
// TODO(theme): `theme::SPACE_XL` once arte's scale is merged.
pub(super) const GROUP_GAP: f32 = 24.0;
/// Inset of a row's content inside a group card (GPUI: 4 px + 16 px).
pub(super) const ROW_X: f32 = 20.0;
pub const LOCAL_TAB: usize = 6;
pub const EQ_TAB: usize = 7;
impl App {
    pub fn set_light_theme(&mut self, light: bool) {
        let mut s = self.settings.lock();
        s.light_theme = light;
        s.save();
        theme::set_light(light);
    }

    /// The accent while the hue strip is dragged (saved when it is let go).
    pub(super) fn preview_accent(&mut self, rgb: u32) {
        theme::set_accent(rgb);
        self.settings.lock().accent = rgb;
    }

    pub fn set_accent(&mut self, rgb: u32) {
        theme::set_accent(rgb);
        let mut s = self.settings.lock();
        s.accent = rgb;
        s.save();
    }
}

/// Colour of hue `h` (0..1) for the custom accent: vivid, and light enough to
/// read on the dark theme.
pub fn hue_color(h: f32) -> u32 {
    let (s, l) = (0.85f32, 0.62f32);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(1.0) * 6.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u32;
    (to(r) << 16) | (to(g) << 8) | to(b)
}

/// The hue of a colour, 0..1.
fn hue_of(rgb: u32) -> f32 {
    let part = |shift: u32| ((rgb >> shift) & 0xff) as f32;
    let (r, g, b) = (part(16), part(8), part(0));
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max == min {
        return 0.0;
    }
    let d = max - min;
    let sector = if max == r {
        (g - b) / d
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (sector / 6.0).rem_euclid(1.0)
}

/// The custom accent's hue strip.
fn hue_strip<'a>(accent: u32) -> Element<'a, Message> {
    let mut rainbow = iced::gradient::Linear::new(iced::Degrees(90.0));
    for k in 0..=6 {
        let at = k as f32 / 6.0;
        rainbow = rainbow.add_stop(at, theme::rgb(hue_color(at)));
    }
    let strip = container(Space::new().width(180).height(10)).style(move |_| container::Style {
        background: Some(iced::Background::Gradient(rainbow.into())),
        border: Border { radius: 5.0.into(), ..Default::default() },
        ..Default::default()
    });
    let knob = iced::widget::slider(0.0..=0.999, hue_of(accent), Message::AccentHue)
        .step(0.001_f32)
        .on_release(Message::AccentSave)
        .width(180)
        .height(16)
        .style(|_, _| iced::widget::slider::Style {
            rail: iced::widget::slider::Rail {
                backgrounds: (iced::Color::TRANSPARENT.into(), iced::Color::TRANSPARENT.into()),
                width: 4.0,
                border: Border::default(),
            },
            handle: iced::widget::slider::Handle {
                shape: iced::widget::slider::HandleShape::Circle { radius: 7.0 },
                background: theme::accent().into(),
                border_width: 2.0,
                border_color: theme::text(),
            },
        });
    iced::widget::stack![container(strip).height(16).align_y(alignment::Vertical::Center), knob].into()
}

// ----------------------------------------------------------------- view pieces

fn row_text<'a>(label: impl text::IntoFragment<'a>, desc: Option<String>) -> Element<'a, Message> {
    column![text(label).size(13).font(widgets::semibold()).color(theme::text())]
        .push(desc.map(|d| text(d).size(12).line_height(text::LineHeight::Absolute(17.0.into())).color(theme::text_secondary())))
        .spacing(3)
        .width(Length::Fill)
        .into()
}

pub(super) fn setting<'a>(label: &'a str, desc: Option<String>, control: Element<'a, Message>) -> Element<'a, Message> {
    // GPUI: 4 px inset in the card, then 16 x 10 px inside the row.
    row![row_text(label, desc), control].spacing(12).align_y(Alignment::Center).padding([13.0, ROW_X]).into()
}

fn toggle_row<'a>(label: &'a str, desc: &'a str, on: bool, key: &'static str) -> Element<'a, Message> {
    // GPUI `toggle_row`: the whole row toggles and fades to ink 5 % under the
    // pointer. The switch is drawn only (a second button would toggle twice).
    let inner =
        row![row_text(label, Some(desc.to_string())), widgets::switch_face(on)].spacing(12).align_y(Alignment::Center).padding([10, 16]);
    let b = button(inner).width(Length::Fill).padding(0).style(widgets::still(11.0)).on_press(Message::ToggleSetting(key));
    container(super::wash::wash(b, theme::ink(0.05), 11.0)).padding([3, 4]).into()
}

fn action_row<'a>(label: &'a str, description: String, msg: Message) -> Element<'a, Message> {
    let arrow = widgets::soft_glyph(Icon::Forward, 15.0, theme::text_secondary());
    let body = row![row_text(label, Some(description)), arrow].spacing(12).align_y(Alignment::Center).padding([10, 16]);
    let b = button(body).padding(0).width(Length::Fill).style(widgets::still(11.0)).on_press(msg);
    container(super::wash::wash(b, theme::ink(0.05), 11.0)).padding([3, 4]).into()
}

pub(super) fn picker_menu_style(_: &iced::Theme) -> iced::overlay::menu::Style {
    iced::overlay::menu::Style {
        background: theme::popover().into(),
        border: Border { color: theme::popover_border(), width: 1.0, radius: 8.0.into() },
        text_color: theme::text(),
        selected_text_color: theme::text(),
        selected_background: theme::field().into(),
        shadow: theme::popover_shadow(),
    }
}

pub(super) fn group<'a>(i: Icon, t: &'a str, sub: &'a str, rows: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut body = column![].spacing(0);
    let n = rows.len();
    for (k, r) in rows.into_iter().enumerate() {
        body = body.push(r);
        if k + 1 < n {
            body = body.push(
                container(Space::new().width(Length::Fill).height(1))
                    .padding([0, 16])
                    .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() }),
            );
        }
    }
    column![
        widgets::panel_heading(i, t, (!sub.is_empty()).then(|| sub.to_string())),
        container(body).width(Length::Fill).style(|_| container::Style {
            background: Some(theme::ink(0.036).into()),
            border: Border { color: theme::ink(0.09), width: 1.0, radius: 16.0.into() },
            ..Default::default()
        }),
    ]
    .spacing(12)
    .into()
}

pub(super) fn small_button<'a>(label: impl text::IntoFragment<'a>, primary: bool, msg: Message) -> Element<'a, Message> {
    let b = button(container(text(label).size(12).font(widgets::semibold())).height(30).align_y(alignment::Vertical::Center))
        .padding([0, 13])
        .style(move |_, _| {
            let h = super::wash::phase().hover;
            button::Style {
                background: Some(
                    if primary { super::wash::mix(theme::button(), theme::button_hover(), h) } else { theme::ink(0.06 + 0.05 * h) }.into(),
                ),
                text_color: if primary { theme::button_text() } else { theme::text() },
                border: Border { color: if primary { theme::divider() } else { theme::ink(0.12) }, width: 1.0, radius: 15.0.into() },
                ..Default::default()
            }
        })
        .on_press(msg);
    super::wash::track(b).into()
}

fn section_summary(tab: usize) -> &'static str {
    let key = [
        "quality_sound_output_and_queue",
        "theme_accent_background_and_scale",
        "track_lists_and_lyrics_reading",
        "discord_privacy_and_terms",
        "your_tidal_profile_and_subscription",
        "version_licenses_and_credits",
        "local.settings_description",
        "eq.tab_description",
    ][tab];
    crate::i18n::t(key)
}

/// A read-only status chip (the summary under a section's title).
fn chip<'a>(label: String, accent: bool) -> Element<'a, Message> {
    let mut r = row![].spacing(6).align_y(Alignment::Center);
    if accent {
        r = r.push(container(Space::new().width(7).height(7)).style(|_| container::Style {
            background: Some(theme::accent().into()),
            border: Border { radius: 3.5.into(), ..Default::default() },
            ..Default::default()
        }));
    }
    r = r.push(widgets::line(label, 11.0, theme::text_secondary_on_glass()).font(widgets::semibold()));
    container(r)
        .height(24)
        .padding([0, 10])
        .align_y(alignment::Vertical::Center)
        .style(|_| container::Style {
            background: Some(theme::ink(0.06).into()),
            border: Border { color: theme::glass_rim(0.0), width: 1.0, radius: 12.0.into() },
            ..Default::default()
        })
        .into()
}

/// What is chosen in a section, as short chips under its title.
fn settings_chips(app: &App, tab: usize) -> Vec<(String, bool)> {
    let s = app.settings.lock().clone();
    match tab {
        0 => {
            let mut chips = vec![
                (s.quality.label().to_string(), true),
                (s.output_device.clone().unwrap_or_else(|| crate::i18n::t("system_default").into()), false),
            ];
            if app.snap.output_exclusive {
                chips.push((crate::i18n::t("exclusive_mode_wasapi").into(), false));
            }
            if s.normalize {
                chips.push((crate::i18n::t("normalize_volume").into(), false));
            }
            chips
        }
        1 => vec![
            ((if s.light_theme { crate::i18n::t("light") } else { crate::i18n::t("dark") }).into(), false),
            (format!("#{:06X}", theme::accent_rgb()), true),
            (
                crate::i18n::text("format.percent_spaced", &[format!("{}", (crate::shared::zoom::user_zoom() * 100.0).round() as i32)]),
                false,
            ),
        ],
        3 => {
            let mut chips = Vec::new();
            if s.discord {
                chips.push((crate::i18n::t("discord").to_string(), true));
            }
            if s.lastfm_session.is_some() {
                chips.push((crate::i18n::t("lastfm.title").to_string(), true));
            }
            chips
        }
        4 => s.username.clone().filter(|name| !name.trim().is_empty()).map(|name| (name, true)).into_iter().collect(),
        ABOUT_TAB => vec![(crate::i18n::text("app.version", &[env!("CARGO_PKG_VERSION").to_string()]), false)],
        _ => Vec::new(),
    }
}

fn sections() -> [(&'static str, Icon); 8] {
    [
        (crate::i18n::t("audio"), Icon::VolumeUp),
        (crate::i18n::t("appearance"), Icon::Sun),
        (crate::i18n::t("library_and_lyrics"), Icon::Library),
        (crate::i18n::t("privacy"), Icon::Link),
        (crate::i18n::t("account"), Icon::Artist),
        (crate::i18n::t("about"), Icon::Track),
        (crate::i18n::t("local_files"), Icon::Upload),
        (crate::i18n::t("eq.title"), Icon::Stats),
    ]
}

pub fn view(app: &App, width: f32) -> Element<'_, Message> {
    let tab = app.settings_tab.min(EQ_TAB);
    let groups: [(&str, &[usize]); 4] = [
        (crate::i18n::t("settings.group.listening"), &[0, EQ_TAB]),
        (crate::i18n::t("interface"), &[1, 2, LOCAL_TAB]),
        (crate::i18n::t("settings.group.connections"), &[3, 4]),
        (crate::i18n::t("settings.group.app"), &[ABOUT_TAB]),
    ];
    // GPUI: 42 px entries, the icon in a 28 px tile, 13 px semibold labels.
    let entry = |i: usize| -> Element<'_, Message> {
        let (label, ic) = sections()[i];
        let active = i == tab;
        // GPUI's settings tabs: the colour brightens with the eased hover and
        // the icon grows 7 % (sinks 10 % pressed); the selection glides.
        let tone = move |p: super::wash::Phase| {
            super::wash::mix(super::wash::mix(theme::text_secondary(), theme::text(), p.hover), theme::text(), p.active)
        };
        let glyph = super::icons::glyph(ic, 15.0, theme::text_secondary())
            .live(move |p, _| {
                super::wash::mix(super::wash::mix(theme::text_secondary(), theme::text(), p.hover), theme::accent(), p.active)
            })
            .scale(|p| 1.0 + 0.07 * p.hover - 0.1 * p.press);
        let tile = container(glyph).center(28).style(move |_| container::Style {
            background: Some(
                super::wash::faded({
                    let p = super::wash::phase();
                    super::wash::mix(theme::ink(0.05 + 0.03 * p.hover), theme::accent_dim(), p.active)
                })
                .into(),
            ),
            border: Border { radius: 8.0.into(), ..Default::default() },
            ..Default::default()
        });
        let name = widgets::line(label, 13.0, theme::text_secondary()).font(widgets::semibold()).live(move |_| tone(super::wash::phase()));
        let b = button(container(row![tile, name].spacing(10).align_y(Alignment::Center)).center_y(Length::Fill))
            .width(Length::Fill)
            .height(42)
            .padding([0, 8])
            .style(move |_, _| {
                let p = super::wash::phase();
                button::Style {
                    background: Some(theme::ink(0.07 * p.hover * (1.0 - p.active) + (0.10 + 0.03 * p.hover) * p.active).into()),
                    text_color: theme::text(),
                    border: Border { radius: theme::concentric(18.0, 11.0).into(), ..Default::default() },
                    ..Default::default()
                }
            })
            .on_press(Message::SettingsTab(i));
        super::wash::track(b).active(active).into()
    };
    let wide = width >= 900.0;
    let nav: Element<'_, Message> = if wide {
        // GPUI: a 236 px flat panel, 10 px inside, the groups' names in capitals.
        let mut nav = column![].spacing(2);
        for (g, (title, tabs)) in groups.into_iter().enumerate() {
            // On glass: the secondary level, not the dimmest (vibrancy, AA).
            let label = text(title.to_uppercase()).size(11).font(widgets::semibold()).color(theme::text_secondary());
            nav = nav.push(container(label).padding(iced::Padding {
                top: if g == 0 { 4.0 } else { 14.0 },
                bottom: 5.0,
                left: 10.0,
                right: 10.0,
            }));
            nav = nav.extend(tabs.iter().map(|&i| entry(i)));
        }
        // Navigation is structure, not a floating pane: the quiet surface of
        // the groups' cards, so the section's glass header is the one slab of
        // glass (two slabs side by side competed on the accent field).
        container(nav)
            .width(236)
            .padding(10)
            .style(|_| container::Style {
                background: Some(theme::ink(0.036).into()),
                border: Border { color: theme::ink(0.09), width: 1.0, radius: super::liquid_glass::PANEL_R.into() },
                ..Default::default()
            })
            .into()
    } else {
        let pills = groups.iter().flat_map(|(_, tabs)| tabs.iter().copied()).map(|i| {
            let (label, glyph) = sections()[i];
            widgets::pill(label, Some(glyph), i == tab, Some(Message::SettingsTab(i)))
        });
        row(pills).spacing(8).wrap().into()
    };
    let body = match tab {
        0 => audio(app),
        1 => appearance(app),
        2 => library(app),
        3 => connections(app),
        4 => account(app),
        ABOUT_TAB => about(app),
        LOCAL_TAB => super::local::view(app),
        _ => super::equalizer::view(app),
    };
    let (title, glyph) = sections()[tab];
    let badge = container(icon(glyph, 22.0, theme::accent())).center(48).style(|_| container::Style {
        background: Some(theme::accent_dim().into()),
        border: Border { color: theme::ink(0.08), width: 1.0, radius: 14.0.into() },
        ..Default::default()
    });
    let mut heading = column![
        text(title).size(22).font(widgets::bold()).color(theme::text()),
        text(section_summary(tab)).size(13).color(theme::text_secondary()),
    ]
    .spacing(4);
    let chips = settings_chips(app, tab);
    if !chips.is_empty() {
        heading = heading.push(
            container(row(chips.into_iter().map(|(label, accent)| chip(label, accent))).spacing(6).wrap())
                .padding(iced::Padding { top: 6.0, ..Default::default() }),
        );
    }
    // The section's header is a pane of glass, as every page header.
    let hero = super::liquid_glass::glass(
        container(row![badge, heading].spacing(16).align_y(Alignment::Center)).width(Length::Fill).padding([18, 20]),
        super::liquid_glass::PANEL_R,
    )
    .shadow(theme::popover_shadow());
    // GPUI: the 32 px title 22 px above the navigation and the section, which
    // sit 12 px apart, as do the hero and the section's groups.
    column![text(crate::i18n::t("settings")).size(32).font(widgets::bold()).color(theme::text()), {
        let page = column![hero, body].spacing(12).width(Length::Fill);
        let split: Element<'_, Message> = if wide { row![nav, page].spacing(12).into() } else { column![nav, page].spacing(12).into() };
        split
    },]
    .spacing(22)
    .padding(iced::Padding { top: 4.0, bottom: 12.0, ..Default::default() })
    .into()
}

fn audio(app: &App) -> Element<'_, Message> {
    let s = app.settings.lock().clone();
    let quality = |q: Quality, desc: &'static str| -> Element<'_, Message> {
        let selected = s.quality == q;
        // The accent, as every other selection (gold stays the MAX badge's).
        let ring = if selected { theme::accent() } else { theme::ink(0.28) };
        let inner = container(Space::new().width(8).height(8)).style(move |_| container::Style {
            background: selected.then(|| theme::accent().into()),
            border: Border { radius: 4.0.into(), ..Default::default() },
            ..Default::default()
        });
        let dot = container(inner).center(18).style(move |_| container::Style {
            background: Some(if selected { theme::accent_dim() } else { theme::field() }.into()),
            border: Border { color: ring, width: 2.0, radius: 9.0.into() },
            ..Default::default()
        });
        let mut r = row![dot, row_text(q.label(), Some(desc.to_string()))].spacing(14).align_y(Alignment::Center).padding([13.0, ROW_X]);
        if q == Quality::Low {
            r = r.push(small_button(crate::i18n::t("aac_320_kbps"), false, Message::SetQuality(q)));
        }
        widgets::soft(button(r).padding(0).width(Length::Fill).on_press(Message::SetQuality(q)), 10.0)
    };
    let quality_rows = vec![
        quality(Quality::Low, crate::i18n::t("balance_audio_quality_and_data_usage")),
        quality(Quality::High, Quality::High.description()),
        quality(Quality::Max, Quality::Max.description()),
    ];
    let (exclusive, force) = (s.exclusive_mode, s.force_volume);
    // As GPUI: a pill with the device and a chevron; the list unfolds in the card.
    let device_label = s.output_device.clone().unwrap_or_else(|| crate::i18n::t("system_default").to_string());
    let open = app.output_list_open;
    let pick = row![
        container(widgets::line(device_label, 12.0, theme::text())).max_width(280),
        super::icons::icon(if open { Icon::ChevUp } else { Icon::ChevDown }, 12.0, theme::text_secondary()),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let device = button(container(pick).height(30).align_y(alignment::Vertical::Center))
        .padding([0, 13])
        .style(move |_, _| {
            let h = if open { 1.0 } else { super::wash::phase().hover };
            button::Style {
                background: Some(theme::ink(0.06 + 0.05 * h).into()),
                text_color: theme::text(),
                border: Border { color: theme::ink(0.12), width: 1.0, radius: 15.0.into() },
                ..Default::default()
            }
        })
        .on_press(Message::ToggleOutputList);
    let device_list: Option<Element<'_, Message>> = open.then(|| {
        let current = s.output_device.clone();
        let entries = std::iter::once(None).chain(app.devices.iter().cloned().map(Some)).map(|d| {
            let selected = d == current;
            let label = d.clone().unwrap_or_else(|| crate::i18n::t("system_default").to_string());
            let body = row![
                super::icons::icon(Icon::VolumeUp, 14.0, if selected { theme::accent() } else { theme::text_dim() }),
                widgets::line(label, theme::TYPE_LABEL, theme::text()),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .padding([8.0, ROW_X - 10.0]);
            let b = button(body)
                .width(Length::Fill)
                .padding(0)
                .style(move |_, _| button::Style {
                    background: selected.then(|| theme::hover().into()),
                    border: Border { radius: 6.0.into(), ..Default::default() },
                    ..Default::default()
                })
                .on_press(Message::SetOutput(d, exclusive, force));
            super::wash::wash(b, theme::field(), 6.0).into()
        });
        container(iced::widget::Column::with_children(entries)).padding(Padding { left: 10.0, right: 10.0, bottom: 8.0, top: 0.0 }).into()
    });
    let status = if app.snap.output.is_empty() {
        crate::i18n::t("current_output_opens_on_playback").to_string()
    } else if app.snap.output_rate > 0 {
        crate::i18n::text("current_output_khz", &[app.snap.output.clone(), format!("{}", app.snap.output_rate as f32 / 1000.0)])
    } else {
        crate::i18n::text("current_output", std::slice::from_ref(&app.snap.output))
    };
    let output_rows = vec![
        toggle_row(
            crate::i18n::t("normalize_volume"),
            crate::i18n::t("match_track_volumes_using_tidal_gain_data"),
            s.normalize,
            "normalize",
        ),
        setting(
            crate::i18n::t("sound_output"),
            Some(crate::i18n::t("output.right_click_tip").to_string()),
            super::wash::track(device).into(),
        ),
    ];
    let mut output_rows = output_rows;
    if let Some(list) = device_list {
        output_rows.push(list);
    }
    output_rows.extend([
        toggle_row(
            crate::i18n::t("exclusive_mode_wasapi"),
            crate::i18n::t("audio.exclusive_description"),
            s.exclusive_mode,
            "exclusive_mode",
        ),
        toggle_row(crate::i18n::t("force_volume"), crate::i18n::t("audio.force_volume_description"), s.force_volume, "force_volume"),
        // The output actually open, quietly at the end (GPUI).
        container(text(status).size(11).color(theme::text_dim())).padding([10.0, ROW_X]).into(),
    ]);
    let queue =
        vec![toggle_row(crate::i18n::t("autoplay"), crate::i18n::t("play_similar_songs_when_the_queue_ends"), s.autoplay, "autoplay")];
    column![
        group(Icon::Track, crate::i18n::t("audio_quality"), crate::i18n::t("streaming_format_and_quality"), quality_rows),
        group(Icon::VolumeUp, crate::i18n::t("audio_output"), crate::i18n::t("device_volume_and_exclusive_mode"), output_rows),
        group(Icon::Play, crate::i18n::t("playback"), crate::i18n::t("queue_behavior"), queue),
    ]
    .spacing(GROUP_GAP)
    .into()
}

fn appearance<'a>(app: &'a App) -> Element<'a, Message> {
    let s = app.settings.lock().clone();
    // GPUI: a small preview of each theme (frame, a heading, a panel).
    let theme_swatch = |label: &'a str, light: bool| {
        let selected = s.light_theme == light;
        let (bg, frame, ink) = if light { (0xf6f6f8, theme::LIGHT_FRAME, 0x14141a) } else { (0x000000, theme::DARK_FRAME, 0xffffff) };
        let fill = move |rgb: u32, r: f32| {
            move |_: &iced::Theme| container::Style {
                background: Some(theme::rgb(rgb).into()),
                border: Border { radius: r.into(), ..Default::default() },
                ..Default::default()
            }
        };
        let preview = row![
            container(Space::new().width(14).height(Length::Fill)).style(fill(frame, 0.0)),
            column![
                container(Space::new().width(20).height(3)).style(fill(ink, 1.5)),
                container(Space::new().width(Length::Fill).height(12)).style(fill(frame, 2.0)),
            ]
            .spacing(4)
            .padding(5),
        ];
        thumbnail(container(preview).style(fill(bg, 0.0)).into(), label, selected, theme::ink(0.12), Some(Message::SetLight(light)))
    };
    let theme_row = setting(
        crate::i18n::t("theme"),
        Some(crate::i18n::t("appearance.theme_description").to_string()),
        row![theme_swatch(crate::i18n::t("dark"), false), theme_swatch(crate::i18n::t("light"), true)].spacing(12).into(),
    );
    let accents = row(theme::accent_presets().into_iter().map(|(rgb, name)| {
        let active = s.accent == rgb;
        let swatch = button(Space::new().width(22).height(22))
            .padding(0)
            .style(move |_, _| button::Style {
                background: Some(theme::rgb(rgb).into()),
                border: Border {
                    color: if active { theme::text() } else { theme::ink(0.10 + 0.25 * super::wash::phase().hover) },
                    width: 2.0,
                    radius: 11.0.into(),
                },
                ..Default::default()
            })
            .on_press(Message::SetAccent(rgb));
        widgets::tip(super::wash::track(swatch), name)
    }))
    .spacing(8);
    let custom = !theme::accent_presets().iter().any(|(c, _)| *c == s.accent);
    let reset = (s.accent != theme::DEFAULT_ACCENT).then(|| {
        let label = widgets::line(crate::i18n::t("reset"), 11.0, theme::text_secondary()).brighten_on_hover(theme::text());
        super::wash::track(button(label).padding(0).style(widgets::still(0.0)).on_press(Message::SetAccent(theme::DEFAULT_ACCENT)))
    });
    let picker = row![hue_strip(s.accent)]
        .push(custom.then(|| text(crate::i18n::t("custom")).size(11).color(theme::text_dim())))
        .push(reset)
        .spacing(10)
        .align_y(Alignment::Center);
    let accent_row = setting(
        crate::i18n::t("accent_color"),
        Some(crate::i18n::t("appearance.accent_description").to_string()),
        column![accents, picker].spacing(10).align_x(alignment::Horizontal::Right).into(),
    );
    // GPUI: every language as a small button, wrapping under the label.
    let chips = std::iter::once((None, crate::i18n::t("system")))
        .chain(crate::i18n::LANGUAGES.iter().map(|(code, name)| (Some(*code), *name)))
        .map(|(code, name)| small_button(name, s.language.as_deref() == code, Message::SetLanguage(code.map(str::to_string))));
    let lang_row: Element<'_, Message> = column![
        row_text(crate::i18n::t("language"), Some(crate::i18n::t("interface_language").to_string())),
        iced::widget::Row::with_children(chips).spacing(6).wrap().vertical_spacing(6),
    ]
    .spacing(10)
    .padding([13.0, ROW_X])
    .into();
    let zoom = crate::shared::zoom::user_zoom();
    let zoom_row = setting(
        crate::i18n::t("interface_zoom"),
        Some(crate::i18n::t("also_with_ctrl_ctrl_ctrl_0_or_ctrl_wheel").to_string()),
        row![
            small_button("−", false, Message::StepZoom(-1)),
            text(crate::i18n::text("format.percent_spaced", &[format!("{}", (zoom * 100.0).round() as i32)]))
                .size(13)
                .width(56)
                .align_x(alignment::Horizontal::Center),
            small_button("+", false, Message::StepZoom(1)),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
    );
    // GPUI: the playing track's colour, and its cover blurred (here its
    // colours flowing into each other, which is what the blur shows).
    let ambient = app.ambient.now();
    let palette = app.palette.now();
    let color_preview = container(Space::new().width(Length::Fill).height(Length::Fill))
        .style(move |_| container::Style { background: Some(ambient.into()), ..Default::default() });
    let blur_preview = container(Space::new().width(Length::Fill).height(Length::Fill)).style(move |_| {
        let background = match palette.as_slice() {
            [a, b, c, ..] if app.now.is_some() => {
                iced::gradient::Linear::new(0.6).add_stop(0.0, *a).add_stop(0.55, *b).add_stop(1.0, *c).into()
            }
            _ => theme::control().into(),
        };
        container::Style { background: Some(background), ..Default::default() }
    });
    let set_blur = |blur: bool| (s.np_blur != blur).then_some(Message::ToggleSetting("np_blur"));
    let bg_row = setting(
        crate::i18n::t("player_background"),
        Some(crate::i18n::t("choose_the_full_screen_player_background").to_string()),
        row![
            thumbnail(color_preview.into(), crate::i18n::t("color"), !s.np_blur, Color::TRANSPARENT, set_blur(false)),
            thumbnail(blur_preview.into(), crate::i18n::t("blur"), s.np_blur, Color::TRANSPARENT, set_blur(true)),
        ]
        .spacing(12)
        .into(),
    );
    let welcome = setting(
        crate::i18n::t("welcome.repeat"),
        Some(crate::i18n::t("welcome.repeat_help").to_string()),
        small_button(crate::i18n::t("welcome.open"), false, Message::WelcomeRepeat),
    );
    column![
        group(
            Icon::Sun,
            crate::i18n::t("interface"),
            crate::i18n::t("theme_accent_and_control_size"),
            vec![lang_row, theme_row, accent_row, zoom_row, welcome]
        ),
        group(Icon::Fullscreen, crate::i18n::t("full_screen"), crate::i18n::t("playback_background"), vec![bg_row]),
    ]
    .spacing(GROUP_GAP)
    .into()
}

/// A 52 x 36 preview with a 2 px frame (the text colour when chosen) over a
/// bold label: GPUI's theme and player background swatches.
fn thumbnail<'a>(preview: Element<'a, Message>, label: &'a str, selected: bool, idle: Color, msg: Option<Message>) -> Element<'a, Message> {
    let frame = container(
        container(preview)
            .width(48)
            .height(32)
            .clip(true)
            .style(|_| container::Style { border: Border { radius: 6.0.into(), ..Default::default() }, ..Default::default() }),
    )
    .padding(2)
    .style(move |_| container::Style {
        border: Border { color: if selected { theme::text() } else { idle }, width: 2.0, radius: 8.0.into() },
        ..Default::default()
    });
    let label = text(label).size(11).font(widgets::bold()).color(if selected { theme::text() } else { theme::text_secondary() });
    let body = column![frame, label].spacing(6).align_x(Alignment::Center);
    super::wash::track(button(body).padding(0).style(widgets::still(8.0)).on_press_maybe(msg)).into()
}

fn library(app: &App) -> Element<'_, Message> {
    let s = app.settings.lock().clone();
    column![
        group(
            Icon::Library,
            crate::i18n::t("library_and_lists"),
            "",
            vec![
                toggle_row(
                    crate::i18n::t("audio_metadata"),
                    crate::i18n::t("show_extra_fields_e_g_bpm_in_track_lists"),
                    s.audio_metadata,
                    "audio_metadata"
                ),
                toggle_row(
                    crate::i18n::t("ratings"),
                    crate::i18n::t("show_the_star_column_in_track_lists"),
                    s.show_ratings,
                    "show_ratings"
                ),
                toggle_row(
                    crate::i18n::t("hide_uploads"),
                    crate::i18n::t("remove_uploads_from_mixes_new_releases"),
                    s.hide_uploads,
                    "hide_uploads"
                ),
            ]
        ),
        group(
            Icon::Lyrics,
            crate::i18n::t("lyrics"),
            "",
            vec![
                toggle_row(
                    crate::i18n::t("lyrics_from_other_providers"),
                    crate::i18n::t("lyrics.external_privacy"),
                    s.external_lyrics,
                    "external_lyrics"
                ),
                toggle_row(
                    crate::i18n::t("lyrics_pronunciation"),
                    crate::i18n::t("lyrics.romanization_description"),
                    s.lyrics_romanize,
                    "lyrics_romanize"
                ),
            ]
        ),
    ]
    .spacing(GROUP_GAP)
    .into()
}

fn connections(app: &App) -> Element<'_, Message> {
    let s = app.settings.lock().clone();
    let discord_id = text_input(crate::i18n::t("discord_application_id"), &s.discord_client_id)
        .on_input(Message::DiscordId)
        .size(13)
        .padding([7, 10])
        .width(220)
        .style(super::pages::field_style);
    let connect = vec![
        toggle_row(crate::i18n::t("discord"), crate::i18n::t("discord.presence_description"), s.discord, "discord"),
        setting(
            crate::i18n::t("discord_application_id"),
            Some(crate::i18n::t("discord.application_required").to_string()),
            discord_id.into(),
        ),
    ];
    let phase = app.scrobbler.status.lock().phase;
    let connected = app.lastfm_connected();
    let action = match (phase, connected) {
        (crate::scrobble::worker::Phase::Connecting, _) => small_button(crate::i18n::t("cancel"), false, Message::LastfmCancel),
        (_, true) => small_button(crate::i18n::t("lastfm.disconnect"), false, Message::LastfmDisconnect),
        (_, false) => small_button(crate::i18n::t("lastfm.connect"), true, Message::LastfmConnect),
    };
    let mut lastfm = vec![setting(crate::i18n::t("lastfm.title"), Some(app.lastfm_status_text()), action)];
    if connected {
        lastfm.push(setting(
            crate::i18n::t("lastfm.scrobble"),
            Some(crate::i18n::t("lastfm.scrobble_help").to_string()),
            switch(s.lastfm_enabled, Message::LastfmToggle),
        ));
        let sync = if app.scrobbler.status.lock().phase == crate::scrobble::worker::Phase::Syncing {
            small_button(crate::i18n::t("cancel"), false, Message::LastfmCancel)
        } else {
            small_button(crate::i18n::t("lastfm.sync"), false, Message::LastfmSync)
        };
        lastfm.push(setting(crate::i18n::t("lastfm.sync"), Some(crate::i18n::t("lastfm.sync_help").to_string()), sync));
    }
    let link = |label: &'static str, url: &'static str| setting(label, None, small_button("↗", false, Message::OpenUrl(url.to_string())));
    column![
        group(Icon::Link, crate::i18n::t("integrations"), "", connect),
        group(Icon::Link, crate::i18n::t("lastfm.title"), crate::i18n::t("lastfm.description"), lastfm),
        group(
            Icon::Link,
            crate::i18n::t("privacy_and_terms"),
            "",
            vec![
                link(crate::i18n::t("terms_and_conditions"), "https://tidal.com/terms"),
                link(crate::i18n::t("privacy_notice"), "https://tidal.com/privacy")
            ]
        ),
    ]
    .spacing(GROUP_GAP)
    .into()
}

fn account(app: &App) -> Element<'_, Message> {
    // As GPUI's `settings_account`: profile card, personal details,
    // subscription, then security and session.
    let s = app.settings.lock().clone();
    let service = crate::i18n::t("tidal");
    let profile_url = "https://tidal.com/profile";
    let account_url = "https://account.tidal.com";
    let name =
        s.username.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| crate::i18n::text("your_account", &[service.to_string()]));
    let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
    let avatar: Element<'_, Message> = match s.user_picture.as_deref() {
        Some(picture) => widgets::cover(Some(crate::tidal::image_url(picture, 160)), 48.0, true),
        None => container(text(initial).size(18).font(widgets::bold()).color(theme::text()))
            .center(48)
            .style(|_| container::Style {
                background: Some(theme::control().into()),
                border: Border { radius: 24.0.into(), ..Default::default() },
                ..Default::default()
            })
            .into(),
    };
    let profile_card: Element<'_, Message> = row![
        avatar,
        container(widgets::line(name.clone(), 14.0, theme::text())).width(Length::Fill),
        small_button(crate::i18n::t("edit_profile"), false, Message::OpenUrl(profile_url.into())),
    ]
    .spacing(14)
    .align_y(Alignment::Center)
    .padding([14.0, ROW_X])
    .into();
    let manage = || small_button(crate::i18n::t("manage"), false, Message::OpenUrl(account_url.into()));
    let personal = vec![
        setting(crate::i18n::t("name"), None, manage()),
        setting(
            crate::i18n::t("email"),
            Some(s.user_email.clone().unwrap_or_else(|| crate::i18n::t("unavailable").into())),
            Space::new().into(),
        ),
    ];
    let subscription = vec![
        setting(crate::i18n::t("current_subscription"), Some(service.to_string()), manage()),
        setting(crate::i18n::t("username"), Some(name), Space::new().into()),
    ];
    let link = |key: &'static str| -> Element<'_, Message> {
        let arrow = widgets::soft_glyph(Icon::Forward, 14.0, theme::text_secondary());
        let body = row![row_text(crate::i18n::t(key), None), arrow].spacing(12).align_y(Alignment::Center).padding([10, 16]);
        let b = button(body).padding(0).width(Length::Fill).style(widgets::still(11.0)).on_press(Message::OpenUrl(account_url.into()));
        container(super::wash::wash(b, theme::ink(0.05), 11.0)).padding([3, 4]).into()
    };
    let session = if app.signed_in {
        setting(
            crate::i18n::t("sign_out"),
            Some(crate::i18n::t("remove_the_session_saved_on_this_computer").to_string()),
            small_button(crate::i18n::t("sign_out"), true, Message::OpenDialog(super::Dialog::SignOut)),
        )
    } else {
        setting(crate::i18n::t("account"), None, small_button(crate::i18n::t("sign_in_to_tidal"), true, Message::StartLogin))
    };
    let more = vec![link("notification_settings"), link("change_password"), session];
    // Lossless: TIDAL serves FLAC only to sessions signed in on the web (Android client).
    let mut lossless = vec![setting(
        crate::i18n::t("lossless.title"),
        Some(crate::i18n::t("lossless.desc").to_string()),
        small_button(crate::i18n::t("lossless.start"), true, Message::LosslessStart),
    )];
    if app.lossless_login.is_some() {
        let field = text_input(crate::i18n::t("lossless.paste"), &app.lossless_paste)
            .on_input(Message::LosslessPaste)
            .on_submit(Message::LosslessFinish)
            .padding([8, 10])
            .size(13)
            .width(Length::Fill);
        lossless.push(
            row![field, small_button(crate::i18n::t("lossless.finish"), true, Message::LosslessFinish)]
                .spacing(10)
                .align_y(Alignment::Center)
                .padding([10.0, ROW_X])
                .into(),
        );
    }
    match &app.lossless_status {
        Some(Ok(())) => lossless.push(setting(crate::i18n::t("lossless.done"), None, Space::new().into())),
        Some(Err(e)) => lossless.push(setting(crate::i18n::t("lossless.failed"), Some(e.clone()), Space::new().into())),
        None => {}
    }
    column![
        group(Icon::Artist, crate::i18n::t("profile"), "", vec![profile_card]),
        group(Icon::Edit, crate::i18n::t("personal_details"), crate::i18n::t("your_tidal_account_information"), personal),
        group(Icon::Settings, crate::i18n::t("subscription"), "", subscription),
        group(Icon::Settings, crate::i18n::t("lossless.group"), "", lossless),
        group(Icon::SignOut, crate::i18n::t("security_and_session"), "", more),
    ]
    .spacing(GROUP_GAP)
    .into()
}

fn about(app: &App) -> Element<'_, Message> {
    let update_desc = match &app.update_check {
        None => crate::i18n::t("about.update_idle").to_string(),
        Some(crate::shared::Load::Loading) => crate::i18n::t("about.update_checking").to_string(),
        Some(crate::shared::Load::Ready(Some(v))) => crate::i18n::text("about.update_newer", std::slice::from_ref(v)),
        Some(crate::shared::Load::Ready(None)) => crate::i18n::text("about.update_latest", &[env!("CARGO_PKG_VERSION").to_string()]),
        Some(_) => crate::i18n::t("about.update_failed").to_string(),
    };
    let header = row![
        iced::widget::image(super::images::logo()).width(56).height(56),
        // As GPUI: the version as the title, what the app is under it.
        row_text(
            crate::i18n::text("app.version", &[env!("CARGO_PKG_VERSION").to_string()]),
            Some(crate::i18n::t("unofficial_tidal_client_built_with_rust_and_iced").to_string()),
        ),
    ]
    .spacing(16)
    .align_y(Alignment::Center)
    .padding([16.0, ROW_X]);
    let data_dir = crate::config::config_dir().display().to_string();
    let help = vec![
        action_row(crate::i18n::t("check_for_updates"), update_desc, Message::CheckUpdates),
        action_row(
            crate::i18n::t("about.report_problem"),
            crate::i18n::t("about.report_problem_desc").to_string(),
            Message::OpenUrl("https://github.com/RaunaCO/Abyss/issues/new".into()),
        ),
        action_row(crate::i18n::t("about.open_data_folder"), data_dir.clone(), Message::OpenUrl(data_dir)),
        action_row(crate::i18n::t("about.export_data"), crate::i18n::t("about.export_desc").to_string(), Message::ExportData),
    ];
    let credits: Vec<Element<'_, Message>> = [
        ("Iced 0.14 + wgpu 27", crate::i18n::t("mit_apache_2_0_interface")),
        (crate::i18n::t("credits.symphonia"), crate::i18n::t("mpl_2_0_flac_and_aac_decoding")),
        (crate::i18n::t("credits.rubato"), crate::i18n::t("mit_sinc_resampling")),
        (crate::i18n::t("credits.audio_output"), crate::i18n::t("apache_2_0_mit_audio_output")),
        (crate::i18n::t("credits.network"), crate::i18n::t("credits.mit_apache")),
        (crate::i18n::t("credits.lofty"), crate::i18n::t("credits.mit_apache")),
        (crate::i18n::t("credits.lindera"), crate::i18n::t("mit_ipadic_license_kanji_readings_romaji")),
        (crate::i18n::t("credits.pinyin"), crate::i18n::t("mit_chinese_pinyin")),
        (crate::i18n::t("credits.inter"), crate::i18n::t("sil_ofl_1_1_typeface")),
        (crate::i18n::t("credits.icons"), crate::i18n::t("apache_2_0_icons")),
    ]
    .into_iter()
    .map(|(n, l)| setting(n, Some(l.to_string()), Space::new().into()))
    .collect();
    let independent =
        setting(crate::i18n::t("independent_project"), Some(crate::i18n::t("about.unofficial_notice").to_string()), Space::new().into());
    column![
        group(Icon::Track, crate::i18n::t("installed_version"), "", vec![header.into()]),
        group(Icon::Link, crate::i18n::t("about.help_and_data"), "", help),
        group(Icon::List, crate::i18n::t("licenses_and_credits"), crate::i18n::t("about.components"), credits),
        group(Icon::Link, crate::i18n::t("about_the_project"), "", vec![independent]),
    ]
    .spacing(GROUP_GAP)
    .into()
}
