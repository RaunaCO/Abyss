//! Custom title bar (full window width, like the official app): logo,
//! back/forward, search, a drag area and the window buttons.
//!
//! The buttons are ordinary app buttons driving Win32 through `winctl`:
//! declared as native caption buttons, a maximised window never delivered
//! their clicks. Dragging starts only once the mouse moves with the button
//! held, so a double-click (maximise / restore) is not swallowed by the
//! native move loop.

use super::sc;
use super::OneLine;
use gpui::{div, prelude::*, px, AnyElement, Context, Div, Focusable, MouseButton, MouseDownEvent, MouseMoveEvent, Stateful, Window};

use super::icons::Icon;
use super::sidebar::RAIL_W;
use super::winctl;
use super::{theme, AppView, Page};

#[derive(Clone, Copy)]
enum WinButton {
    Min,
    Max,
    Close,
}

fn window_button(id: &'static str, icon: Icon, which: WinButton, width: f32, cx: &mut Context<AppView>) -> AnyElement {
    let close = matches!(which, WinButton::Close);
    let key = format!("wb-{id}");
    let (h, p) = (super::interact::hover(&key), super::interact::press(&key));
    // Close turns red with a white glyph; the others get the hover surface.
    let bg = if close { super::fx::with_alpha(gpui::rgb(0xd13a2c), h * (1.0 - 0.25 * p)) } else { theme::ink(0.10 * h + 0.06 * p) };
    let fg = if close {
        super::interact::mix(theme::text_secondary(), gpui::rgb(0xffffff), h)
    } else {
        super::interact::mix(theme::text_secondary(), theme::text(), h)
    };
    let el = div()
        .id(id)
        .w(sc(width))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .rounded(sc(7.))
        .bg(bg)
        .child(icon.el_scaled(14., fg, 1. + 0.06 * h - 0.12 * p))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |_, _, window, cx| {
            cx.stop_propagation();
            match which {
                WinButton::Close => winctl::close(window),
                WinButton::Max => winctl::toggle_maximize(window),
                WinButton::Min => winctl::minimize(window),
            }
        }));
    super::interact::track(el, key, cx).into_any_element()
}

/// Empty title bar space: drag to move, double-click to maximise/restore.
pub fn drag_area(el: Stateful<Div>, cx: &mut Context<AppView>) -> Stateful<Div> {
    el.on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, ev: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            if ev.click_count >= 2 {
                // Act on release, like the caption buttons: Windows ignores a
                // maximise request while the button is still held down.
                this.title_press = None;
                this.title_double = true;
            } else {
                this.title_press = Some(ev.position);
            }
        }),
    )
    .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, window, _| {
        let Some(start) = this.title_press else { return };
        if ev.pressed_button != Some(MouseButton::Left) {
            this.title_press = None;
            return;
        }
        let moved = (f32::from(ev.position.x) - f32::from(start.x)).abs() + (f32::from(ev.position.y) - f32::from(start.y)).abs();
        if moved > 4.0 {
            this.title_press = None;
            this.win_drag = winctl::drag_begin(window);
        }
    }))
    .on_mouse_up(
        MouseButton::Left,
        cx.listener(|this, _, window, _| {
            this.title_press = None;
            if std::mem::take(&mut this.title_double) {
                winctl::toggle_maximize(window);
            }
        }),
    )
}

