//! Parametric equaliser: a chain of biquad filters applied in the real-time
//! output callback (see `output::fill`).
//!
//! * The chain is built from [`EqConfig`]: Peaking, Low/High Shelf and
//!   High/Low Pass filters (RBJ Audio EQ Cookbook) plus a global pre-amp.
//! * **Bit-perfect when idle.** With the equaliser off, an empty chain, only
//!   neutral filters (0 dB peaking or shelf, or disabled) and no pre-amp,
//!   [`Dsp::process`] returns without touching a sample.
//! * **No locks, no allocation in the callback.** The UI writes the
//!   configuration into [`EqShared`] (a seqlock over atomic words) and the
//!   callback reads the latest consistent copy; if a write is in progress it
//!   keeps the previous one and tries again on the next callback.
//! * **No clicks.** Switching the chain on or off is a ~10 ms dry/wet blend
//!   and every other change interpolates the coefficients over ~10 ms.
//!
//! Filter state and coefficients are `f64`; samples in and out are `f32`.

use std::sync::atomic::{fence, AtomicU32, Ordering};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Filters in a chain.
pub const MAX_FILTERS: usize = 32;
pub const MIN_FREQ: f32 = 10.0;
pub const MAX_FREQ: f32 = 24_000.0;
pub const MIN_Q: f32 = 0.1;
pub const MAX_Q: f32 = 20.0;
pub const MAX_GAIN_DB: f32 = 30.0;
pub const MIN_PREAMP_DB: f32 = -40.0;
pub const MAX_PREAMP_DB: f32 = 20.0;
/// Butterworth Q for a second order filter.
pub const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// A gain or pre-amp below this (dB) does nothing audible and counts as zero.
const NEUTRAL_DB: f32 = 0.005;
/// Length of every transition.
const RAMP_SECS: f64 = 0.010;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    Peaking,
    LowShelf,
    HighShelf,
    HighPass,
    LowPass,
}

impl FilterKind {
    pub const ALL: [FilterKind; 5] = [Self::Peaking, Self::LowShelf, Self::HighShelf, Self::HighPass, Self::LowPass];

    fn code(self) -> u32 {
        match self {
            Self::Peaking => 0,
            Self::LowShelf => 1,
            Self::HighShelf => 2,
            Self::HighPass => 3,
            Self::LowPass => 4,
        }
    }

    fn from_code(code: u32) -> Option<Self> {
        Some(match code {
            0 => Self::Peaking,
            1 => Self::LowShelf,
            2 => Self::HighShelf,
            3 => Self::HighPass,
            4 => Self::LowPass,
            _ => return None,
        })
    }

