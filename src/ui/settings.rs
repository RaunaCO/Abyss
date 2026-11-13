//! Settings organized into the same rounded widgets as the listening pages.

use super::sc;
use gpui::{div, prelude::*, AnyElement, Context, ElementId, FontWeight, SharedString};

use super::icons::Icon;
use super::widgets::{cover, widget_panel, PANEL_GAP};
use super::{theme, AppView};
use crate::config::Quality;

pub(super) const ABOUT_TAB: usize = 5;
pub(super) const LOCAL_TAB: usize = 6;
pub(super) const EQ_TAB: usize = 7;

pub(super) fn group(icon: Icon, title: &'static str, description: &'static str, rows: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .child(div().pb(sc(12.)).flex().items_center().child(super::widgets::panel_heading(
            icon,
            title,
            (!description.is_empty()).then(|| description.into()),
        )))
        .child(div().rounded(sc(13.)).bg(theme::ink(0.036)).border_1().border_color(theme::panel_border()).flex().flex_col().children(
            rows.into_iter().enumerate().map(|(index, row)| {
                div().px(sc(ROW_INSET)).py(sc(3.)).when(index > 0, |d| d.border_t_1().border_color(theme::divider())).child(row)
            }),
        ))
        .into_any_element()
}

fn row_text(title: impl Into<SharedString>, desc: Option<SharedString>) -> AnyElement {
    div()
        .flex_1()
        .min_w(sc(0.))
        .flex()
        .flex_col()
        .gap(sc(3.))
        .child(div().text_size(sc(13.)).font_weight(FontWeight::SEMIBOLD).child(title.into()))
        .when_some(desc, |d, t| d.child(div().text_size(sc(11.5)).line_height(sc(17.)).text_color(theme::text_secondary()).child(t)))
        .into_any_element()
}

/// Gap between a card's edge and the highlight of the rows inside it.
const ROW_INSET: f32 = 4.0;

pub(super) fn row(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    desc: Option<SharedString>,
    right: Option<AnyElement>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px(sc(super::widgets::PANEL_PAD - ROW_INSET))
        .py(sc(14. - ROW_INSET))
        .rounded(sc(10.))
        .flex()
        .items_center()
        .gap(sc(12.))
        .child(row_text(title, desc))
        .children(right)
}

pub(super) fn switch(on: bool, key: &str) -> AnyElement {
    // The knob slides and the track fades between colours.
    let v = super::interact::follow(&format!("sw-{key}"), if on { 1.0 } else { 0.0 }, super::motion::seconds(super::motion::CONTROL));
    div()
        .flex_none()
        .w(sc(40.))
        .h(sc(22.))
        .rounded(sc(11.))
        .p(sc(2.))
        .bg(super::interact::mix(theme::switch_colors().0, theme::switch_colors().1, v))
        .flex()
        .child(div().ml(sc(18. * v)).size(sc(18.)).rounded(sc(9.)).bg(super::interact::mix(
            theme::switch_colors().2,
            theme::switch_colors().3,
            v,
        )))
        .into_any_element()
}

pub(super) fn small_button(id: impl Into<ElementId>, label: impl Into<SharedString>, white: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .min_w(sc(0.))
        .h(sc(30.))
        .px(sc(12.))
        .flex()
        .items_center()
        .gap(sc(6.))
        .rounded(sc(15.))
        .relative()
        .border_1()
        .border_color(if white { theme::divider() } else { theme::glass_rim(0.) })
        .cursor_pointer()
        .text_size(sc(11.))
        .font_weight(FontWeight::BOLD)
        .bg(if white { theme::button().into() } else { theme::liquid_glass(0.) })
        .text_color(if white { theme::button_text() } else { theme::text() })
        .hover(|s| s.opacity(0.9).shadow_md())
        .active(|s| s.opacity(0.65))
        .child(div().min_w(sc(0.)).truncate().child(label.into()))
}

