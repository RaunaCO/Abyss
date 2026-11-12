//! One-time, repeatable welcome. No provider credentials are invented here.
use gpui::{div, prelude::*, AnimationExt, AnyElement, Context, FontWeight, Window};

use super::{icons::Icon, interact, motion, sc, theme, widgets, AppView, Page};

impl AppView {
    pub fn repeat_welcome(&mut self, cx: &mut Context<Self>) {
        self.welcome_step = Some(0);
        self.np_fullscreen_request = Some(false);
        self.menu = None;
        self.app_menu = None;
        self.search_open = false;
        cx.notify();
    }

    pub fn finish_welcome(&mut self, cx: &mut Context<Self>) {
        let local = {
            let mut s = self.settings.lock();
            s.welcome_complete = true;
            s.save();
            s.preferred_service == "local"
        };
        self.welcome_step = None;
        if local {
            // Preserve saved service tokens; selecting local never signs the account out.
            self.signed_in = false;
            self.reset_data();
            self.page = Page::MyTracks;
        } else if !self.signed_in && self.settings.lock().signed_in() {
            self.signed_in = true;
            self.after_login(cx);
            self.page = Page::Home;
        }
        self.page_changed(cx);
        cx.notify();
    }

    pub fn set_interface_language(&mut self, code: Option<&str>, cx: &mut Context<Self>) {
        {
            let mut s = self.settings.lock();
            s.language = code.map(str::to_string);
            s.save();
        }
        crate::i18n::set_language(code);
        self.replay_cache = None;
        self.dlg_title.update(cx, |i, cx| i.set_placeholder(crate::i18n::t("my_playlist"), cx));
        self.discord_input.update(cx, |i, cx| i.set_placeholder(crate::i18n::t("discord_application_id"), cx));
        self.search_input.update(cx, |i, cx| i.set_placeholder(crate::i18n::t("search"), cx));
        self.login_input.update(cx, |i, cx| i.set_placeholder(crate::i18n::t("type_or_paste_the_tidal_login_address"), cx));
        self.filter_input.update(cx, |i, cx| i.set_placeholder(crate::i18n::t("filter"), cx));
        self.content_changed_pub();
        cx.notify();
    }
}

fn choice(
    index: usize,
    label: &'static str,
    selected: bool,
    enabled: bool,
    action: impl Fn(&mut AppView, &mut Context<AppView>) + 'static,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let key = format!("welcome-choice-{index}");
    let h = interact::hover(&key);
    let p = interact::press(&key);
    interact::track(
        div()
            .id(("welcome-choice", index))
            .p(sc(16.))
            .min_w(sc(120.))
            .rounded(sc(12.))
            .border_2()
            .border_color(if selected { theme::accent() } else { theme::divider() })
            .bg(theme::ink(0.04 + h * 0.06 + p * 0.05))
            .text_size(sc(theme::TYPE_BODY))
            .font_weight(FontWeight::SEMIBOLD)
            .opacity(if enabled { 1. } else { 0.45 })
            .when(enabled, |d| d.cursor_pointer())
            .child(label)
            .on_click(cx.listener(move |v, _, _, cx| {
                if enabled {
                    action(v, cx);
                }
            })),
        key,
        cx,
    )
    .into_any_element()
}

