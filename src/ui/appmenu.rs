//! The hamburger menu of the title bar, like the official app's: File,
//! Navigation, Playback, View and Help, each opening a submenu (Repeat and
//! Troubleshooting open one more level). Hovering an entry unfolds its
//! submenu; clicking an action runs it and closes the menu.

use std::rc::Rc;

use super::sc;
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
    let mark = |on: bool, text: &str| format!("{}{text}", if on { "✓ " } else { "    " });
    vec![
        Entry::Sub {
            label: "Archivo",
            entries: vec![
                item("Cerrar sesión", "", |this, _, cx| this.sign_out(cx)),
                item("Ajustes", "Ctrl+P", |this, _, cx| this.navigate(Page::Settings, cx)),
                Entry::Sep,
                item("Salir", "", |_, _, cx| cx.quit()),
            ],
        },
        Entry::Sub {
            label: "Navegación",
            entries: vec![
                item("Inicio", "Ctrl+H", |this, _, cx| this.navigate(Page::Home, cx)),
                item("Atrás", "Alt+←", |this, _, cx| this.go_back(cx)),
                item("Adelante", "Alt+→", |this, _, cx| this.go_forward(cx)),
                item("Buscar", "Ctrl+F", |this, window, cx| {
                    let fh = this.search_input.focus_handle(cx);
                    window.focus(&fh);
                    if !matches!(this.page, Page::Search(_)) {
                        this.navigate(Page::Search(String::new()), cx);
                    }
                }),
            ],
        },
        Entry::Sub {
            label: "Reproducción",
            entries: vec![
                item(if playing { "Pausar" } else { "Reproducir" }, "Espacio", |this, _, cx| this.toggle_play(cx)),
                Entry::Sep,
                item("Anterior", "Ctrl+←", |this, _, cx| this.prev(cx)),
                item("Siguiente", "Ctrl+→", |this, _, cx| this.next(cx)),
                Entry::Sep,
                item("Retroceder 10 s", "Ctrl+Mayús+←", |this, _, cx| {
                    let p = (this.snap.position - 10.0).max(0.0);
                    this.seek(p, cx)
                }),
                item("Avanzar 10 s", "Ctrl+Mayús+→", |this, _, cx| {
                    let p = this.snap.position + 10.0;
                    this.seek(p, cx)
                }),
                Entry::Sep,
                Entry::Sub {
                    label: "Repetir",
                    entries: vec![
                        item(&mark(repeat == Repeat::Off, "No repetir"), "", |this, _, cx| set_repeat(this, Repeat::Off, cx)),
                        item(&mark(repeat == Repeat::All, "Repetir todo"), "", |this, _, cx| set_repeat(this, Repeat::All, cx)),
                        item(&mark(repeat == Repeat::One, "Repetir una"), "", |this, _, cx| set_repeat(this, Repeat::One, cx)),
                    ],
                },
                item(&mark(view.shuffle, "Aleatorio"), "Ctrl+S", |this, _, cx| this.toggle_shuffle(cx)),
            ],
        },
        Entry::Sub {
            label: "Ver",
            entries: vec![
                item("Recargar", "", |this, _, cx| {
                    this.reset_data();
                    this.load_page_data(cx);
                    cx.notify();
                }),
                Entry::Sep,
                item("Acercar", "Ctrl++", |this, _, cx| this.step_zoom(1, cx)),
                item("Alejar", "Ctrl+-", |this, _, cx| this.step_zoom(-1, cx)),
                item("Restablecer zoom", "Ctrl+0", |this, _, cx| this.set_zoom_level(1.0, cx)),
            ],
        },
        Entry::Sub {
            label: "Ayuda",
            entries: vec![
                item("Buscar actualizaciones", "", |_, _, _| {
                    let _ = open::that("https://github.com/RaunaCO/TIDE/releases");
                }),
                item("Acerca de TIDE", "", |this, _, cx| {
                    this.settings_tab = 2;
                    this.navigate(Page::Settings, cx);
                }),
                item("Soporte", "", |_, _, _| {
                    let _ = open::that("https://github.com/RaunaCO/TIDE/issues");
                }),
                Entry::Sep,
                Entry::Sub {
                    label: "Solución de problemas",
                    entries: vec![
                        item("Abrir carpeta de ajustes", "", |_, _, _| {
                            let _ = open::that(crate::config::config_dir());
                        }),
                        item("Abrir carpeta de caché", "", |_, _, _| {
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

fn panel(width: f32, rows: Vec<AnyElement>) -> gpui::Div {
    div()
        .w(sc(width))
        .py(sc(PANEL_PAD))
        .px(sc(PANEL_PAD))
        .rounded(sc(12.))
        .bg(gpui::rgb(0x1e1e21))
        .border_1()
        .border_color(gpui::rgba(0xffffff10))
        .shadow_lg()
        .flex()
        .flex_col()
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
                Entry::Sep => div().h(sc(SEP_H)).flex().items_center().child(div().h(sc(1.)).w_full().bg(gpui::rgba(0xffffff12))).into_any_element(),
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
                        .when(open, |d| d.bg(gpui::rgb(0x2c2c31)))
                        .hover(|s| s.bg(gpui::rgb(0x2c2c31)))
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
                        .hover(|s| s.bg(gpui::rgb(0x2c2c31)).pl(sc(14.)))
                        .active(|s| s.opacity(0.7))
                        .child(div().flex_1().whitespace_nowrap().child(label.clone()))
                        .when(!shortcut.is_empty(), |d| d.child(div().text_size(sc(12.)).text_color(theme::text_secondary()).child(*shortcut)))
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
        columns.push(div().mt(sc(top)).child(panel(widths[depth.min(2)], rows)).into_any_element());
        // The next column lines up with the entry that opened it.
        if let Some(&i) = path.get(depth) {
            top += entries_height_before(list, i);
        }
    }

    Some(
        deferred(
            div()
                .absolute()
                .left(sc(8.))
                .top(sc(theme::CAPTION_H + 2.))
                .child(
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
                        .children(columns),
                ),
        )
        .with_priority(3)
        .into_any_element(),
    )
}
