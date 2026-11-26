//! Synchronised lyrics: which line is current, how sharp each line is and
//! where the view scrolls, as a function of the playback clock. Shared by the
//! side panel and Now Playing, in every interface.

use std::time::{Duration, Instant};

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

/// Windows accessibility preference ("Show animations in Windows" off),
/// refreshed every two seconds without a per-line system call.
pub fn reduced_motion() -> bool {
    if let Some(forced) = std::env::var_os("ABYSS_REDUCED_MOTION") {
        return forced != "0";
    }
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
            // SPI_GETCLIENTAREAANIMATION
            let ok = unsafe { SystemParametersInfoW(0x1042, 0, &mut enabled, 0) };
            let reduced = ok != 0 && enabled == 0;
            *pref = Some((Instant::now(), reduced));
            reduced
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (Instant::now(), Duration::ZERO);
        false
    }
}

/// Playback-relative emphasis shared by the side panel and fullscreen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LyricTiming {
    pub current: Option<usize>,
    /// The line the scroll is moving towards and how far (0..1, eased).
    pub next_scroll: Option<(usize, f32)>,
    next_blur: f32,
    previous_blur: f32,
}

impl LyricTiming {
    pub fn new(lines: &[(f64, String)], position: f64) -> Self {
        Self::at_position(lines, position, reduced_motion())
    }

    pub fn at_position(lines: &[(f64, String)], position: f64, reduced_motion: bool) -> Self {
        const SCROLL_PREVIEW: f64 = 0.35;
        const INTRO_PREVIEW: f64 = 2.0;
        const SETTLE: f64 = 0.18;

        let current = lines.iter().rposition(|(at, _)| *at <= position);
        let mut timing = Self { current, next_scroll: None, next_blur: 1.0, previous_blur: 1.0 };
        if reduced_motion {
            return timing;
        }

        let next_index = current.map_or(0, |i| i + 1);
        if let Some(&(next_at, _)) = lines.get(next_index) {
            let start = current.map_or(next_at - INTRO_PREVIEW, |i| lines[i].0);
            // Bring the next lyric into focus across the interval. Scrolling still starts near the cue.
            let progress = ((position - start) / (next_at - start)).clamp(0.0, 1.0) as f32;
            timing.next_blur = 1.0 - progress * progress * (3.0 - 2.0 * progress);
            let blend = ((position - next_at + SCROLL_PREVIEW) / SCROLL_PREVIEW).clamp(0.0, 1.0) as f32;
            timing.next_scroll = Some((next_index, lyrics_ease(blend)));
        }
        if let Some(i) = current.filter(|&i| i > 0 && lines[i - 1].0 < lines[i].0) {
            let elapsed = ((position - lines[i].0) / SETTLE).clamp(0.0, 1.0) as f32;
            timing.previous_blur = lyrics_ease(elapsed);
        }
        timing
    }

    /// Blur of `line`, 0 (sharp) to 1 (fully soft): the active line is sharp,
    /// the next one sharpens as its cue approaches, the one just left softens
    /// quickly, and every other line stays soft.
    pub fn blur(&self, line: usize) -> f32 {
        if self.current == Some(line) {
            0.0
        } else if line == self.current.map_or(0, |i| i + 1) {
            self.next_blur
        } else if self.current == Some(line + 1) {
            self.previous_blur
        } else {
            1.0
        }
    }
}

/// Opacity of a line at `top..bottom` inside a visible area `h` tall: lines
/// fade out as they approach the top and bottom edges (zones in px), so no
/// line is ever shown cut in half against the panel edges or the tabs.
#[cfg_attr(not(feature = "ui-iced"), allow(dead_code))]
pub fn edge_fade(top: f32, bottom: f32, h: f32, top_zone: f32, bottom_zone: f32) -> f32 {
    if h <= 1.0 {
        return 1.0;
    }
    if top >= h || bottom <= 0.0 {
        return 0.0;
    }
    let mid = (top.max(0.0) + bottom.min(h)) / 2.0;
    let smooth = |t: f32| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    smooth(mid / top_zone.max(1.0)).min(smooth((h - mid) / bottom_zone.max(1.0)))
}

