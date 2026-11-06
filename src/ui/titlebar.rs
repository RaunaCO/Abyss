//! Custom title bar (full window width, like the official app): logo,
//! back/forward, search, a drag area and the window buttons.
//!
//! The buttons are ordinary app buttons driving Win32 through `winctl`:
//! declared as native caption buttons, a maximised window never delivered
//! their clicks. Dragging starts only once the mouse moves with the button
//! held, so a double-click (maximise / restore) is not swallowed by the
//! native move loop.

use super::sc;
use gpui::{div, prelude::*, px, AnyElement, Context, Div, Focusable, MouseButton, MouseDownEvent, MouseMoveEvent, Rgba, Stateful, Window};

use super::icons::Icon;
use super::sidebar::RAIL_W;
use super::widgets::{cover, icon_button};
use super::winctl;
use super::{theme, AppView, Page};


#[derive(Clone, Copy)]
enum WinButton {
    Min,
    Max,
    Close,
}

fn window_button(id: &'static str, icon: Icon, which: WinButton, cx: &mut Context<AppView>) -> AnyElement {
    let hover_bg: Rgba = if matches!(which, WinButton::Close) { gpui::rgb(0xc42b1c) } else { theme::hover() };
    div()
        .id(id)
        .w(sc(42.))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .hover(move |s| s.bg(hover_bg))
        .active(move |s| s.bg(hover_bg).opacity(0.7))
        .child(icon.el(13., theme::text_secondary()))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |_, _, window, cx| {
            cx.stop_propagation();
            match which {
                WinButton::Close => winctl::close(window),
                WinButton::Max => winctl::toggle_maximize(window),
                WinButton::Min => winctl::minimize(window),
            }
        }))
        .into_any_element()
}

/// Empty title bar space: drag to move, double-click to maximise/restore.
fn drag_area(el: Stateful<Div>, cx: &mut Context<AppView>) -> Stateful<Div> {
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

pub fn render(view: &mut AppView, window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    let back_c = if view.can_go_back() { theme::text() } else { theme::text_dim() };
    let fwd_c = if view.can_go_forward() { theme::text() } else { theme::text_dim() };
    let search_focused = view.search_input.read(cx).focus_handle(cx).is_focused(window);
    let maximized = winctl::is_maximized(window);
    let side_w = view.sidebar_width_animated();
    let avatar = view.settings.lock().user_picture.clone().map(|p| crate::tidal::image_url(&p, 80));

    // Row 1: window caption: menu, name, drag area, window buttons.
    let caption = div()
        .id("caption")
        .h(sc(theme::CAPTION_H))
        .flex_none()
        .w_full()
        .flex()
        .items_center()
        .child(
            div()
                .id("menu-toggle")
                .w(sc(RAIL_W))
                .h_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .active(|s| s.opacity(0.6))
                .child(Icon::Menu.el(15., theme::text_secondary()))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, _, cx| this.toggle_app_menu(cx))),
        )
        .child(
            drag_area(div().id("title-drag"), cx)
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .text_size(sc(11.))
                .font_weight(gpui::FontWeight::BOLD)
                .child("TIDE"),
        )
        .child(
            div()
                .h_full()
                .flex()
                .child(window_button("win-min", Icon::WinMin, WinButton::Min, cx))
                .child(window_button("win-max", if maximized { Icon::WinRestore } else { Icon::WinMax }, WinButton::Max, cx))
                .child(window_button("win-close", Icon::WinClose, WinButton::Close, cx)),
        );

    // Row 2: sidebar toggle (over the rail), back/forward pill, search and account.
    let header = div()
        .h(sc(theme::HEADER_H))
        .flex_none()
        .w_full()
        .flex()
        .items_center()
        .child(
            div()
                .w(sc(RAIL_W))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .id("sidebar-toggle")
                        .size(sc(30.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(sc(8.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .active(|s| s.opacity(0.6))
                        .child(Icon::Collapse.el(18., theme::text_secondary()))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
                ),
        )
        .child(
            div()
                .flex_none()
                .h(sc(30.))
                .px(sc(3.))
                .flex()
                .items_center()
                .rounded(sc(15.))
                .bg(theme::surface())
                .child(icon_button("back", Icon::Back, 17., back_c, |this, _, _, cx| this.go_back(cx), cx))
                .child(icon_button("fwd", Icon::Forward, 17., fwd_c, |this, _, _, cx| this.go_forward(cx), cx)),
        )
        .child(drag_area(div().id("header-drag"), cx).flex_1().h_full())
        .child(
            div()
                .id("searchbox")
                .w(sc(210.))
                .h(sc(30.))
                .px(sc(11.))
                .flex()
                .flex_none()
                .items_center()
                .gap(sc(8.))
                .rounded(sc(15.))
                .bg(theme::surface())
                .border_1()
                .border_color(if search_focused { theme::text_dim() } else { theme::surface() })
                .cursor_text()
                .hover(|s| s.border_color(theme::border()).bg(theme::hover()))
                .child(Icon::Search.el(14., theme::text_secondary()))
                .child(view.search_input.clone())
                .on_click(cx.listener(|this, _, window, cx| {
                    let fh = this.search_input.focus_handle(cx);
                    window.focus(&fh);
                    if !matches!(this.page, Page::Search(_)) {
                        this.navigate(Page::Search(String::new()), cx);
                    }
                })),
        )
        .child(
            div()
                .id("avatar")
                .mx(sc(14.))
                .flex_none()
                .cursor_pointer()
                .active(|s| s.opacity(0.6))
                .child(cover(avatar, 28., true, cx))
                .on_click(cx.listener(|this, _, _, cx| this.navigate(Page::Settings, cx))),
        );

    div()
        .id("titlebar")
        .flex_none()
        .w_full()
        .flex()
        .flex_col()
        .child(caption)
        .child(header)
        .into_any_element()
}
