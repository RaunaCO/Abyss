//! Pointer-driven lighting for glass surfaces. The canvas paints behind the
//! content and never consumes clicks or scroll events.

use std::{cell::Cell, time::Instant};

use gpui::{
    canvas, quad, Bounds, DispatchPhase, ElementId, HitboxBehavior, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Styled,
};

use super::{fx::with_alpha, sc, theme};

#[derive(Clone, Copy)]
struct Light {
    x: f32,
    y: f32,
    hover: f32,
    press: f32,
    held: bool,
    last_frame: Instant,
}

impl Default for Light {
    fn default() -> Self {
        Self { x: 0., y: -0.5, hover: 0., press: 0., held: false, last_frame: Instant::now() }
    }
}

/// Place this first in a surface's children so labels stay above the lighting.
pub fn sheen(key: impl Into<ElementId>, radius: f32, strength: f32) -> impl IntoElement {
    let key = key.into();
    let tint = gpui::rgb(if theme::is_light() { 0x6e6e6e } else { 0xffffff });
    canvas(
        move |bounds, window, cx| {
            let state = window.use_keyed_state(key, cx, |_, _| Cell::new(Light::default()));
            let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
            (state, hitbox)
        },
        move |bounds, (state, hitbox), window, cx| {
            let now = Instant::now();
            let mut light = state.read(cx).get();
            let hovered = hitbox.is_hovered(window) && (window.is_window_active() || test_active());
            let dt = now.duration_since(light.last_frame).as_secs_f32().clamp(1. / 240., 0.05);
            light.last_frame = now;
            let approach = |value: f32, target: f32, rate: f32| {
                let next = target + (value - target) * (-rate * dt).exp();
                if (next - target).abs() < 0.004 {
                    target
                } else {
                    next
                }
            };
            let mut moving = false;
            if hovered {
                let pointer = window.mouse_position();
                let center = bounds.center();
                let target_x = normalized(pointer.x - center.x, bounds.size.width);
                let target_y = normalized(pointer.y - center.y, bounds.size.height);
                if light.hover == 0. {
                    (light.x, light.y) = (target_x, target_y);
                } else {
                    light.x = approach(light.x, target_x, 46.);
                    light.y = approach(light.y, target_y, 46.);
                }
                moving = light.x != target_x || light.y != target_y;
            }
            let hover = if hovered { 1. } else { 0. };
            let press = if hovered && light.held { 1. } else { 0. };
            light.hover = approach(light.hover, hover, if hovered { 23. } else { 10. });
            light.press = approach(light.press, press, if press > 0. { 36. } else { 13. });
            state.read(cx).set(light);
            if moving || light.hover != hover || light.press != press {
                window.request_animation_frame();
            }
            if light.hover > 0. {
                paint(bounds, radius, strength, light, tint, window);
            }

            let (movement_hitbox, down_hitbox) = (hitbox.clone(), hitbox);
            let (movement_state, down_state) = (state.clone(), state.clone());
            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let mut light = movement_state.read(cx).get();
                if light.held && ev.pressed_button != Some(MouseButton::Left) {
                    light.held = false;
                    movement_state.read(cx).set(light);
                }
                if movement_hitbox.is_hovered(window) || light.hover > 0. {
                    window.refresh();
                }
            });
            window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture && ev.button == MouseButton::Left && down_hitbox.is_hovered(window) {
                    let mut light = down_state.read(cx).get();
                    light.held = true;
                    light.press = light.press.max(0.55);
                    down_state.read(cx).set(light);
                    window.refresh();
                }
            });
            window.on_mouse_event(move |ev: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture && ev.button == MouseButton::Left {
                    let mut light = state.read(cx).get();
                    if light.held {
                        light.held = false;
                        state.read(cx).set(light);
                        window.refresh();
                    }
                }
            });
        },
    )
    .absolute()
    .top(sc(0.))
    .left(sc(0.))
    .size_full()
}