impl AppView {
    pub(super) fn toggle_row(
        &mut self,
        id: &'static str,
        title: &'static str,
        desc: &'static str,
        on: bool,
        f: impl Fn(&mut AppView, &mut Context<AppView>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = format!("srow-{id}");
        let h = super::interact::hover(&k);
        let el = row(id, title, Some(desc.into()), Some(switch(on, id)))
            .cursor_pointer()
            .bg(theme::ink(0.05 * h))
            .on_click(cx.listener(move |this, _, _, cx| f(this, cx)));
        super::interact::track(el, k, cx).into_any_element()
    }

    fn link_row(&mut self, id: &'static str, title: &'static str, url: &'static str, cx: &mut Context<Self>) -> AnyElement {
        row(id, title, None, Some(Icon::Forward.el(14., theme::text_secondary()).into_any_element()))
            .cursor_pointer()
            .hover(|s| s.bg(theme::ink(0.05)))
            .on_click(cx.listener(move |_, _, _, _| {
                let _ = open::that(url);
            }))
            .into_any_element()
    }

    pub fn set_light_theme(&mut self, light: bool, cx: &mut Context<Self>) {
        {
            let mut settings = self.settings.lock();
            if settings.light_theme == light {
                return;
            }
            settings.light_theme = light;
            settings.save();
        }
        theme::set_light(light);
        cx.notify();
    }

    pub fn settings_panel(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let tab = self.settings_tab;
        let wide = width >= 830.;
        let body_width = if wide { width - 210. - PANEL_GAP } else { width };
        let sections = [
            (crate::i18n::t("audio"), Icon::VolumeUp, crate::i18n::t("quality_sound_output_and_queue")),
            (crate::i18n::t("appearance"), Icon::Sun, crate::i18n::t("theme_accent_background_and_scale")),
            (crate::i18n::t("library_and_lyrics"), Icon::Library, crate::i18n::t("track_lists_and_lyrics_reading")),
            (crate::i18n::t("privacy"), Icon::Link, crate::i18n::t("discord_privacy_and_terms")),
            (crate::i18n::t("account"), Icon::Artist, crate::i18n::t("your_tidal_profile_and_subscription")),
            (crate::i18n::t("about"), Icon::Track, crate::i18n::t("version_licenses_and_credits")),
            (crate::i18n::t("local_files"), Icon::Upload, crate::i18n::t("local.settings_description")),
            (crate::i18n::t("eq.title"), Icon::Stats, crate::i18n::t("eq.tab_description")),
        ];
        let tabs = widget_panel("settings-navigation")
            .flex()
            .flex_none()
            .p(sc(8.))
            .gap(sc(4.))
            .when(wide, |d| d.w(sc(210.)).flex_col())
            .when(!wide, |d| d.w_full().flex_wrap())
            .children(sections.into_iter().enumerate().map(|(i, (label, icon, _))| {
                let selected = i == tab;
                let key = format!("settings-tab-{i}");
                let hover = super::interact::hover(&key);
                let press = super::interact::press(&key);
                let color = super::interact::mix(theme::text_secondary(), theme::text(), hover);
                let button = div()
                    .id(("settings-tab", i))
                    .h(sc(40.))
                    .px(sc(11.))
                    .flex()
                    .items_center()
                    .gap(sc(9.))
                    .rounded(sc(11.))
                    .border_1()
                    .border_color(if selected { theme::glass_rim(0.3) } else { theme::ink(0.) })
                    .bg(theme::ink(if selected { 0.10 + 0.03 * hover } else { 0.07 * hover }))
                    .cursor_pointer()
                    .text_size(sc(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if selected { theme::text() } else { color })
                    .child(icon.el_scaled(16., if selected { theme::accent() } else { color }, 1. + 0.07 * hover - 0.1 * press))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.settings_tab != i {
                            this.settings_tab = i;
                            this.settings_dev_open = false;
                            this.content_changed_pub();
                            cx.notify();
                        }
                    }));
                super::interact::track(button, key, cx)
            }));
        let body = match tab {
            1 => self.settings_appearance(body_width, cx),
            2 => self.settings_library(cx),
            3 => self.settings_privacy(cx),
            4 => self.settings_account(cx),
            ABOUT_TAB => self.settings_about(),
            LOCAL_TAB => self.settings_local(cx),
            EQ_TAB => self.settings_equalizer(cx),
            _ => self.settings_audio(body_width, cx),
        };
        let (title, _, description) = sections[tab];
        div()
            .w_full()
            .pt(sc(22.))
            .pb(sc(12.))
            .flex()
            .flex_col()
            .gap(sc(24.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(sc(4.))
                    .child(div().text_size(sc(32.)).font_weight(FontWeight::BOLD).child(crate::i18n::t("settings"))),
            )
            .child(
                div().flex().gap(sc(PANEL_GAP)).items_start().when(!wide, |d| d.flex_col()).child(tabs).child(
                    div()
                        .flex_1()
                        .w_full()
                        .min_w(sc(0.))
                        .flex()
                        .flex_col()
                        .gap(sc(PANEL_GAP))
                        .child(
                            div()
                                .px(sc(3.))
                                .pb(sc(3.))
                                .child(div().text_size(sc(22.)).font_weight(FontWeight::BOLD).child(title))
                                .child(div().text_size(sc(12.5)).text_color(theme::text_secondary()).child(description)),
                        )
                        .child(body),
                ),
            )
            .into_any_element()
    }

