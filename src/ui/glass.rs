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

/// Lighting for a surface without a border. Place it first in the surface's
/// children so labels stay above it. `radius` is the surface's corner radius.
pub fn sheen(key: impl Into<ElementId>, radius: f32, strength: f32) -> impl IntoElement {
    lighting(key.into(), radius, strength, false)
}

/// Lighting for a surface with a 1 px border (`border_1`). `radius` is the
/// surface's own (outer) corner radius. The hover rim is drawn exactly over
/// that border, so the edge itself brightens instead of a second ring
/// appearing inside it, and the light is clipped to the concentric inner
/// curve. It also draws the lit hairline along the top edge.
pub fn sheen_rim(key: impl Into<ElementId>, radius: f32, strength: f32) -> impl IntoElement {
    lighting(key.into(), radius, strength, true)
}

fn lighting(key: ElementId, radius: f32, strength: f32, bordered: bool) -> impl IntoElement {
    let tint = gpui::rgb(if theme::is_light() { 0x2a2c36 } else { 0xc9cdd9 });
    let forced = forced(&key);
    canvas(
        move |bounds, window, cx| {
            let state = window.use_keyed_state(key, cx, |_, _| Cell::new(Light::default()));
            let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
            (state, hitbox)
        },
        move |bounds, (state, hitbox), window, cx| {
            let now = Instant::now();
            let mut light = state.read(cx).get();
            let hovered = forced || (hitbox.is_hovered(window) && (window.is_window_active() || test_active()));
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
                let (mut target_x, mut target_y) =
                    (normalized(pointer.x - center.x, bounds.size.width), normalized(pointer.y - center.y, bounds.size.height));
                if forced {
                    (target_x, target_y) = (0.45, -0.35);
                }
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
            if bordered {
                paint_glass(bounds, radius, strength, light, window);
            }
            if light.hover > 0. {
                paint(bounds, radius, strength, bordered, light, tint, window);
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
    // All four insets (not `size_full`): the canvas then spans the surface's
    // padding box, i.e. everything inside its border, however much padding the
    // surface has. With a percentage size it only covered the content box, so
    // on a padded panel the light and the rim were drawn well inside the edge.
    .absolute()
    .top(gpui::px(0.))
    .left(gpui::px(0.))
    .right(gpui::px(0.))
    .bottom(gpui::px(0.))
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

/// Width of `border_1`: one logical pixel, whatever the zoom.
const BORDER: f32 = 1.0;

/// `TIDE_GLASS_FORCE=<text>` holds every glass surface whose key contains
/// `<text>` in its hovered state (light at a fixed point), so the hover can be
/// photographed without moving the pointer.
fn forced(key: &ElementId) -> bool {
    static WANT: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let want = WANT.get_or_init(|| std::env::var("TIDE_GLASS_FORCE").ok().filter(|v| !v.is_empty()));
    want.as_ref().is_some_and(|w| key.to_string().contains(w.as_str()))
}

/// A closed rounded outline for a surface whose padding box is `bounds`,
/// `offset` px inside that box (negative: outside it, over the border; the
/// middle of the 1 px border is -0.5) and `width` px wide.
fn outline(bounds: Bounds<Pixels>, radius: f32, offset: f32, width: f32) -> Option<gpui::Path<Pixels>> {
    let (x0, y0) = (f32::from(bounds.origin.x) + offset, f32::from(bounds.origin.y) + offset);
    let (x1, y1) = (x0 + f32::from(bounds.size.width) - 2. * offset, y0 + f32::from(bounds.size.height) - 2. * offset);
    if x1 - x0 < 4. || y1 - y0 < 4. {
        return None;
    }
    // Concentric: the border box's radius, less the border and the offset.
    let r = (radius * super::zoom() - BORDER - offset).clamp(0., (x1 - x0).min(y1 - y0) * 0.5);
    let p = |x: f32, y: f32| gpui::point(gpui::px(x), gpui::px(y));
    let arc = gpui::point(gpui::px(r), gpui::px(r));
    let mut path = gpui::PathBuilder::stroke(gpui::px(width));
    path.move_to(p(x0 + r, y0));
    path.line_to(p(x1 - r, y0));
    path.arc_to(arc, gpui::px(0.), false, true, p(x1, y0 + r));
    path.line_to(p(x1, y1 - r));
    path.arc_to(arc, gpui::px(0.), false, true, p(x1 - r, y1));
    path.line_to(p(x0 + r, y1));
    path.arc_to(arc, gpui::px(0.), false, true, p(x0, y1 - r));
    path.line_to(p(x0, y0 + r));
    path.arc_to(arc, gpui::px(0.), false, true, p(x0 + r, y0));
    path.close();
    path.build().ok()
}

/// What makes a surface read as glass rather than as a grey box:
///
/// * a soft specular wash over its upper part, where light catches the pane;
/// * a bright rim at the edge facing the light and a fainter one on the
///   opposite edge, each fading out toward the middle of the sides (the way a
///   bevelled pane catches a light), drawn over the border itself. At rest the
///   light is at the top left; on hover it moves to the pointer and the rim
///   brightens along every edge, top and bottom included.
fn paint_glass(bounds: Bounds<Pixels>, radius: f32, strength: f32, light: Light, window: &mut gpui::Window) {
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    if w < 6. || h < 6. {
        return;
    }
    let dark = !theme::is_light();
    let (hover, press) = (light.hover, light.press);
    let s = strength.clamp(0.3, 1.2);
    // Not pure white: a cool grey on the dark theme, so the edge reads as
    // reflected light and not as a drawn white line; real white on the light
    // theme (where the far edge is a soft dark one instead).
    let white = |a: f32| with_alpha(gpui::rgb(if dark { 0xc9cdd9 } else { 0xffffff }), a.clamp(0., 1.));
    let shade = |a: f32| with_alpha(gpui::rgb(if dark { 0xc9cdd9 } else { 0x20222c }), a.clamp(0., 1.));

    // Specular wash, in the surface's own rounded shape.
    let r_in = (radius * super::zoom() - BORDER).max(0.).min(w.min(h) * 0.5);
    let wash = if dark { 0.055 + 0.03 * hover } else { 0.55 + 0.1 * hover };
    window.paint_quad(quad(
        bounds,
        gpui::px(r_in),
        theme::gradient(180., gpui::linear_color_stop(white(wash), 0.), gpui::linear_color_stop(white(0.), 0.62)),
        gpui::px(0.),
        gpui::rgba(0x00000000),
        gpui::BorderStyle::default(),
    ));

    // Direction the light comes from: the top left at rest, the pointer on hover.
    let (px_, py_) = (light.x, light.y);
    let pointer_len = (px_ * px_ + py_ * py_).sqrt().max(0.001);
    let (tx, ty) = (-px_ / pointer_len, -py_ / pointer_len);
    let (rx, ry) = (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    // The light only leans toward the pointer: the rim must not swing around as it moves.
    let lean = 0.4 * hover;
    let (dx, dy) = (rx + (tx - rx) * lean, ry + (ty - ry) * lean);
    // Gradient direction (CSS angles: 0 = up, 90 = right) runs from the lit edge to the far one.
    let angle = dx.atan2(-dy).to_degrees();
    let lit = (if dark { 0.30 } else { 0.9 }) + (0.14 * hover + 0.06 * press) * s;
    let far = (if dark { 0.10 } else { 0.07 }) + 0.05 * hover * s;
    // The lit rim never fades to nothing (a floor of a third), so the edge is
    // one continuous line all the way round and no stretch of it goes missing.
    let to_lit = |a: f32| theme::gradient(angle, gpui::linear_color_stop(white(a), 0.), gpui::linear_color_stop(white(a * 0.33), 0.62));
    let to_far = |a: f32| theme::gradient(angle + 180., gpui::linear_color_stop(shade(a), 0.), gpui::linear_color_stop(shade(0.), 0.5));
    if let Some(rim) = outline(bounds, radius, -BORDER * 0.5, BORDER) {
        window.paint_path(rim.clone(), to_lit(lit));
        window.paint_path(rim, to_far(far));
    }
    // The thickness of the pane: a second, softer rim just inside the first,
    // which is what lets the edge read as a bevel and not as a drawn line.
    if let Some(inner) = outline(bounds, radius, 1.4, 1.6) {
        window.paint_path(inner.clone(), to_lit(if dark { 0.07 + 0.07 * hover } else { 0.6 }));
        window.paint_path(inner, to_far(if dark { 0.03 + 0.03 * hover } else { 0.025 }));
    }
}

/// Two diffuse layers follow the pointer together across the whole surface,
/// and the edge brightens. `bounds` is the area inside the border, if any.
fn paint(bounds: Bounds<Pixels>, radius: f32, strength: f32, bordered: bool, light: Light, tint: gpui::Rgba, window: &mut gpui::Window) {
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    if w < 2. || h < 2. {
        return;
    }
    let inset = if bordered { BORDER } else { 0. };
    // Concentric with the surface's outer corner.
    let r = (radius * super::zoom() - inset).max(0.).min(w.min(h) * 0.5);
    let glow = strength * light.hover;
    // Soft: it only lifts the surface a little. On the light theme it darkens
    // even less, since a grey patch on white soon looks like a stain.
    let spot_alpha = (glow * (0.085 + 0.04 * light.press) * if theme::is_light() { 0.4 } else { 1.0 }).min(0.2);

    // Scale the glow to the surface and cap its spread on large panels.
    let size = (w.min(h) * 0.95).clamp(26., 340.).min(w.max(h));
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
                blur_radius: gpui::px(diameter * 0.7),
                spread_radius: gpui::px(-diameter * 0.08),
            };
            window.paint_shadows(spot, gpui::Corners::all(half), &[shadow]);
        }
    });

    // Surfaces with a border get the gradient rim from `paint_glass`; the
    // others get a plain one on their edge.
    if bordered {
        return;
    }
    let edge = bounds.dilate(gpui::px(inset));
    window.paint_quad(quad(
        edge,
        gpui::px(r + inset),
        gpui::rgba(0x00000000),
        gpui::px(BORDER),
        with_alpha(tint, glow * (0.10 + 0.08 * light.press)),
        gpui::BorderStyle::default(),
    ));
}

/// Run `f` once per rectangle of a set that tiles the inside of a rounded
/// rectangle (GPUI only clips to rectangles): the straight middle band, then
/// thin strips that follow each pair of corners along the arc. The strips are
/// at most 1.5 px tall, so the light ends on the curve itself and never shows
/// outside it or stops short of it in steps.
fn clip_rounded(b: Bounds<Pixels>, r: f32, window: &mut gpui::Window, mut f: impl FnMut(&mut gpui::Window)) {
    let (l, t) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (rt, bt) = (l + f32::from(b.size.width), t + f32::from(b.size.height));
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| Bounds {
        origin: gpui::point(gpui::px(x0), gpui::px(y0)),
        size: gpui::size(gpui::px((x1 - x0).max(0.)), gpui::px((y1 - y0).max(0.))),
    };
    let mut rects = vec![rect(l, t + r, rt, bt - r)];
    let strips = ((r / 1.5).ceil() as usize).clamp(1, 24);
    let step = r / strips as f32;
    for i in 0..strips {
        if r <= 0.01 {
            break;
        }
        // Distance of the strip's middle from the top (or bottom) edge.
        let (y0, y1) = (i as f32 * step, (i + 1) as f32 * step);
        let dy = r - (y0 + y1) * 0.5;
        let dx = r - (r * r - dy * dy).max(0.).sqrt();
        rects.push(rect(l + dx, t + y0, rt - dx, t + y1));
        rects.push(rect(l + dx, bt - y1, rt - dx, bt - y0));
    }
    for mask in rects {
        if mask.size.width > gpui::px(0.) && mask.size.height > gpui::px(0.) {
            window.with_content_mask(Some(gpui::ContentMask { bounds: mask }), |window| f(window));
        }
    }
}
