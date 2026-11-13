//! Graphic equaliser: a row of bars at frequencies of your choice, built from
//! the same peaking filters as the parametric chain (so the audio thread knows
//! nothing about it).
//!
//! Neighbouring peaking filters add up, so setting every bar to +6 dB with
//! the bars' own gains would give more than +6 dB. The internal gains are
//! therefore solved so that the response at the centre of each bar is the level
//! the user set, to a few hundredths of a dB.

use serde::{Deserialize, Serialize};

use super::dsp::{design, Channels, FilterKind, FilterSpec, MAX_FILTERS, MAX_FREQ, MAX_GAIN_DB, MIN_FREQ};

/// The reference rate the gains are solved at (the real filters are designed at
/// the output rate; at other rates the levels differ by a fraction of a dB near the top).
pub const REFERENCE_RATE: f64 = 48_000.0;
/// How much wider than the distance to the nearest neighbour each bar is.
const OVERLAP: f64 = 1.3;
/// Range of the bar faders (typed values may go further, up to the filter limit).
pub const SLIDER_GAIN_DB: f32 = 12.0;

/// Parametric chain or graphic bars: which one feeds the audio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqMode {
    #[default]
    Parametric,
    Graphic,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub freq: f32,
    /// The level the user wants at this frequency (dB).
    pub gain_db: f32,
}

impl Band {
    pub fn new(freq: f32) -> Self {
        Self { freq, gain_db: 0.0 }
    }

    fn clamped(self) -> Self {
        let f = if self.freq.is_finite() { self.freq.clamp(MIN_FREQ, MAX_FREQ) } else { 1000.0 };
        let g = if self.gain_db.is_finite() { self.gain_db.clamp(-MAX_GAIN_DB, MAX_GAIN_DB) } else { 0.0 };
        Self { freq: f, gain_db: g }
    }
}

/// Standard frequencies for the usual bar counts (octave, two-thirds and
/// one-third octave), and an even spread on the log axis for any other number.
pub fn iso_bands(count: usize) -> Vec<Band> {
    let list: Vec<f32> = match count {
        5 => vec![63.0, 250.0, 1000.0, 4000.0, 16000.0],
        10 => vec![31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0],
        15 => vec![25.0, 40.0, 63.0, 100.0, 160.0, 250.0, 400.0, 630.0, 1000.0, 1600.0, 2500.0, 4000.0, 6300.0, 10000.0, 16000.0],
        31 => vec![
            20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0,
            1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
        ],
        n => {
            let n = n.clamp(1, MAX_FILTERS);
            (0..n)
                .map(|i| {
                    let t = if n == 1 { 0.5 } else { i as f64 / (n - 1) as f64 };
                    let f = 31.5 * (16_000.0f64 / 31.5).powf(t);
                    // Three significant digits read better than 1034.7.
                    let digits = f.log10().floor() as i32 - 2;
                    let step = 10f64.powi(digits);
                    ((f / step).round() * step) as f32
                })
                .collect()
        }
    };
    list.into_iter().map(Band::new).collect()
}

/// Bandwidth in octaves of the bar at `i`, from the distance to its neighbours,
/// and the Q of a peaking filter with that bandwidth.
fn band_q(freqs: &[f32], i: usize) -> f64 {
    let f = f64::from(freqs[i]);
    let prev = (i > 0).then(|| f64::from(freqs[i - 1]));
    let next = freqs.get(i + 1).map(|v| f64::from(*v));
    // The distance to the nearest neighbour: with even spacing that is the spacing,
    // and a bar that is close to another does not get a broad shape that fights it.
    let octaves = match (prev, next) {
        (Some(p), Some(n)) => (f / p).log2().min((n / f).log2()),
        (Some(p), None) => (f / p).log2(),
        (None, Some(n)) => (n / f).log2(),
        (None, None) => 1.0,
    }
    .clamp(0.1, 4.0);
    // A little wider than the spacing, so the curve between two bars set alike stays level.
    let octaves = octaves * OVERLAP;
    (1.0 / (2.0 * (std::f64::consts::LN_2 / 2.0 * octaves).sinh())).clamp(0.25, 25.0)
}

