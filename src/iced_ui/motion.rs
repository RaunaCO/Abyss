//! One rhythm for the whole interface, as GPUI's `src/ui/motion.rs` and
//! `src/ui/interact.rs`: the same durations, the same quartic ease-out and the
//! same "glide from wherever it is now" transitions, so a hover, a switch or a
//! panel takes exactly as long and decelerates exactly as it does there.
//!
//! Everything here is pure time arithmetic (no frames are asked for): widgets
//! read the values at draw time and request the next vsync frame only while
//! something is still moving (`Shell::request_redraw`). Durations are in time,
//! never in frames: on the 200 Hz monitor a 91 ms hover is ~18 frames, on a
//! 60 Hz one ~5, and both end at the same moment.
//!
//! # Motion table (by function)
//!
//! | Function | Duration | Curve | Where |
//! | --- | --- | --- | --- |
//! | Press feedback | **immediate**: the first frame after the click already shows [`PRESS_LEAD`] of it, full at [`PRESS_IN`] 52 ms | quartic ease-out with a head start ([`Anim::go_ahead`]) | `wash::Wash`, glyph dips, play button sink |
//! | Release | [`PRESS_OUT`] 143 ms | quartic | `wash::Wash` |
//! | Play button spring | [`CONTROL`] 143 ms, sinks to 92 %, overshoots once | [`press_spring`] | `wash::Wash::pulse` |
//! | Hover in / out | [`HOVER_IN`] 91 ms / [`HOVER_OUT`] 143 ms (leaving is slower) | quartic | every wash, toggle, link |
//! | Selection, toggles, switches | [`CONTROL`] 143 ms | quartic | `Wash::active` |
//! | Icon swap (play/pause, heart) | [`CONTROL`] | smoothstep cross-dissolve | `icons::Glyph::swap` |
//! | Like pop | [`HEART_POP`] 195 ms, peaks ~1.4x | [`heart_scale`] | player bar, rows |
//! | Menu / popover enter | [`CONTROL`] 143 ms, grows from [`ENTER_SCALE`] at its pivot | quartic | `wash::enter_from` |
//! | Menu exit (click elsewhere) | [`EXIT`] 91 ms, from the opacity and size it had reached | quartic | `wash::leave` |
//! | Dialog enter | [`PANEL`] 195 ms from [`ENTER_SCALE`] at the centre | quartic | `wash::enter` |
//! | Page change | [`PANEL`] 195 ms: fade + 26 px slide from the side it comes from | quartic | `panel_motion::slide`, `pages::PageFade` |
//! | Queue / lyrics panel, sidebar | [`PANEL`] 195 ms, reversible from where it is (sidebar folds after [`SIDEBAR_CLOSE_DELAY`]) | quartic | `panel_motion::Reveal` |
//! | Toast | [`PANEL`] 195 ms, rises 8 px | quartic | `wash::rise` |
//! | Track info on a new track | [`FAST`] 91 ms from 70 % and 3 px; a skip mid-way continues from where it is | quartic | `wash::appear` |
//! | Bar cover on a new track | [`COVER_FADE`] 195 ms cross-fade over the previous cover | quartic | `player_bar` |
//! | Ambient colour after a track change | [`AMBIENT`] 585 ms | cubic ([`ease_out_soft`]) | `theme_fade`, palette |
//! | Ambient drift (banners, bar, backdrop) | 30 fps focused / 10 fps behind ([`ambient_interval`]); 20 / 8 fps for the slowest ([`ambient_slow_interval`]); comes to rest over [`AMBIENT_SETTLE`] after [`AMBIENT_IDLE`] without input while nothing plays | [`ambient_clock`] | `liquid_glass`, `gpu/backdrop` |
//! | Wheel | 99 px a notch, 63 % in 75 ms | exponential | `glide` |
//! | Opened by a key | instant ([`keyed`]) | — | search, pages, menus |
//!
//! Reduced motion (`ABYSS_REDUCED_MOTION`, [`reduced`]): movement and growth
//! go, fades stay (shortened by nothing: they carry information).

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

/// Share of [`PRESS_IN`] a press starts with: the frame right after the click
/// already shows two thirds of the press (ease 0.25 = 68 %), so the answer is
/// on screen at once and the rest settles within 40 ms. A press is never a
/// delay curve.
pub const PRESS_LEAD: f32 = 0.25;

/// The bar's cover cross-fading into the next track's.
pub const COVER_FADE: Duration = PANEL;

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

/// For tests of the animations themselves: motion on, whatever the machine's
/// setting (CI runners report reduced motion, and then everything jumps to its end).
#[cfg(test)]
pub fn full_motion_for_tests() {
    std::env::set_var("ABYSS_REDUCED_MOTION", "0");
}

