//! The hamburger menu of the title bar, like the official app's: File,
//! Navigation, Playback, View and Help, each opening a submenu (Repeat and
//! Troubleshooting open one more level). Hovering an entry unfolds its
//! submenu; clicking an action runs it and closes the menu.

use std::rc::Rc;

use super::sc;
use gpui::AnimationExt;
use gpui::{deferred, div, prelude::*, AnyElement, Context, Focusable, FontWeight, SharedString, Window};

use super::{theme, AppView, Page};
use crate::config::Repeat;

type Run = Rc<dyn Fn(&mut AppView, &mut Window, &mut Context<AppView>)>;

enum Entry {
    Item { label: SharedString, shortcut: &'static str, run: Run },
    Sub { label: &'static str, entries: Vec<Entry> },
    Sep,
}

const ROW_H: f32 = 34.0;
const SEP_H: f32 = 9.0;
const PANEL_PAD: f32 = 6.0;

fn item(label: &str, shortcut: &'static str, run: impl Fn(&mut AppView, &mut Window, &mut Context<AppView>) + 'static) -> Entry {
    Entry::Item { label: SharedString::from(label.to_string()), shortcut, run: Rc::new(run) }
}

fn menu(view: &AppView) -> Vec<Entry> {
    let playing = view.now.is_some() && !view.snap.paused && !view.ended;
    let repeat = view.repeat;
    // The check goes after the label, so labels stay aligned with the other rows.
    let mark = |on: bool, text: &str| format!("{text}{}", if on { "  ✓" } else { "" });
    vec![
        Entry::Sub {
            label: crate::i18n::t("file"),
            entries: vec![
                item(crate::i18n::t("sign_out"), "", |this, _, cx| this.sign_out(cx)),
                item(crate::i18n::t("settings"), crate::i18n::t("ctrl_p"), |this, _, cx| this.navigate(Page::Settings, cx)),
                Entry::Sep,
                item(crate::i18n::t("quit"), "", |_, _, cx| cx.quit()),
            ],
        },
        Entry::Sub {
            label: crate::i18n::t("navigation"),
            entries: vec![
                item(crate::i18n::t("home"), crate::i18n::t("ctrl_h"), |this, _, cx| this.navigate(Page::Home, cx)),
                item(crate::i18n::t("explore"), "", |this, _, cx| this.navigate(Page::Explore, cx)),
                item(crate::i18n::t("mixes_and_radio"), "", |this, _, cx| this.navigate(Page::MyMixes, cx)),
                item(crate::i18n::t("back"), crate::i18n::t("shortcut.back"), |this, _, cx| this.go_back(cx)),
                item(crate::i18n::t("forward"), crate::i18n::t("shortcut.forward"), |this, _, cx| this.go_forward(cx)),
                item(crate::i18n::t("search"), crate::i18n::t("ctrl_f"), |this, window, cx| {
                    let fh = this.search_input.focus_handle(cx);
                    window.focus(&fh);
                    this.search_open = true;
                }),
            ],
        },
        Entry::Sub {
            label: crate::i18n::t("playback"),
            entries: vec![
                item(if playing { crate::i18n::t("pause") } else { crate::i18n::t("play") }, crate::i18n::t("space"), |this, _, cx| {
                    this.toggle_play(cx)
                }),
                Entry::Sep,
                item(crate::i18n::t("previous"), crate::i18n::t("shortcut.previous"), |this, _, cx| this.prev(cx)),
                item(crate::i18n::t("next"), crate::i18n::t("shortcut.next"), |this, _, cx| this.next(cx)),
                Entry::Sep,
                item(crate::i18n::t("back_10_s"), crate::i18n::t("shortcut.seek_back"), |this, _, cx| {
                    let p = (this.snap.position - 10.0).max(0.0);
                    this.seek(p, cx)
                }),
                item(crate::i18n::t("forward_10_s"), crate::i18n::t("shortcut.seek_forward"), |this, _, cx| {
                    let p = this.snap.position + 10.0;
                    this.seek(p, cx)
                }),
                Entry::Sep,
                Entry::Sub {
                    label: crate::i18n::t("repeat"),
                    entries: vec![
                        item(&mark(repeat == Repeat::Off, crate::i18n::t("repeat_off")), "", |this, _, cx| {
                            set_repeat(this, Repeat::Off, cx)
                        }),
                        item(&mark(repeat == Repeat::All, crate::i18n::t("repeat_all")), "", |this, _, cx| {
                            set_repeat(this, Repeat::All, cx)
                        }),
                        item(&mark(repeat == Repeat::One, crate::i18n::t("repeat_one")), "", |this, _, cx| {
                            set_repeat(this, Repeat::One, cx)
                        }),
                    ],
                },
                item(&mark(view.shuffle, crate::i18n::t("shuffle")), crate::i18n::t("ctrl_s"), |this, _, cx| this.toggle_shuffle(cx)),
            ],
        },
        Entry::Sub {
            label: crate::i18n::t("view"),
            entries: vec![
                item(crate::i18n::t("reload"), "", |this, _, cx| {
                    this.reset_data();
                    this.load_page_data(cx);
                    cx.notify();
                }),
                Entry::Sep,
                item(crate::i18n::t("zoom_in"), crate::i18n::t("shortcut.zoom_in"), |this, _, cx| this.step_zoom(1, cx)),
                item(crate::i18n::t("zoom_out"), crate::i18n::t("shortcut.zoom_out"), |this, _, cx| this.step_zoom(-1, cx)),
                item(crate::i18n::t("reset_zoom"), crate::i18n::t("shortcut.zoom_reset"), |this, _, cx| this.set_zoom_level(1.0, cx)),
            ],
        },
        Entry::Sub {
            label: crate::i18n::t("help"),
            entries: vec![
                item(crate::i18n::t("check_for_updates"), "", |_, _, _| {
                    let _ = open::that("https://github.com/RaunaCO/TIDE/releases");
                }),
                item(crate::i18n::t("about.title"), "", |this, _, cx| {
                    this.settings_tab = super::settings::ABOUT_TAB;
                    this.content_changed_pub();
                    this.navigate(Page::Settings, cx);
                }),
                item(crate::i18n::t("support"), "", |_, _, _| {
                    let _ = open::that("https://github.com/RaunaCO/TIDE/issues");
                }),
                Entry::Sep,
                Entry::Sub {
                    label: crate::i18n::t("troubleshooting"),
                    entries: vec![
                        item(crate::i18n::t("open_settings_folder"), "", |_, _, _| {
                            let _ = open::that(crate::config::config_dir());
                        }),
                        item(crate::i18n::t("open_cache_folder"), "", |_, _, _| {
                            let _ = open::that(crate::config::cache_dir());
                        }),
                    ],
                },
            ],
        },
    ]
}

fn set_repeat(this: &mut AppView, r: Repeat, cx: &mut Context<AppView>) {
    this.repeat = r;
    this.requeue(cx);
    this.save_settings();
    cx.notify();
}

fn entries_height_before(entries: &[Entry], idx: usize) -> f32 {
    entries.iter().take(idx).map(|e| if matches!(e, Entry::Sep) { SEP_H } else { ROW_H }).sum()
}

impl AppView {
    pub fn toggle_app_menu(&mut self, cx: &mut Context<Self>) {
        self.app_menu = if self.app_menu.is_some() { None } else { Some(Vec::new()) };
        cx.notify();
    }
}

fn panel(depth: usize, width: f32, rows: Vec<AnyElement>) -> gpui::Div {
    super::widgets::floating_panel(12.)
        .w(sc(width))
        .py(sc(PANEL_PAD))
        .px(sc(PANEL_PAD))
        .flex()
        .flex_col()
        .child(super::glass::sheen(("app-menu-glass", depth), 12., 0.6))
        .children(rows)
}

/// The open menu as an overlay, or nothing.
pub fn overlay(view: &mut AppView, _window: &mut Window, cx: &mut Context<AppView>) -> Option<AnyElement> {
    let path = view.app_menu.clone()?;
    let root = menu(view);

    // Walk the hovered path to find the list shown at each depth.
    let mut lists: Vec<&[Entry]> = Vec::new();
    // Depth 0 is the list of groups itself.
    lists.push(&root);
    let mut cur: &[Entry] = &root;
    for &i in &path {
        match cur.get(i) {
            Some(Entry::Sub { entries, .. }) => {
                lists.push(entries);
                cur = entries;
            }
            _ => break,
        }
    }

    let widths = [150.0, 270.0, 190.0];
    let mut columns: Vec<AnyElement> = Vec::new();
    let mut top = 0.0;
    for (depth, list) in lists.iter().enumerate() {
        let rows: Vec<AnyElement> = list
            .iter()
            .enumerate()
            .map(|(i, e)| match e {
                Entry::Sep => {
                    div().h(sc(SEP_H)).flex().items_center().child(div().h(sc(1.)).w_full().bg(theme::divider())).into_any_element()
                }
                Entry::Sub { label, .. } => {
                    let open = path.get(depth) == Some(&i);
                    div()
                        .id(("app-sub", depth * 100 + i))
                        .h(sc(ROW_H))
                        .px(sc(10.))
                        .flex()
                        .items_center()
                        .rounded(sc(6.))
                        .cursor_pointer()
                        .text_size(sc(13.))
                        .when(open, |d| d.bg(theme::menu_hover()))
                        .hover(|s| s.bg(theme::menu_hover()))
                        .child(div().flex_1().child(*label))
                        .child(super::icons::Icon::Forward.el(14., theme::text_secondary()))
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered {
                                if let Some(p) = this.app_menu.as_mut() {
                                    p.truncate(depth);
                                    p.push(i);
                                    cx.notify();
                                }
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(p) = this.app_menu.as_mut() {
                                p.truncate(depth);
                                p.push(i);
                                cx.notify();
                            }
                        }))
                        .into_any_element()
                }
                Entry::Item { label, shortcut, run } => {
                    let run = run.clone();
                    div()
                        .id(("app-item", depth * 100 + i))
                        .h(sc(ROW_H))
                        .px(sc(10.))
                        .flex()
                        .items_center()
                        .gap(sc(18.))
                        .rounded(sc(6.))
                        .cursor_pointer()
                        .text_size(sc(13.))
                        .hover(|s| s.bg(theme::menu_hover()).pl(sc(14.)))
                        .active(|s| s.opacity(0.7))
                        .child(div().flex_1().whitespace_nowrap().child(label.clone()))
                        .when(!shortcut.is_empty(), |d| {
                            d.child(div().text_size(sc(12.)).text_color(theme::text_secondary()).child(*shortcut))
                        })
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered {
                                if let Some(p) = this.app_menu.as_mut() {
                                    if p.len() > depth {
                                        p.truncate(depth);
                                        cx.notify();
                                    }
                                }
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.app_menu = None;
                            run(this, window, cx);
                            cx.notify();
                        }))
                        .into_any_element()
                }
            })
            .collect();
        columns.push(div().mt(sc(top)).child(panel(depth, widths[depth.min(2)], rows)).into_any_element());
        // The next column lines up with the entry that opened it.
        if let Some(&i) = path.get(depth) {
            top += entries_height_before(list, i);
        }
    }

    Some(
        deferred(
            div().absolute().left(sc(8.)).top(sc(theme::CAPTION_H + 2.)).child(
                div()
                    .id("app-menu")
                    .occlude()
                    .flex()
                    .items_start()
                    .gap(sc(4.))
                    .font_weight(FontWeight::NORMAL)
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.app_menu = None;
                        cx.notify();
                    }))
                    .children(columns)
                    .with_animation("appmenu-entrance", super::motion::animation(super::motion::FAST), super::motion::enter),
            ),
        )
        .with_priority(3)
        .into_any_element(),
    )
}
