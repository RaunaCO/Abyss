//! Smooth state transitions for every interactive element.
//!
//! GPUI applies `.hover()` / `.active()` styles instantly. This module gives
//! elements eased values instead: `hover(key)` and `press(key)` glide between
//! 0 and 1 when the pointer enters / leaves or the button goes down / up, and
//! `follow(key, target)` eases any number toward a target that changes (a
//! switch knob, an underline position...). Builders read these values to style
//! backgrounds, sizes, shadows and offsets, so everything fades and slides.
//!
//! State lives in a thread-local map (the UI is single-threaded), so any
//! builder can use it without plumbing. Frames are requested only while some
//! value is still moving; settled entries at 0 are dropped.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use gpui::{prelude::*, Context, MouseButton, SharedString, Stateful};

use super::AppView;

/// Durations (s): entering is quick, leaving a little slower (feels softer).
const HOVER_IN: f32 = 0.18;
const HOVER_OUT: f32 = 0.32;
const PRESS_IN: f32 = 0.07;
const PRESS_OUT: f32 = 0.34;

#[derive(Clone, Copy)]
struct Anim {
    from: f32,
    to: f32,
    at: Instant,
    dur: f32,
}

impl Anim {
    fn t(&self) -> f32 {
        if self.dur <= 0.0 {
            return 1.0;
        }
        (self.at.elapsed().as_secs_f32() / self.dur).clamp(0.0, 1.0)
    }
    fn value(&self) -> f32 {
        let t = self.t();
        // Smooth start and end (quintic smootherstep): no visible jolt either side.
        let e = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
        self.from + (self.to - self.from) * e
    }
    fn done(&self) -> bool {
        self.t() >= 1.0
    }
}

thread_local! {
    static MAP: RefCell<HashMap<String, Anim>> = RefCell::new(HashMap::new());
}

/// Start gliding `key` toward `target` (no-op if already heading there).
pub fn set(key: &str, target: f32, dur: f32) {
    MAP.with(|m| {
        let mut m = m.borrow_mut();
        match m.get_mut(key) {
            Some(a) if (a.to - target).abs() < 1e-4 => {}
            Some(a) => {
                let now = a.value();
                *a = Anim { from: now, to: target, at: Instant::now(), dur };
            }
            None => {
                m.insert(key.to_string(), Anim { from: 0.0, to: target, at: Instant::now(), dur });
            }
        }
    })
}

/// Current value of `key` (0 when never set).
pub fn get(key: &str) -> f32 {
    MAP.with(|m| m.borrow().get(key).map(Anim::value).unwrap_or(0.0))
}

/// Ease toward a target that the caller recomputes every frame. The first time
/// a key is seen it starts at the target (no animation on first show).
pub fn follow(key: &str, target: f32, dur: f32) -> f32 {
    MAP.with(|m| {
        let mut m = m.borrow_mut();
        match m.get_mut(key) {
            None => {
                m.insert(key.to_string(), Anim { from: target, to: target, at: Instant::now(), dur: 0.0 });
                target
            }
            Some(a) => {
                if (a.to - target).abs() > 1e-4 {
                    let now = a.value();
                    *a = Anim { from: now, to: target, at: Instant::now(), dur };
                }
                a.value()
            }
        }
    })
}

pub fn hover(key: &str) -> f32 {
    get(&format!("h:{key}"))
}

pub fn press(key: &str) -> f32 {
    get(&format!("p:{key}"))
}

/// Release every pressed element (the button went up somewhere else).
pub fn release_all() {
    let keys: Vec<String> = MAP.with(|m| m.borrow().iter().filter(|(k, a)| k.starts_with("p:") && a.to > 0.0).map(|(k, _)| k.clone()).collect());
    for k in keys {
        set(&k, 0.0, PRESS_OUT);
    }
}

/// True while any value is still moving; also forgets settled idle entries.
pub fn animating() -> bool {
    MAP.with(|m| {
        let mut m = m.borrow_mut();
        m.retain(|k, a| !(a.done() && a.to == 0.0 && (k.starts_with("h:") || k.starts_with("p:"))));
        let busy = m.iter().find(|(_, a)| !a.done()).map(|(k, _)| k.clone());
        if let Some(k) = &busy {
            if std::env::var_os("TIDE_PERF").is_some() {
                thread_local!(static LAST: std::cell::Cell<Option<Instant>> = std::cell::Cell::new(None));
                LAST.with(|l| {
                    if l.get().map_or(true, |t| t.elapsed().as_secs_f32() > 1.0) {
                        l.set(Some(Instant::now()));
                        log::info!("interact busy: {k}");
                    }
                });
            }
        }
        busy.is_some()
    })
}

/// Make an element report hover and press to the transition store under `key`.
/// (It uses the element's hover listener: do not combine with another `on_hover`.)
pub fn track(el: Stateful<gpui::Div>, key: impl Into<SharedString>, cx: &mut Context<AppView>) -> Stateful<gpui::Div> {
    let key: SharedString = key.into();
    let (k1, k2, k3) = (key.clone(), key.clone(), key);
    el.on_hover(cx.listener(move |_, hovered: &bool, _, cx| {
        set(&format!("h:{k1}"), if *hovered { 1.0 } else { 0.0 }, if *hovered { HOVER_IN } else { HOVER_OUT });
        if !*hovered {
            set(&format!("p:{k1}"), 0.0, PRESS_OUT);
        }
        cx.notify();
    }))
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |_, _, _, cx| {
            set(&format!("p:{k2}"), 1.0, PRESS_IN);
            cx.notify();
        }),
    )
    .on_mouse_up(
        MouseButton::Left,
        cx.listener(move |_, _, _, cx| {
            set(&format!("p:{k3}"), 0.0, PRESS_OUT);
            cx.notify();
        }),
    )
}

/// Linear blend of two colours.
pub fn mix(a: gpui::Rgba, b: gpui::Rgba, t: f32) -> gpui::Rgba {
    let t = t.clamp(0.0, 1.0);
    gpui::Rgba { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
}
