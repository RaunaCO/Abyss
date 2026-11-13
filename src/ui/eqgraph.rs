//! Frequency response graph of the equaliser chain (a Bode magnitude plot):
//! pure geometry (tested) and the painting that uses it.
//!
//! X is logarithmic from 20 Hz to 20 kHz; Y is linear in dB around 0 dB with a
//! scale picked from the curves (6, 12, 18, 24 or 36 dB each way).

use gpui::{point, px, size, Bounds, Hsla, PathBuilder, Pixels, Window};

pub const F_MIN: f64 = 20.0;
pub const F_MAX: f64 = 20_000.0;
/// Points of each curve.
pub const POINTS: usize = 256;
/// Vertical half-ranges to choose from, in dB.
const SCALES: [f32; 5] = [6.0, 12.0, 18.0, 24.0, 36.0];
/// Vertical grid lines: every 1 to 9 times a power of ten inside the range. The
/// labelled ones (20, 50, 100, 200, 500, 1k, 2k, 5k, 10k and 20k) are the major lines.
pub fn grid_freqs() -> Vec<(f64, bool)> {
    let mut lines = Vec::new();
    for decade in [10.0, 100.0, 1000.0, 10_000.0] {
        for k in 1..=9 {
            let f = decade * f64::from(k);
            if (F_MIN..=F_MAX).contains(&f) {
                lines.push((f, matches!(k, 1 | 2 | 5)));
            }
        }
    }
    lines
}

/// The text of a frequency label: `100`, `1k`, `20k`.
pub fn freq_label(freq: f64) -> String {
    if freq >= 1000.0 {
        format!("{}k", freq / 1000.0)
    } else {
        format!("{freq}")
    }
}

/// The text of a level label: `0`, `+6`, `-12`.
pub fn db_label(db: f32) -> String {
    if db == 0.0 {
        "0".to_string()
    } else {
        format!("{db:+.0}")
    }
}
/// Q range of the slider (logarithmic).
pub const Q_RANGE: (f32, f32) = (0.1, 10.0);

/// The frequencies of a curve, evenly spaced on the log axis.
pub fn freqs() -> Vec<f64> {
    (0..POINTS).map(|i| freq_of_frac(i as f64 / (POINTS - 1) as f64)).collect()
}

/// 0.0 at 20 Hz to 1.0 at 20 kHz.
pub fn frac_of_freq(freq: f64) -> f32 {
    ((freq.max(F_MIN).ln() - F_MIN.ln()) / (F_MAX.ln() - F_MIN.ln())).clamp(0.0, 1.0) as f32
}

pub fn freq_of_frac(frac: f64) -> f64 {
    F_MIN * (F_MAX / F_MIN).powf(frac.clamp(0.0, 1.0))
}

pub fn frac_of_q(q: f32) -> f32 {
    ((q.max(Q_RANGE.0).ln() - Q_RANGE.0.ln()) / (Q_RANGE.1.ln() - Q_RANGE.0.ln())).clamp(0.0, 1.0)
}

pub fn q_of_frac(frac: f32) -> f32 {
    Q_RANGE.0 * (Q_RANGE.1 / Q_RANGE.0).powf(frac.clamp(0.0, 1.0))
}