/// Solve `a x = b` for a small dense system (partial pivoting). `None` if singular.
#[allow(clippy::needless_range_loop)] // elimination reads two rows of the matrix by column
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|x, y| a[*x][col].abs().total_cmp(&a[*y][col].abs()))?;
        if a[pivot][col].abs() < 1e-9 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..n {
            let k = a[row][col] / a[col][col];
            for c in col..n {
                a[row][c] -= k * a[col][c];
            }
            b[row] -= k * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let tail: f64 = (row + 1..n).map(|c| a[row][c] * x[c]).sum();
        x[row] = (b[row] - tail) / a[row][row];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// The peaking filters that make the bars: sorted by frequency, with a Q from the
/// spacing and gains solved so each bar's level is the response at its centre.
pub fn bands_to_filters(bands: &[Band], rate: f64) -> Vec<FilterSpec> {
    let mut bars: Vec<Band> = bands.iter().take(MAX_FILTERS).map(|b| b.clamped()).collect();
    bars.sort_by(|a, b| a.freq.total_cmp(&b.freq));
    let n = bars.len();
    if n == 0 {
        return Vec::new();
    }
    let freqs: Vec<f32> = bars.iter().map(|b| b.freq).collect();
    let qs: Vec<f64> = (0..n).map(|i| band_q(&freqs, i)).collect();
    let target: Vec<f64> = bars.iter().map(|b| f64::from(b.gain_db)).collect();

    let filter = |i: usize, gain: f64| FilterSpec {
        enabled: true,
        kind: FilterKind::Peaking,
        channels: Channels::Both,
        freq: freqs[i],
        gain_db: gain as f32,
        q: qs[i] as f32,
    };
    // What each unit of gain of bar j does at the centre of bar i (a nearly linear relation).
    let unit: Vec<Vec<f64>> =
        (0..n).map(|i| (0..n).map(|j| design(&filter(j, 1.0), rate).magnitude_db(f64::from(freqs[i]), rate)).collect()).collect();
    let regularised = |m: &[Vec<f64>]| -> Vec<Vec<f64>> {
        m.iter().enumerate().map(|(i, row)| row.iter().enumerate().map(|(j, v)| if i == j { v + 1e-3 } else { *v }).collect()).collect()
    };
    let matrix = regularised(&unit);
    let respond = |gains: &[f64]| -> Vec<f64> {
        (0..n).map(|i| (0..n).map(|j| design(&filter(j, gains[j]), rate).magnitude_db(f64::from(freqs[i]), rate)).sum()).collect()
    };
    let gains = solve(matrix.clone(), target.clone()).and_then(|mut gains| {
        // The relation is not exactly linear, so correct with the real response.
        for _ in 0..12 {
            for g in &mut gains {
                *g = g.clamp(-f64::from(MAX_GAIN_DB), f64::from(MAX_GAIN_DB));
            }
            let error: Vec<f64> = respond(&gains).iter().zip(&target).map(|(r, t)| t - r).collect();
            if error.iter().all(|e| e.abs() < 0.005) {
                break;
            }
            let step = solve(matrix.clone(), error)?;
            for (g, s) in gains.iter_mut().zip(step) {
                *g += s;
            }
        }
        Some(gains)
    });
    // If bars are so close that the system is singular, fall back to the plain gains.
    let gains = gains.unwrap_or(target);
    (0..n).map(|i| filter(i, gains[i].clamp(-f64::from(MAX_GAIN_DB), f64::from(MAX_GAIN_DB)))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::dsp::EqConfig;

    fn config(filters: Vec<FilterSpec>) -> EqConfig {
        EqConfig { enabled: true, preamp_db: 0.0, trim_db: [0.0; 2], filters }
    }

    fn level_at(filters: &[FilterSpec], freq: f32) -> f64 {
        config(filters.to_vec()).response_db(0, f64::from(freq), REFERENCE_RATE)
    }

    /// A small deterministic spread of numbers in -8..8.
    fn pseudo(i: usize, seed: u32) -> f32 {
        let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(i as u32 * 40_503);
        x ^= x >> 13;
        x = x.wrapping_mul(1_274_126_177);
        ((x >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 16.0
    }

    #[test]
    fn the_standard_lists_have_the_standard_counts_and_frequencies() {
        for (n, first, last) in [(5, 63.0, 16000.0), (10, 31.5, 16000.0), (15, 25.0, 16000.0), (31, 20.0, 20000.0)] {
            let bands = iso_bands(n);
            assert_eq!(bands.len(), n);
            assert_eq!((bands[0].freq, bands[n - 1].freq), (first, last));
            assert!(bands.windows(2).all(|w| w[1].freq > w[0].freq));
            assert!(bands.iter().all(|b| b.gain_db == 0.0));
        }
        // Any other number is spread evenly on the log axis, with readable values.
        let seven = iso_bands(7);
        assert_eq!(seven.len(), 7);
        assert_eq!((seven[0].freq, seven[6].freq), (31.5, 16000.0));
        assert!(seven.windows(2).all(|w| w[1].freq > w[0].freq));
        assert_eq!(iso_bands(1).len(), 1);
        assert_eq!(iso_bands(100).len(), MAX_FILTERS);
    }

    #[test]
    fn bars_at_zero_do_nothing() {
        let filters = bands_to_filters(&iso_bands(10), REFERENCE_RATE);
        assert_eq!(filters.len(), 10);
        assert!(filters.iter().all(FilterSpec::is_neutral));
        assert!(config(filters).is_bypass(), "the audio would pass untouched");
        assert!(bands_to_filters(&[], REFERENCE_RATE).is_empty());
    }

    #[test]
    fn each_bar_reaches_the_level_set_at_its_own_frequency() {
        for count in [5, 10, 15, 31] {
            for seed in 1..=3 {
                let mut bands = iso_bands(count);
                for (i, b) in bands.iter_mut().enumerate() {
                    b.gain_db = pseudo(i, seed);
                }
                let filters = bands_to_filters(&bands, REFERENCE_RATE);
                for b in &bands {
                    let got = level_at(&filters, b.freq);
                    assert!(
                        (got - f64::from(b.gain_db)).abs() < 0.1,
                        "{count} bars, seed {seed}: {} Hz wants {:.2} gets {got:.2}",
                        b.freq,
                        b.gain_db
                    );
                }
            }
        }
    }

    #[test]
    fn custom_and_crowded_frequencies_also_reach_their_levels() {
        let bands = [(50.0, 5.0), (120.0, -4.0), (150.0, 6.0), (1000.0, 3.0), (1300.0, -6.0), (9000.0, 7.0)]
            .map(|(freq, gain_db)| Band { freq, gain_db });
        let filters = bands_to_filters(&bands, REFERENCE_RATE);
        for b in &bands {
            let got = level_at(&filters, b.freq);
            assert!((got - f64::from(b.gain_db)).abs() < 0.15, "{} Hz wants {} gets {got:.2}", b.freq, b.gain_db);
        }
    }

    #[test]
    fn all_bars_up_gives_a_nearly_flat_boost_across_the_band() {
        let mut bands = iso_bands(10);
        for b in &mut bands {
            b.gain_db = 6.0;
        }
        let filters = bands_to_filters(&bands, REFERENCE_RATE);
        let mut hz = 40.0f32;
        while hz < 14_000.0 {
            let got = level_at(&filters, hz);
            // Between two bars a graphic equaliser dips a little: about 1.5 dB with octave bars.
            assert!((4.0..=7.5).contains(&got), "{hz} Hz: {got:.2}");
            hz *= 1.07;
        }
    }

    #[test]
    fn one_bar_alone_is_a_peak_at_its_frequency_and_the_filters_come_out_sorted() {
        let filters = bands_to_filters(&[Band { freq: 2000.0, gain_db: 9.0 }], REFERENCE_RATE);
        assert_eq!(filters.len(), 1);
        assert!((level_at(&filters, 2000.0) - 9.0).abs() < 0.05);
        assert!(level_at(&filters, 200.0).abs() < 1.0 && level_at(&filters, 20_000.0).abs() < 1.5);
        let unsorted = [Band { freq: 4000.0, gain_db: 1.0 }, Band { freq: 100.0, gain_db: 2.0 }, Band { freq: 900.0, gain_db: 3.0 }];
        let freqs: Vec<f32> = bands_to_filters(&unsorted, REFERENCE_RATE).iter().map(|f| f.freq).collect();
        assert_eq!(freqs, [100.0, 900.0, 4000.0]);
    }

    #[test]
    fn identical_or_extreme_bars_never_blow_up() {
        let twins = [Band { freq: 1000.0, gain_db: 6.0 }, Band { freq: 1000.0, gain_db: 6.0 }, Band { freq: 1000.5, gain_db: -6.0 }];
        for f in bands_to_filters(&twins, REFERENCE_RATE) {
            assert!(f.gain_db.is_finite() && f.gain_db.abs() <= MAX_GAIN_DB && f.q.is_finite());
        }
        let wild = [Band { freq: f32::NAN, gain_db: 500.0 }, Band { freq: 1e9, gain_db: f32::NAN }];
        for f in bands_to_filters(&wild, REFERENCE_RATE) {
            assert!(f.freq >= MIN_FREQ && f.freq <= MAX_FREQ && f.gain_db.is_finite());
        }
        assert_eq!(bands_to_filters(&iso_bands(31).repeat(3), REFERENCE_RATE).len(), MAX_FILTERS, "at most as many as filters");
    }

    #[test]
    fn q_follows_the_spacing() {
        let narrow = [20.0, 25.0, 31.5].map(|f| f as f32);
        let wide = [63.0, 250.0, 1000.0].map(|f| f as f32);
        assert!(band_q(&narrow, 1) > band_q(&wide, 1) * 2.0, "third-octave bars are much narrower than two-octave ones");
        // Third-octave spacing is Q 4.3 for the spacing itself, and 1.3 times wider here.
        let third = iso_bands(31).iter().map(|b| b.freq).collect::<Vec<_>>();
        assert!((band_q(&third, 15) - 3.3).abs() < 0.3, "{}", band_q(&third, 15));
    }

    #[test]
    fn modes_and_bands_round_trip_through_json() {
        let json = serde_json::to_string(&(EqMode::Graphic, Band { freq: 440.0, gain_db: 2.5 })).unwrap();
        let (mode, band): (EqMode, Band) = serde_json::from_str(&json).unwrap();
        assert_eq!((mode, band), (EqMode::Graphic, Band { freq: 440.0, gain_db: 2.5 }));
        assert_eq!(EqMode::default(), EqMode::Parametric);
    }
}
