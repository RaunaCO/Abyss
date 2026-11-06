//! Tactile feedback and gestures: controls give a small springy "pop" when
//! pressed, the play button sends out a ring, a double-click on the Now Playing
//! cover likes the track with a big heart, and horizontal touchpad swipes (or a
//! tilt wheel) go back / forward with an arrow that follows the finger.
//!
//! Everything here is time-based and only asks for frames while something is
//! actually moving, so it costs nothing at rest.

use std::time::{Duration, Instant};

use super::sc;
use gpui::{div, prelude::*, AnyElement, ClickEvent, Context, ElementId, Rgba, Window};

use super::icons::Icon;
use super::{theme, AppView};

const POP: Duration = Duration::from_millis(380);
const RIPPLE: Duration = Duration::from_millis(520);
const BURST: Duration = Duration::from_millis(950);
/// Horizontal distance (px) a swipe needs to navigate.
const SWIPE_TRIGGER: f32 = 150.0;

fn t_of(at: Instant, len: Duration) -> f32 {
    (at.elapsed().as_secs_f32() / len.as_secs_f32()).clamp(0.0, 1.0)
}

/// Damped spring: a quick swell past 1 and a settle back, like a button that
/// gives under the finger and springs back.
fn spring(t: f32) -> f32 {
    if t >= 1.0 {
        return 1.0;
    }
    1.0 + 0.24 * (t * std::f32::consts::PI * 1.6).sin() * (-3.2 * t).exp()
}

/// The play button: it sinks under the finger (to 78 %) and springs back past
/// its size before settling, so the button itself feels pressed.
fn press_spring(t: f32) -> f32 {
    const DOWN: f32 = 0.16;
    if t >= 1.0 {
        return 1.0;
    }
    if t < DOWN {
        let k = t / DOWN;
        return 1.0 - 0.22 * (1.0 - (1.0 - k).powi(2));
    }
    let u = (t - DOWN) / (1.0 - DOWN);
    1.0 - 0.22 * (u * 9.0).cos() * (-5.0 * u).exp()
}

#[derive(Default)]
pub struct Haptics {
    pulses: Vec<(String, Instant)>,
    ripple: Option<Instant>,
    burst: Option<Instant>,
    swipe: f32,
    swipe_at: Option<Instant>,
}

impl Haptics {
    /// True while anything is animating (the caller then asks for a frame).
    pub fn active(&mut self) -> bool {
        self.pulses.retain(|(_, at)| at.elapsed() < POP);
        if self.ripple.is_some_and(|at| at.elapsed() >= RIPPLE) {
            self.ripple = None;
        }
        if self.burst.is_some_and(|at| at.elapsed() >= BURST) {
            self.burst = None;
        }
        // An unfinished swipe slides back once the fingers stop.
        if self.swipe != 0.0 && self.swipe_at.is_some_and(|at| at.elapsed() > Duration::from_millis(160)) {
            self.swipe *= 0.82;
            if self.swipe.abs() < 2.0 {
                self.swipe = 0.0;
            }
        }
        !self.pulses.is_empty() || self.ripple.is_some() || self.burst.is_some() || self.swipe != 0.0
    }
}

impl AppView {
    /// Give the control `key` a springy pop.
    pub fn pulse(&mut self, key: impl Into<String>) {
        let key = key.into();
        self.haptics.pulses.retain(|(k, _)| *k != key);
        self.haptics.pulses.push((key, Instant::now()));
    }

    /// Current size factor of the control `key` (1.0 at rest).
    pub fn pulse_scale(&self, key: &str) -> f32 {
        let press = key == "playpause";
        self.haptics
            .pulses
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, at)| if press { press_spring(t_of(*at, POP)) } else { spring(t_of(*at, POP)) })
            .unwrap_or(1.0)
    }

    pub fn ripple_play(&mut self) {
        self.haptics.ripple = Some(Instant::now());
    }

    pub fn heart_burst(&mut self) {
        self.haptics.burst = Some(Instant::now());
    }

    /// Horizontal touchpad / tilt-wheel movement over the page.
    /// Positive = fingers move right = back (like a browser).
    pub fn swipe(&mut self, dx: f32, cx: &mut Context<Self>) {
        if self.haptics.swipe.signum() != dx.signum() && self.haptics.swipe != 0.0 {
            self.haptics.swipe = 0.0;
        }
        self.haptics.swipe += dx;
        self.haptics.swipe_at = Some(Instant::now());
        if self.haptics.swipe >= SWIPE_TRIGGER {
            self.haptics.swipe = 0.0;
            if self.can_go_back() {
                self.go_back(cx);
            }
        } else if self.haptics.swipe <= -SWIPE_TRIGGER {
            self.haptics.swipe = 0.0;
            if self.can_go_forward() {
                self.go_forward(cx);
            }
        }
        cx.notify();
    }
}