/// `TIDE_GLASS_TEST=1` lights surfaces in an inactive window, so screenshots of
/// an isolated test instance can show the lighting without taking focus.
fn test_active() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("TIDE_GLASS_TEST").is_ok_and(|v| v == "1"))
}

fn normalized(offset: Pixels, size: Pixels) -> f32 {
    (2. * f32::from(offset) / f32::from(size).max(1.)).clamp(-1., 1.)
}

/// Two diffuse layers follow the pointer together across the whole surface.
fn paint(bounds: Bounds<Pixels>, radius: f32, strength: f32, light: Light, tint: gpui::Rgba, window: &mut gpui::Window) {
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    if w < 2. || h < 2. {
        return;
    }
    let r = (radius * super::zoom()).min(w.min(h) * 0.5);
    let glow = strength * light.hover;
    let spot_alpha = (glow * (0.12 + 0.06 * light.press)).min(0.25);

    // Scale the glow to the surface and cap its spread on large panels.
    let size = (w.min(h) * 0.95).clamp(26., 240.).min(w.max(h));
    let center = bounds.center();
    let pointer = gpui::point(center.x + gpui::px(light.x * w * 0.5), center.y + gpui::px(light.y * h * 0.5));
    clip_rounded(bounds, r, window, |window| {
        for (diameter, opacity) in [(size, spot_alpha), (size * 1.6, spot_alpha * 0.35)] {
            let half = gpui::px(diameter * 0.5);
            let spot = Bounds {
                origin: gpui::point(pointer.x - half, pointer.y - half),
                size: gpui::size(gpui::px(diameter), gpui::px(diameter)),
            };
            let shadow = gpui::BoxShadow {
                color: gpui::Hsla::from(with_alpha(tint, opacity)),
                offset: gpui::point(gpui::px(0.), gpui::px(0.)),
                blur_radius: gpui::px(diameter * 0.55),
                spread_radius: gpui::px(-diameter * 0.08),
            };
            window.paint_shadows(spot, gpui::Corners::all(half), &[shadow]);
        }
    });

    window.paint_quad(quad(
        bounds,
        gpui::px(r),
        gpui::rgba(0x00000000),
        sc(1.),
        with_alpha(tint, glow * (0.08 + 0.08 * light.press)),
        gpui::BorderStyle::default(),
    ));
}

/// Run `f` once per rectangle of a set that tiles the inside of a rounded
/// rectangle (GPUI only clips to rectangles): the middle band, the side bands
/// and, at each corner, the square that lies inside the arc. Light painted this
/// way never shows outside the rounded corners.
fn clip_rounded(b: Bounds<Pixels>, r: f32, window: &mut gpui::Window, mut f: impl FnMut(&mut gpui::Window)) {
    let (l, t) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (rt, bt) = (l + f32::from(b.size.width), t + f32::from(b.size.height));
    // 1 - 1/sqrt(2): a square inset this far from the corner touches the arc.
    let k = r * 0.2929;
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| Bounds {
        origin: gpui::point(gpui::px(x0), gpui::px(y0)),
        size: gpui::size(gpui::px((x1 - x0).max(0.)), gpui::px((y1 - y0).max(0.))),
    };
    let rects = [
        rect(l + r, t, rt - r, bt),
        rect(l, t + r, l + r, bt - r),
        rect(rt - r, t + r, rt, bt - r),
        rect(l + k, t + k, l + r, t + r),
        rect(rt - r, t + k, rt - k, t + r),
        rect(l + k, bt - r, l + r, bt - k),
        rect(rt - r, bt - r, rt - k, bt - k),
    ];
    for mask in rects {
        if mask.size.width > gpui::px(0.) && mask.size.height > gpui::px(0.) {
            window.with_content_mask(Some(gpui::ContentMask { bounds: mask }), |window| f(window));
        }
    }
}
