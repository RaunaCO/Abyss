//! Is anybody there? The interface "rests" when nothing plays and nobody has
//! touched the window for a while (or it is behind other windows). Ambient
//! motion comes to rest on its own (`motion` on the motion branch); this
//! tells the interface when a rest ends, so the first input after one asks
//! for a new view (a frame) even when the pointer moves over nothing that
//! reacts. [`ambient_clock`] is a motion clock that does not advance while resting.
//!
//! Fed by the interface: [`input`] on every keyboard / mouse / touch event,
//! [`set_playing`] from the tick, [`set_focused`] from the window events.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Without input for this long (window focused, nothing playing), the interface rests
/// (the same as `motion::AMBIENT_IDLE` on the motion branch).
pub const REST_AFTER: Duration = Duration::from_secs(30);
/// Behind other windows, nothing playing: it rests this soon after the last input.
pub const REST_AFTER_UNFOCUSED: Duration = Duration::from_secs(3);

static PLAYING: AtomicBool = AtomicBool::new(false);
static FOCUSED: AtomicBool = AtomicBool::new(true);

struct Clock {
    epoch: Instant,
    last_input: Instant,
    /// Time spent resting before the current stretch of activity.
    rested: Duration,
    /// When the current rest began (set once it is noticed).
    resting_since: Option<Instant>,
}

static CLOCK: Mutex<Option<Clock>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Clock) -> T) -> T {
    let mut g = CLOCK.lock();
    let c = g.get_or_insert_with(|| {
        let now = Instant::now();
        Clock { epoch: now, last_input: now, rested: Duration::ZERO, resting_since: None }
    });
    f(c)
}

fn rest_after() -> Duration {
    if FOCUSED.load(Ordering::Relaxed) {
        REST_AFTER
    } else {
        REST_AFTER_UNFOCUSED
    }
}

/// When the rest began, if the interface is resting at `now`.
fn rest_start(c: &Clock, now: Instant) -> Option<Instant> {
    if PLAYING.load(Ordering::Relaxed) {
        return None;
    }
    // (Never before the clock started: a rest cannot begin before Abyss did.)
    let due = (c.last_input + rest_after()).max(c.epoch);
    (now >= due).then(|| c.resting_since.unwrap_or(due))
}

/// The user did something. True when this ends a rest (the caller should
/// ask for a frame so the motion starts again).
pub fn input() -> bool {
    let now = Instant::now();
    with(|c| {
        let woke = match rest_start(c, now) {
            Some(since) => {
                c.rested += now.saturating_duration_since(since);
                true
            }
            None => false,
        };
        c.resting_since = None;
        c.last_input = now;
        woke
    })
}

/// Something plays (motion follows the music: no rest while it does).
pub fn set_playing(on: bool) {
    let was = PLAYING.swap(on, Ordering::Relaxed);
    if was != on {
        let now = Instant::now();
        with(|c| {
            if on {
                // A rest ends when playback starts, as with an input.
                if let Some(since) = c.resting_since.take() {
                    c.rested += now.saturating_duration_since(since);
                }
            }
            // Stopping counts as the last thing the user did.
            c.last_input = c.last_input.max(now);
        });
    }
}

/// The window gained or lost the focus. Gaining it counts as an input (true
/// when that ends a rest).
pub fn set_focused(on: bool) -> bool {
    // The input is judged with the focus as it was (a rest behind other windows ends here).
    let woke = on && input();
    FOCUSED.store(on, Ordering::Relaxed);
    woke
}

/// Nothing plays and nobody is there: hold ambient motion still.
pub fn at_rest() -> bool {
    let now = Instant::now();
    with(|c| match rest_start(c, now) {
        Some(since) => {
            c.resting_since.get_or_insert(since);
            true
        }
        None => false,
    })
}

/// Seconds of motion time: the wall clock minus the time spent resting, so
/// an ambient effect driven by it resumes from the frame it stopped on.
pub fn ambient_clock() -> f32 {
    let now = Instant::now();
    with(|c| {
        let resting = rest_start(c, now).map_or(Duration::ZERO, |since| now.saturating_duration_since(since));
        (now.saturating_duration_since(c.epoch).saturating_sub(c.rested + resting)).as_secs_f32()
    })
}

/// For tests: pretend the last input was `ago`.
#[cfg(test)]
pub fn input_ago(ago: Duration) {
    with(|c| {
        c.last_input = Instant::now().checked_sub(ago).unwrap_or(c.epoch);
        c.resting_since = None;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test: the state is global.
    #[test]
    fn rests_without_input_and_resumes_the_clock_where_it_stopped() {
        let _ = set_focused(true);
        set_playing(false);
        input();
        assert!(!at_rest());
        input_ago(REST_AFTER + Duration::from_secs(1));
        assert!(at_rest());
        let frozen = ambient_clock();
        std::thread::sleep(Duration::from_millis(60));
        assert!((ambient_clock() - frozen).abs() < 0.01, "the clock holds while resting");
        assert!(input(), "the first input after a rest wakes it");
        assert!(!at_rest());
        assert!(!input(), "later inputs are no news");
        std::thread::sleep(Duration::from_millis(30));
        assert!(ambient_clock() > frozen + 0.02, "and then it runs again");
        // Playing never rests.
        input_ago(REST_AFTER * 3);
        set_playing(true);
        assert!(!at_rest());
        set_playing(false);
        assert!(!at_rest(), "stopping counts as an input");
        // Behind other windows it rests sooner.
        let _ = set_focused(false);
        input_ago(REST_AFTER_UNFOCUSED + Duration::from_millis(10));
        assert!(at_rest());
        assert!(set_focused(true), "coming back to the window wakes it");
        assert!(!at_rest());
        input();
    }
}
