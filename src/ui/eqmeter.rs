//! Level meters of the equaliser page: ballistics and geometry (pure, tested)
//! and the painting. One meter per channel shows the real peak that reaches the
//! device; a gain fader next to it trims that channel.

use gpui::{point, px, size, Bounds, Hsla, Pixels, Window};

/// Bottom and top of the scale, in dBFS.
pub const MIN_DB: f32 = -60.0;
pub const MAX_DB: f32 = 6.0;
/// Marks of the scale.
pub const TICKS: [f32; 6] = [-60.0, -48.0, -36.0, -24.0, -12.0, 0.0];
/// How fast the bar falls and how long and how fast the held peak does.
const RELEASE_DB_PER_S: f32 = 24.0;
const HOLD_SECS: f32 = 1.5;
const HOLD_FALL_DB_PER_S: f32 = 18.0;
/// Range of the channel gain fader and the zone around 0 dB that snaps to it.
pub const TRIM_RANGE_DB: f32 = 12.0;
const TRIM_DETENT_DB: f32 = 0.25;

pub fn to_db(linear: f32) -> f32 {
    if linear > 0.0 {
        20.0 * linear.log10()
    } else {
        f32::NEG_INFINITY
    }
}

/// 0.0 at the bottom of the scale to 1.0 at the top.
pub fn frac(db: f32) -> f32 {
    ((db - MIN_DB) / (MAX_DB - MIN_DB)).clamp(0.0, 1.0)
}

/// The fader: 0.0 at the bottom (-12 dB) to 1.0 at the top (+12 dB), snapping to 0 dB near the middle.
pub fn trim_of_frac(frac: f32) -> f32 {
    let db = ((frac.clamp(0.0, 1.0) * 2.0 - 1.0) * TRIM_RANGE_DB * 10.0).round() / 10.0;
    if db.abs() <= TRIM_DETENT_DB {
        0.0
    } else {
        db
    }
}

pub fn frac_of_trim(db: f32) -> f32 {
    ((db / TRIM_RANGE_DB + 1.0) / 2.0).clamp(0.0, 1.0)
}

/// Bar, held peak and clip warning of both channels.
#[derive(Clone, Default)]
pub struct Meter {
    level: [f32; 2],
    hold: [f32; 2],
    hold_age: [f32; 2],
    clip: [bool; 2],
}

impl Meter {
    /// Feed the loudest sample of each channel since the last call (linear),
    /// `dt` seconds after it. The bar rises at once and falls slowly; the held peak
    /// stays for a moment; a sample over full scale lights the clip warning.
    #[allow(clippy::needless_range_loop)] // four parallel per-channel arrays
    pub fn update(&mut self, peaks: [f32; 2], dt: f32) {
        let fall = 10f32.powf(-RELEASE_DB_PER_S * dt / 20.0);
        let hold_fall = 10f32.powf(-HOLD_FALL_DB_PER_S * dt / 20.0);
        for ch in 0..2 {
            let p = peaks[ch];
            self.level[ch] = if p > self.level[ch] { p } else { self.level[ch] * fall };
            if self.level[ch] < 1e-4 {
                self.level[ch] = 0.0;
            }
            if p >= self.hold[ch] {
                self.hold[ch] = p;
                self.hold_age[ch] = 0.0;
            } else {
                self.hold_age[ch] += dt;
                if self.hold_age[ch] > HOLD_SECS {
                    self.hold[ch] = (self.hold[ch] * hold_fall).max(self.level[ch]);
                }
            }
            if p > 1.0 {
                self.clip[ch] = true;
            }
        }
    }

    pub fn level_db(&self, ch: usize) -> f32 {
        to_db(self.level[ch])
    }

    pub fn hold_db(&self, ch: usize) -> f32 {
        to_db(self.hold[ch])
    }

    pub fn clip(&self, ch: usize) -> bool {
        self.clip[ch]
    }

    pub fn clear_clip(&mut self, ch: usize) {
        self.clip[ch] = false;
    }

    /// Loudest of the two channels right now, if there is sound.
    pub fn loudest_db(&self) -> Option<f32> {
        let db = self.level_db(0).max(self.level_db(1));
        db.is_finite().then_some(db)
    }
}

/// Colour of a bar at `db`: calm up to -12 dBFS, amber up to -3, red above.
pub fn zone(db: f32) -> u8 {
    if db > -3.0 {
        2
    } else if db > -12.0 {
        1
    } else {
        0
    }
}

