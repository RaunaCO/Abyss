//! Offscreen renders of panel states `page_shots` does not reach: hovered
//! tooltips (drawn through their overlay), selected / playing / hovered
//! track rows, menus at the window's edges, long texts and narrow widths.
//! `cargo test --bin Abyss -- --include-ignored panel_shots` (same
//! environment as `page_shots`; PNGs go to `ABYSS_SHOT_DIR`).

use super::super::*;
use super::shots::{renderer_new, save};
use iced::advanced::{
    layout,
    renderer::{Headless, Renderer as _},
    widget::Tree,
    Layout,
};
use iced::widget::container;
use iced::{Color, Rectangle, Renderer, Vector};

/// Draw `el` at `size` with the pointer at `cursor`, its overlays included
/// (the tooltip's bubble lives in one), after `wait` with the pointer resting.
fn draw_with_overlay(renderer: &mut Renderer, mut el: Element<'_, Message>, size: Size, cursor: Point, wait: Duration) -> Vec<u8> {
    let mut tree = Tree::new(el.as_widget());
    let frame = Rectangle::with_size(size);
    let cur = mouse::Cursor::Available(cursor);
    let mut messages = Vec::new();
    let mut node = el.as_widget_mut().layout(&mut tree, renderer, &layout::Limits::new(size, size));
    let mut send = |el: &mut Element<'_, Message>, tree: &mut Tree, node: &layout::Node, event: iced::Event| {
        el.as_widget_mut().update(
            tree,
            &event,
            Layout::new(node),
            cur,
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut iced::advanced::Shell::new(&mut messages),
            &frame,
        );
    };
    send(&mut el, &mut tree, &node, iced::Event::Mouse(mouse::Event::CursorMoved { position: cursor }));
    let start = Instant::now();
    while start.elapsed() < wait {
        std::thread::sleep(Duration::from_millis(40));
        send(&mut el, &mut tree, &node, iced::Event::Window(window::Event::RedrawRequested(Instant::now())));
    }
    node = el.as_widget_mut().layout(&mut tree, renderer, &layout::Limits::new(size, size));
    send(&mut el, &mut tree, &node, iced::Event::Window(window::Event::RedrawRequested(Instant::now())));
    renderer.reset(frame);
    let theme = iced::Theme::Dark;
    let style = iced::advanced::renderer::Style { text_color: theme::text() };
    el.as_widget().draw(&tree, renderer, &theme, &style, Layout::new(&node), cur, &frame);
    if let Some(mut overlay) = el.as_widget_mut().overlay(&mut tree, Layout::new(&node), renderer, &frame, Vector::ZERO) {
        let over = overlay.as_overlay_mut().layout(renderer, size);
        overlay.as_overlay().draw(renderer, &theme, &style, Layout::new(&over), cur);
    }
    Headless::screenshot(renderer, Size::new(size.width as u32, size.height as u32), 1.0, Color::BLACK)
}

/// Pixels of `px` (RGBA, `w` wide) inside `r` that differ clearly from the
/// colour at its top-left corner: something (text) is drawn there.
fn ink_in(px: &[u8], w: u32, r: Rectangle) -> usize {
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2]]
    };
    let base = at(r.x as u32, r.y as u32);
    let mut n = 0;
    for y in r.y as u32..(r.y + r.height) as u32 {
        for x in r.x as u32..(r.x + r.width) as u32 {
            let c = at(x, y);
            if c.iter().zip(base).any(|(a, b)| a.abs_diff(b) > 60) {
                n += 1;
            }
        }
    }
    n
}

/// A hovered tooltip shows its label (it used to draw an empty capsule: the
/// tooltip overlay draws its bubble with an infinite viewport), centred
/// under its button, and stays inside the window at the right edge.
#[test]
#[ignore = "requires a wgpu adapter; writes PNGs for review"]
fn panel_shots_tooltips() {
    let _theme = theme::THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut renderer = renderer_new();
    let size = Size::new(360.0, 120.0);
    for light in [false, true] {
        theme::set_light(light);
        for (name, align) in [("middle", iced::alignment::Horizontal::Center), ("right-edge", iced::alignment::Horizontal::Right)] {
            let button = widgets::icon_button(icons::Icon::Settings, 16.0, theme::text(), Message::Navigate(crate::shared::Page::Settings));
            let el: Element<'_, Message> = container(widgets::tip(button, "Settings and preferences"))
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(8)
                .align_x(align)
                .style(|_| container::Style { background: Some(theme::frame().into()), ..Default::default() })
                .into();
            // The 28 px button: centred, or against the right edge.
            let bx = if name == "middle" { (size.width - 28.0) / 2.0 } else { size.width - 8.0 - 28.0 };
            let px = draw_with_overlay(&mut renderer, el, size, Point::new(bx + 14.0, 22.0), Duration::from_millis(700));
            let file = format!("{}-tooltip-{name}", if light { "light" } else { "dark" });
            save(&file, size, &px);
            // The bubble's label sits under the button (36..70 px down): its glyphs must show.
            let band = Rectangle { x: 8.0, y: 44.0, width: size.width - 16.0, height: 14.0 };
            assert!(ink_in(&px, size.width as u32, band) > 40, "{file}: the tooltip shows no label");
        }
    }
    theme::set_light(false);
}
