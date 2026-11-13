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

/// Which channels a filter works on (like `Channel: L R` in Equalizer APO).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channels {
    #[default]
    Both,
    Left,
    Right,
}

impl Channels {
    fn code(self) -> u32 {
        match self {
            Self::Both => 0,
            Self::Left => 1,
            Self::Right => 2,
        }
    }

    fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Left,
            2 => Self::Right,
            _ => Self::Both,
        }
    }

    /// `channel` is 0 for left, 1 for right.
    pub fn applies_to(self, channel: usize) -> bool {
        matches!((self, channel), (Self::Both, _) | (Self::Left, 0) | (Self::Right, 1))
    }

    /// Both, then Left, then Right, then Both again.
    pub fn next(self) -> Self {
        match self {
            Self::Both => Self::Left,
            Self::Left => Self::Right,
            Self::Right => Self::Both,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilterSpec {
    pub enabled: bool,
    pub kind: FilterKind,
    /// Channels the filter is applied to (files from before this existed mean both).
    #[serde(default)]
    pub channels: Channels,
    /// Hz.
    pub freq: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl FilterSpec {
    pub fn new(kind: FilterKind) -> Self {
        let q = if kind.has_gain() { 1.0 } else { BUTTERWORTH_Q };
        Self { enabled: true, kind, channels: Channels::Both, freq: 1000.0, gain_db: 0.0, q }
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

    /// The filters that work on `channel` (0 left, 1 right), as coefficients.
    fn designs(&self, channel: usize, rate: f64) -> Vec<Coef> {
        self.filters.iter().filter(|s| !s.is_neutral() && s.channels.applies_to(channel)).map(|s| design(s, rate)).collect()
    }

    /// Response of the chain of `channel` (0 left, 1 right), pre-amp included, in dB at `freq` Hz.
    #[cfg(test)]
    pub fn response_db(&self, channel: usize, freq: f64, rate: f64) -> f64 {
        f64::from(self.preamp_db) + chain_db(&self.designs(channel, rate), freq, rate)
    }

    /// The response (dB, pre-amp included) of `channel` at each of `freqs`: the
    /// filters are designed once, not once per point. For the response graph.
    pub fn response_curve(&self, channel: usize, rate: f64, freqs: &[f64]) -> Vec<f32> {
        let coefs = self.designs(channel, rate);
        let pre = f64::from(self.preamp_db);
        freqs.iter().map(|f| (pre + chain_db(&coefs, *f, rate)) as f32).collect()
    }

    /// The pre-amp (dB) that keeps the loudest point of either channel's
    /// response at 0 dB or below, rounded up to 0.1 dB. It looks from 1 Hz to
    /// just under Nyquist and refines every local maximum, so a narrow peak or a
    /// shelf below 20 Hz is not missed.
    pub fn auto_preamp(&self, rate: f64) -> f32 {
        let peak = (0..2).map(|ch| peak_db(&self.designs(ch, rate), rate)).fold(0.0, f64::max);
        // Round up to 0.1 dB, ignoring numerical dust (a peak of 12.0000000001 dB is 12.0).
        let pre = -(((peak - 1e-6).max(0.0) * 10.0).ceil() / 10.0) as f32;
        pre.clamp(MIN_PREAMP_DB, MAX_PREAMP_DB)
    }
}

/// Highest the response analysis looks, as a fraction of the sample rate.
const ANALYSIS_TOP: f64 = 0.499;

/// Response in dB of a chain of sections at one frequency.
fn chain_db(coefs: &[Coef], freq: f64, rate: f64) -> f64 {
    coefs.iter().map(|c| c.magnitude_db(freq, rate)).sum()
}

/// Highest point of the response of a chain (dB), never below 0 dB.
fn peak_db(coefs: &[Coef], rate: f64) -> f64 {
    if coefs.is_empty() {
        return 0.0;
    }
    const POINTS: usize = 1500;
    let (lo, hi) = (1.0f64, rate * ANALYSIS_TOP);
    let freq_at = |i: f64| lo * (hi / lo).powf(i / POINTS as f64);
    let db: Vec<f64> = (0..=POINTS).map(|i| chain_db(coefs, freq_at(i as f64), rate)).collect();
    let mut best = db.iter().copied().fold(0.0, f64::max);
    for i in 0..=POINTS {
        let left = if i > 0 { db[i - 1] } else { f64::MIN };
        let right = if i < POINTS { db[i + 1] } else { f64::MIN };
        if db[i] < left || db[i] < right || db[i] <= best - 1.0 {
            continue;
        }
        // A local maximum: golden-section search between its neighbours.
        let (mut a, mut b) = (freq_at((i as f64 - 1.0).max(0.0)), freq_at((i as f64 + 1.0).min(POINTS as f64)));
        let g = 0.618_033_988_749_895;
        for _ in 0..40 {
            let (x1, x2) = (b - g * (b - a), a + g * (b - a));
            if chain_db(coefs, x1, rate) < chain_db(coefs, x2, rate) {
                a = x1;
            } else {
                b = x2;
            }
        }
        best = best.max(chain_db(coefs, 0.5 * (a + b), rate));
    }
    best
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
            word(base, u32::from(f.enabled) | (f.kind.code() << 1) | (f.channels.code() << 4));
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
                kind: FilterKind::from_code((flags >> 1) & 7).unwrap_or(FilterKind::Peaking),
                channels: Channels::from_code((flags >> 4) & 3),
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

/// The sections of one channel.
struct Chain {
    /// Sections in use now and in the target (the rest are the identity).
    live: usize,
    goal: usize,
    cur: [Coef; MAX_FILTERS],
    step: [Coef; MAX_FILTERS],
    /// Where the current slide ends (exact, so no rounding drift builds up).
    tgt: [Coef; MAX_FILTERS],
    z: [[f64; 2]; MAX_FILTERS],
}

impl Chain {
    fn new() -> Self {
        Self {
            live: 0,
            goal: 0,
            cur: [IDENTITY; MAX_FILTERS],
            step: [IDENTITY; MAX_FILTERS],
            tgt: [IDENTITY; MAX_FILTERS],
            z: [[0.0; 2]; MAX_FILTERS],
        }
    }

    fn reset(&mut self) {
        self.z = [[0.0; 2]; MAX_FILTERS];
    }

    /// Start from untouched audio: nothing to slide from.
    fn start(&mut self, goal: &[Coef; MAX_FILTERS], count: usize) {
        self.reset();
        self.cur = *goal;
        self.tgt = *goal;
        self.live = count;
        self.goal = count;
    }

    /// Already filtering: sections that appear or vanish slide from or to the identity.
    fn slide(&mut self, goal: &[Coef; MAX_FILTERS], count: usize, frames: f64) {
        for ((step, cur), target) in self.step.iter_mut().zip(&self.cur).zip(goal) {
            *step = cur.step_to(target, frames);
        }
        self.tgt = *goal;
        self.live = self.live.max(count);
        self.goal = count;
    }

    fn advance(&mut self) {
        for k in 0..self.live {
            let d = self.step[k];
            self.cur[k].add(&d);
        }
    }

    fn finish(&mut self) {
        self.cur[..self.live].copy_from_slice(&self.tgt[..self.live]);
        self.live = self.goal;
    }

    #[inline]
    fn run(&mut self, mut x: f64) -> f64 {
        for k in 0..self.live {
            let c = &self.cur[k];
            let z = &mut self.z[k];
            let y = c.b0 * x + z[0];
            z[0] = c.b1 * x - c.a1 * y + z[1];
            z[1] = c.b2 * x - c.a2 * y;
            x = y;
        }
        x
    }
}

/// Filter chains of one output (create it with the output's real rate): one
/// per channel, so a filter can work on the left, the right or both.
pub struct Dsp {
    rate: f64,
    ramp_frames: f64,
    /// Version of the configuration applied last.
    seen: u32,
    chains: [Chain; 2],
    coef_left: u32,
    pre: f64,
    pre_goal: f64,
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
            chains: [Chain::new(), Chain::new()],
            coef_left: 0,
            pre: 1.0,
            pre_goal: 1.0,
            pre_step: 0.0,
            wet: 0.0,
            wet_goal: 0.0,
            wet_step: 0.0,
        }
    }

    /// Forget the filter memory (seek, new track): the old audio is gone.
    pub fn reset(&mut self) {
        for chain in &mut self.chains {
            chain.reset();
        }
    }

    /// The equaliser is changing the audio right now (or fading in or out).
    pub fn is_active(&self) -> bool {
        self.wet != 0.0 || self.wet_goal != 0.0
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
        if !self.is_active() {
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
                    for chain in &mut self.chains {
                        chain.finish();
                    }
                    self.pre = self.pre_goal;
                } else {
                    for chain in &mut self.chains {
                        chain.advance();
                    }
                    self.pre += self.pre_step;
                }
            }
            for (ch, sample) in frame.iter_mut().enumerate() {
                let dry = f64::from(*sample);
                // Pre-amp first, then the filters, all in f64; the only step down
                // to f32 is this one, at the very end. No clipping here: the
                // volume still comes after, and the device stage limits last.
                let x = self.chains[ch].run(dry * self.pre);
                let out = if self.wet >= 1.0 { x } else { dry + (x - dry) * self.wet };
                let out = out as f32;
                *sample = if out.is_finite() { out } else { 0.0 };
            }
        }
        // Long silences must not leave the state in the slow denormal range.
        for state in self.chains.iter_mut().flat_map(|c| c.z.iter_mut().flatten()) {
            if state.abs() < 1e-200 {
                *state = 0.0;
            }
        }
    }

    fn apply(&mut self, raw: &Raw) {
        let mut goals = [[IDENTITY; MAX_FILTERS]; 2];
        let mut counts = [0usize; 2];
        if raw.enabled {
            for spec in raw.filters[..raw.count].iter().filter(|s| !s.is_neutral()) {
                for ch in 0..2 {
                    if spec.channels.applies_to(ch) {
                        goals[ch][counts[ch]] = design(spec, self.rate);
                        counts[ch] += 1;
                    }
                }
            }
        }
        let preamp = if raw.enabled { raw.preamp_db } else { 0.0 };
        let pre_neutral = preamp.abs() < NEUTRAL_DB;
        if counts == [0, 0] && pre_neutral {
            // Nothing to do: blend out to the untouched audio, keeping the old
            // filters running meanwhile.
            self.wet_goal = 0.0;
            self.wet_step = if self.wet > 0.0 { -1.0 / self.ramp_frames } else { 0.0 };
            return;
        }
        // Exactly 1.0 without a pre-amp, so a channel with no filters is untouched bit for bit.
        let pre = if pre_neutral { 1.0 } else { 10f64.powf(f64::from(preamp) / 20.0) };
        if !self.is_active() {
            // From untouched audio: start fresh and blend in.
            for ch in 0..2 {
                self.chains[ch].start(&goals[ch], counts[ch]);
            }
            self.pre = pre;
            self.pre_goal = pre;
            self.coef_left = 0;
            self.wet_step = 1.0 / self.ramp_frames;
        } else {
            // Already filtering: slide to the new coefficients.
            for ch in 0..2 {
                self.chains[ch].slide(&goals[ch], counts[ch], self.ramp_frames);
            }
            self.pre_goal = pre;
            self.pre_step = (pre - self.pre) / self.ramp_frames;
            self.coef_left = self.ramp_frames as u32;
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
        FilterSpec { enabled: true, kind, channels: Channels::Both, freq, gain_db, q }
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
            let want = cfg.response_db(0, freq, f64::from(RATE));
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
    fn a_boost_is_not_clipped_inside_the_equaliser() {
        // The volume still comes after the equaliser, so a signal that goes over
        // full scale here must reach it intact (the device stage limits last).
        let shared = EqShared::default();
        shared.write(&config(vec![spec(FilterKind::Peaking, 1000.0, 12.0, 1.0)]));
        let mut dsp = Dsp::new(RATE);
        let out = run(&mut dsp, &shared, &sine(1000.0, 0.5, 0.5));
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 1.8 && peak < 2.1, "+12 dB on a 0.5 tone is about 2.0, not clipped at 1.0: {peak}");
    }

    #[test]
    fn the_auto_pre_amp_prevents_overs_for_a_full_scale_tone() {
        let cfg = config(vec![
            spec(FilterKind::Peaking, 1000.0, 9.0, 1.0),
            spec(FilterKind::LowShelf, 100.0, 5.0, 0.7),
            spec(FilterKind::HighShelf, 8000.0, 3.0, 0.7),
        ]);
        let cfg = EqConfig { preamp_db: cfg.auto_preamp(f64::from(RATE)), ..cfg };
        for freq in [60.0, 1000.0, 2000.0, 12_000.0] {
            let shared = EqShared::default();
            shared.write(&cfg);
            let mut dsp = Dsp::new(RATE);
            let input = sine(freq, 1.0, 0.6);
            let out = run(&mut dsp, &shared, &input);
            let peak = out[out.len() / 2..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak <= 1.0005, "{freq} Hz at 0 dBFS goes over full scale: {peak}");
        }
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
            (0..=2000).map(|i| with.response_db(0, 20.0 * 1000f64.powf(f64::from(i) / 2000.0), f64::from(RATE))).fold(f64::MIN, f64::max);
        assert!(peak <= 1e-6, "peak {peak}");
        // Cuts only: nothing to compensate.
        assert_eq!(config(vec![spec(FilterKind::Peaking, 1000.0, -6.0, 1.0)]).auto_preamp(f64::from(RATE)), 0.0);
    }

    #[test]
    fn configuration_is_clamped() {
        let cfg = EqConfig {
            enabled: true,
            preamp_db: f32::NAN,
            filters: (0..40)
                .map(|_| FilterSpec {
                    enabled: true,
                    kind: FilterKind::Peaking,
                    channels: Channels::Both,
                    freq: 1e9,
                    gain_db: 500.0,
                    q: -3.0,
                })
                .collect(),
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

    // ------------------------------------------------ reference and quality

    type Golden = (FilterKind, f32, f32, f32, u32, [f64; 5], [f64; 10]);

    /// Coefficients and the first ten samples of the impulse response, computed
    /// with a separate implementation (plain Python, doubles, direct form I).
    const GOLDEN: [Golden; 6] = [
        (
            FilterKind::Peaking,
            1000.0,
            6.0,
            1.0,
            48000,
            [1.043953086990335, -1.8953207239365961, 0.8677222847598566, -1.8953207239365961, 0.9116753717501915],
            [
                1.043953086990335,
                0.08330519665377012,
                0.0738660317176868,
                0.06405252458144103,
                0.05405823533372989,
                0.04406258456813627,
                0.034229167888159995,
                0.024704478095104808,
                0.015617019948551848,
                0.007076797303372413,
            ],
        ),
        (
            FilterKind::Peaking,
            100.0,
            -9.0,
            2.5,
            48000,
            [0.9971768299395882, -1.9910779211883296, 0.994071686339886, -1.9910779211883296, 0.9912485162794742],
            [
                0.9971768299395882,
                -0.005621151575045724,
                -0.005570517598726954,
                -0.005519376441870165,
                -0.005467741267487895,
                -0.005415625207673756,
                -0.0053630413616327505,
                -0.005310002793734435,
                -0.0052565225315890675,
                -0.005202613564146855,
            ],
        ),
        (
            FilterKind::LowShelf,
            120.0,
            4.0,
            0.8,
            48000,
            [1.002274250082894, -1.9824014979832798, 0.9804351785825827, -1.9824583177616169, 0.9826526088871401],
            [
                1.002274250082894,
                0.004565425771840381,
                0.004598538213733017,
                0.004630182785980132,
                0.004660378804031011,
                0.004689145529703511,
                0.004716502168372025,
                0.004742467866215196,
                0.004767061707522878,
                0.004790302712061835,
            ],
        ),
        (
            FilterKind::HighShelf,
            8000.0,
            -5.0,
            0.7,
            44100,
            [0.6972784197758476, -0.24218808505735906, 0.13129038941057372, -0.6588621070309384, 0.2452428311600009],
            [
                0.6972784197758476,
                0.21722224378335903,
                0.10340736087106539,
                0.014858993609659893,
                -0.015569886104784205,
                -0.013902469626251204,
                -0.005341407481709455,
                -0.00010976997665214502,
                0.0012376186150878682,
                0.000842340308288044,
            ],
        ),
        (
            FilterKind::LowPass,
            2000.0,
            0.0,
            BUTTERWORTH_Q,
            48000,
            [0.01440144034651121, 0.02880288069302242, 0.01440144034651121, -1.6329931618554523, 0.6905989232414971],
            [
                0.01440144034651121,
                0.05232033429974444,
                0.08989456928755786,
                0.11066485040348445,
                0.11863385121140108,
                0.1173032412634674,
                0.10962698094013705,
                0.09801061812081367,
                0.08434239418504004,
                0.07004452561821996,
            ],
        ),
        (
            FilterKind::HighPass,
            80.0,
            0.0,
            BUTTERWORTH_Q,
            96000,
            [0.9963044429693492, -1.9926088859386983, 0.9963044429693492, -1.992595228750302, 0.992622543127095],
            [
                0.9963044429693492,
                -0.007377406495245875,
                -0.00734999192268837,
                -0.0073225788395087955,
                -0.007295167983490612,
                -0.007267760086913482,
                -0.007240355876573873,
                -0.007212956073805651,
                -0.0071855613945006865,
                -0.007158172549129443,
            ],
        ),
    ];

    #[test]
    fn coefficients_and_impulse_responses_match_an_independent_implementation() {
        for (kind, freq, gain, q, rate, want, impulse) in GOLDEN {
            let coef = design(&spec(kind, freq, gain, q), f64::from(rate));
            let got = [coef.b0, coef.b1, coef.b2, coef.a1, coef.a2];
            for (g, w) in got.iter().zip(want) {
                // Q and gain are stored as f32, hence a few parts per million at most.
                assert!((g - w).abs() < 2e-6, "{kind:?} {freq} Hz: coefficient {g} vs {w}");
            }
            let mut chain = Chain::new();
            let mut goal = [IDENTITY; MAX_FILTERS];
            goal[0] = coef;
            chain.start(&goal, 1);
            for (n, want) in impulse.iter().enumerate() {
                let y = chain.run(if n == 0 { 1.0 } else { 0.0 });
                assert!((y - want).abs() < 2e-6, "{kind:?} {freq} Hz: impulse response sample {n}: {y} vs {want}");
            }
        }
    }

    #[test]
    fn the_definitions_have_the_properties_they_promise() {
        let rate = f64::from(RATE);
        let at = |f: FilterSpec, hz: f64| design(&f, rate).magnitude_db(hz, rate);
        // A peaking filter has exactly its gain at the centre; a shelf has half of it at its corner.
        assert!((at(spec(FilterKind::Peaking, 2500.0, 7.5, 3.0), 2500.0) - 7.5).abs() < 1e-3);
        assert!((at(spec(FilterKind::LowShelf, 300.0, 8.0, BUTTERWORTH_Q), 300.0) - 4.0).abs() < 0.01);
        assert!((at(spec(FilterKind::HighShelf, 6000.0, -10.0, BUTTERWORTH_Q), 6000.0) + 5.0).abs() < 0.01);
        // Butterworth pass filters are 3 dB down at the corner.
        assert!((at(spec(FilterKind::LowPass, 1500.0, 0.0, BUTTERWORTH_Q), 1500.0) + 3.0103).abs() < 0.01);
        assert!((at(spec(FilterKind::HighPass, 1500.0, 0.0, BUTTERWORTH_Q), 1500.0) + 3.0103).abs() < 0.01);
        // Shelves reach the full gain at the far end and nothing at the other.
        assert!((at(spec(FilterKind::LowShelf, 300.0, 8.0, BUTTERWORTH_Q), 5.0) - 8.0).abs() < 0.05);
        assert!(at(spec(FilterKind::LowShelf, 300.0, 8.0, BUTTERWORTH_Q), 20_000.0).abs() < 0.05);
    }

    #[test]
    fn the_chain_adds_next_to_no_noise_or_distortion() {
        // A tone through boosts and a cut, with the auto pre-amp. Whatever is not the
        // fundamental (scaled and rotated by the filter) is the error.
        let cfg = config(vec![
            spec(FilterKind::Peaking, 1000.0, 6.0, 1.0),
            spec(FilterKind::LowShelf, 100.0, 3.0, 0.7),
            spec(FilterKind::HighShelf, 8000.0, -4.0, 0.7),
        ]);
        let cfg = EqConfig { preamp_db: cfg.auto_preamp(f64::from(RATE)), ..cfg };
        let shared = EqShared::default();
        shared.write(&cfg);
        let mut dsp = Dsp::new(RATE);
        let input = sine(1000.0, 0.7, 1.0);
        let out = run(&mut dsp, &shared, &input);
        // Whole cycles (500 of them) from the end, where the filter has settled.
        let frames = 24_000;
        let tail = &out[out.len() - frames * 2..];
        let w = 2.0 * std::f64::consts::PI * 1000.0 / f64::from(RATE);
        let (mut sc, mut ss, mut energy) = (0.0f64, 0.0f64, 0.0f64);
        for (n, f) in tail.as_chunks::<2>().0.iter().enumerate() {
            let x = f64::from(f[0]);
            sc += x * (w * n as f64).cos();
            ss += x * (w * n as f64).sin();
            energy += x * x;
        }
        let (a, b) = (2.0 * sc / frames as f64, 2.0 * ss / frames as f64);
        let fundamental = 0.5 * (a * a + b * b);
        let noise = (energy / frames as f64 - fundamental).max(1e-30);
        let ratio_db = 10.0 * (noise / fundamental).log10();
        assert!(ratio_db < -120.0, "noise and distortion {ratio_db:.1} dB below the tone");
    }

    // ------------------------------------------------ channels

    /// Different content in each channel.
    fn two_tones(secs: f64) -> Vec<f32> {
        let left = sine(1000.0, 0.3, secs);
        let right = sine(1700.0, 0.2, secs);
        left.as_chunks::<2>().0.iter().zip(right.as_chunks::<2>().0).flat_map(|(l, r)| [l[0], r[0]]).collect()
    }

    fn channel(samples: &[f32], ch: usize) -> Vec<f32> {
        samples.as_chunks::<2>().0.iter().map(|f| f[ch]).collect()
    }

    fn on(mut f: FilterSpec, channels: Channels) -> FilterSpec {
        f.channels = channels;
        f
    }

    fn rms_of(samples: &[f32]) -> f64 {
        (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
    }

    #[test]
    fn a_filter_works_only_on_its_channel_and_the_other_stays_bit_exact() {
        let input = two_tones(0.5);
        for (channels, filtered, untouched) in [(Channels::Left, 0, 1), (Channels::Right, 1, 0)] {
            let shared = EqShared::default();
            shared.write(&config(vec![on(spec(FilterKind::LowPass, 300.0, 0.0, BUTTERWORTH_Q), channels)]));
            let mut dsp = Dsp::new(RATE);
            let out = run(&mut dsp, &shared, &input);
            assert!(dsp.is_active());
            // The equaliser is processing, yet the channel it does not touch is identical bit for bit.
            assert_eq!(bits(&channel(&out, untouched)), bits(&channel(&input, untouched)), "{channels:?}: the other channel changed");
            let half = out.len() / 4;
            assert!(
                rms_of(&channel(&out, filtered)[half..]) < 0.1 * rms_of(&channel(&input, filtered)[half..]),
                "{channels:?}: not filtered"
            );
        }
    }

    #[test]
    fn both_channels_get_the_same_chain() {
        let tone = sine(1000.0, 0.2, 0.5);
        let cfg = config(vec![spec(FilterKind::Peaking, 1000.0, 6.0, 1.0), spec(FilterKind::HighPass, 60.0, 0.0, BUTTERWORTH_Q)]);
        let shared = EqShared::default();
        shared.write(&cfg);
        let mut dsp = Dsp::new(RATE);
        let out = run(&mut dsp, &shared, &tone);
        assert_eq!(bits(&channel(&out, 0)), bits(&channel(&out, 1)), "identical channels in, identical channels out");
        assert!(measured_db(&cfg, 1000.0) > 5.9);
    }

    #[test]
    fn the_pre_amp_applies_to_both_channels_even_when_filters_do_not() {
        let input = two_tones(0.3);
        let cfg =
            EqConfig { enabled: true, preamp_db: -6.0, filters: vec![on(spec(FilterKind::Peaking, 1000.0, 6.0, 1.0), Channels::Left)] };
        let shared = EqShared::default();
        shared.write(&cfg);
        let mut dsp = Dsp::new(RATE);
        let out = run(&mut dsp, &shared, &input);
        let half = out.len() / 4;
        let ratio = rms_of(&channel(&out, 1)[half..]) / rms_of(&channel(&input, 1)[half..]);
        assert!((20.0 * ratio.log10() + 6.0).abs() < 0.01, "the right channel should only get the pre-amp");
    }

    #[test]
    fn moving_a_filter_to_another_channel_makes_no_clicks() {
        let input = sine(700.0, 0.2, 1.0);
        let a = spec(FilterKind::Peaking, 700.0, 12.0, 1.0);
        let shared = EqShared::default();
        let mut dsp = Dsp::new(RATE);
        let mut out = Vec::new();
        for (i, channels) in [Channels::Both, Channels::Left, Channels::Right, Channels::Both].into_iter().enumerate() {
            shared.write(&config(vec![on(a, channels)]));
            out.extend(run(&mut dsp, &shared, &input[i * 24_000..(i + 1) * 24_000]));
        }
        // The tone reaches 0.8 at most (0.2 plus 12 dB).
        let bound = 0.8 * 2.0 * std::f64::consts::PI * 700.0 / f64::from(RATE) * 1.15;
        for ch in 0..2 {
            let one: Vec<f32> = channel(&out, ch).into_iter().flat_map(|v| [v, v]).collect();
            assert!(max_step(&one, 0) <= bound, "channel {ch} jumps {:.4} vs {bound:.4}", max_step(&one, 0));
        }
    }

    #[test]
    fn auto_pre_amp_looks_at_both_channels_and_below_20_hz() {
        let rate = f64::from(RATE);
        let left_only = config(vec![on(spec(FilterKind::Peaking, 1000.0, 9.0, 1.0), Channels::Left)]);
        assert!(left_only.auto_preamp(rate) <= -9.0);
        let right_only = config(vec![on(spec(FilterKind::Peaking, 1000.0, 9.0, 1.0), Channels::Right)]);
        assert!(right_only.auto_preamp(rate) <= -9.0);
        // A shelf whose top is below the old 20 Hz limit of the analysis.
        let subsonic = config(vec![spec(FilterKind::LowShelf, 10.0, 6.0, BUTTERWORTH_Q)]);
        assert!(subsonic.auto_preamp(rate) <= -5.9, "{}", subsonic.auto_preamp(rate));
        // A very narrow peak is still found: compare with a dense scan.
        let narrow = config(vec![spec(FilterKind::Peaking, 3333.0, 12.0, 20.0)]);
        let dense = (0..200_000).map(|i| narrow.response_db(0, 3000.0 + f64::from(i) * 0.003, rate)).fold(f64::MIN, f64::max);
        assert!((f64::from(-narrow.auto_preamp(rate)) - dense).abs() <= 0.1, "auto {} vs scan {dense}", narrow.auto_preamp(rate));
    }

    #[test]
    fn filters_written_before_channels_existed_mean_both() {
        let old: FilterSpec = serde_json::from_str(r#"{"enabled":true,"kind":"peaking","freq":1000.0,"gain_db":3.0,"q":1.0}"#).unwrap();
        assert_eq!(old.channels, Channels::Both);
        let new: FilterSpec =
            serde_json::from_str(r#"{"enabled":true,"kind":"low_pass","channels":"right","freq":900.0,"gain_db":0.0,"q":0.7}"#).unwrap();
        assert_eq!(new.channels, Channels::Right);
    }

    #[test]
    fn the_seqlock_carries_the_channel() {
        let cfg = config(vec![
            on(spec(FilterKind::Peaking, 500.0, 3.0, 1.0), Channels::Left),
            on(spec(FilterKind::HighPass, 40.0, 0.0, 0.7), Channels::Right),
            spec(FilterKind::LowShelf, 90.0, 2.0, 0.7),
        ]);
        let shared = EqShared::default();
        shared.write(&cfg);
        let raw = shared.read(shared.version()).unwrap();
        let channels: Vec<Channels> = raw.filters[..raw.count].iter().map(|f| f.channels).collect();
        assert_eq!(channels, [Channels::Left, Channels::Right, Channels::Both]);
        assert_eq!(raw.filters[1].kind, FilterKind::HighPass);
    }
}
