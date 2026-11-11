//! Time-based animations: sidebar width,
//! ambient lighting and the progress bar between engine updates.

use std::time::{Duration, Instant};

use gpui::Window;

use super::AppView;

// One rhythm for the entire UI. Never transform raster artwork in GPUI 0.2.2.
pub const PRESS: Duration = Duration::from_millis(80);
pub const FAST: Duration = Duration::from_millis(140);
pub const CONTROL: Duration = Duration::from_millis(220);
pub const PANEL: Duration = Duration::from_millis(300);
pub const AMBIENT_FADE: Duration = Duration::from_millis(900);
pub const STAGGER: f32 = 0.035;
pub const SIDEBAR_ANIM: Duration = CONTROL;

pub fn duration(normal: Duration) -> Duration {
    if reduced_motion() {
        Duration::from_millis(1)
    } else {
        normal
    }
}

pub fn seconds(normal: Duration) -> f32 {
    duration(normal).as_secs_f32()
}

pub fn animation(normal: Duration) -> gpui::Animation {
    gpui::Animation::new(duration(normal)).with_easing(ease_out)
}

/// Shared entrance for floating surfaces. Called on the surface, not its backdrop.
pub fn enter<E: gpui::Styled>(el: E, t: f32) -> E {
    el.opacity(if reduced_motion() { 1.0 } else { t })
}

/// Smooth deceleration.
pub fn ease_out(t: f32) -> f32 {
    if reduced_motion() {
        return 1.0;
    }
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(4)
}

/// Progress (0..1) of an animation that started at `at`.
pub fn progress(at: Instant, len: Duration) -> f32 {
    if reduced_motion() {
        return 1.0;
    }
    (at.elapsed().as_secs_f32() / len.as_secs_f32()).clamp(0.0, 1.0)
}

/// Slow motion that can use a lower rate than pointer-driven interactions.
const AMBIENT: [&str; 8] = ["playing", "bg-spin", "np-light", "bar-light", "ambient", "eq-bars", "lyrics-layout", "loading"];
/// Frame rate for ambient-only motion.
const AMBIENT_FPS: f64 = 30.0;
const INTERACTIVE_FPS: f64 = 120.0;

thread_local! {
    static WANT_FRAME: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static WANT_NORMAL_RATE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Something interactive is moving.
    static WANT_FAST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Why frames were asked for this second (`TIDE_PERF` only).
    static FRAME_REASONS: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Something on screen is still moving: draw another frame. Interactive motion
/// and ambient motion use separate frame budgets.
pub fn want_frame(reason: &'static str) {
    WANT_FRAME.with(|w| w.set(true));
    if reason != "bar-light" {
        WANT_NORMAL_RATE.with(|w| w.set(true));
    }
    if !AMBIENT.contains(&reason) {
        WANT_FAST.with(|w| w.set(true));
    }
    if perf_on() {
        FRAME_REASONS.with(|r| {
            let mut r = r.borrow_mut();
            if !r.contains(&reason) {
                r.push(reason);
            }
        });
    }
}

/// A looping animation driven by the clock: calls `f(el, t)` with `t` going
/// 0..1 every `period`, and asks for a paced frame. Replaces GPUI's repeating
/// `with_animation`, which redraws as fast as the GPU allows (about 200 frames
/// a second on a 60 Hz screen) for as long as it is on screen.
pub fn looping<E>(el: E, period: Duration, reason: &'static str, f: impl FnOnce(E, f32) -> E) -> E {
    if reduced_motion() {
        return f(el, 0.0);
    }
    thread_local!(static EPOCH: Instant = Instant::now());
    let t = EPOCH.with(|e| (e.elapsed().as_secs_f64() / period.as_secs_f64().max(0.001)).fract()) as f32;
    want_frame(reason);
    f(el, t)
}

thread_local!(static PACE: std::cell::Cell<[u32; 4]> = const { std::cell::Cell::new([0; 4]) });

/// `TIDE_PERF`: count how frames ended (0 nothing wanted, 1 immediate, 2 timer scheduled, 3 timer already pending).
fn pace_count(kind: usize) {
    if perf_on() {
        PACE.with(|p| {
            let mut v = p.get();
            v[kind] += 1;
            p.set(v);
        });
    }
}

thread_local!(pub static LAST_HZ: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });

pub fn take_pace_counts() -> [u32; 4] {
    PACE.with(|p| p.replace([0; 4]))
}

/// What is on screen for something that changes with the track (cover,
/// background frames): the new content fades in OVER the old one, which stays
/// fully opaque underneath until the fade ends, so there is never a gap, a dip
/// in brightness or a flash of the background. While the new content is not
/// ready (still loading), the old one simply stays. Content that was already
/// cached when it was asked for switches faster.
pub struct Crossfade<T: Clone> {
    shown: Option<(String, T)>,
    prev: Option<T>,
    at: Instant,
    dur: f32,
    waiting: Option<(String, Instant)>,
}

impl<T: Clone> Default for Crossfade<T> {
    fn default() -> Self {
        Self { shown: None, prev: None, at: Instant::now(), dur: 0.0, waiting: None }
    }
}

impl<T: Clone> Crossfade<T> {
    /// `key` identifies what should be shown now and `value` is it, if ready.
    /// Returns the previous content (drawn first, opaque) and the current
    /// content with its opacity.
    pub fn frame(&mut self, key: Option<&str>, value: Option<T>) -> (Option<T>, Option<(T, f32)>) {
        if let Some(k) = key {
            let showing = self.shown.as_ref().is_some_and(|(sk, _)| sk == k);
            match value {
                Some(v) if !showing => {
                    // Ready at once (cached) or after a wait (downloaded / generated).
                    let waited = self.waiting.take().filter(|(wk, _)| wk == k).map(|(_, t)| t.elapsed().as_secs_f32()).unwrap_or(0.0);
                    self.dur = if self.shown.is_none() || reduced_motion() {
                        0.0
                    } else if waited < 0.05 {
                        FAST.as_secs_f32()
                    } else {
                        PANEL.as_secs_f32()
                    };
                    self.prev = self.shown.take().map(|(_, v)| v);
                    self.at = Instant::now();
                    self.shown = Some((k.to_string(), v));
                }
                Some(v) => {
                    // Same content, maybe a fresher copy (other size): just use it.
                    if let Some(s) = self.shown.as_mut() {
                        s.1 = v;
                    }
                }
                None if !showing && self.waiting.as_ref().is_none_or(|(wk, _)| wk != k) => {
                    self.waiting = Some((k.to_string(), Instant::now()));
                }
                None => {}
            }
        }
        let t = if self.dur <= 0.0 || reduced_motion() { 1.0 } else { (self.at.elapsed().as_secs_f32() / self.dur).min(1.0) };
        if t >= 1.0 {
            self.prev = None;
        } else {
            want_frame("crossfade");
        }
        let eased = ease_out(t);
        (self.prev.clone(), self.shown.as_ref().map(|(_, v)| (v.clone(), eased)))
    }
}

pub fn perf_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("TIDE_PERF").is_some())
}

/// Reasons collected since the last call (for the perf log).
pub fn take_frame_reasons() -> String {
    FRAME_REASONS.with(|r| std::mem::take(&mut *r.borrow_mut()).join(","))
}