    /// Whether the gain setting means anything (not for High / Low Pass).
    pub fn has_gain(self) -> bool {
        matches!(self, Self::Peaking | Self::LowShelf | Self::HighShelf)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilterSpec {
    pub enabled: bool,
    pub kind: FilterKind,
    /// Hz.
    pub freq: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl FilterSpec {
    pub fn new(kind: FilterKind) -> Self {
        let q = if kind.has_gain() { 1.0 } else { BUTTERWORTH_Q };
        Self { enabled: true, kind, freq: 1000.0, gain_db: 0.0, q }
    }

    /// Values inside the supported ranges (NaN becomes the default).
    pub fn clamped(mut self) -> Self {
        let fix = |v: f32, lo: f32, hi: f32, default: f32| if v.is_finite() { v.clamp(lo, hi) } else { default };
        self.freq = fix(self.freq, MIN_FREQ, MAX_FREQ, 1000.0);
        self.gain_db = fix(self.gain_db, -MAX_GAIN_DB, MAX_GAIN_DB, 0.0);
        self.q = fix(self.q, MIN_Q, MAX_Q, 1.0);
        self
    }

    /// Does nothing to the audio: disabled, or a peaking / shelf at 0 dB.
    /// High and Low Pass always shape the sound.
    pub fn is_neutral(&self) -> bool {
        !self.enabled || (self.kind.has_gain() && self.gain_db.abs() < NEUTRAL_DB)
    }
}

impl Default for FilterSpec {
    fn default() -> Self {
        Self::new(FilterKind::Peaking)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EqConfig {
    pub enabled: bool,
    pub preamp_db: f32,
    pub filters: Vec<FilterSpec>,
}

impl EqConfig {
    /// At most [`MAX_FILTERS`] filters and every value inside its range.
    pub fn clamped(mut self) -> Self {
        self.filters.truncate(MAX_FILTERS);
        self.filters = self.filters.into_iter().map(FilterSpec::clamped).collect();
        self.preamp_db = if self.preamp_db.is_finite() { self.preamp_db.clamp(MIN_PREAMP_DB, MAX_PREAMP_DB) } else { 0.0 };
        self
    }

    /// The audio passes through untouched.
    pub fn is_bypass(&self) -> bool {
        !self.enabled || (self.preamp_db.abs() < NEUTRAL_DB && self.filters.iter().all(FilterSpec::is_neutral))
    }

    /// Response of the whole chain (pre-amp included) in dB at `freq` Hz.
    pub fn response_db(&self, freq: f64, rate: f64) -> f64 {
        let mut db = f64::from(self.preamp_db);
        for spec in self.filters.iter().filter(|s| !s.is_neutral()) {
            db += design(spec, rate).magnitude_db(freq, rate);
        }
        db
    }

    /// The pre-amp that keeps the loudest point of the response at 0 dB or below.
    pub fn auto_preamp(&self, rate: f64) -> f32 {
        let mut flat = self.clone();
        flat.preamp_db = 0.0;
        let top = rate.min(48_000.0) * 0.45;
        let mut peak = f64::MIN;
        for i in 0..=1000 {
            let freq = 20.0 * (top / 20.0).powf(f64::from(i) / 1000.0);
            peak = peak.max(flat.response_db(freq, rate));
        }
        let pre = -((peak.max(0.0) * 10.0).ceil() / 10.0) as f32;
        pre.clamp(MIN_PREAMP_DB, MAX_PREAMP_DB)
    }
}

// ----------------------------------------------------------------- coefficients

/// Normalised biquad: `y = b0 x + z1`, `z1' = b1 x - a1 y + z2`, `z2' = b2 x - a2 y`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Coef {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

const IDENTITY: Coef = Coef { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

impl Coef {
    fn step_to(&self, target: &Coef, frames: f64) -> Coef {
        Coef {
            b0: (target.b0 - self.b0) / frames,
            b1: (target.b1 - self.b1) / frames,
            b2: (target.b2 - self.b2) / frames,
            a1: (target.a1 - self.a1) / frames,
            a2: (target.a2 - self.a2) / frames,
        }
    }

    fn add(&mut self, d: &Coef) {
        self.b0 += d.b0;
        self.b1 += d.b1;
        self.b2 += d.b2;
        self.a1 += d.a1;
        self.a2 += d.a2;
    }

    fn magnitude_db(&self, freq: f64, rate: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * freq / rate;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).max(1e-300).log10()
    }
}

/// RBJ Audio EQ Cookbook coefficients for `spec` at sample rate `rate`.
fn design(spec: &FilterSpec, rate: f64) -> Coef {
    let spec = spec.clamped();
    // Keep the centre below Nyquist whatever the output rate is.
    let freq = f64::from(spec.freq).min(rate * 0.45);
    let q = f64::from(spec.q);
    let w0 = 2.0 * std::f64::consts::PI * freq / rate;
    let (sin, cos) = (w0.sin(), w0.cos());
    let alpha = sin / (2.0 * q);
    let a = 10f64.powf(f64::from(spec.gain_db) / 40.0);
    let (b0, b1, b2, a0, a1, a2) = match spec.kind {
        FilterKind::Peaking => (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a),
        FilterKind::LowShelf => {
            let beta = 2.0 * a.sqrt() * alpha;
            (
                a * ((a + 1.0) - (a - 1.0) * cos + beta),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - beta),
                (a + 1.0) + (a - 1.0) * cos + beta,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - beta,
            )
        }
        FilterKind::HighShelf => {
            let beta = 2.0 * a.sqrt() * alpha;
            (
                a * ((a + 1.0) + (a - 1.0) * cos + beta),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - beta),
                (a + 1.0) - (a - 1.0) * cos + beta,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - beta,
            )
        }
        FilterKind::LowPass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        FilterKind::HighPass => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
    };
    Coef { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
}

// ------------------------------------------------------- UI to audio hand-over

/// Longest configuration as it travels through [`EqShared`]: no heap.
#[derive(Clone, Copy)]
struct Raw {
    enabled: bool,
    preamp_db: f32,
    count: usize,
    filters: [FilterSpec; MAX_FILTERS],
}

const HEADER_WORDS: usize = 3;
const FILTER_WORDS: usize = 4;
const WORDS: usize = HEADER_WORDS + MAX_FILTERS * FILTER_WORDS;

/// The configuration the UI wants, shared with the real-time callback.
///
/// A seqlock over atomic words: the writer makes the version odd, stores the
/// words, then makes it even again; the reader accepts a copy only if the
/// version was even and did not change. The reader never waits and never takes
/// a lock; the (rare) torn read is simply retried on the next callback.
pub struct EqShared {
    version: AtomicU32,
    words: [AtomicU32; WORDS],
    /// Writers only (the UI thread); the audio thread never touches it.
    writer: Mutex<()>,
}

impl Default for EqShared {
    fn default() -> Self {
        Self { version: AtomicU32::new(0), words: std::array::from_fn(|_| AtomicU32::new(0)), writer: Mutex::new(()) }
    }
}

impl EqShared {
    /// Publish a configuration (values are clamped). Called from the UI thread.
    pub fn write(&self, config: &EqConfig) {
        let config = config.clone().clamped();
        let _guard = self.writer.lock();
        let version = self.version.load(Ordering::Relaxed);
        self.version.store(version.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        let word = |i: usize, value: u32| self.words[i].store(value, Ordering::Relaxed);
        word(0, u32::from(config.enabled));
        word(1, config.preamp_db.to_bits());
        word(2, config.filters.len() as u32);
        for (i, f) in config.filters.iter().enumerate() {
            let base = HEADER_WORDS + i * FILTER_WORDS;
            word(base, u32::from(f.enabled) | (f.kind.code() << 1));
            word(base + 1, f.freq.to_bits());
            word(base + 2, f.gain_db.to_bits());
            word(base + 3, f.q.to_bits());
        }
        self.version.store(version.wrapping_add(2), Ordering::Release);
    }

    /// The configuration of the version just seen, or `None` if a write was in
    /// progress (keep what you have and try again).
    fn read(&self, version: u32) -> Option<Raw> {
        let word = |i: usize| self.words[i].load(Ordering::Relaxed);
        let mut raw = Raw {
            enabled: word(0) != 0,
            preamp_db: f32::from_bits(word(1)),
            count: (word(2) as usize).min(MAX_FILTERS),
            filters: [FilterSpec::default(); MAX_FILTERS],
        };
        for i in 0..raw.count {
            let base = HEADER_WORDS + i * FILTER_WORDS;
            let flags = word(base);
            raw.filters[i] = FilterSpec {
                enabled: flags & 1 != 0,
                kind: FilterKind::from_code(flags >> 1).unwrap_or(FilterKind::Peaking),
                freq: f32::from_bits(word(base + 1)),
                gain_db: f32::from_bits(word(base + 2)),
                q: f32::from_bits(word(base + 3)),
            };
        }
        fence(Ordering::Acquire);
        (self.version.load(Ordering::Relaxed) == version).then_some(raw)
    }

    fn version(&self) -> u32 {
        self.version.load(Ordering::Acquire)
    }
}

// ------------------------------------------------------------------ processor

/// Filter chain state of one output (create it with the output's real rate).
pub struct Dsp {
    rate: f64,
    ramp_frames: f64,
    /// Version of the configuration applied last.
    seen: u32,
    /// Sections in use now and in the target (the rest are the identity).
    live: usize,
    goal: usize,
    cur: [Coef; MAX_FILTERS],
    step: [Coef; MAX_FILTERS],
    /// Where the current slide ends (exact, so no rounding drift builds up).
    tgt: [Coef; MAX_FILTERS],
    pre_goal: f64,
    /// Left and right state of every section.
    z: [[[f64; 2]; 2]; MAX_FILTERS],
    coef_left: u32,
    pre: f64,
    pre_step: f64,
    /// 0 = untouched audio, 1 = fully processed.
    wet: f64,
    wet_goal: f64,
    wet_step: f64,
}

impl Dsp {
    pub fn new(rate: u32) -> Self {
        let rate = f64::from(rate.max(8000));
        Self {
            rate,
            ramp_frames: (rate * RAMP_SECS).max(64.0).round(),
            // Version 0 is the empty configuration every `EqShared` starts with.
            seen: 0,
            live: 0,
            goal: 0,
            cur: [IDENTITY; MAX_FILTERS],
            step: [IDENTITY; MAX_FILTERS],
            tgt: [IDENTITY; MAX_FILTERS],
            pre_goal: 1.0,
            z: [[[0.0; 2]; 2]; MAX_FILTERS],
            coef_left: 0,
            pre: 1.0,
            pre_step: 0.0,
            wet: 0.0,
            wet_goal: 0.0,
            wet_step: 0.0,
        }
    }

    /// Forget the filter memory (seek, new track): the old audio is gone.
    pub fn reset(&mut self) {
        self.z = [[[0.0; 2]; 2]; MAX_FILTERS];
    }

    /// Apply the newest configuration, if any, and filter `buf` (interleaved
    /// stereo) in place. With nothing to do this returns without touching it.
    pub fn process(&mut self, shared: &EqShared, buf: &mut [f32]) {
        let version = shared.version();
        if version != self.seen && version & 1 == 0 {
            if let Some(raw) = shared.read(version) {
                self.apply(&raw);
                self.seen = version;
            }
        }
        if self.wet == 0.0 && self.wet_goal == 0.0 {
            return;
        }
        for frame in buf.as_chunks_mut::<2>().0 {
            if self.wet != self.wet_goal {
                self.wet += self.wet_step;
                if (self.wet_step > 0.0 && self.wet >= self.wet_goal) || (self.wet_step < 0.0 && self.wet <= self.wet_goal) {
                    self.wet = self.wet_goal;
                    self.wet_step = 0.0;
                    if self.wet == 0.0 {
                        // Fully dry again: the exact path takes over for the rest.
                        return;
                    }
                }
            }
            if self.coef_left > 0 {
                self.coef_left -= 1;
                if self.coef_left == 0 {
                    self.cur[..self.live].copy_from_slice(&self.tgt[..self.live]);
                    self.pre = self.pre_goal;
                    self.live = self.goal;
                } else {
                    for k in 0..self.live {
                        let d = self.step[k];
                        self.cur[k].add(&d);
                    }
                    self.pre += self.pre_step;
                }
            }
            for (ch, sample) in frame.iter_mut().enumerate() {
                let dry = f64::from(*sample);
                let mut x = dry * self.pre;
                for k in 0..self.live {
                    let c = &self.cur[k];
                    let z = &mut self.z[k][ch];
                    let y = c.b0 * x + z[0];
                    z[0] = c.b1 * x - c.a1 * y + z[1];
                    z[1] = c.b2 * x - c.a2 * y;
                    x = y;
                }
                let wet = if self.wet >= 1.0 { x } else { dry + (x - dry) * self.wet };
                *sample = wet.clamp(-1.0, 1.0) as f32;
            }
        }
        // Long silences must not leave the state in the slow denormal range.
        for state in self.z.iter_mut().flatten().flatten() {
            if state.abs() < 1e-200 {
                *state = 0.0;
            }
        }
    }

    fn apply(&mut self, raw: &Raw) {
        let mut goal = [IDENTITY; MAX_FILTERS];
        let mut count = 0;
        if raw.enabled {
            for spec in raw.filters[..raw.count].iter().filter(|s| !s.is_neutral()) {
                goal[count] = design(spec, self.rate);
                count += 1;
            }
        }
        let preamp = if raw.enabled { raw.preamp_db } else { 0.0 };
        if count == 0 && preamp.abs() < NEUTRAL_DB {
            // Nothing to do: blend out to the untouched audio, keeping the old
            // filters running meanwhile.
            self.wet_goal = 0.0;
            self.wet_step = if self.wet > 0.0 { -1.0 / self.ramp_frames } else { 0.0 };
            return;
        }
        let pre = 10f64.powf(f64::from(preamp) / 20.0);
        if self.wet == 0.0 && self.wet_goal == 0.0 {
            // From untouched audio: start fresh and blend in.
            self.reset();
            self.cur = goal;
            self.tgt = goal;
            self.pre_goal = pre;
            self.live = count;
            self.goal = count;
            self.coef_left = 0;
            self.pre = pre;
            self.wet_step = 1.0 / self.ramp_frames;
        } else {
            // Already filtering: slide to the new coefficients (sections that
            // appear or vanish slide from or to the identity).
            let reach = self.live.max(count);
            for ((step, cur), target) in self.step.iter_mut().zip(&self.cur).zip(&goal) {
                *step = cur.step_to(target, self.ramp_frames);
            }
            self.tgt = goal;
            self.pre_goal = pre;
            self.live = reach;
            self.goal = count;
            self.coef_left = self.ramp_frames as u32;
            self.pre_step = (pre - self.pre) / self.ramp_frames;
            if self.wet != 1.0 {
                self.wet_step = (1.0 - self.wet) / self.ramp_frames;
            }
        }
        self.wet_goal = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn config(filters: Vec<FilterSpec>) -> EqConfig {
        EqConfig { enabled: true, preamp_db: 0.0, filters }
    }

    fn spec(kind: FilterKind, freq: f32, gain_db: f32, q: f32) -> FilterSpec {
        FilterSpec { enabled: true, kind, freq, gain_db, q }
    }

    /// Interleaved stereo sine, `secs` long.
    fn sine(freq: f64, amp: f32, secs: f64) -> Vec<f32> {
        let frames = (f64::from(RATE) * secs) as usize;
        (0..frames)
            .flat_map(|n| {
                let v = (amp as f64 * (2.0 * std::f64::consts::PI * freq * n as f64 / f64::from(RATE)).sin()) as f32;
                [v, v]
            })
            .collect()
    }

    /// Run `input` through the chain in callback-sized pieces.
    fn run(dsp: &mut Dsp, shared: &EqShared, input: &[f32]) -> Vec<f32> {
        let mut out = input.to_vec();
        for chunk in out.chunks_mut(960) {
            dsp.process(shared, chunk);
        }
        out
    }

    fn rms_left(samples: &[f32]) -> f64 {
        let n = samples.len() / 2;
        (samples.as_chunks::<2>().0.iter().map(|f| f64::from(f[0]).powi(2)).sum::<f64>() / n as f64).sqrt()
    }

    /// Gain in dB the chain gives a steady tone, measured on the second half.
    fn measured_db(cfg: &EqConfig, freq: f64) -> f64 {
        let shared = EqShared::default();
        shared.write(cfg);
        let mut dsp = Dsp::new(RATE);
        let input = sine(freq, 0.1, 1.0);
        let out = run(&mut dsp, &shared, &input);
        let half = input.len() / 2;
        20.0 * (rms_left(&out[half..]) / rms_left(&input[half..])).log10()
    }

    fn bits(samples: &[f32]) -> Vec<u32> {
        samples.iter().map(|s| s.to_bits()).collect()
    }

    fn noise(frames: usize) -> Vec<f32> {
        let mut state = 0x1234_5678_u32;
        (0..frames * 2)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) as f32 / (1u32 << 23) as f32 - 1.0
            })
            .collect()
    }

    #[test]
    fn low_pass_cuts_the_highs_and_keeps_the_lows() {
        let cfg = config(vec![spec(FilterKind::LowPass, 1000.0, 0.0, BUTTERWORTH_Q)]);
        let high = measured_db(&cfg, 10_000.0);
        assert!(high < -30.0, "10 kHz through a 1 kHz low pass: {high:.1} dB");
        let low = measured_db(&cfg, 100.0);
        assert!(low.abs() < 0.1, "100 Hz must pass: {low:.3} dB");
        let corner = measured_db(&cfg, 1000.0);
        assert!((corner + 3.01).abs() < 0.1, "-3 dB at the corner: {corner:.2}");
    }

    #[test]
    fn high_pass_cuts_the_lows_and_keeps_the_highs() {
        let cfg = config(vec![spec(FilterKind::HighPass, 1000.0, 0.0, BUTTERWORTH_Q)]);
        let low = measured_db(&cfg, 100.0);
        assert!(low < -30.0, "100 Hz through a 1 kHz high pass: {low:.1} dB");
        let high = measured_db(&cfg, 10_000.0);
        assert!(high.abs() < 0.1, "10 kHz must pass: {high:.3} dB");
    }

    #[test]
    fn peaking_changes_only_its_own_frequency() {
        let cfg = config(vec![spec(FilterKind::Peaking, 1000.0, 6.0, 2.0)]);
        let centre = measured_db(&cfg, 1000.0);
        assert!((centre - 6.0).abs() < 0.1, "+6 dB at the centre: {centre:.3}");
        for far in [50.0, 15_000.0] {
            let db = measured_db(&cfg, far);
            assert!(db.abs() < 0.3, "{far} Hz should be left alone: {db:.3} dB");
        }
        let cut = config(vec![spec(FilterKind::Peaking, 1000.0, -9.0, 1.0)]);
        assert!((measured_db(&cut, 1000.0) + 9.0).abs() < 0.1);
    }

    #[test]
    fn shelves_lift_one_end_and_leave_the_other() {
        let low = config(vec![spec(FilterKind::LowShelf, 300.0, 6.0, BUTTERWORTH_Q)]);
        assert!((measured_db(&low, 40.0) - 6.0).abs() < 0.3);
        assert!(measured_db(&low, 8000.0).abs() < 0.2);
        let high = config(vec![spec(FilterKind::HighShelf, 3000.0, -6.0, BUTTERWORTH_Q)]);
        assert!((measured_db(&high, 16_000.0) + 6.0).abs() < 0.3);
        assert!(measured_db(&high, 100.0).abs() < 0.2);
    }

    #[test]
    fn measured_response_matches_the_analytic_one() {
        let cfg = EqConfig {
            enabled: true,
            preamp_db: -2.0,
            filters: vec![
                spec(FilterKind::LowShelf, 120.0, 4.0, 0.8),
                spec(FilterKind::Peaking, 800.0, -5.0, 1.4),
                spec(FilterKind::Peaking, 4000.0, 7.0, 3.0),
                spec(FilterKind::HighPass, 30.0, 0.0, BUTTERWORTH_Q),
                spec(FilterKind::HighShelf, 9000.0, -3.0, 0.7),
            ],
        };
        for freq in [60.0, 250.0, 800.0, 2500.0, 4000.0, 7000.0, 12_000.0] {
            let want = cfg.response_db(freq, f64::from(RATE));
            let got = measured_db(&cfg, freq);
            assert!((want - got).abs() < 0.1, "{freq} Hz: analytic {want:.3} dB, measured {got:.3} dB");
        }
    }

    #[test]
    fn nothing_to_do_leaves_every_bit_untouched() {
        let input = noise(4800);
        let off = EqConfig { enabled: false, preamp_db: -6.0, filters: vec![spec(FilterKind::LowPass, 500.0, 0.0, 1.0)] };
        let empty = config(Vec::new());
        let neutral = config(vec![spec(FilterKind::Peaking, 1000.0, 0.0, 1.0), spec(FilterKind::HighShelf, 8000.0, 0.001, 1.0)]);
        let disabled = config(vec![FilterSpec { enabled: false, ..spec(FilterKind::LowPass, 500.0, 0.0, 1.0) }]);
        for (name, cfg) in [("off", off), ("empty", empty), ("neutral", neutral), ("disabled", disabled)] {
            assert!(cfg.is_bypass(), "{name}");
            let shared = EqShared::default();
            shared.write(&cfg);
            let mut dsp = Dsp::new(RATE);
            let out = run(&mut dsp, &shared, &input);
            assert_eq!(bits(&out), bits(&input), "{name} must be bit-perfect");
        }
        // Never configured at all.
        let mut dsp = Dsp::new(RATE);
        assert_eq!(bits(&run(&mut dsp, &EqShared::default(), &input)), bits(&input));
    }

    #[test]
    fn pre_amp_or_a_pass_filter_does_process() {
        let input = sine(1000.0, 0.1, 0.5);
        for cfg in [
            EqConfig { enabled: true, preamp_db: -6.0, filters: Vec::new() },
            config(vec![spec(FilterKind::HighPass, 20.0, 0.0, BUTTERWORTH_Q)]),
        ] {
            assert!(!cfg.is_bypass());
            let shared = EqShared::default();
            shared.write(&cfg);
            let mut dsp = Dsp::new(RATE);
            assert_ne!(bits(&run(&mut dsp, &shared, &input)), bits(&input));
        }
        let cfg = EqConfig { enabled: true, preamp_db: -6.0, filters: Vec::new() };
        assert!((measured_db(&cfg, 1000.0) + 6.0).abs() < 0.01);
    }

    #[test]
    fn going_back_to_flat_returns_to_the_exact_path() {
        let shared = EqShared::default();
        shared.write(&config(vec![spec(FilterKind::LowPass, 800.0, 0.0, BUTTERWORTH_Q)]));
        let mut dsp = Dsp::new(RATE);
        let tone = sine(2000.0, 0.2, 0.3);
        let processed = run(&mut dsp, &shared, &tone);
        assert_ne!(bits(&processed), bits(&tone));
        shared.write(&config(Vec::new()));
        // The blend out takes ~10 ms; after that every bit is the input again.
        let more = sine(2000.0, 0.2, 0.3);
        let out = run(&mut dsp, &shared, &more);
        let settled = (f64::from(RATE) * 0.02) as usize * 2;
        assert_eq!(bits(&out[settled..]), bits(&more[settled..]));
    }

    /// Largest jump between neighbouring left samples, after the first `skip`.
    fn max_step(samples: &[f32], skip: usize) -> f64 {
        samples
            .as_chunks::<2>()
            .0
            .iter()
            .skip(skip)
            .map(|f| f64::from(f[0]))
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn switching_and_changing_the_chain_makes_no_clicks() {
        let amp = 0.1f32;
        let loud = 0.4; // amplitude after +12 dB
        let bound = loud * 2.0 * std::f64::consts::PI * 1000.0 / f64::from(RATE) * 1.15;
        let input = sine(1000.0, amp, 1.2);
        let at = |secs: f64| (f64::from(RATE) * secs) as usize * 2;

        let shared = EqShared::default();
        let mut dsp = Dsp::new(RATE);
        let mut out = Vec::new();
        // Off, then +12 dB on, then -12 dB, then off again, switched mid-signal.
        let plan = [
            (0.0, config(Vec::new())),
            (0.3, config(vec![spec(FilterKind::Peaking, 1000.0, 12.0, 1.0)])),
            (0.6, config(vec![spec(FilterKind::Peaking, 1000.0, -12.0, 1.0)])),
            (0.9, config(Vec::new())),
        ];
        for (i, (start, cfg)) in plan.iter().enumerate() {
            shared.write(cfg);
            let end = plan.get(i + 1).map_or(input.len(), |p| at(p.0));
            out.extend(run(&mut dsp, &shared, &input[at(*start)..end]));
        }
        let step = max_step(&out, 0);
        assert!(step <= bound, "largest jump {step:.4} exceeds {bound:.4}");
    }

    #[test]
    fn reordering_and_removing_filters_slides_without_clicks() {
        let a = spec(FilterKind::Peaking, 500.0, 6.0, 1.0);
        let b = spec(FilterKind::HighShelf, 4000.0, -6.0, 0.7);
        let c = spec(FilterKind::LowPass, 12_000.0, 0.0, BUTTERWORTH_Q);
        let input = sine(700.0, 0.2, 1.0);
        let shared = EqShared::default();
        shared.write(&config(vec![a, b, c]));
        let mut dsp = Dsp::new(RATE);
        let mut out = run(&mut dsp, &shared, &input[..24_000]);
        shared.write(&config(vec![c, b, a])); // reordered
        out.extend(run(&mut dsp, &shared, &input[24_000..48_000]));
        shared.write(&config(vec![a])); // two removed
        out.extend(run(&mut dsp, &shared, &input[48_000..]));
        let bound = 0.4 * 2.0 * std::f64::consts::PI * 700.0 / f64::from(RATE);
        assert!(max_step(&out, 0) <= bound, "largest jump {:.4} vs {bound:.4}", max_step(&out, 0));
    }

    #[test]
    fn output_never_exceeds_full_scale() {
        let shared = EqShared::default();
        shared.write(&config(vec![spec(FilterKind::Peaking, 1000.0, 30.0, 1.0)]));
        let mut dsp = Dsp::new(RATE);
        let out = run(&mut dsp, &shared, &sine(1000.0, 0.9, 0.3));
        assert!(out.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn auto_pre_amp_keeps_the_peak_at_or_below_zero() {
        let cfg = config(vec![
            spec(FilterKind::Peaking, 1000.0, 9.0, 1.0),
            spec(FilterKind::LowShelf, 100.0, 5.0, 0.7),
            spec(FilterKind::Peaking, 3000.0, -4.0, 2.0),
        ]);
        let pre = cfg.auto_preamp(f64::from(RATE));
        assert!(pre < -8.0 && pre > -20.0, "{pre}");
        let with = EqConfig { preamp_db: pre, ..cfg.clone() };
        let peak =
            (0..=2000).map(|i| with.response_db(20.0 * 1000f64.powf(f64::from(i) / 2000.0), f64::from(RATE))).fold(f64::MIN, f64::max);
        assert!(peak <= 1e-6, "peak {peak}");
        // Cuts only: nothing to compensate.
        assert_eq!(config(vec![spec(FilterKind::Peaking, 1000.0, -6.0, 1.0)]).auto_preamp(f64::from(RATE)), 0.0);
    }

    #[test]
    fn configuration_is_clamped() {
        let cfg = EqConfig {
            enabled: true,
            preamp_db: f32::NAN,
            filters: (0..40).map(|_| FilterSpec { enabled: true, kind: FilterKind::Peaking, freq: 1e9, gain_db: 500.0, q: -3.0 }).collect(),
        }
        .clamped();
        assert_eq!(cfg.filters.len(), MAX_FILTERS);
        assert_eq!(cfg.preamp_db, 0.0);
        let f = cfg.filters[0];
        assert_eq!((f.freq, f.gain_db, f.q), (MAX_FREQ, MAX_GAIN_DB, MIN_Q));
        // A frequency above Nyquist at a low output rate still designs a stable filter.
        let coef = design(&spec(FilterKind::Peaking, 20_000.0, 6.0, 1.0), 22_050.0);
        assert!(coef.a2.abs() < 1.0 && coef.b0.is_finite());
    }

    #[test]
    fn shared_configuration_round_trips() {
        let cfg = EqConfig {
            enabled: true,
            preamp_db: -4.5,
            filters: vec![
                spec(FilterKind::LowShelf, 90.0, 3.5, 0.7),
                FilterSpec { enabled: false, ..spec(FilterKind::HighPass, 25.0, 0.0, 0.9) },
            ],
        };
        let shared = EqShared::default();
        shared.write(&cfg);
        let raw = shared.read(shared.version()).unwrap();
        assert_eq!((raw.enabled, raw.preamp_db, raw.count), (true, -4.5, 2));
        assert_eq!(raw.filters[0], cfg.filters[0]);
        assert_eq!(raw.filters[1], cfg.filters[1]);
        assert!(shared.read(shared.version().wrapping_sub(2)).is_none(), "a stale version is refused");
    }

    #[test]
    fn the_reader_never_sees_a_torn_configuration() {
        use std::sync::atomic::AtomicBool;
        let shared = EqShared::default();
        let stop = AtomicBool::new(false);
        std::thread::scope(|s| {
            s.spawn(|| {
                let mut n = 0u32;
                while !stop.load(Ordering::Relaxed) {
                    n += 1;
                    // Every field of a generation is tied to `n`.
                    let filters = (0..MAX_FILTERS)
                        .map(|i| spec(FilterKind::Peaking, 100.0 + (n % 1000) as f32 + i as f32 * 0.001, (n % 20) as f32, 1.0))
                        .collect();
                    shared.write(&EqConfig { enabled: true, preamp_db: -((n % 20) as f32), filters });
                }
            });
            let mut seen = 0;
            for _ in 0..200_000 {
                let v = shared.version();
                if v & 1 == 1 {
                    continue;
                }
                if let Some(raw) = shared.read(v) {
                    seen += 1;
                    let gain = -raw.preamp_db;
                    for (i, f) in raw.filters[..raw.count].iter().enumerate() {
                        assert_eq!(f.gain_db, gain, "gain of filter {i} belongs to another generation");
                        assert!((f.freq - (f.freq.floor() + i as f32 * 0.001)).abs() < 0.01, "frequency of filter {i} is torn");
                    }
                }
            }
            stop.store(true, Ordering::Relaxed);
            assert!(seen > 1000, "the reader should usually get a clean copy ({seen})");
        });
    }
}