    fn settings_audio(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.lock().clone();
        let compact = width < 650.;
        // ------------------------------------------------ audio quality
        // The radio dot sits before the text, like the official rows.
        let dot = |selected: bool| {
            div()
                .flex_none()
                .size(sc(18.))
                .rounded(sc(9.))
                .bg(theme::field())
                // A visible ring, so the unselected options read as choices.
                .border_2()
                .border_color(if selected { theme::gold() } else { theme::ink(0.28) })
                .flex()
                .items_center()
                .justify_center()
                .when(selected, |d| d.bg(theme::gold_solid()).child(div().size(sc(8.)).rounded(sc(4.)).bg(theme::gold())))
        };
        let quality_rows: Vec<AnyElement> = [
            (Quality::Low, crate::i18n::t("balance_audio_quality_and_data_usage")),
            (Quality::High, Quality::High.description()),
            (Quality::Max, Quality::Max.description()),
        ]
        .into_iter()
        .map(|(q, sub)| {
            let selected = s.quality == q;
            div()
                .id(SharedString::from(format!("q-{}", q.label())))
                .px(sc(super::widgets::PANEL_PAD - ROW_INSET))
                .py(sc(14. - ROW_INSET))
                .rounded(sc(10.))
                .flex()
                .items_center()
                .gap(sc(14.))
                .cursor_pointer()
                .hover(|st| st.bg(theme::ink(0.05)))
                .child(dot(selected))
                .child(row_text(q.label(), Some(sub.into())))
                .when(q == Quality::Low && !compact, |d| {
                    d.child(small_button(("q-low-detail", 0usize), crate::i18n::t("aac_320_kbps"), false))
                })
                .on_click(cx.listener(move |this, _, _, cx| this.set_quality(q, cx)))
                .into_any_element()
        })
        .collect();

        // ------------------------------------------------ playback
        let device_label = s.output_device.clone().unwrap_or_else(|| crate::i18n::t("system_default").into());
        let device_open = self.settings_dev_open;
        let mut playback = vec![
            self.toggle_row(
                "normalize",
                crate::i18n::t("normalize_volume"),
                crate::i18n::t("match_track_volumes_using_tidal_gain_data"),
                s.normalize,
                |this, cx| this.toggle_setting("normalize", cx),
                cx,
            ),
            row(
                "sound-output",
                crate::i18n::t("sound_output"),
                None,
                Some(
                    small_button("sound-output-pick", device_label, false)
                        .max_w(sc((width * 0.42).min(300.)))
                        .child(Icon::ChevDown.el(12., theme::text_secondary()))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings_dev_open = !this.settings_dev_open;
                            this.content_changed_pub();
                            cx.notify();
                        }))
                        .into_any_element(),
                ),
            )
            .into_any_element(),
        ];
        if device_open {
            let current = s.output_device.clone();
            let devices = self.devices.clone();
            playback.push(
                div()
                    .px(sc(8.))
                    .pb(sc(8.))
                    .flex()
                    .flex_col()
                    .children(std::iter::once(None).chain(devices.into_iter().map(Some)).enumerate().map(|(i, d)| {
                        let selected = d == current;
                        let label = d.clone().unwrap_or_else(|| crate::i18n::t("system_default").into());
                        let dd = d.clone();
                        div()
                            .id(("dev", i))
                            .px(sc(10.))
                            .py(sc(8.))
                            .rounded(sc(6.))
                            .flex()
                            .items_center()
                            .gap(sc(10.))
                            .cursor_pointer()
                            .text_size(sc(theme::TYPE_LABEL))
                            .when(selected, |x| x.bg(theme::hover()))
                            .hover(|x| x.bg(theme::field()))
                            .child(Icon::VolumeUp.el(14., if selected { theme::accent() } else { theme::text_dim() }))
                            .child(div().truncate().child(label))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let s = this.settings.lock().clone();
                                this.settings_dev_open = false;
                                this.set_output(dd.clone(), s.exclusive_mode, s.force_volume, cx)
                            }))
                    }))
                    .into_any_element(),
            );
        }
        playback.push(self.toggle_row(
            "exclusive",
            crate::i18n::t("exclusive_mode_wasapi"),
            crate::i18n::t("audio.exclusive_description"),
            s.exclusive_mode,
            |this, cx| {
                let s = this.settings.lock().clone();
                this.set_output(s.output_device, !s.exclusive_mode, s.force_volume, cx)
            },
            cx,
        ));
        playback.push(self.toggle_row(
            "forcevol",
            crate::i18n::t("force_volume"),
            crate::i18n::t("audio.force_volume_description"),
            s.force_volume,
            |this, cx| {
                let s = this.settings.lock().clone();
                this.set_output(s.output_device, s.exclusive_mode, !s.force_volume, cx)
            },
            cx,
        ));
        playback.push(
            div()
                .px(sc(16.))
                .py(sc(10.))
                .text_size(sc(11.))
                .text_color(theme::text_dim())
                .child(if self.snap.output.is_empty() {
                    crate::i18n::t("current_output_opens_on_playback").to_string()
                } else if self.snap.output_rate > 0 {
                    let rate = crate::audio::output::fmt_khz(self.snap.output_rate);
                    crate::i18n::text("current_output_khz", &[self.snap.output.to_string(), rate.to_string()])
                } else {
                    crate::i18n::text("current_output", &[self.snap.output.to_string()])
                })
                .into_any_element(),
        );
        let queue = vec![self.toggle_row(
            "autoplay",
            crate::i18n::t("autoplay"),
            crate::i18n::t("play_similar_songs_when_the_queue_ends"),
            s.autoplay,
            |this, cx| this.toggle_setting("autoplay", cx),
            cx,
        )];

        div()
            .flex()
            .flex_col()
            .gap(sc(PANEL_GAP))
            .child(group(Icon::Track, crate::i18n::t("audio_quality"), crate::i18n::t("streaming_format_and_quality"), quality_rows))
            .child(group(Icon::VolumeUp, crate::i18n::t("audio_output"), crate::i18n::t("device_volume_and_exclusive_mode"), playback))
            .child(group(Icon::Play, crate::i18n::t("playback"), crate::i18n::t("queue_behavior"), queue))
            .into_any_element()
    }

    fn settings_appearance(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.lock().clone();
        let compact = width < 650.;
        let ambient = self.ambient(cx);
        let cover_url = self.now.as_ref().and_then(|n| n.track.cover(160));
        let swatch = |id: &'static str, label: &'static str, blur: bool, cx: &mut Context<Self>| {
            let selected = s.np_blur == blur;
            let url = cover_url.clone();
            let preview: AnyElement = if blur {
                match url.as_deref().and_then(|u| super::images::blurred(u, cx)) {
                    Some(img) => gpui::img(img).size_full().object_fit(gpui::ObjectFit::Cover).into_any_element(),
                    None => div().size_full().bg(theme::control()).into_any_element(),
                }
            } else {
                div().size_full().bg(ambient).into_any_element()
            };
            div()
                .id(id)
                .flex()
                .flex_col()
                .items_center()
                .gap(sc(6.))
                .cursor_pointer()
                .child(
                    div()
                        .w(sc(52.))
                        .h(sc(36.))
                        .rounded(sc(8.))
                        .overflow_hidden()
                        .border_2()
                        .border_color(if selected { theme::text() } else { gpui::rgba(0x00000000) })
                        .child(preview),
                )
                .child(
                    div()
                        .text_size(sc(11.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(if selected { theme::text() } else { theme::text_secondary() })
                        .child(label),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.settings.lock().np_blur = blur;
                    this.settings.lock().save();
                    cx.notify();
                }))
                .into_any_element()
        };
        let player_bg = div()
            .px(sc(16.))
            .py(sc(14.))
            .flex()
            .when(compact, |d| d.flex_col().items_start())
            .when(!compact, |d| d.items_center())
            .gap(sc(12.))
            .child(row_text(crate::i18n::t("player_background"), Some(crate::i18n::t("choose_the_full_screen_player_background").into())))
            .child(div().flex().gap(sc(12.)).child(swatch("bg-color", crate::i18n::t("color"), false, cx)).child(swatch(
                "bg-blur",
                crate::i18n::t("blur"),
                true,
                cx,
            )))
            .into_any_element();
        // Theme: dark (default) or light, with a small preview of each.
        let theme_swatch = |id: &'static str, label: &'static str, light: bool, cx: &mut Context<Self>| {
            let selected = s.light_theme == light;
            let (bg, frame, line) = if light { (0xf6f6f8, theme::LIGHT_FRAME, 0x14141a) } else { (0x000000, theme::DARK_FRAME, 0xffffff) };
            let preview = div().size_full().flex().bg(gpui::rgb(bg)).child(div().w(sc(14.)).h_full().bg(gpui::rgb(frame))).child(
                div()
                    .flex_1()
                    .p(sc(5.))
                    .flex()
                    .flex_col()
                    .gap(sc(4.))
                    .child(div().w(sc(20.)).h(sc(3.)).rounded(sc(1.5)).bg(gpui::rgb(line)))
                    .child(div().w_full().h(sc(12.)).rounded(sc(2.)).bg(gpui::rgb(frame))),
            );
            div()
                .id(id)
                .flex()
                .flex_col()
                .items_center()
                .gap(sc(6.))
                .cursor_pointer()
                .child(
                    div()
                        .w(sc(52.))
                        .h(sc(36.))
                        .rounded(sc(8.))
                        .overflow_hidden()
                        .border_2()
                        .border_color(if selected { theme::text() } else { theme::ink(0.12) })
                        .child(preview),
                )
                .child(
                    div()
                        .text_size(sc(11.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(if selected { theme::text() } else { theme::text_secondary() })
                        .child(label),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_light_theme(light, cx)))
                .into_any_element()
        };
        let theme_row = div()
            .px(sc(16.))
            .py(sc(14.))
            .flex()
            .when(compact, |d| d.flex_col().items_start())
            .when(!compact, |d| d.items_center())
            .gap(sc(12.))
            .child(row_text(crate::i18n::t("theme"), Some(crate::i18n::t("appearance.theme_description").into())))
            .child(div().flex().gap(sc(12.)).child(theme_swatch("theme-dark", crate::i18n::t("dark"), false, cx)).child(theme_swatch(
                "theme-light",
                crate::i18n::t("light"),
                true,
                cx,
            )))
            .into_any_element();
        let language_row = row("language-row", crate::i18n::t("language"), Some(crate::i18n::t("interface_language").into()), None)
            .flex_col()
            .items_start()
            .child(
                div().flex().flex_wrap().gap(sc(6.)).children(
                    std::iter::once((None, crate::i18n::t("system")))
                        .chain(crate::i18n::LANGUAGES.into_iter().map(|(code, label)| (Some(code), label)))
                        .enumerate()
                        .map(|(index, (code, label))| {
                            small_button(("language", index), label, s.language.as_deref() == code).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.set_interface_language(code, cx);
                                },
                            ))
                        }),
                ),
            )
            .into_any_element();
        let accent_row = self.accent_row(width, cx);
        let zoom_pct = crate::i18n::text("format.percent_spaced", &[format!("{}", (super::zoom() * 100.0).round() as i32)]);
        let zoom_row = row(
            "zoom-row",
            crate::i18n::t("interface_zoom"),
            Some(crate::i18n::t("also_with_ctrl_ctrl_ctrl_0_or_ctrl_wheel").into()),
            Some(
                div()
                    .flex()
                    .items_center()
                    .gap(sc(8.))
                    .child(small_button("zoom-out", "−", false).on_click(cx.listener(|this, _, _, cx| this.step_zoom(-1, cx))))
                    .child(div().w(sc(46.)).text_center().text_size(sc(11.)).font_weight(FontWeight::BOLD).child(zoom_pct))
                    .child(small_button("zoom-in", "+", false).on_click(cx.listener(|this, _, _, cx| this.step_zoom(1, cx))))
                    .into_any_element(),
            ),
        )
        .into_any_element();
        div()
            .flex()
            .flex_col()
            .gap(sc(PANEL_GAP))
            .child(group(
                Icon::Sun,
                crate::i18n::t("interface"),
                crate::i18n::t("theme_accent_and_control_size"),
                vec![
                    language_row,
                    theme_row,
                    accent_row,
                    zoom_row,
                    row(
                        "welcome-repeat",
                        crate::i18n::t("welcome.repeat"),
                        Some(crate::i18n::t("welcome.repeat_help").into()),
                        Some(
                            small_button("welcome-repeat-button", crate::i18n::t("welcome.open"), false)
                                .on_click(cx.listener(|v, _, _, cx| v.repeat_welcome(cx)))
                                .into_any_element(),
                        ),
                    )
                    .into_any_element(),
                ],
            ))
            .child(group(Icon::Fullscreen, crate::i18n::t("full_screen"), crate::i18n::t("playback_background"), vec![player_bg]))
            .into_any_element()
    }

    fn settings_library(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.lock().clone();
        let library = vec![
            self.toggle_row(
                "audio-meta",
                crate::i18n::t("audio_metadata"),
                crate::i18n::t("show_extra_fields_e_g_bpm_in_track_lists"),
                s.audio_metadata,
                |this, cx| this.toggle_setting("audio_metadata", cx),
                cx,
            ),
            self.toggle_row(
                "show-ratings",
                crate::i18n::t("ratings"),
                crate::i18n::t("show_the_star_column_in_track_lists"),
                s.show_ratings,
                |this, cx| this.toggle_setting("show_ratings", cx),
                cx,
            ),
            self.toggle_row(
                "hide-uploads",
                crate::i18n::t("hide_uploads"),
                crate::i18n::t("remove_uploads_from_mixes_new_releases"),
                s.hide_uploads,
                |this, cx| this.toggle_setting("hide_uploads", cx),
                cx,
            ),
        ];
        let lyrics = vec![
            self.toggle_row(
                "lyrics-ext",
                crate::i18n::t("lyrics_from_other_providers"),
                crate::i18n::t("lyrics.external_privacy"),
                s.external_lyrics,
                |this, cx| this.toggle_setting("external_lyrics", cx),
                cx,
            ),
            self.toggle_row(
                "lyrics-roman",
                crate::i18n::t("lyrics_pronunciation"),
                crate::i18n::t("lyrics.romanization_description"),
                s.lyrics_romanize,
                |this, cx| this.toggle_setting("lyrics_romanize", cx),
                cx,
            ),
        ];
        div()
            .flex()
            .flex_col()
            .gap(sc(PANEL_GAP))
            .child(group(Icon::Library, crate::i18n::t("library_and_lists"), "", library))
            .child(group(Icon::Lyrics, crate::i18n::t("lyrics"), "", lyrics))
            .into_any_element()
    }

    fn settings_privacy(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let discord = self.settings.lock().discord;
        let mut connect = vec![self.toggle_row(
            "discord",
            crate::i18n::t("discord"),
            crate::i18n::t("discord.presence_description"),
            discord,
            |this, cx| this.toggle_setting("discord", cx),
            cx,
        )];
        connect.push(
            row("discord-app", crate::i18n::t("discord_application_id"), Some(crate::i18n::t("discord.application_required").into()), None)
                .flex_col()
                .items_start()
                .gap(sc(8.))
                .child(self.discord_input.clone())
                .into_any_element(),
        );
        let lastfm = self.lastfm_rows(cx);
        let legal = vec![
            self.link_row("terms", crate::i18n::t("terms_and_conditions"), "https://tidal.com/terms", cx),
            self.link_row("privacy", crate::i18n::t("privacy_notice"), "https://tidal.com/privacy", cx),
        ];

        div()
            .flex()
            .flex_col()
            .gap(sc(PANEL_GAP))
            .child(group(Icon::Link, crate::i18n::t("integrations"), "", connect))
            .child(group(Icon::Link, crate::i18n::t("lastfm.title"), crate::i18n::t("lastfm.description"), lastfm))
            .child(group(Icon::Link, crate::i18n::t("privacy_and_terms"), "", legal))
            .into_any_element()
    }

    /// Last.fm: account, scrobbling switch and history sync.
    fn lastfm_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use crate::scrobble::worker::Phase;
        let connected = self.lastfm_connected();
        let phase = self.lastfm_phase();
        let account_button = match (phase, connected) {
            (Phase::Connecting, _) => {
                small_button("lastfm-cancel", crate::i18n::t("cancel"), false).on_click(cx.listener(|v, _, _, cx| v.lastfm_cancel(cx)))
            }
            (_, true) => small_button("lastfm-disconnect", crate::i18n::t("lastfm.disconnect"), false)
                .on_click(cx.listener(|v, _, _, cx| v.lastfm_disconnect(cx))),
            (_, false) => small_button("lastfm-connect", crate::i18n::t("lastfm.connect"), true)
                .on_click(cx.listener(|v, _, _, cx| v.lastfm_connect(cx))),
        };
        let mut rows = vec![row(
            "lastfm-account",
            crate::i18n::t("lastfm.title"),
            Some(self.lastfm_status_text().into()),
            Some(account_button.into_any_element()),
        )
        .into_any_element()];
        if connected {
            let enabled = self.settings.lock().lastfm_enabled;
            rows.push(self.toggle_row(
                "lastfm-scrobble",
                crate::i18n::t("lastfm.scrobble"),
                crate::i18n::t("lastfm.scrobble_help"),
                enabled,
                |this, cx| this.lastfm_toggle(cx),
                cx,
            ));
            let sync_button = if phase == Phase::Syncing {
                small_button("lastfm-sync-cancel", crate::i18n::t("cancel"), false).on_click(cx.listener(|v, _, _, cx| v.lastfm_cancel(cx)))
            } else {
                small_button("lastfm-sync", crate::i18n::t("lastfm.sync"), false).on_click(cx.listener(|v, _, _, cx| v.lastfm_sync(cx)))
            };
            rows.push(
                row(
                    "lastfm-sync-row",
                    crate::i18n::t("lastfm.sync"),
                    Some(crate::i18n::t("lastfm.sync_help").into()),
                    Some(sync_button.into_any_element()),
                )
                .into_any_element(),
            );
        }
        rows
    }

    fn settings_account(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.lock().clone();
        let service = crate::i18n::t("tidal");
        let profile_url = "https://tidal.com/profile";
        let account_url = "https://account.tidal.com";
        let name = s
            .username
            .clone()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| crate::i18n::text("your_account", &[service.to_string()]));
        let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
        let avatar = match s.user_picture.as_deref() {
            Some(picture) => cover(Some(crate::tidal::image_url(picture, 160)), 48., true, cx),
            None => div()
                .flex_none()
                .size(sc(48.))
                .rounded(sc(24.))
                .bg(theme::control())
                .flex()
                .items_center()
                .justify_center()
                .text_size(sc(18.))
                .font_weight(FontWeight::BOLD)
                .child(initial)
                .into_any_element(),
        };
        let profile_card = div()
            .px(sc(16.))
            .py(sc(14.))
            .flex()
            .items_center()
            .gap(sc(14.))
            .child(avatar)
            .child(div().flex_1().min_w(sc(0.)).truncate().text_size(sc(14.)).font_weight(FontWeight::BOLD).child(name.clone()))
            .child(small_button("edit-profile", crate::i18n::t("edit_profile"), false).on_click(cx.listener(move |_, _, _, _| {
                let _ = open::that(profile_url);
            })))
            .into_any_element();
        let manage = |id: &'static str| {
            small_button(id, crate::i18n::t("manage"), false).on_click(move |_, _, _| {
                let _ = open::that(account_url);
            })
        };
        let personal = vec![
            row("first-name", crate::i18n::t("name"), None, Some(manage("manage-name").into_any_element())).into_any_element(),
            row(
                "email",
                crate::i18n::t("email"),
                Some(s.user_email.clone().unwrap_or_else(|| crate::i18n::t("unavailable").into()).into()),
                None,
            )
            .into_any_element(),
        ];
        let account = vec![
            row("sub", crate::i18n::t("current_subscription"), Some(service.into()), Some(manage("manage-sub").into_any_element()))
                .into_any_element(),
            row("username", crate::i18n::t("username"), Some(name.into()), None).into_any_element(),
        ];
        let more = vec![
            self.link_row("notif", crate::i18n::t("notification_settings"), account_url, cx),
            self.link_row("pass", crate::i18n::t("change_password"), account_url, cx),
            row(
                "signout",
                crate::i18n::t("sign_out"),
                Some(crate::i18n::t("remove_the_session_saved_on_this_computer").into()),
                Some(
                    small_button("signout-btn", crate::i18n::t("sign_out"), true)
                        .on_click(cx.listener(|this, _, window, cx| this.confirm_sign_out(window, cx)))
                        .into_any_element(),
                ),
            )
            .into_any_element(),
        ];
        div()
            .flex()
            .flex_col()
            .gap(sc(PANEL_GAP))
            .child(group(Icon::Artist, crate::i18n::t("profile"), "", vec![profile_card]))
            .child(group(Icon::Edit, crate::i18n::t("personal_details"), crate::i18n::t("your_tidal_account_information"), personal))
            .child(group(Icon::Settings, crate::i18n::t("subscription"), "", account))
            .child(group(Icon::SignOut, crate::i18n::t("security_and_session"), "", more))
            .into_any_element()
    }

    fn settings_about(&self) -> AnyElement {
        let header = div()
            .px(sc(16.))
            .py(sc(16.))
            .flex()
            .items_center()
            .gap(sc(16.))
            .child(super::widgets::brand_mark(56.))
            .child(row_text(
                crate::i18n::text("app.version", &[env!("CARGO_PKG_VERSION").to_string()]),
                Some(crate::i18n::t("unofficial_tidal_client_built_with_rust_and_gpui").into()),
            ))
            .into_any_element();
        let credits: &[(&str, &str)] = &[
            (crate::i18n::t("credits.gpui"), crate::i18n::t("apache_2_0_interface")),
            (crate::i18n::t("credits.symphonia"), crate::i18n::t("mpl_2_0_flac_and_aac_decoding")),
            (crate::i18n::t("credits.rubato"), crate::i18n::t("mit_sinc_resampling")),
            (crate::i18n::t("credits.audio_output"), crate::i18n::t("apache_2_0_mit_audio_output")),
            (crate::i18n::t("credits.network"), crate::i18n::t("credits.mit_apache")),
            (crate::i18n::t("credits.lofty"), crate::i18n::t("credits.mit_apache")),
            (crate::i18n::t("credits.lindera"), crate::i18n::t("mit_ipadic_license_kanji_readings_romaji")),
            (crate::i18n::t("credits.pinyin"), crate::i18n::t("mit_chinese_pinyin")),
            (crate::i18n::t("credits.inter"), crate::i18n::t("sil_ofl_1_1_typeface")),
            (crate::i18n::t("credits.icons"), crate::i18n::t("apache_2_0_icons")),
        ];
        let credits = credits
            .iter()
            .enumerate()
            .map(|(i, (name, license))| row(("credit", i), *name, Some((*license).into()), None).into_any_element())
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(sc(PANEL_GAP))
            .child(group(Icon::Track, crate::i18n::t("installed_version"), "", vec![header]))
            .child(group(Icon::List, crate::i18n::t("licenses_and_credits"), crate::i18n::t("about.components"), credits))
            .child(group(
                Icon::Link,
                crate::i18n::t("about_the_project"),
                "",
                vec![row(
                    "about-tidal",
                    crate::i18n::t("independent_project"),
                    Some(crate::i18n::t("about.unofficial_notice").into()),
                    None,
                )
                .into_any_element()],
            ))
            .into_any_element()
    }
}

