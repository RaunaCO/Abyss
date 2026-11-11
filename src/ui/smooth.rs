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
        let step = if super::motion::reduced_motion() || rest.abs() <= 0.5 { rest } else { rest * (1.0 - (-dt / GLIDE_TAU_S).exp()) };
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

/// Slider dragging (progress, volume): mouse moves and the release are taken in
/// the capture phase, so the drag follows the pointer wherever it is, over
/// Now Playing, menus or outside the window, until the button is released.
pub fn drag_catcher(view: Entity<AppView>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let v = view.clone();
            window.on_mouse_event(move |ev: &gpui::MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture || v.read(cx).drag.is_none() {
                    return;
                }
                if ev.pressed_button == Some(gpui::MouseButton::Left) {
                    v.update(cx, |v, cx| v.drag_to(ev.position, cx));
                }
            });
            let v = view.clone();
            window.on_mouse_event(move |ev: &gpui::MouseUpEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture || ev.button != gpui::MouseButton::Left || v.read(cx).drag.is_none() {
                    return;
                }
                super::winctl::release_mouse();
                v.update(cx, |v, cx| {
                    v.end_drag(cx);
                    cx.notify();
                });
            });
        },
    )
    .absolute()
    .size_full()
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

// ------------------------------------------------------------------ panels
//
// The same glide for the panels that scroll on their own (lyrics, queue,
// search menu, credits, sidebar playlists...). Each panel adds a `glide(key)`
// child; during a wheel event every glide under the pointer claims the event
// (the innermost one painted last wins) and `wheel_resolver`, painted after
// everything, applies it. Nothing runs while no glide is moving.

use std::cell::RefCell;
use std::collections::HashMap;

struct Glide {
    handle: gpui::ScrollHandle,
    pending: f32,
    last: std::time::Instant,
}

thread_local! {
    static GLIDES: RefCell<HashMap<&'static str, Glide>> = RefCell::new(HashMap::new());
    /// The panel that claimed the wheel event being dispatched (and where).
    static CLAIM: RefCell<Option<(&'static str, gpui::Point<Pixels>)>> = const { RefCell::new(None) };
}

/// The scroll handle of panel `key` (created on first use), for
/// `.track_scroll(...)` on the panel's scroll container.
pub fn handle(key: &'static str) -> gpui::ScrollHandle {
    GLIDES.with(|g| {
        g.borrow_mut()
            .entry(key)
            .or_insert_with(|| Glide { handle: gpui::ScrollHandle::new(), pending: 0.0, last: std::time::Instant::now() })
            .handle
            .clone()
    })
}

/// Use an existing handle for panel `key` (e.g. the lyrics, whose handle the
/// view also drives for the auto-follow).
pub fn register(key: &'static str, h: &gpui::ScrollHandle) {
    GLIDES.with(|g| {
        let mut g = g.borrow_mut();
        let e = g.entry(key).or_insert_with(|| Glide { handle: h.clone(), pending: 0.0, last: std::time::Instant::now() });
        e.handle = h.clone();
    });
}

/// Stop panel `key`'s glide (something else moves it now, e.g. the lyrics follow).
pub fn stop(key: &'static str) {
    GLIDES.with(|g| {
        if let Some(e) = g.borrow_mut().get_mut(key) {
            e.pending = 0.0;
        }
    });
}

/// Advance `key`'s glide one frame; true while it is still moving.
fn step(key: &'static str) -> bool {
    GLIDES.with(|g| {
        let mut g = g.borrow_mut();
        let Some(e) = g.get_mut(key) else { return false };
        if e.pending == 0.0 {
            return false;
        }
        let now = std::time::Instant::now();
        let dt = now.duration_since(e.last).as_secs_f32().min(0.05);
        e.last = now;
        let rest = e.pending;
        let s = if super::motion::reduced_motion() || rest.abs() <= 0.5 { rest } else { rest * (1.0 - (-dt / GLIDE_TAU_S).exp()) };
        e.pending -= s;
        let off = e.handle.offset();
        let max = f32::from(e.handle.max_offset().height);
        // Offsets are negative going down; distances are positive going down.
        let y = (f32::from(off.y) - s * super::zoom()).clamp(-max, 0.0);
        if (y - f32::from(off.y)).abs() < 0.01 && s != 0.0 {
            // Hit an end: drop the rest instead of pushing against it.
            e.pending = 0.0;
        }
        e.handle.set_offset(gpui::point(off.x, px(y)));
        if e.pending.abs() < 0.5 {
            e.pending = 0.0;
            false
        } else {
            super::motion::want_frame("scroll");
            true
        }
    })
}

/// Invisible child of a scrolling panel: steps its glide each frame and claims
/// wheel notches over it (unless something covers the panel there).
pub fn glide(key: &'static str) -> impl IntoElement {
    canvas(
        move |_, window, _| {
            // This runs while painting, after the frame was scheduled: ask
            // for the next one here, or an idle window would stop the glide.
            if step(key) {
                window.request_animation_frame();
            }
            let b = GLIDES.with(|g| g.borrow().get(key).map(|e| e.handle.bounds()))?;
            Some(window.insert_hitbox(b, gpui::HitboxBehavior::Normal))
        },
        move |_, hitbox: Option<gpui::Hitbox>, window, _| {
            let Some(hitbox) = hitbox else { return };
            window.on_mouse_event(move |ev: &ScrollWheelEvent, phase, window, _| {
                if phase != DispatchPhase::Capture || !matches!(ev.delta, ScrollDelta::Lines(_)) {
                    return;
                }
                if hitbox.bounds.contains(&ev.position) && hitbox.is_hovered(window) {
                    CLAIM.with(|c| *c.borrow_mut() = Some((key, ev.position)));
                }
            });
        },
    )
    .absolute()
    .size(px(0.))
}

/// Applies the wheel notch claimed by a panel (see `glide`). Must be painted
/// after every panel, so its capture listener runs last.
pub fn wheel_resolver(view: Entity<AppView>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let view = view.clone();
            window.on_mouse_event(move |ev: &ScrollWheelEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let Some((key, at)) = CLAIM.with(|c| c.borrow_mut().take()) else { return };
                let ScrollDelta::Lines(lines) = ev.delta else { return };
                if at != ev.position || ev.modifiers.control {
                    return;
                }
                // Shift + wheel scrolls sideways: leave it to the panel.
                let dy = if ev.modifiers.shift { 0.0 } else { lines.y };
                if dy == 0.0 {
                    return;
                }
                let d = -dy * PX_PER_LINE;
                GLIDES.with(|g| {
                    if let Some(e) = g.borrow_mut().get_mut(key) {
                        if e.pending.signum() != d.signum() {
                            e.pending = 0.0;
                        }
                        if e.pending == 0.0 {
                            e.last = std::time::Instant::now();
                        }
                        e.pending += d;
                    }
                });
                view.update(cx, |v, cx| {
                    // Scrolling the lyrics by hand pauses the auto-follow.
                    if key == "lyrics" {
                        v.lyrics_follow = false;
                        v.lyrics_sync_at = None;
                    }
                    cx.notify();
                });
                cx.stop_propagation();
            });
        },
    )
    .absolute()
    .size(px(0.))
}