/// Whether the window has the focus (kept by the window events).
static FOCUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_focused(on: bool) {
    FOCUSED.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn focused() -> bool {
    FOCUSED.load(std::sync::atomic::Ordering::Relaxed)
}

// ------------------------------------------------------------------ ambient

/// With nothing playing and no input for this long, the ambient drift (the
/// Home and album panes lit by a cover, a paused bar, the full-screen
/// backdrop) comes to rest and stops asking for frames: an idle window draws
/// nothing. 1.7.1 kept Home's featured pane at 20 frames a second forever
/// (8 behind other windows), ~5.7 presents a second measured with nothing playing.
pub const AMBIENT_IDLE: Duration = Duration::from_secs(30);
/// The drift slows to a stop over about this long (it never freezes mid-move)...
pub const AMBIENT_SETTLE: Duration = Duration::from_millis(1600);
/// ...and picks up its pace again over this long when something happens.
pub const AMBIENT_WAKE: Duration = Duration::from_millis(600);
/// The wake-up asked for by ambient motion at rest: effectively none (the
/// next input, tick or animation draws a frame and starts it again).
const AMBIENT_ASLEEP: Duration = Duration::from_secs(3600);

/// The ambient motion's clock: seconds of drift, advancing at `rate` (0..1),
/// which eases to 1 while the window is in use or music plays and to 0 once
/// it has been idle for [`AMBIENT_IDLE`]. The lights read it instead of the
/// wall clock, so stopping and starting again glide (no jump after a pause).
#[derive(Clone, Copy, Debug)]
struct AmbientClock {
    secs: f64,
    rate: f32,
    last: Option<Instant>,
    input_at: Instant,
    playing: bool,
}

impl AmbientClock {
    fn new(now: Instant) -> Self {
        Self { secs: 0.0, rate: 1.0, last: None, input_at: now, playing: false }
    }

    /// Whether the drift should be moving at `now` (`focused`: the window has the focus).
    fn awake(&self, now: Instant, focused: bool) -> bool {
        self.playing || (focused && now.saturating_duration_since(self.input_at) < AMBIENT_IDLE)
    }

    /// Advance to `now` and return the drift's seconds.
    fn advance(&mut self, now: Instant, focused: bool) -> f64 {
        let dt = self.last.map_or(0.0, |l| now.saturating_duration_since(l).as_secs_f32()).min(0.25);
        self.last = Some(self.last.map_or(now, |l| l.max(now)));
        let (target, span) = if self.awake(now, focused) { (1.0, AMBIENT_WAKE) } else { (0.0, AMBIENT_SETTLE) };
        if dt > 0.0 {
            // Exponential approach: ~95 % of the way in `span`, the same at any frame rate.
            let k = 1.0 - (-3.0 * dt / span.as_secs_f32()).exp();
            let before = self.rate;
            self.rate += (target - self.rate) * k;
            if target == 0.0 && self.rate < 0.01 {
                self.rate = 0.0;
            }
            self.secs += f64::from((before + self.rate) * 0.5 * dt);
        }
        self.secs
    }

    /// Moving (or still coming to rest) at `now`.
    fn moving(&mut self, now: Instant, focused: bool) -> bool {
        self.advance(now, focused);
        self.rate > 0.0 || self.awake(now, focused)
    }
}

thread_local! {
    static AMBIENT_CLOCK: std::cell::RefCell<AmbientClock> = std::cell::RefCell::new(AmbientClock::new(Instant::now()));
}

/// Pointer, wheel or keyboard input reached the window: the ambient drift keeps going.
pub fn note_input() {
    AMBIENT_CLOCK.with(|c| c.borrow_mut().input_at = Instant::now());
}

/// Music plays (`on`) or not: while it plays the ambient drift never rests.
pub fn note_playing(on: bool) {
    AMBIENT_CLOCK.with(|c| c.borrow_mut().playing = on);
}

/// Seconds on the ambient drift's clock (0 with reduced motion): what the
/// slow lights move by. It slows to a stop when the window goes idle and
/// resumes from there, instead of jumping ahead by the time it rested.
pub fn ambient_clock() -> f32 {
    if reduced() {
        return 0.0;
    }
    AMBIENT_CLOCK.with(|c| c.borrow_mut().advance(Instant::now(), focused())) as f32
}

/// Whether ambient motion should keep drawing frames at `now`.
pub fn ambient_moving(now: Instant) -> bool {
    !reduced() && AMBIENT_CLOCK.with(|c| c.borrow_mut().moving(now, focused()))
}

/// When an ambient effect drawn at `now` wants its next frame (`slow`: the
/// album banners' lights, which take 5-8 s to cross), or `None` once the
/// drift has come to rest.
pub fn ambient_next(now: Instant, slow: bool) -> Option<Instant> {
    ambient_moving(now).then(|| now + if slow { slow_interval() } else { interval() })
}

fn interval() -> Duration {
    if focused() {
        Duration::from_millis(33)
    } else {
        Duration::from_millis(100)
    }
}

fn slow_interval() -> Duration {
    if focused() {
        Duration::from_millis(50)
    } else {
        Duration::from_millis(125)
    }
}

/// Frame interval of the slow ambient motion (the cover's drifting lights,
/// the liquid flow, the playing bars): 30 fps while the window has the focus,
/// 10 fps behind other windows. Every frame redraws the whole window (about
/// 5 ms of CPU), so an ambient effect left at 30 fps in the background cost
/// ~15 % of a core for a picture nobody was looking at closely. Once the
/// drift has come to rest (see [`AMBIENT_IDLE`]) it is an hour: no frames
/// (prefer [`ambient_next`], which says so).
pub fn ambient_interval() -> Duration {
    if ambient_moving(Instant::now()) {
        interval()
    } else {
        AMBIENT_ASLEEP
    }
}

/// The slowest ambient motion (album banners and panes lit by a cover, whose
/// lights take 5-8 s to cross): 20 fps with the focus, 8 fps without — the
/// eye cannot tell it from 30 at that speed. At rest, as [`ambient_interval`].
pub fn ambient_slow_interval() -> Duration {
    if ambient_moving(Instant::now()) {
        slow_interval()
    } else {
        AMBIENT_ASLEEP
    }
}

/// For tests: the window has been idle since long ago.
#[cfg(test)]
pub fn idle_for_tests() {
    AMBIENT_CLOCK.with(|c| {
        let mut c = c.borrow_mut();
        c.input_at = Instant::now().checked_sub(AMBIENT_IDLE * 2).unwrap_or(c.input_at);
    });
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
        self.go_ahead(target, dur, now, 0.0);
    }

    /// [`Self::go`], already `lead` (0..1) of the way through its curve at
    /// `now`: the value steps `ease(lead)` of the way at once, so the first
    /// frame shows the change (see [`PRESS_LEAD`]). It starts from wherever
    /// the value is and only ever steps toward `target`, so a press during a
    /// release never jumps back.
    pub fn go_ahead(&mut self, target: f32, dur: Duration, now: Instant, lead: f32) {
        if (self.to - target).abs() < 1e-4 {
            return;
        }
        let from = self.value_at(now);
        let at = now.checked_sub(dur.mul_f32(lead.clamp(0.0, 1.0))).unwrap_or(now);
        *self = Self { from, to: target, at, dur };
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
        full_motion_for_tests();
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

    #[test]
    fn a_press_shows_on_the_first_frame_and_never_jumps_back() {
        full_motion_for_tests();
        let t0 = Instant::now();
        let mut a = Anim::still(0.0);
        a.go_ahead(1.0, PRESS_IN, t0, PRESS_LEAD);
        // The frame drawn right after the click: two thirds of the press already.
        assert!(a.value_at(t0) > 0.6, "{}", a.value_at(t0));
        assert!(a.done_at(t0 + PRESS_IN));
        // A press during a release steps forward from where the release is.
        let mut b = Anim::still(1.0);
        b.go(0.0, PRESS_OUT, t0);
        let mid = t0 + PRESS_OUT / 4;
        let v = b.value_at(mid);
        b.go_ahead(1.0, PRESS_IN, mid, PRESS_LEAD);
        assert!(b.value_at(mid) >= v, "{} < {v}", b.value_at(mid));
        assert!(b.value_at(mid) < 1.0);
    }

    #[test]
    fn the_ambient_drift_comes_to_rest_when_idle_and_resumes_without_a_jump() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut c = AmbientClock::new(t0);
        // In use: the drift runs at wall-clock pace.
        let mut last = c.advance(t0, true);
        for i in 1..=100 {
            last = c.advance(ms(i * 33), true);
        }
        assert!((last - 3.3).abs() < 0.05, "{last}");
        assert!(c.moving(ms(3300), true));
        // Idle past AMBIENT_IDLE (nothing playing): it slows down smoothly, then stops asking.
        let idle = AMBIENT_IDLE.as_millis() as u64;
        let mut frame = idle;
        let mut prev_step = f64::MAX;
        let mut steps = 0;
        let mut secs = c.advance(ms(frame), true);
        while c.moving(ms(frame), true) && steps < 1000 {
            frame += 33;
            let next = c.advance(ms(frame), true);
            let step = next - secs;
            assert!(step <= prev_step + 1e-6, "it decelerates: {step} after {prev_step}");
            prev_step = step;
            secs = next;
            steps += 1;
        }
        let settle = Duration::from_millis(steps * 33);
        assert!(settle <= AMBIENT_SETTLE * 2 && settle >= AMBIENT_SETTLE / 2, "came to rest in {settle:?}");
        // At rest, time passing does not move it.
        let rest = c.advance(ms(frame + 60_000), true);
        assert!((rest - secs).abs() < 1e-9);
        // Input again: it picks up from where it rested, gently.
        c.input_at = ms(frame + 60_000);
        let first = c.advance(ms(frame + 60_033), true) - rest;
        assert!(first > 0.0 && first < 0.033, "{first}");
        // Music playing keeps it going without input, also behind other windows.
        let mut p = AmbientClock::new(t0);
        p.playing = true;
        assert!(p.moving(ms(idle * 3), false));
        // Behind other windows with nothing playing: at rest.
        let mut q = AmbientClock::new(t0);
        q.advance(t0, false);
        assert!(!q.awake(ms(10), false));
    }
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod app_tests;