/// Breadcrumb of the current page: (label, where the segment leads).
fn crumbs(view: &AppView) -> Vec<(String, Option<Page>)> {
    let browse = (crate::i18n::t("home").to_string(), Some(Page::Home));
    let for_you = (crate::i18n::t("for_you").to_string(), Some(Page::Replay));
    let playlists = (crate::i18n::t("playlists").to_string(), Some(Page::MyPlaylists));
    let here = |s: &str| (s.to_string(), None);
    match &view.page {
        Page::Home => vec![here(crate::i18n::t("home"))],
        Page::MyTracks | Page::MyAlbums | Page::MyArtists => vec![browse, here(crate::i18n::t("library"))],
        Page::Recent => vec![browse, here(crate::i18n::t("history"))],
        Page::Explore => vec![browse, here(crate::i18n::t("explore"))],
        Page::Browse(_, title) => vec![browse, here(title)],
        Page::Search(q) if q.is_empty() => vec![browse, here(crate::i18n::t("search"))],
        Page::Search(q) => vec![browse, (crate::i18n::text("results_for", &[q.to_string()]), None)],
        Page::Album(id) => {
            let t =
                view.albums.get(id).and_then(|l| l.ready()).map(|(a, _)| a.title.clone()).unwrap_or_else(|| crate::i18n::t("album").into());
            vec![browse, here(&t)]
        }
        Page::Artist(id) => {
            let t = view
                .artists
                .get(id)
                .and_then(|l| l.ready())
                .map(|a| a.artist.name.clone())
                .unwrap_or_else(|| crate::i18n::t("artist").into());
            vec![browse, here(&t)]
        }
        Page::Mix(id) => {
            let t =
                view.mixes.get(id).and_then(|l| l.ready()).map(|(m, _)| m.title.clone()).unwrap_or_else(|| crate::i18n::t("mix").into());
            vec![for_you, here(&t)]
        }
        Page::Replay => vec![for_you, here(crate::i18n::t("replay"))],
        Page::Discover => vec![for_you, here(crate::i18n::t("mixes_new_releases"))],
        Page::MyMixes => vec![for_you, here(crate::i18n::t("mixes_and_radio"))],
        Page::MyPlaylists => vec![here(crate::i18n::t("playlists"))],
        Page::Playlist(uuid) => {
            let t = view
                .playlists
                .get(uuid)
                .and_then(|l| l.ready())
                .map(|(p, _)| p.title.clone())
                .or_else(|| view.my_playlists.ready().and_then(|l| l.iter().find(|p| &p.uuid == uuid)).map(|p| p.title.clone()))
                .unwrap_or_else(|| crate::i18n::t("playlist").into());
            vec![playlists, here(&t)]
        }
        Page::Settings => vec![here(crate::i18n::t("settings"))],
    }
}

/// Search and caption geometry shared with the quick search popover.
pub struct Layout {
    pub search_w: f32,
    pub height: f32,
    stacked: bool,
    crumb_w: f32,
    inset: f32,
    icon: f32,
    gap: f32,
    window_button: f32,
}

pub fn layout(view: &AppView, window: &Window) -> Layout {
    let bar_w = f32::from(window.viewport_size().width) / super::zoom() - view.sidebar_width() - theme::FRAME_GAP - 2.0 / super::zoom();
    let compact = bar_w < 380.;
    let (inset, icon, gap, window_button) = if compact { (8., 24., 4., 26.) } else { (16., 30., 8., 34.) };
    let actions_w = 3.0 * (icon + gap + window_button) + 8.0 + 2.0 / super::zoom();
    let stacked = view.signed_in && bar_w < 640.;
    let search_w = if stacked { bar_w - 2.0 * inset } else { bar_w - 2.0 * (actions_w + inset + 10.0) }.clamp(1.0, 380.);
    let crumb_w = if stacked || !view.signed_in { bar_w - 2.0 * inset - actions_w - gap } else { (bar_w - search_w) / 2.0 - inset - 10.0 };
    Layout {
        search_w,
        height: theme::TOP_H + if stacked { 42. } else { 0. },
        stacked,
        crumb_w: crumb_w.max(0.),
        inset,
        icon,
        gap,
        window_button,
    }
}

fn toolbar_button(
    id: &'static str,
    icon: Icon,
    label: &'static str,
    size: f32,
    on_click: fn(&mut AppView, &mut Context<AppView>),
    cx: &mut Context<AppView>,
) -> AnyElement {
    let (hover, press) = (super::interact::hover(id), super::interact::press(id));
    super::interact::track(
        div()
            .id(id)
            .size(sc(size))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(sc(8.))
            .border_1()
            .border_color(theme::control_border())
            .cursor_pointer()
            .bg(theme::liquid_glass((0.8 * hover + 0.2 * press).clamp(0., 1.)))
            .child(super::glass::sheen("toolbar-button-glass", 8., 1.05))
            .child(icon.el_scaled(15., super::interact::mix(theme::text_secondary(), theme::text(), hover), 1.0 - 0.07 * press))
            .tooltip(super::sidebar::tip(label))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx))),
        id,
        cx,
    )
    .into_any_element()
}

