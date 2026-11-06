//! Time-based animations (60 fps while something moves): sidebar width,
//! the Now Playing panel, and the progress bar between engine updates.

use std::time::{Duration, Instant};

use gpui::Window;

use super::AppView;

pub const SIDEBAR_ANIM: Duration = Duration::from_millis(220);
pub const PANEL_ANIM: Duration = Duration::from_millis(260);

/// Smooth deceleration.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(4)
}

/// Progress (0..1) of an animation that started at `at`.
pub fn progress(at: Instant, len: Duration) -> f32 {
    (at.elapsed().as_secs_f32() / len.as_secs_f32()).clamp(0.0, 1.0)
}

impl AppView {
    /// Current sidebar width, animated between the two modes.
    pub fn sidebar_width_animated(&self) -> f32 {
        let target = self.sidebar_width();
        match self.sidebar_anim {
            Some((from, at)) => {
                let t = ease_out(progress(at, SIDEBAR_ANIM));
                from + (target - from) * t
            }
            None => target,
        }
    }

    /// Now Playing slide: 0 = hidden, 1 = fully open.
    pub fn now_playing_amount(&self) -> f32 {
        match self.np_anim {
            Some((opening, at)) => {
                let t = ease_out(progress(at, PANEL_ANIM));
                if opening {
                    t
                } else {
                    1.0 - t
                }
            }
            None => {
                if self.now_playing_open {
                    1.0
                } else {
                    0.0
                }
            }
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
        if let Some((opening, at)) = self.np_anim {
            if progress(at, PANEL_ANIM) >= 1.0 {
                self.np_anim = None;
                if !opening {
                    self.now_playing_open = false;
                }
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
        if self.ambient_animating() {
            running = true;
        }
        let playing = self.now.is_some() && !self.snap.paused && !self.ended;
        if self.haptics.active() {
            running = true;
        }
        if running || playing || self.scroll_animating {
            window.request_animation_frame();
        }
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