/// Colour of hue `h` (0..1) for the custom accent: vivid, light enough to
/// read on the dark theme (the light theme darkens it as needed).
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

impl AppView {
    /// Choose the accent colour (applied at once; saved unless `save` is false,
    /// e.g. while dragging along the hue strip).
    pub fn set_accent(&mut self, rgb: u32, save: bool, cx: &mut Context<Self>) {
        theme::set_accent(rgb);
        {
            let mut s = self.settings.lock();
            s.accent = rgb;
            if save {
                s.save();
            }
        }
        cx.notify();
    }

    fn pick_hue_at(&mut self, x: gpui::Pixels, save: bool, cx: &mut Context<Self>) {
        let b = self.hue_bounds;
        let w = f32::from(b.size.width).max(1.0);
        let h = ((f32::from(x) - f32::from(b.origin.x)) / w).clamp(0.0, 0.999);
        self.set_accent(hue_color(h), save, cx);
    }

    /// Ajustes -> Pantalla -> Color de acento: ready-made colours and a hue
    /// strip for any other; the current value is shown as hex.
    fn accent_row(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let compact = width < 650.;
        let strip_width = (width - 40.).clamp(32., 180.);
        let segment = strip_width / 6.;
        let current = theme::accent_rgb();
        let swatch = |i: usize, rgb: u32, name: &'static str, cx: &mut Context<Self>| {
            let selected = rgb == current;
            let key = format!("accent-{i}");
            let h = super::interact::hover(&key);
            // Each swatch as it will look in the current theme.
            let prev = theme::accent_rgb();
            theme::set_accent(rgb);
            let shown = theme::accent();
            theme::set_accent(prev);
            let el = div()
                .id(("accent", i))
                .size(sc(22.))
                .rounded(sc(11.))
                .flex_none()
                .cursor_pointer()
                .bg(shown)
                .border_2()
                .border_color(if selected { theme::text() } else { theme::ink(0.10 + 0.25 * h) })
                .tooltip(super::sidebar::tip(name))
                .on_click(cx.listener(move |this, _, _, cx| this.set_accent(rgb, true, cx)));
            super::interact::track(el, key, cx).into_any_element()
        };
        let presets: Vec<AnyElement> =
            theme::accent_presets().iter().enumerate().map(|(i, (rgb, name))| swatch(i, *rgb, name, cx)).collect();
        let custom = !theme::accent_presets().iter().any(|(c, _)| *c == current);
        // Hue strip: six gradient segments (GPUI gradients have two stops).
        let strip = div()
            .id("accent-hue")
            .relative()
            .w(sc(strip_width))
            .h(sc(14.))
            .rounded(sc(7.))
            .overflow_hidden()
            .cursor_pointer()
            // Absolute segments overlapping by a pixel: no seams between them.
            .children((0..6).map(|k| {
                let a = hue_color(k as f32 / 6.0);
                let b = hue_color((k + 1) as f32 / 6.0);
                div().absolute().top(sc(0.)).left(sc(segment * k as f32)).w(sc(segment + 1.)).h_full().bg(gpui::linear_gradient(
                    90.,
                    gpui::linear_color_stop(theme::rgb_u32(a), 0.),
                    gpui::linear_color_stop(theme::rgb_u32(b), 1.),
                ))
            }))
            .child(
                gpui::canvas(
                    {
                        let e = cx.entity();
                        move |b, _, cx| e.update(cx, |v, _| v.hue_bounds = b)
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, ev: &gpui::MouseDownEvent, _, cx| {
                    this.hue_drag = true;
                    this.pick_hue_at(ev.position.x, false, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, _, cx| {
                if this.hue_drag && ev.pressed_button == Some(gpui::MouseButton::Left) {
                    this.pick_hue_at(ev.position.x, false, cx);
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, ev: &gpui::MouseUpEvent, _, cx| {
                    if std::mem::take(&mut this.hue_drag) {
                        this.pick_hue_at(ev.position.x, true, cx);
                    }
                }),
            );
        let reset = div()
            .id("accent-reset")
            .text_size(sc(11.))
            .text_color(theme::text_secondary())
            .cursor_pointer()
            .hover(|s| s.text_color(theme::text()).underline())
            .child(crate::i18n::t("reset"))
            .on_click(cx.listener(|this, _, _, cx| this.set_accent(theme::DEFAULT_ACCENT, true, cx)));
        div()
            .px(sc(16.))
            .py(sc(14.))
            .flex()
            .when(compact, |d| d.flex_col().items_start())
            .when(!compact, |d| d.items_center())
            .gap(sc(12.))
            .child(row_text(crate::i18n::t("accent_color"), Some(crate::i18n::t("appearance.accent_description").into())))
            .child(
                div()
                    .min_w(sc(0.))
                    .max_w_full()
                    .flex()
                    .flex_col()
                    .when(compact, |d| d.items_start())
                    .when(!compact, |d| d.items_end())
                    .gap(sc(10.))
                    .child(div().flex().flex_wrap().gap(sc(8.)).children(presets))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(sc(10.))
                            .child(div().text_size(sc(11.)).text_color(theme::text_dim()).child(crate::i18n::t("custom")))
                            .child(strip)
                            .child(
                                div()
                                    .w(sc(62.))
                                    .text_size(sc(11.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if custom { theme::accent() } else { theme::text_dim() })
                                    .child(format!("#{current:06X}")),
                            )
                            .child(reset),
                    ),
            )
            .into_any_element()
    }
}