/// The scroll offset that puts a line whose top is `line_top` (content px,
/// blended towards the next line by the caller) at `fraction` of a viewport
/// `viewport_h` tall, clamped to the scrollable range.
#[cfg_attr(not(feature = "ui-iced"), allow(dead_code))]
pub fn follow_target(line_top: f32, viewport_h: f32, fraction: f32, max_offset: f32) -> f32 {
    (line_top - viewport_h * fraction).clamp(0.0, max_offset.max(0.0))
}

/// One step of the follow glide from `offset` towards `target` after `dt`
/// seconds (time constant 140 ms, so it glides the same at any refresh
/// rate). With reduced motion it jumps.
#[cfg_attr(not(feature = "ui-iced"), allow(dead_code))]
pub fn follow_step(offset: f32, target: f32, dt: f32, reduced: bool) -> f32 {
    let delta = target - offset;
    if reduced || delta.abs() <= 0.6 {
        return target;
    }
    offset + delta * (1.0 - (-dt.min(0.05) / 0.14).exp())
}

#[cfg(test)]
mod lyric_tests {
    use super::{edge_fade, follow_step, follow_target, LyricTiming};

    #[test]
    fn only_the_next_lyric_comes_into_focus_before_its_cue() {
        let lines = [(1.0, String::new()), (5.0, String::new()), (8.0, String::new()), (11.0, String::new())];
        let early = LyricTiming::at_position(&lines, 2.0, false);
        let middle = LyricTiming::at_position(&lines, 3.0, false);
        let late = LyricTiming::at_position(&lines, 4.0, false);
        let switching = LyricTiming::at_position(&lines, 4.999, false);
        let activated = LyricTiming::at_position(&lines, 5.0, false);
        let settled = LyricTiming::at_position(&lines, 5.3, false);

        assert_eq!(early.blur(0), 0.0);
        assert_eq!(early.blur(2), 1.0);
        assert_eq!(early.blur(3), 1.0);
        assert!(1.0 > early.blur(1) && early.blur(1) > middle.blur(1));
        assert!(middle.blur(1) > late.blur(1));
        assert_eq!(middle.next_scroll, Some((1, 0.0)));
        assert_eq!(switching.current, Some(0));
        assert!(switching.blur(1) < 0.001);
        assert_eq!(activated.current, Some(1));
        assert_eq!(activated.blur(1), 0.0);
        assert_eq!(activated.blur(2), 1.0);
        assert_eq!(settled.blur(0), 1.0);
    }

    #[test]
    fn reduced_motion_switches_focus_at_the_cue() {
        let lines = [(1.0, String::new()), (3.0, String::new())];
        let before = LyricTiming::at_position(&lines, 2.9, true);
        let after = LyricTiming::at_position(&lines, 3.0, true);

        assert_eq!((before.blur(0), before.blur(1)), (0.0, 1.0));
        assert_eq!((after.blur(0), after.blur(1)), (1.0, 0.0));
        assert!(before.next_scroll.is_none());
    }

    #[test]
    fn lines_fade_at_the_edges_and_vanish_outside() {
        assert_eq!(edge_fade(-100.0, -10.0, 500.0, 60.0, 130.0), 0.0);
        assert_eq!(edge_fade(510.0, 560.0, 500.0, 60.0, 130.0), 0.0);
        assert_eq!(edge_fade(200.0, 260.0, 500.0, 60.0, 130.0), 1.0);
        let near_top = edge_fade(0.0, 40.0, 500.0, 60.0, 130.0);
        assert!(near_top > 0.0 && near_top < 1.0);
    }

    #[test]
    fn the_follow_glide_reaches_its_target_and_jumps_with_reduced_motion() {
        assert_eq!(follow_target(400.0, 600.0, 0.32, 10_000.0), 400.0 - 192.0);
        assert_eq!(follow_target(10.0, 600.0, 0.32, 10_000.0), 0.0);
        assert_eq!(follow_step(0.0, 300.0, 0.016, true), 300.0);
        let mut at = 0.0;
        for _ in 0..200 {
            at = follow_step(at, 300.0, 0.016, false);
        }
        assert_eq!(at, 300.0);
        assert!(follow_step(0.0, 300.0, 0.016, false) < 300.0);
    }
}