/// Breadcrumb, search, theme and window controls. The search takes its own
/// row when it would cover controls on a narrow or zoomed window.
pub fn render(view: &mut AppView, window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    let search_focused = view.search_input.read(cx).focus_handle(cx).is_focused(window);
    let maximized = winctl::is_maximized(window);
    let layout = layout(view, window);

    // Breadcrumb: parents lead somewhere (gradual brighten); the last is the page.
    let mut parts = crumbs(view);
    // Tight: only the current page (the parent comes back when there is room).
    if layout.crumb_w < 300.0 && parts.len() > 1 {
        parts.drain(..parts.len() - 1);
    }
    let n = parts.len();
    let mut crumb_row = div().flex().items_center().gap(sc(6.)).min_w(sc(0.)).overflow_hidden();
    for (i, (label, target)) in parts.into_iter().enumerate() {
        if i > 0 {
            crumb_row = crumb_row.child(div().flex_none().text_color(theme::text_dim()).child("›"));
        }
        let last = i + 1 == n;
        let el = div().id(("crumb", i)).text_size(sc(12.5)).whitespace_nowrap();
        crumb_row = crumb_row.child(match target {
            Some(p) if !last => super::widgets::nav_link_secondary(el.flex_none(), format!("crumb-{i}"), cx)
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.navigate(p.clone(), cx)))
                .into_any_element(),
            _ => el
                .min_w(sc(0.))
                .one_line()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child(label)
                .into_any_element(),
        });
    }

    let light = theme::is_light();
    let theme_switch = toolbar_button(
        "tb-theme",
        if light { Icon::Moon } else { Icon::Sun },
        if light { crate::i18n::t("switch_to_dark_mode") } else { crate::i18n::t("switch_to_light_mode") },
        layout.icon,
        |this, cx| this.set_light_theme(!theme::is_light(), cx),
        cx,
    );
    let settings = toolbar_button(
        "tb-settings",
        Icon::Settings,
        crate::i18n::t("settings"),
        layout.icon,
        |this, cx| this.navigate(Page::Settings, cx),
        cx,
    );

    let upload = toolbar_button(
        "upload",
        Icon::Upload,
        crate::i18n::t("add_local_files"),
        layout.icon,
        |this, cx| {
            this.open_local_settings(cx);
        },
        cx,
    );

    // Window controls in one container.
    let controls = div()
        .h(sc(30.))
        .p(sc(2.))
        .flex()
        .flex_none()
        .items_center()
        .gap(sc(2.))
        .rounded(sc(9.))
        .bg(theme::liquid_glass(0.05))
        .border_1()
        .border_color(theme::control_border())
        .child(super::glass::sheen("window-controls-glass", 9., 0.85))
        .child(window_button("win-min", Icon::WinMin, WinButton::Min, layout.window_button, cx))
        .child(window_button("win-max", if maximized { Icon::WinRestore } else { Icon::WinMax }, WinButton::Max, layout.window_button, cx))
        .child(window_button("win-close", Icon::WinClose, WinButton::Close, layout.window_button, cx));

    let left = div()
        .max_w(sc(layout.crumb_w))
        .min_w(sc(0.))
        .flex()
        .items_center()
        .gap(sc(12.))
        .child(div().flex_shrink().min_w(sc(0.)).child(crumb_row));

    let search = div()
        .id("searchbox")
        .w(sc(layout.search_w))
        .h(sc(34.))
        .px(sc(12.))
        .flex()
        .flex_none()
        .items_center()
        .gap(sc(8.))
        .rounded(sc(10.))
        .bg(theme::liquid_glass(if search_focused { 0.45 } else { 0.05 }))
        .border_1()
        .border_color(if search_focused { theme::text_dim() } else { theme::control_border() })
        .cursor_text()
        .child(super::glass::sheen("search-box-glass", 10., 1.05))
        .child(Icon::Search.el(14., theme::text_secondary()))
        .child(div().flex_1().min_w(sc(0.)).overflow_hidden().child(view.search_input.clone()))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, window, cx| {
            let fh = this.search_input.focus_handle(cx);
            window.focus(&fh);
            // Quick search opens a small menu; the full page only on Enter.
            this.search_open = true;
            cx.notify();
        }));

    let row = div()
        .relative()
        .h(sc(theme::TOP_H) - px(1.))
        .flex_none()
        .w_full()
        .px(sc(layout.inset))
        .flex()
        .items_center()
        .justify_between()
        .child(div().flex().items_center().gap(sc(10.)).when(!view.signed_in, |d| d.child(super::widgets::brand_mark(28.))).child(left))
        .child(
            div().flex().flex_none().items_center().gap(sc(layout.gap)).child(theme_switch).child(settings).child(upload).child(controls),
        );
    let search_row = div().flex().items_center().justify_center();
    let (row, below) = if !view.signed_in {
        (row, None)
    } else if layout.stacked {
        (row, Some(search_row.pb(sc(8.)).child(search)))
    } else {
        (row.child(search_row.absolute().top(sc(0.)).left(sc(0.)).size_full().child(search)), None)
    };
    drag_area(div().id("topbar"), cx)
        .relative()
        .h(sc(layout.height))
        .flex_none()
        .w_full()
        .flex()
        .flex_col()
        .rounded_t(sc(theme::PANEL_R - 1.0))
        .bg(theme::bg())
        .border_b_1()
        .border_color(theme::divider())
        .child(row)
        .children(below)
        .into_any_element()
}
