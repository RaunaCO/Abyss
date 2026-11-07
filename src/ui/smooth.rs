//! Smooth scrolling for mouse wheels: wheel "lines" are turned into a short
//! eased glide instead of an instant jump. Touchpads (pixel deltas) keep their
//! native, already-smooth behaviour.

use super::sc;

use gpui::{canvas, px, Bounds, Context, DispatchPhase, Entity, IntoElement, Pixels, ScrollDelta, ScrollWheelEvent, Styled};

use super::AppView;

/// Pixels per wheel line (a notch is 3 lines, ~100 px, like Chromium / TIDAL).
const PX_PER_LINE: f32 = 33.0;
/// Time constant of the glide: 63 % of the distance is covered in this time.
const GLIDE_TAU_S: f32 = 0.075;

impl AppView {
    /// Queue a wheel distance; the glide itself runs in `step_smooth_scroll`, once
    /// per displayed frame, so it is in step with the screen at any refresh rate
    /// (a timer-driven glide stuttered: Windows timers tick every ~15 ms).
    pub fn smooth_scroll(&mut self, distance: f32, cx: &mut Context<Self>) {
        // Reversing direction cancels the rest of the previous glide.
        if self.scroll_pending.signum() != distance.signum() {
            self.scroll_pending = 0.0;
        }
        self.scroll_pending += distance;
        if !self.scroll_animating {
            self.scroll_animating = true;
            self.scroll_last = std::time::Instant::now();
        }
        cx.notify();
    }

    /// Advance the glide by the time since the last frame (exponential ease-out).
    pub fn step_smooth_scroll(&mut self) {
        if !self.scroll_animating {
            return;
        }
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.scroll_last).as_secs_f32().min(0.05);
        self.scroll_last = now;
        let rest = self.scroll_pending;
        let step = if rest.abs() <= 0.5 { rest } else { rest * (1.0 - (-dt / GLIDE_TAU_S).exp()) };
        self.scroll_pending -= step;
        // Stop at the top instead of eating the rest of the glide.
        let top = self.list.logical_scroll_top();
        let at_top = top.item_ix == 0 && top.offset_in_item <= sc(0.);
        if step < 0.0 && at_top {
            self.scroll_pending = 0.0;
        } else if step != 0.0 {
            self.list.scroll_by(sc(step));
        }
        if self.scroll_pending.abs() < 0.5 {
            self.scroll_pending = 0.0;
            self.scroll_animating = false;
        }
    }

    pub fn stop_smooth_scroll(&mut self) {
        self.scroll_pending = 0.0;
    }
}

/// Ctrl + wheel anywhere in the window changes the zoom (touchpad pinch too).
pub fn zoom_catcher(view: Entity<AppView>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let view = view.clone();
            window.on_mouse_event(move |ev: &ScrollWheelEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture || !ev.modifiers.control {
                    return;
                }
                let dy = match ev.delta {
                    ScrollDelta::Lines(l) => l.y,
                    ScrollDelta::Pixels(p) => f32::from(p.y),
                };
                if dy == 0.0 {
                    return;
                }
                log::info!("ctrl-wheel dy={dy} delta={:?} at {:?}", ev.delta, ev.position);
                view.update(cx, |v, cx| v.step_zoom(if dy > 0.0 { 1 } else { -1 }, cx));
                cx.stop_propagation();
            });
        },
    )
    .absolute()
    .size_full()
}

/// An invisible layer over the page list that catches wheel events first.
pub fn wheel_catcher(view: Entity<AppView>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let view = view.clone();
            window.on_mouse_event(move |ev: &ScrollWheelEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture || !bounds.contains(&ev.position) {
                    return;
                }
                // Gesture: horizontal touchpad swipe / tilt wheel = back / forward.
                let (dx, dy) = match ev.delta {
                    ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
                    ScrollDelta::Lines(l) => (l.x * 60.0, l.y),
                };
                if dx != 0.0 && dx.abs() > dy.abs() * 1.5 && !ev.modifiers.shift {
                    view.update(cx, |v, cx| v.swipe(dx, cx));
                    cx.stop_propagation();
                    return;
                }
                let ScrollDelta::Lines(lines) = ev.delta else { return };
                // Ctrl + wheel is the zoom, not scrolling.
                if lines.y == 0.0 || ev.modifiers.shift || ev.modifiers.control {
                    return;
                }
                let blocked = view.read(cx).dialog.is_some()
                    || view.read(cx).menu.is_some()
                    || view.read(cx).now_playing_open
                    || view.read(cx).search_open;
                if blocked {
                    return;
                }
                view.update(cx, |v, cx| v.smooth_scroll(-lines.y * PX_PER_LINE, cx));
                cx.stop_propagation();
            });
        },
    )
    .absolute()
    .size_full()
}