/// Paint one meter inside `bounds`: the track, the bar in colour zones, the
/// scale marks and the held peak.
pub fn paint(bounds: Bounds<Pixels>, level_db: f32, hold_db: f32, colors: [Hsla; 3], track: Hsla, mark: Hsla, window: &mut Window) {
    let (w, h) = (bounds.size.width, bounds.size.height);
    window.paint_quad(gpui::fill(bounds, track));
    // The bar, in slices so each zone has its colour.
    let top = frac(level_db);
    if top > 0.0 {
        let mut from = 0.0f32;
        for (limit, color) in [(frac(-12.0), colors[0]), (frac(-3.0), colors[1]), (1.0, colors[2])] {
            let to = top.min(limit);
            if to > from {
                let y0 = bounds.origin.y + h * (1.0 - to);
                let slice = Bounds::new(point(bounds.origin.x, y0), size(w, h * (to - from)));
                window.paint_quad(gpui::fill(slice, color));
                from = to;
            }
        }
    }
    for tick in TICKS {
        let y = bounds.origin.y + h * (1.0 - frac(tick));
        window.paint_quad(gpui::fill(Bounds::new(point(bounds.origin.x, y), size(w, px(1.))), mark));
    }
    if hold_db.is_finite() {
        let y = bounds.origin.y + h * (1.0 - frac(hold_db));
        let color = colors[zone(hold_db) as usize];
        window.paint_quad(gpui::fill(Bounds::new(point(bounds.origin.x, y), size(w, px(2.))), color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_maps_dbfs_to_height() {
        assert_eq!(frac(MIN_DB), 0.0);
        assert_eq!(frac(MAX_DB), 1.0);
        assert_eq!(frac(-100.0), 0.0);
        assert_eq!(frac(f32::NEG_INFINITY), 0.0);
        assert!((frac(0.0) - 60.0 / 66.0).abs() < 1e-6, "0 dBFS is just under the top");
        assert!(frac(-12.0) < frac(-3.0));
        assert_eq!(to_db(1.0), 0.0);
        assert!((to_db(0.5) + 6.0206).abs() < 1e-3);
        assert_eq!(to_db(0.0), f32::NEG_INFINITY);
    }

    #[test]
    fn the_bar_rises_at_once_and_falls_at_a_steady_rate() {
        let mut m = Meter::default();
        m.update([1.0, 0.1], 0.033);
        assert_eq!(m.level_db(0), 0.0);
        assert!((m.level_db(1) + 20.0).abs() < 1e-3);
        // 0.5 s of silence: 24 dB per second, so -12 dB.
        for _ in 0..15 {
            m.update([0.0, 0.0], 0.033);
        }
        assert!((m.level_db(0) + 24.0 * 0.033 * 15.0).abs() < 0.1, "{}", m.level_db(0));
        // A louder sound wins over the fall.
        m.update([0.9, 0.9], 0.033);
        assert!((m.level_db(0) - to_db(0.9)).abs() < 1e-3);
        // Long silence ends at nothing, not at a tiny number.
        for _ in 0..400 {
            m.update([0.0, 0.0], 0.033);
        }
        assert_eq!(m.level_db(0), f32::NEG_INFINITY);
        assert_eq!(m.loudest_db(), None);
    }

    #[test]
    fn the_held_peak_stays_then_falls() {
        let mut m = Meter::default();
        m.update([0.8, 0.0], 0.033);
        let held = m.hold_db(0);
        for _ in 0..40 {
            m.update([0.0, 0.0], 0.033); // 1.32 s
        }
        assert_eq!(m.hold_db(0), held, "kept for 1.5 s");
        for _ in 0..60 {
            m.update([0.0, 0.0], 0.033);
        }
        assert!(m.hold_db(0) < held - 6.0, "then it falls: {}", m.hold_db(0));
        assert!(m.hold_db(0) >= m.level_db(0));
    }

    #[test]
    fn going_over_full_scale_lights_the_warning_until_cleared() {
        let mut m = Meter::default();
        m.update([1.0, 0.5], 0.033);
        assert!(!m.clip(0) && !m.clip(1), "exactly full scale is not an over");
        m.update([1.02, 0.5], 0.033);
        assert!(m.clip(0) && !m.clip(1));
        m.update([0.0, 0.0], 0.033);
        assert!(m.clip(0), "it stays on");
        m.clear_clip(0);
        assert!(!m.clip(0));
    }

    #[test]
    fn channels_are_independent() {
        let mut m = Meter::default();
        m.update([0.9, 0.0], 0.033);
        assert!(m.level_db(0) > -2.0 && m.level_db(1) == f32::NEG_INFINITY);
        assert!(m.loudest_db().is_some_and(|d| d > -2.0));
    }

    #[test]
    fn the_fader_runs_from_minus_12_to_plus_12_and_snaps_to_zero() {
        assert_eq!(trim_of_frac(0.0), -12.0);
        assert_eq!(trim_of_frac(1.0), 12.0);
        assert_eq!(trim_of_frac(0.5), 0.0);
        assert_eq!(trim_of_frac(0.5 + 0.004), 0.0, "close to the middle it snaps to 0 dB");
        assert!((trim_of_frac(0.75) - 6.0).abs() < 1e-6);
        assert!((frac_of_trim(6.0) - 0.75).abs() < 1e-6);
        for db in [-12.0, -7.3, 0.0, 3.5, 12.0] {
            assert!((trim_of_frac(frac_of_trim(db)) - db).abs() <= 0.1, "{db}");
        }
        assert_eq!(frac_of_trim(100.0), 1.0);
    }

    #[test]
    fn the_colour_zones_are_calm_amber_and_red() {
        assert_eq!((zone(-40.0), zone(-12.0), zone(-6.0), zone(-3.0), zone(-2.9), zone(3.0)), (0, 0, 1, 1, 2, 2));
    }
}