/// An icon button that pops when pressed (and still runs `on_click`).
pub fn button(
    view: &AppView,
    id: &'static str,
    icon: Icon,
    size: f32,
    color: Rgba,
    on_click: impl Fn(&mut AppView, &ClickEvent, &mut Window, &mut Context<AppView>) + 'static,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let scale = view.pulse_scale(id);
    div()
        .id(ElementId::Name(id.into()))
        .flex_none()
        .size(sc(size + 12.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(sc(999.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .active(|s| s.bg(theme::active()))
        .child(icon.el(size * scale, color))
        .on_click(cx.listener(move |this, ev, window, cx| {
            cx.stop_propagation();
            this.pulse(id);
            on_click(this, ev, window, cx);
            cx.notify();
        }))
        .into_any_element()
}

/// The ring that spreads from the play button when it is pressed.
pub fn play_ripple(view: &AppView, base: f32) -> Option<AnyElement> {
    let at = view.haptics.ripple?;
    let t = t_of(at, RIPPLE);
    let e = 1.0 - (1.0 - t).powi(3);
    let d = base * (1.0 + 0.9 * e);
    Some(
        div()
            .absolute()
            .left(sc((base - d) / 2.0))
            .top(sc((base - d) / 2.0))
            .size(sc(d))
            .rounded(sc(d / 2.0))
            .border_2()
            .border_color(super::fx::with_alpha(theme::text(), 0.45 * (1.0 - t)))
            .into_any_element(),
    )
}

/// Big heart over the Now Playing cover after a double-click.
pub fn heart_overlay(view: &AppView, cover: f32) -> Option<AnyElement> {
    let at = view.haptics.burst?;
    let t = t_of(at, BURST);
    // Pop in past full size, settle, then fade away.
    let s = if t < 0.25 { 1.25 * (1.0 - (1.0 - t / 0.25).powi(3)) } else { 1.0 + 0.25 * (1.0 - ((t - 0.25) / 0.2).min(1.0)) };
    let alpha = if t < 0.6 { 1.0 } else { 1.0 - (t - 0.6) / 0.4 };
    let size = cover * 0.3 * s;
    Some(
        div()
            .absolute()
            .top(sc(0.))
            .left(sc(0.))
            .size(sc(cover))
            .flex()
            .items_center()
            .justify_center()
            .child(Icon::HeartFilled.el(size, super::fx::with_alpha(theme::text(), 0.92 * alpha)))
            .into_any_element(),
    )
}

/// Arrow at the window edge that grows with an ongoing swipe.
pub fn swipe_indicator(view: &AppView) -> Option<AnyElement> {
    let s = view.haptics.swipe;
    if s == 0.0 {
        return None;
    }
    let k = (s.abs() / SWIPE_TRIGGER).min(1.0);
    let back = s > 0.0;
    let d = 34.0 + 12.0 * k;
    let el = div()
        .absolute()
        .top(gpui::relative(0.45))
        .size(sc(d))
        .rounded(sc(d / 2.0))
        .bg(super::fx::with_alpha(gpui::rgb(0x242429), 0.6 + 0.4 * k))
        .border_1()
        .border_color(super::fx::with_alpha(theme::text(), 0.2 * k))
        .flex()
        .items_center()
        .justify_center()
        .opacity(k)
        .child((if back { Icon::Back } else { Icon::Forward }).el(18. + 4. * k, theme::text()));
    let off = -d + (d + 18.0) * k;
    Some(if back { el.left(sc(off)) } else { el.right(sc(off)) }.into_any_element())
}