/// The smallest half-range that holds every curve, so the picture is as large as it can be.
pub fn scale_for(curves: &[&[f32]]) -> f32 {
    let top = curves.iter().flat_map(|c| c.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
    SCALES.into_iter().find(|s| top <= *s).unwrap_or(SCALES[SCALES.len() - 1])
}

/// Distance from the top (0.0) to the bottom (1.0) of the graph for a level in dB,
/// with 0 dB in the middle. Levels outside the scale are held at the edges.
pub fn frac_of_db(db: f32, scale: f32) -> f32 {
    (0.5 - db / (2.0 * scale)).clamp(0.0, 1.0)
}

/// Levels of the horizontal grid lines (including 0 dB).
pub fn db_ticks(scale: f32) -> Vec<f32> {
    let step = if scale <= 12.0 { 6.0 } else { 12.0 };
    let n = (scale / step).floor() as i32;
    (-n..=n).map(|i| i as f32 * step).collect()
}

/// A curve to draw.
pub struct Curve {
    /// Level in dB at each of [`freqs`].
    pub db: Vec<f32>,
    pub color: Hsla,
}

/// Draw the grid and the curves inside `bounds`.
pub fn paint(bounds: Bounds<Pixels>, curves: &[Curve], scale: f32, grid: Hsla, zero: Hsla, window: &mut Window) {
    let (w, h) = (bounds.size.width, bounds.size.height);
    let at = |fx: f32, fy: f32| point(bounds.origin.x + w * fx, bounds.origin.y + h * fy);
    // Grid: vertical lines at the frequency ticks (the labelled ones twice as bright),
    // horizontal at the dB ticks (0 dB brightest).
    let faint = Hsla { a: grid.a * 0.5, ..grid };
    for (freq, major) in grid_freqs() {
        let x = frac_of_freq(freq);
        // A line exactly on the right edge would be cut off.
        let x = if x >= 1.0 { 1.0 - 1.0 / f32::from(w).max(1.0) } else { x };
        window.paint_quad(gpui::fill(Bounds::new(at(x, 0.0), size(px(1.), h)), if major { grid } else { faint }));
    }
    for db in db_ticks(scale) {
        let y = frac_of_db(db, scale);
        let color = if db == 0.0 { zero } else { grid };
        window.paint_quad(gpui::fill(Bounds::new(at(0.0, y), size(w, px(1.))), color));
    }
    for curve in curves {
        let mut path = PathBuilder::stroke(px(2.));
        for (i, db) in curve.db.iter().enumerate() {
            let p = at(i as f32 / (curve.db.len() - 1).max(1) as f32, frac_of_db(*db, scale));
            if i == 0 {
                path.move_to(p);
            } else {
                path.line_to(p);
            }
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, curve.color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::dsp::{Channels, EqConfig, FilterKind, FilterSpec};

    #[test]
    fn the_frequency_axis_is_logarithmic_from_20_hz_to_20_khz() {
        assert_eq!(frac_of_freq(20.0), 0.0);
        assert_eq!(frac_of_freq(20_000.0), 1.0);
        // A decade is a third of the way: 200 Hz and 2 kHz are equally far from their neighbours.
        let (a, b, c) = (frac_of_freq(100.0), frac_of_freq(1000.0), frac_of_freq(10_000.0));
        assert!(((b - a) - (c - b)).abs() < 1e-6);
        assert!((frac_of_freq(1000.0) - 0.5663).abs() < 1e-3);
        for f in [20.0, 55.0, 440.0, 3000.0, 20_000.0] {
            assert!((freq_of_frac(f64::from(frac_of_freq(f))) - f).abs() / f < 1e-5, "{f}");
        }
        // Out of range is held at the edges.
        assert_eq!((frac_of_freq(5.0), frac_of_freq(96_000.0)), (0.0, 1.0));
        let f = freqs();
        assert_eq!(f.len(), POINTS);
        assert!((f[0] - F_MIN).abs() < 1e-9 && (f[POINTS - 1] - F_MAX).abs() < 1e-6);
        assert!(f.windows(2).all(|w| w[1] > w[0]));
    }

    #[test]
    fn the_grid_has_a_line_per_1_to_9_times_a_power_of_ten_and_labels_the_usual_ones() {
        let grid = grid_freqs();
        let majors: Vec<f64> = grid.iter().filter(|(_, m)| *m).map(|(f, _)| *f).collect();
        assert_eq!(majors, [20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0]);
        assert!(grid.iter().any(|(f, m)| *f == 30.0 && !m) && grid.iter().any(|(f, m)| *f == 700.0 && !m));
        assert!(grid.iter().all(|(f, _)| (F_MIN..=F_MAX).contains(f)));
        assert!(grid.windows(2).all(|w| w[1].0 >= w[0].0), "in order");
        let labels: Vec<String> = majors.iter().map(|f| freq_label(*f)).collect();
        assert_eq!(labels, ["20", "50", "100", "200", "500", "1k", "2k", "5k", "10k", "20k"]);
        assert_eq!((db_label(0.0), db_label(6.0), db_label(-12.0)), ("0".to_string(), "+6".to_string(), "-12".to_string()));
    }

    #[test]
    fn the_scale_is_the_smallest_that_holds_the_curves() {
        assert_eq!(scale_for(&[&[0.0, 0.0]]), 6.0);
        assert_eq!(scale_for(&[&[1.0, -5.9]]), 6.0);
        assert_eq!(scale_for(&[&[1.0, 6.1]]), 12.0);
        assert_eq!(scale_for(&[&[0.0], &[-13.0, 2.0]]), 18.0);
        assert_eq!(scale_for(&[&[24.0]]), 24.0);
        assert_eq!(scale_for(&[&[100.0]]), 36.0, "beyond the largest scale the curve is held at the edge");
        assert_eq!(scale_for(&[]), 6.0);
    }

    #[test]
    fn levels_map_to_the_vertical_axis_with_zero_in_the_middle() {
        assert_eq!(frac_of_db(0.0, 12.0), 0.5);
        assert_eq!(frac_of_db(12.0, 12.0), 0.0);
        assert_eq!(frac_of_db(-12.0, 12.0), 1.0);
        assert_eq!(frac_of_db(30.0, 12.0), 0.0);
        assert!(frac_of_db(6.0, 12.0) < frac_of_db(-6.0, 12.0), "louder is higher");
        assert_eq!(db_ticks(6.0), [-6.0, 0.0, 6.0]);
        assert_eq!(db_ticks(12.0), [-12.0, -6.0, 0.0, 6.0, 12.0]);
        assert_eq!(db_ticks(24.0), [-24.0, -12.0, 0.0, 12.0, 24.0]);
    }

    #[test]
    fn q_slider_is_logarithmic_and_round_trips() {
        assert!((q_of_frac(0.0) - 0.1).abs() < 1e-6 && (q_of_frac(1.0) - 10.0).abs() < 1e-4);
        assert!((q_of_frac(0.5) - 1.0).abs() < 1e-4, "1.0 sits in the middle");
        for q in [0.1, 0.3, 0.707, 1.0, 4.0, 10.0] {
            assert!((q_of_frac(frac_of_q(q)) - q).abs() / q < 1e-4, "{q}");
        }
    }

    fn config(filters: Vec<FilterSpec>) -> EqConfig {
        EqConfig { enabled: true, preamp_db: 0.0, trim_db: [0.0; 2], filters }
    }

    #[test]
    fn the_curve_of_a_peaking_filter_peaks_at_its_frequency_and_matches_the_analytic_response() {
        let cfg = config(vec![FilterSpec { gain_db: 9.0, freq: 1000.0, q: 1.4, ..FilterSpec::new(FilterKind::Peaking) }]);
        let f = freqs();
        let curve = cfg.response_curve(0, 48_000.0, &f);
        assert_eq!(curve.len(), POINTS);
        let top = curve.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert!((f[top] - 1000.0).abs() / 1000.0 < 0.03, "peak at {} Hz", f[top]);
        assert!((curve[top] - 9.0).abs() < 0.1);
        for (hz, db) in f.iter().zip(&curve).step_by(17) {
            let want = cfg.response_db(0, *hz, 48_000.0);
            assert!((f64::from(*db) - want).abs() < 1e-4, "{hz} Hz");
        }
        // Far from the centre it is flat.
        assert!(curve[0].abs() < 0.3 && curve[POINTS - 1].abs() < 0.6);
    }

    #[test]
    fn an_empty_chain_draws_a_flat_line_and_the_pre_amp_moves_it() {
        let f = freqs();
        let flat = config(Vec::new()).response_curve(0, 48_000.0, &f);
        assert!(flat.iter().all(|v| *v == 0.0));
        let down = EqConfig { preamp_db: -6.0, ..config(Vec::new()) }.response_curve(1, 48_000.0, &f);
        assert!(down.iter().all(|v| (*v + 6.0).abs() < 1e-6));
    }

    #[test]
    fn each_channel_has_its_own_curve() {
        let left = FilterSpec { gain_db: 6.0, channels: Channels::Left, ..FilterSpec::new(FilterKind::Peaking) };
        let cfg = config(vec![left]);
        let f = freqs();
        let (l, r) = (cfg.response_curve(0, 48_000.0, &f), cfg.response_curve(1, 48_000.0, &f));
        assert!(l.iter().any(|v| *v > 5.0));
        assert!(r.iter().all(|v| *v == 0.0));
    }
}