impl AppView {
    /// End of a render: if anything asked for another frame, schedule it.
    /// Give interaction priority over ambient motion and leave static views idle.
    pub fn pace_frames(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        let fast = WANT_FAST.with(|w| w.replace(false));
        let normal = WANT_NORMAL_RATE.with(|w| w.replace(false));
        if !WANT_FRAME.with(|w| w.replace(false)) {
            pace_count(0);
            return;
        }
        let hz = super::winctl::refresh_hz(window);
        LAST_HZ.with(|h| h.set(hz));
        let cap = if fast {
            INTERACTIVE_FPS
        } else if normal {
            AMBIENT_FPS
        } else {
            16.0
        };
        let interval = Duration::from_secs_f64(1.0 / (hz as f64).min(cap));
        let since = self.last_paced.elapsed();
        if (fast && f64::from(hz) <= INTERACTIVE_FPS) || since + Duration::from_millis(1) >= interval {
            pace_count(1);
            self.last_paced = std::time::Instant::now();
            window.request_animation_frame();
        } else if self.frame_timer {
            pace_count(3);
        } else {
            pace_count(2);
            self.frame_timer = true;
            // Windows timers fire a few ms late; aim early, `last_paced`
            // keeps the long-run rate at the cap.
            let wait = (interval - since).saturating_sub(Duration::from_millis(3)).max(Duration::from_millis(1));
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |this, cx| {
                    this.frame_timer = false;
                    this.last_paced = std::time::Instant::now();
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Position shown in the UI: interpolated between engine snapshots so the
    /// progress bar moves every frame instead of in 200 ms steps.
    pub fn display_position(&self) -> f64 {
        let playing = self.now.is_some() && !self.snap.paused && !self.ended && !self.snap.buffering;
        let base = self.snap.position;
        if !playing || self.drag == Some(super::Drag::Progress) {
            return base;
        }
        let dur = self.duration();
        let p = base + self.snap_at.elapsed().as_secs_f64();
        if dur > 0.0 {
            p.min(dur)
        } else {
            p
        }
    }

    /// Finish animations that are done; keep frames coming while any runs.
    pub fn drive_animations(&mut self, window: &mut Window) {
        let mut running = false;
        if let Some((_, at)) = self.sidebar_anim {
            if progress(at, SIDEBAR_ANIM) >= 1.0 {
                self.sidebar_anim = None;
            } else {
                running = true;
            }
        }
        if let Some((_, at)) = self.heart_pop {
            if at.elapsed() >= super::fx::HEART_POP {
                self.heart_pop = None;
            } else {
                running = true;
            }
        }
        if running {
            want_frame("panels");
        }
        if self.ambient_animating() {
            want_frame("ambient");
        }
        if self.now.is_some() && !self.snap.paused && !self.ended && !self.snap.buffering {
            want_frame("playing");
        }
        if self.haptics.active() {
            want_frame("haptics");
        }
        // Hover / press / follow transitions still gliding, or a cover fading in.
        if super::interact::animating() {
            want_frame("interact");
        }
        if super::images::fading() {
            want_frame("fade-in");
        }
        if self.scroll_animating {
            want_frame("scroll");
        }
        let _ = window;
    }
}

/// `cubic-bezier(.4, 0, .2, 1)`, the official lyrics easing (`--lyrics-ease`).
pub fn lyrics_ease(t: f32) -> f32 {
    cubic_bezier(0.4, 0.0, 0.2, 1.0, t)
}

/// CSS-style cubic bezier easing (x solved with a few Newton steps).
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let bez = |a: f32, b: f32, u: f32| 3.0 * (1.0 - u).powi(2) * u * a + 3.0 * (1.0 - u) * u * u * b + u.powi(3);
    let mut u = t;
    for _ in 0..8 {
        let x = bez(x1, x2, u) - t;
        let dx = 3.0 * (1.0 - u).powi(2) * x1 + 6.0 * (1.0 - u) * u * (x2 - x1) + 3.0 * u * u * (1.0 - x2);
        if dx.abs() < 1e-5 {
            break;
        }
        u = (u - x / dx).clamp(0.0, 1.0);
    }
    bez(y1, y2, u)
}

/// Windows accessibility preference, refreshed without a per-line system call.
pub fn reduced_motion() -> bool {
    #[cfg(windows)]
    {
        thread_local! { static PREF: std::cell::RefCell<Option<(Instant, bool)>> = const { std::cell::RefCell::new(None) }; }
        PREF.with(|pref| {
            let mut pref = pref.borrow_mut();
            if let Some((at, value)) = *pref {
                if at.elapsed() < Duration::from_secs(2) {
                    return value;
                }
            }
            #[link(name = "user32")]
            unsafe extern "system" {
                fn SystemParametersInfoW(action: u32, param: u32, value: *mut u32, flags: u32) -> i32;
            }
            let mut enabled = 1u32;
            let ok = unsafe { SystemParametersInfoW(0x1042, 0, &mut enabled, 0) };
            let reduced = ok != 0 && enabled == 0;
            *pref = Some((Instant::now(), reduced));
            reduced
        })
    }
    #[cfg(not(windows))]
    {
        false
    }
}
