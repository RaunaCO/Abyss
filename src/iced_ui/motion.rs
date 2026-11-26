//! One rhythm for the whole interface, as GPUI's `src/ui/motion.rs` and
//! `src/ui/interact.rs`: the same durations, the same quartic ease-out and the
//! same "glide from wherever it is now" transitions, so a hover, a switch or a
//! panel takes exactly as long and decelerates exactly as it does there.
//!
//! Everything here is pure time arithmetic (no frames are asked for): widgets
//! read the values at draw time and request the next vsync frame only while
//! something is still moving (`Shell::request_redraw`).

use std::time::{Duration, Instant};

pub const PRESS: Duration = Duration::from_millis(52);
pub const FAST: Duration = Duration::from_millis(91);
pub const CONTROL: Duration = Duration::from_millis(143);
pub const PANEL: Duration = Duration::from_millis(195);

/// GPUI's `interact` timings: entering is quick, leaving a little slower.
pub const HOVER_IN: Duration = FAST;
pub const HOVER_OUT: Duration = CONTROL;
pub const PRESS_IN: Duration = PRESS;
pub const PRESS_OUT: Duration = CONTROL;

/// Slow colour changes that follow the music (GPUI `AMBIENT_FADE`): the
/// ambient tint and palette after a track change, the heart burst.
pub const AMBIENT: Duration = Duration::from_millis(585);

/// A floating surface leaves faster than it came (exits are answers, not
/// news): menus dismissed by a click elsewhere fade out over this.
pub const EXIT: Duration = FAST;

/// The size a floating surface grows from as it fades in: never from
/// nothing, 3 % short of its size (menus from their trigger, dialogs from
/// their centre).
pub const ENTER_SCALE: f32 = 0.97;

/// The sidebar waits this long after the pointer left before folding (GPUI `CLOSE_DELAY`).
pub const SIDEBAR_CLOSE_DELAY: Duration = Duration::from_millis(150);
/// The heart pop after liking (GPUI `fx::HEART_POP`).
pub const HEART_POP: Duration = PANEL;

pub fn reduced() -> bool {
    crate::shared::lyrics::reduced_motion()
}

