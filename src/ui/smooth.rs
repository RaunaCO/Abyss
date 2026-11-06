//! Smooth scrolling for mouse wheels: wheel "lines" are turned into a short
//! eased glide instead of an instant jump. Touchpads (pixel deltas) keep their
//! native, already-smooth behaviour.

use super::sc;
use std::time::Duration;

use gpui::{canvas, px, Bounds, Context, DispatchPhase, Entity, IntoElement, Pixels, ScrollDelta, ScrollWheelEvent, Styled};

use super::AppView;

/// Pixels per wheel line: same distance as gpui's native list scrolling.
const PX_PER_LINE: f32 = 20.0;
/// Fraction of the remaining distance covered each frame (ease-out).
const EASE: f32 = 0.2;
const FRAME: Duration = Duration::from_millis(7);

impl AppView {
    pub fn smooth_scroll(&mut self, distance: f32, cx: &mut Context<Self>) {
        // Reversing direction cancels the rest of the previous glide.
        if self.scroll_pending.signum() != distance.signum() {
            self.scroll_pending = 0.0;
        }
        self.scroll_pending += distance;
        if self.scroll_animating {
            return;
        }
        self.scroll_animating = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(FRAME).await;
            let more = this
                .update(cx, |this, cx| {
                    let rest = this.scroll_pending;
                    let step = if rest.abs() <= 1.0 { rest } else { rest * EASE };
                    this.scroll_pending -= step;
                    // Stop at the top instead of eating the rest of the glide.
                    let top = this.list.logical_scroll_top();
                    let at_top = top.item_ix == 0 && top.offset_in_item <= sc(0.);
                    if step < 0.0 && at_top {
                        this.scroll_pending = 0.0;
                    } else {
                        this.list.scroll_by(sc(step));
                        cx.notify();
                    }
                    if this.scroll_pending.abs() < 0.5 {
                        this.scroll_pending = 0.0;
                        this.scroll_animating = false;
                        false
                    } else {
                        true
                    }
                })
                .unwrap_or(false);
            if !more {
                break;
            }
        })
        .detach();
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
                let ScrollDelta::Lines(lines) = ev.delta else { return };
                // Ctrl + wheel is the zoom, not scrolling.
                if lines.y == 0.0 || ev.modifiers.shift || ev.modifiers.control {
                    return;
                }
                let blocked = view.read(cx).dialog.is_some() || view.read(cx).menu.is_some() || view.read(cx).now_playing_open;
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