pub fn render(view: &mut AppView, window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    let step = view.welcome_step.unwrap_or(0).min(3);
    let wide = f32::from(window.viewport_size().width) / super::zoom() >= 850.;
    let title = ["welcome.title", "welcome.service", "welcome.language", "welcome.library"][step];
    let description = ["welcome.intro", "welcome.service_help", "interface_language", "welcome.library_help"][step];
    let mut content = div().flex().flex_col().gap(sc(18.));
    match step {
        0 => {
            content = content.child(widgets::brand_mark(100.)).child(
                div()
                    .text_size(sc(16.))
                    .line_height(sc(25.))
                    .text_color(theme::text_secondary())
                    .child(crate::i18n::t("about.unofficial_notice")),
            );
        }
        1 => {
            let local = view.settings.lock().preferred_service == "local";
            content = content.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(sc(12.))
                    .child(choice(
                        0,
                        crate::i18n::t("welcome.tidal"),
                        !local,
                        true,
                        |v, cx| {
                            v.settings.lock().preferred_service = "tidal".into();
                            v.settings.lock().save();
                            cx.notify();
                        },
                        cx,
                    ))
                    .child(choice(
                        1,
                        crate::i18n::t("welcome.local"),
                        local,
                        true,
                        |v, cx| {
                            v.settings.lock().preferred_service = "local".into();
                            v.settings.lock().save();
                            cx.notify();
                        },
                        cx,
                    )),
            );
            if !local {
                // Plain-language account risk, shown only when TIDAL is the chosen service.
                content = content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(sc(8.))
                        .p(sc(16.))
                        .rounded(sc(12.))
                        .border_1()
                        .border_color(theme::divider())
                        .bg(theme::ink(0.04))
                        .child(
                            div()
                                .text_size(sc(theme::TYPE_BODY))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(crate::i18n::t("welcome.risk_title")),
                        )
                        .child(
                            div()
                                .text_size(sc(14.))
                                .line_height(sc(22.))
                                .text_color(theme::text_secondary())
                                .child(crate::i18n::t("welcome.risk_body")),
                        ),
                );
            }
        }
        2 => {
            let selected = view.settings.lock().language.clone();
            content = content.child(
                div().flex().flex_wrap().gap(sc(10.)).children(
                    std::iter::once((None, crate::i18n::t("system")))
                        .chain(crate::i18n::LANGUAGES.into_iter().map(|(c, l)| (Some(c), l)))
                        .enumerate()
                        .map(|(i, (code, label))| {
                            choice(i, label, selected.as_deref() == code, true, move |v, cx| v.set_interface_language(code, cx), cx)
                        }),
                ),
            );
        }
        3 => {
            content = content
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(sc(12.))
                        .child(widgets::pill(
                            "welcome-folder",
                            crate::i18n::t("add_music_folder"),
                            Some(Icon::Upload),
                            true,
                            |v, _, _, cx| v.choose_local_files(true, cx),
                            cx,
                        ))
                        .child(widgets::pill(
                            "welcome-files",
                            crate::i18n::t("add_audio_files"),
                            Some(Icon::Track),
                            false,
                            |v, _, _, cx| v.choose_local_files(false, cx),
                            cx,
                        )),
                )
                .child(div().text_size(sc(14.)).text_color(theme::text_secondary()).child(if view.local_busy {
                    crate::i18n::t("adding_to_your_library").to_string()
                } else {
                    crate::i18n::text("welcome.imported", &[view.local_tracks.len().to_string()])
                }));
        }
        _ => {}
    }
    div()
        .id("welcome-root")
        .key_context("Welcome")
        .track_focus(&view.focus)
        .size_full()
        .bg(theme::frame())
        .text_color(theme::text())
        .font_family(theme::FONT)
        .text_size(sc(theme::TYPE_BODY))
        .on_key_down(cx.listener(move |v, ev: &gpui::KeyDownEvent, _, cx| {
            if ev.is_held {
                return;
            }
            match ev.keystroke.key.as_str() {
                "escape" => v.finish_welcome(cx),
                "enter" => {
                    if step == 3 {
                        v.finish_welcome(cx);
                    } else {
                        v.welcome_step = Some(step + 1);
                        cx.notify();
                    }
                }
                "left" if step > 0 => {
                    v.welcome_step = Some(step - 1);
                    cx.notify();
                }
                "right" if step < 3 => {
                    v.welcome_step = Some(step + 1);
                    cx.notify();
                }
                _ => return,
            }
            cx.stop_propagation();
        }))
        .flex()
        .items_center()
        .justify_center()
        .p(sc(if wide { 40. } else { 16. }))
        .child(
            widgets::floating_panel(20.)
                .id("welcome-scroll")
                .w(sc(760.))
                .max_w_full()
                .max_h_full()
                .overflow_y_scroll()
                .p(sc(if wide { 40. } else { 24. }))
                .flex()
                .flex_col()
                .gap(sc(24.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(sc(8.))
                        .children((0..4).map(|i| div().flex_1().h(sc(4.)).bg(if i <= step { theme::accent() } else { theme::divider() }))),
                )
                .child(
                    div()
                        .text_size(sc(theme::TYPE_LABEL))
                        .text_color(theme::text_secondary())
                        .child(crate::i18n::text("welcome.step", &[(step + 1).to_string()])),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(sc(12.))
                        .child(div().text_size(sc(theme::TYPE_TITLE)).font_weight(FontWeight::BOLD).child(crate::i18n::t(title)))
                        .child(
                            div()
                                .text_size(sc(16.))
                                .line_height(sc(24.))
                                .text_color(theme::text_secondary())
                                .child(crate::i18n::t(description)),
                        )
                        .child(content)
                        .with_animation(("welcome-step", step), motion::animation(motion::PANEL), motion::enter),
                )
                .child(
                    div()
                        .pt(sc(20.))
                        .border_t_1()
                        .border_color(theme::divider())
                        .flex()
                        .flex_wrap()
                        .gap(sc(12.))
                        .child(widgets::pill(
                            "welcome-skip",
                            crate::i18n::t("welcome.skip"),
                            None,
                            false,
                            |v, _, _, cx| v.finish_welcome(cx),
                            cx,
                        ))
                        .child(div().flex_1())
                        .when(step > 0, |d| {
                            d.child(widgets::pill(
                                "welcome-back",
                                crate::i18n::t("previous"),
                                Some(Icon::Back),
                                false,
                                move |v, _, _, cx| {
                                    v.welcome_step = Some(step - 1);
                                    cx.notify();
                                },
                                cx,
                            ))
                        })
                        .child(widgets::pill(
                            "welcome-next",
                            if step == 3 { crate::i18n::t("welcome.finish") } else { crate::i18n::t("next") },
                            Some(Icon::Forward),
                            true,
                            move |v, _, _, cx| {
                                if step == 3 {
                                    v.finish_welcome(cx);
                                } else {
                                    v.welcome_step = Some(step + 1);
                                    cx.notify();
                                }
                            },
                            cx,
                        )),
                ),
        )
        .into_any_element()
}