/// Whether the window has the focus (kept by the window events).
static FOCUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_focused(on: bool) {
    FOCUSED.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Frame interval of the slow ambient motion (the cover's drifting lights,
/// the liquid flow, the playing bars): 30 fps while the window has the focus,
/// 10 fps behind other windows. Every frame redraws the whole window (about
/// 5 ms of CPU), so an ambient effect left at 30 fps in the background cost
/// ~15 % of a core for a picture nobody was looking at closely.
pub fn ambient_interval() -> Duration {
    if FOCUSED.load(std::sync::atomic::Ordering::Relaxed) {
        Duration::from_millis(33)
    } else {
        Duration::from_millis(100)
    }
}

/// The slowest ambient motion (album banners and panes lit by a cover, whose
/// lights take 5-8 s to cross): 20 fps with the focus, 8 fps without — the
/// eye cannot tell it from 30 at that speed.
pub fn ambient_slow_interval() -> Duration {
    if FOCUSED.load(std::sync::atomic::Ordering::Relaxed) {
        Duration::from_millis(50)
    } else {
        Duration::from_millis(125)
    }
}

/// Smooth deceleration (GPUI `motion::ease_out`): the curve of everything
/// that enters, leaves or answers the pointer.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(4)
}

/// A gentler deceleration for slow colour changes ([`AMBIENT`]) and the
/// lyrics' emphasis, where the quartic's fast start would read as a flash.
pub fn ease_out_soft(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// The transformation that scales by `k` around `pivot` (in window px), so a
/// surface grows from the point it is attached to.
pub fn scale_about(k: f32, pivot: iced::Point) -> iced::Transformation {
    iced::Transformation::translate(pivot.x, pivot.y) * iced::Transformation::scale(k) * iced::Transformation::translate(-pivot.x, -pivot.y)
}

// ------------------------------------------------------------------ keyboard

thread_local! {
    static KEYED_AT: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// How long after a handled key press what appears counts as its answer.
const KEY_ECHO: Duration = Duration::from_millis(120);

/// A key press was just handled. What it opens (a search, a page) appears at
/// once: shortcuts are pressed hundreds of times a day and an entrance would
/// only make them feel slow (Raycast, Spotlight and the Finder do the same).
pub fn keyed() {
    KEYED_AT.with(|k| k.set(Some(Instant::now())));
}

/// True while drawing the answer to a key press (see [`keyed`]).
pub fn by_keyboard() -> bool {
    by_keyboard_at(KEYED_AT.with(std::cell::Cell::get), Instant::now())
}

fn by_keyboard_at(keyed: Option<Instant>, now: Instant) -> bool {
    keyed.is_some_and(|at| now.saturating_duration_since(at) < KEY_ECHO)
}

/// Linear progress 0..1 of something that started at `at` and lasts `len`.
pub fn progress(at: Instant, now: Instant, len: Duration) -> f32 {
    if len.is_zero() {
        return 1.0;
    }
    (now.saturating_duration_since(at).as_secs_f32() / len.as_secs_f32()).clamp(0.0, 1.0)
}

/// A value gliding from `from` to `to`: GPUI's `interact::Anim`. Retargeting
/// starts from wherever the value is now, so reversing midway never jumps.
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    from: f32,
    to: f32,
    at: Instant,
    dur: Duration,
}

impl Anim {
    /// Resting at `v` (no motion).
    pub fn still(v: f32) -> Self {
        Self { from: v, to: v, at: Instant::now(), dur: Duration::ZERO }
    }

    fn t(&self, now: Instant) -> f32 {
        if self.dur.is_zero() || reduced() {
            1.0
        } else {
            progress(self.at, now, self.dur)
        }
    }

    pub fn value_at(&self, now: Instant) -> f32 {
        self.from + (self.to - self.from) * ease_out(self.t(now))
    }

    pub fn done_at(&self, now: Instant) -> bool {
        self.t(now) >= 1.0
    }

    /// Glide toward `target` over `dur` (no-op if already heading there).
    pub fn go(&mut self, target: f32, dur: Duration, now: Instant) {
        if (self.to - target).abs() < 1e-4 {
            return;
        }
        *self = Self { from: self.value_at(now), to: target, at: now, dur };
    }
}

/// Fade of the sidebar's labels as it unfolds (GPUI `sidebar::label_alpha`):
/// they appear once there is room for them.
pub fn label_alpha(t: f32) -> f32 {
    let x = ((t - 0.2) / 0.6).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Size factor of a heart that was just liked, `t` 0..1 (GPUI `heart_scale`).
pub fn heart_scale(t: f32) -> f32 {
    if t >= 1.0 || t <= 0.0 {
        return 1.0;
    }
    1.0 + 0.55 * (std::f32::consts::PI * t).sin() * (1.0 - t * 0.6)
}

/// Share of [`press_spring`] spent sinking.
pub const SPRING_DOWN: f32 = 0.16;
/// How deep [`press_spring`] sinks.
pub const SPRING_DEPTH: f32 = 0.08;

/// The play button under the finger (GPUI `haptics::press_spring`): it sinks
/// to 92 % and springs back past its size before settling.
pub fn press_spring(t: f32) -> f32 {
    const DOWN: f32 = SPRING_DOWN;
    if t >= 1.0 {
        return 1.0;
    }
    if t < DOWN {
        let k = t / DOWN;
        return 1.0 - SPRING_DEPTH * (1.0 - (1.0 - k).powi(2));
    }
    let u = (t - DOWN) / (1.0 - DOWN);
    1.0 - SPRING_DEPTH * (u * 9.0).cos() * (-5.0 * u).exp()
}

/// Where in [`press_spring`] (0..[`SPRING_DOWN`]) the button already is when
/// released after being held `depth` (0..1) of the way down: the spring then
/// carries on from that size instead of popping back to full size first.
pub fn spring_resume(depth: f32) -> f32 {
    let d = depth.clamp(0.0, 1.0);
    SPRING_DOWN * (1.0 - (1.0 - d).sqrt())
}

/// GPUI `icons::swap`'s smoothstep between two glyphs.
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_out_is_gpuis_quartic() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!((ease_out(0.5) - 0.9375).abs() < 1e-6);
        assert_eq!(ease_out(2.0), 1.0);
    }

    #[test]
    fn anim_retargets_from_where_it_is() {
        let t0 = Instant::now();
        let mut a = Anim::still(0.0);
        a.go(1.0, CONTROL, t0);
        assert_eq!(a.value_at(t0), 0.0);
        let mid = t0 + CONTROL / 2;
        let v = a.value_at(mid);
        assert!((v - 0.9375).abs() < 1e-3);
        a.go(0.0, CONTROL, mid);
        assert!((a.value_at(mid) - v).abs() < 1e-6, "no jump when reversing");
        assert!(a.done_at(mid + CONTROL));
        assert_eq!(a.value_at(mid + CONTROL), 0.0);
        // Same target again: nothing restarts.
        let before = a;
        a.go(0.0, CONTROL, mid + CONTROL * 3);
        assert_eq!(a.at, before.at);
    }

    #[test]
    fn hover_timings_match_gpui() {
        assert_eq!(HOVER_IN.as_millis(), 91);
        assert_eq!(HOVER_OUT.as_millis(), 143);
        assert_eq!(PRESS_IN.as_millis(), 52);
        assert_eq!(PRESS_OUT.as_millis(), 143);
        assert_eq!(PANEL.as_millis(), 195);
    }

    #[test]
    fn label_alpha_waits_for_room() {
        assert_eq!(label_alpha(0.0), 0.0);
        assert_eq!(label_alpha(0.2), 0.0);
        assert!((label_alpha(0.5) - 0.5).abs() < 1e-6);
        assert_eq!(label_alpha(0.8), 1.0);
        assert_eq!(label_alpha(1.0), 1.0);
    }

    #[test]
    fn heart_pops_and_settles() {
        assert_eq!(heart_scale(0.0), 1.0);
        assert_eq!(heart_scale(1.0), 1.0);
        let peak = (1..100).map(|i| heart_scale(i as f32 / 100.0)).fold(0.0, f32::max);
        assert!(peak > 1.35 && peak < 1.45, "peak {peak}");
    }

    #[test]
    fn the_soft_curve_is_gentler_than_the_quartic_but_still_decelerates() {
        assert_eq!(ease_out_soft(0.0), 0.0);
        assert_eq!(ease_out_soft(1.0), 1.0);
        assert!((ease_out_soft(0.5) - 0.875).abs() < 1e-6);
        assert!(ease_out_soft(0.25) < ease_out(0.25));
        // Never ease-in: more than linear progress at every point.
        for i in 1..10 {
            let t = i as f32 / 10.0;
            assert!(ease_out_soft(t) > t && ease_out(t) > t);
        }
    }

    #[test]
    fn surfaces_never_grow_from_nothing_and_exits_beat_entrances() {
        assert!((0.9..=0.98).contains(&ENTER_SCALE));
        assert!(EXIT < CONTROL);
        assert_eq!(AMBIENT.as_millis(), 585);
    }

    #[test]
    fn scaling_about_a_pivot_keeps_the_pivot_in_place() {
        let pivot = iced::Point::new(120.0, 40.0);
        let t = scale_about(0.5, pivot);
        let still = pivot * t;
        assert!((still.x - 120.0).abs() < 1e-4 && (still.y - 40.0).abs() < 1e-4);
        let far = iced::Point::new(220.0, 140.0) * t;
        assert!((far.x - 170.0).abs() < 1e-4 && (far.y - 90.0).abs() < 1e-4);
    }

    #[test]
    fn only_what_a_key_press_just_opened_skips_its_entrance() {
        let t0 = Instant::now();
        assert!(!by_keyboard_at(None, t0));
        assert!(by_keyboard_at(Some(t0), t0 + Duration::from_millis(30)));
        assert!(!by_keyboard_at(Some(t0), t0 + KEY_ECHO));
    }

    #[test]
    fn a_held_press_springs_back_from_where_it_is() {
        assert_eq!(spring_resume(0.0), 0.0);
        assert!((spring_resume(1.0) - SPRING_DOWN).abs() < 1e-6);
        for d in [0.25_f32, 0.5, 0.9] {
            let size = press_spring(spring_resume(d));
            assert!((size - (1.0 - SPRING_DEPTH * d)).abs() < 1e-4, "{d}: {size}");
        }
    }

    #[test]
    fn press_spring_sinks_then_settles() {
        assert!((press_spring(0.0) - 1.0).abs() < 1e-6);
        assert!((press_spring(0.16) - 0.92).abs() < 1e-3);
        assert_eq!(press_spring(1.0), 1.0);
        assert!((press_spring(0.999) - 1.0).abs() < 0.01);
    }
}
