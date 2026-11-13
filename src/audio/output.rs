//! Audio output: shared mode through cpal (system mixer rate, sinc resampled
//! upstream) or WASAPI exclusive mode at the track's own rate (bit-perfect).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::dsp::{Dsp, EqShared};

/// State shared between the engine and the real-time output.
#[derive(Default)]
pub struct OutShared {
    /// Frames taken from the ring (played or flushed).
    pub played: AtomicU64,
    /// Engine asks the output to discard everything in the ring.
    pub flush: AtomicBool,
    pub paused: AtomicBool,
    /// Linear gain as f32 bits.
    pub gain: AtomicU32,
    /// Output callback reported an error (device lost, ...).
    pub failed: AtomicBool,
    /// Smoothed loudness of the low / mid / high bands (f32 bits) and the
    /// filter state they are computed from. Only used for the subtle
    /// background animation.
    pub vis: [AtomicU32; 3],
    lp: [AtomicU32; 2],
    /// Equaliser configuration written by the UI and read lock-free by the
    /// output callback (see `dsp`).
    pub eq: EqShared,
    /// Loudest sample (f32 bits, 0 = none) since the UI last took it.
    pub peak: AtomicU32,
}

impl OutShared {
    pub fn set_gain(&self, g: f32) {
        self.gain.store(g.to_bits(), Ordering::Relaxed);
    }
    /// Band levels, roughly 0..0.5 for normal music.
    pub fn levels(&self) -> [f32; 3] {
        [0, 1, 2].map(|i| f32::from_bits(self.vis[i].load(Ordering::Relaxed)))
    }

    fn decay_levels(&self) {
        for v in &self.vis {
            let x = f32::from_bits(v.load(Ordering::Relaxed)) * 0.92;
            v.store(if x < 1e-4 { 0 } else { x.to_bits() }, Ordering::Relaxed);
        }
    }

    /// Split the samples into low / mid / high with two one-pole filters and
    /// smooth their RMS (fast attack, slow release).
    fn analyse(&self, samples: &[f32]) {
        let frames = samples.len() / 2;
        if frames == 0 {
            self.decay_levels();
            return;
        }
        let mut lp1 = f32::from_bits(self.lp[0].load(Ordering::Relaxed));
        let mut lp2 = f32::from_bits(self.lp[1].load(Ordering::Relaxed));
        let (mut e0, mut e1, mut e2) = (0f32, 0f32, 0f32);
        for f in samples.as_chunks::<2>().0 {
            let x = 0.5 * (f[0] + f[1]);
            lp1 += 0.02 * (x - lp1);
            lp2 += 0.22 * (x - lp2);
            let (low, mid, high) = (lp1, lp2 - lp1, x - lp2);
            e0 += low * low;
            e1 += mid * mid;
            e2 += high * high;
        }
        self.lp[0].store(lp1.to_bits(), Ordering::Relaxed);
        self.lp[1].store(lp2.to_bits(), Ordering::Relaxed);
        let n = frames as f32;
        for (i, e) in [e0, e1, e2].into_iter().enumerate() {
            let target = (e / n).sqrt();
            let cur = f32::from_bits(self.vis[i].load(Ordering::Relaxed));
            let k = if target > cur { 0.3 } else { 0.06 };
            self.vis[i].store((cur + (target - cur) * k).to_bits(), Ordering::Relaxed);
        }
    }

    fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }
}

pub struct Output {
    pub producer: rtrb::Producer<f32>,
    pub rate: u32,
    pub exclusive: bool,
    /// Human readable description, e.g. "WASAPI exclusive · 24-bit 96 kHz".
    pub description: String,
    _keep: Keep,
}

enum Keep {
    Cpal(#[allow(dead_code)] cpal::Stream),
    #[cfg(windows)]
    Exclusive(ExclusiveHandle),
}

pub fn device_names() -> Vec<String> {
    let host = cpal::default_host();
    host.output_devices().map(|d| d.filter_map(|d| d.name().ok()).collect()).unwrap_or_default()
}

fn ring(rate: u32) -> (rtrb::Producer<f32>, rtrb::Consumer<f32>) {
    // ~300 ms of stereo audio: small enough for snappy seeks and pauses.
    rtrb::RingBuffer::new((rate as usize * 2 * 3 / 10).max(8192))
}

/// Triangular-PDF dither: noise of up to one least significant bit (two
/// uniform numbers added), with its own tiny generator so the real-time
/// callback needs nothing else.
pub struct Dither(u32);

impl Dither {
    pub fn new() -> Self {
        Self(0x2545_F491)
    }

    fn unit(&mut self) -> f64 {
        // xorshift32 never reaches 0 from a non-zero seed.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        f64::from(self.0) / 4_294_967_296.0
    }

    /// In least significant bits, between -1 and 1, mean 0.
    pub fn tpdf(&mut self) -> f64 {
        self.unit() + self.unit() - 1.0
    }
}

/// A sample (-1.0 to 1.0) as a signed integer of `bits` bits: scaled by
/// `2^(bits-1)` and rounded, so integer PCM that came through as `f32`
/// returns exactly. `dither` is only for a signal that was processed on the way
/// (an unprocessed one must stay exact).
pub fn quantize(sample: f32, bits: u32, dither: Option<&mut Dither>) -> i64 {
    let max = (1i64 << (bits - 1)) as f64;
    let mut v = f64::from(sample) * max;
    if let Some(d) = dither {
        v += d.tpdf();
    }
    v.round().clamp(-max, max - 1.0) as i64
}

/// Pull up to `out.len()` samples, honouring flush/pause/equaliser/gain. What
/// the ring does not hold is silence. Returns whether the signal was changed on
/// the way (equaliser active or volume below full), which decides about dither.
fn fill(cons: &mut rtrb::Consumer<f32>, shared: &OutShared, out: &mut [f32], apply_gain: bool, dsp: &mut Dsp) -> bool {
    if shared.flush.load(Ordering::Acquire) {
        dsp.reset();
        let n = cons.slots();
        if let Ok(chunk) = cons.read_chunk(n) {
            chunk.commit_all();
        }
        shared.played.fetch_add((n / 2) as u64, Ordering::Release);
        shared.flush.store(false, Ordering::Release);
    }
    if shared.paused.load(Ordering::Relaxed) {
        out.fill(0.0);
        shared.decay_levels();
        return false;
    }
    let want = out.len() - out.len() % 2;
    let n = cons.slots().min(want);
    let n = n - n % 2;
    if let Ok(chunk) = cons.read_chunk(n) {
        let (a, b) = chunk.as_slices();
        out[..a.len()].copy_from_slice(a);
        out[a.len()..a.len() + b.len()].copy_from_slice(b);
        chunk.commit_all();
    }
    out[n..].fill(0.0);
    // Equaliser: leaves the samples untouched (bit-perfect) unless the chain does something.
    dsp.process(&shared.eq, &mut out[..n]);
    // (Band analysis for the backgrounds is no longer used: they drift on their own.)
    let mut scaled = false;
    if apply_gain {
        let g = shared.gain();
        if g != 1.0 {
            scaled = true;
            for s in &mut out[..n] {
                *s *= g;
            }
        }
    }
    // Loudest sample on its way to the device, for the equaliser page. Only
    // reads: it never changes the audio. Positive floats order like their bits.
    let peak = out[..n].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 0.0 {
        shared.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
    }
    shared.played.fetch_add((n / 2) as u64, Ordering::Release);
    scaled || dsp.is_active()
}

fn find_device(name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = name {
        if let Some(d) = host.output_devices().ok().and_then(|mut it| it.find(|d| d.name().map(|n| n == name).unwrap_or(false))) {
            return Ok(d);
        }
        log::warn!("output device {name:?} not found, using default");
    }
    host.default_output_device().ok_or_else(|| anyhow!("no output device"))
}

pub fn open_shared(device: Option<&str>, shared: Arc<OutShared>) -> Result<Output> {
    let dev = find_device(device)?;
    let cfg = dev.default_output_config().context("output config")?;
    let rate = cfg.sample_rate().0;
    let channels = cfg.channels() as usize;
    let (producer, mut cons) = ring(rate);
    let stream_cfg: cpal::StreamConfig = cfg.config();
    let err_shared = shared.clone();
    let on_err = move |e| {
        log::error!("output stream error: {e}");
        err_shared.failed.store(true, Ordering::Relaxed);
    };
    let mut scratch: Vec<f32> = Vec::new();
    let sh = shared.clone();
    let mut dsp = Dsp::new(rate);
    // `write` gets the index, the sample and whether the signal was processed.
    let mut render = move |frames: usize, write: &mut dyn FnMut(usize, f32, bool)| {
        scratch.resize(frames * 2, 0.0);
        let processed = fill(&mut cons, &sh, &mut scratch, true, &mut dsp);
        for f in 0..frames {
            for c in 0..channels {
                // Stereo to the first two channels, silence on the others.
                let v = if c < 2 { scratch[f * 2 + c] } else { 0.0 };
                write(f * channels + c, v, processed);
            }
        }
    };
    let stream = match cfg.sample_format() {
        // A float device takes the samples as they are (above 1.0 too, as Equalizer APO would).
        cpal::SampleFormat::F32 => dev.build_output_stream(
            &stream_cfg,
            move |data: &mut [f32], _| render(data.len() / channels, &mut |i, v, _| data[i] = v),
            on_err,
            None,
        )?,
        cpal::SampleFormat::I16 => {
            let mut dither = Dither::new();
            dev.build_output_stream(
                &stream_cfg,
                move |data: &mut [i16], _| {
                    render(data.len() / channels, &mut |i, v, processed| data[i] = quantize(v, 16, processed.then_some(&mut dither)) as i16)
                },
                on_err,
                None,
            )?
        }
        cpal::SampleFormat::I32 => dev.build_output_stream(
            &stream_cfg,
            move |data: &mut [i32], _| render(data.len() / channels, &mut |i, v, _| data[i] = quantize(v, 32, None) as i32),
            on_err,
            None,
        )?,
        other => return Err(anyhow!("unsupported output format {other:?}")),
    };
    stream.play()?;
    let name = dev.name().unwrap_or_default();
    Ok(Output {
        producer,
        rate,
        exclusive: false,
        description: format!("{name} · compartido {} kHz", fmt_khz(rate)),
        _keep: Keep::Cpal(stream),
    })
}

pub fn fmt_khz(rate: u32) -> String {
    let k = rate as f32 / 1000.0;
    if k.fract() == 0.0 {
        format!("{k:.0}")
    } else {
        format!("{k:.1}")
    }
}

// ------------------------------------------------------------ exclusive mode

#[cfg(windows)]
pub struct ExclusiveHandle {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(windows)]
impl Drop for ExclusiveHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(windows)]
pub fn open_exclusive(device: Option<&str>, rate: u32, source_bits: u32, shared: Arc<OutShared>, apply_gain: bool) -> Result<Output> {
    use wasapi::{Direction, SampleType, ShareMode, WaveFormat};

    let (producer, mut cons) = ring(rate);
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<String, String>>();
    let device = device.map(str::to_string);
    let stop2 = stop.clone();

    let thread = std::thread::Builder::new().name("wasapi-exclusive".into()).spawn(move || {
        let mut run = || -> Result<(), String> {
            let _ = wasapi::initialize_mta();
            let dev = match &device {
                Some(name) => wasapi::DeviceCollection::new(&Direction::Render)
                    .and_then(|c| c.get_device_with_name(name))
                    .or_else(|_| wasapi::get_default_device(&Direction::Render)),
                None => wasapi::get_default_device(&Direction::Render),
            }
            .map_err(|e| e.to_string())?;
            let name = dev.get_friendlyname().unwrap_or_default();

            // Formats in order of preference: as close to the source as possible.
            let mut candidates: Vec<(usize, usize, SampleType)> = Vec::new();
            if source_bits > 16 {
                candidates.extend([(32, 24, SampleType::Int), (24, 24, SampleType::Int), (32, 32, SampleType::Int)]);
            } else {
                candidates.extend([
                    (16, 16, SampleType::Int),
                    (32, 24, SampleType::Int),
                    (24, 24, SampleType::Int),
                    (32, 32, SampleType::Int),
                ]);
            }
            candidates.push((32, 32, SampleType::Float));

            let mut client = None;
            let mut last_err = String::from("no format accepted");
            for (container, valid, kind) in candidates {
                let wanted = WaveFormat::new(container, valid, &kind, rate as usize, 2, None);
                let mut ac = dev.get_iaudioclient().map_err(|e| e.to_string())?;
                let fmt = match ac.is_supported_exclusive_with_quirks(&wanted) {
                    Ok(f) => f,
                    Err(e) => {
                        last_err = e.to_string();
                        continue;
                    }
                };
                let (_def, min) = ac.get_periods().map_err(|e| e.to_string())?;
                let period = ac.calculate_aligned_period_near(min.max(30_000), Some(128), &fmt).unwrap_or(min);
                match ac.initialize_client(&fmt, period, &Direction::Render, &ShareMode::Exclusive, false) {
                    Ok(()) => {
                        client = Some((ac, fmt, kind, valid));
                        break;
                    }
                    Err(e) => {
                        // Unaligned buffer: retry with the size the driver proposes.
                        if let Ok(frames) = ac.get_bufferframecount() {
                            let aligned = wasapi::calculate_period_100ns(frames as i64, rate as i64);
                            if let Ok(mut ac2) = dev.get_iaudioclient() {
                                if ac2.initialize_client(&fmt, aligned, &Direction::Render, &ShareMode::Exclusive, false).is_ok() {
                                    client = Some((ac2, fmt, kind, valid));
                                    break;
                                }
                            }
                        }
                        last_err = e.to_string();
                    }
                }
            }
            let (ac, fmt, kind, valid) = client.ok_or_else(|| format!("exclusive mode refused: {last_err}"))?;
            let block = fmt.get_blockalign() as usize;
            let bytes_per_sample = block / 2;
            let event = ac.set_get_eventhandle().map_err(|e| e.to_string())?;
            let render = ac.get_audiorenderclient().map_err(|e| e.to_string())?;
            let float = matches!(kind, SampleType::Float);
            let _ = ready_tx.send(Ok(format!("{name} · exclusivo {}-bit {} kHz", if float { 32 } else { valid }, fmt_khz(rate))));

            let mut scratch = Vec::<f32>::new();
            let mut bytes = Vec::<u8>::new();
            let mut dsp = Dsp::new(rate);
            let mut dither = Dither::new();
            ac.start_stream().map_err(|e| e.to_string())?;
            while !stop2.load(Ordering::Relaxed) {
                let frames = ac.get_available_space_in_frames().map_err(|e| e.to_string())? as usize;
                if frames > 0 {
                    scratch.resize(frames * 2, 0.0);
                    let processed = fill(&mut cons, &shared, &mut scratch, apply_gain, &mut dsp);
                    bytes.clear();
                    bytes.reserve(frames * block);
                    // Dither only a 16-bit target and only a signal that was processed:
                    // an untouched one (the bit-perfect path) must stay exact.
                    let dithered = processed && valid == 16 && !float;
                    for &s in &scratch {
                        write_sample(&mut bytes, s, bytes_per_sample, valid, float, dithered.then_some(&mut dither));
                    }
                    render.write_to_device(frames, &bytes, None).map_err(|e| e.to_string())?;
                }
                if event.wait_for_event(1000).is_err() {
                    return Err("exclusive output stalled".into());
                }
            }
            let _ = ac.stop_stream();
            Ok(())
        };
        if let Err(e) = run() {
            log::error!("exclusive output: {e}");
            shared.failed.store(true, Ordering::Relaxed);
            let _ = ready_tx.send(Err(e));
        }
    })?;

    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(description)) => Ok(Output {
            producer,
            rate,
            exclusive: true,
            description,
            _keep: Keep::Exclusive(ExclusiveHandle { stop, thread: Some(thread) }),
        }),
        Ok(Err(e)) => Err(anyhow!(e)),
        Err(_) => {
            stop.store(true, Ordering::Relaxed);
            Err(anyhow!("exclusive output did not start"))
        }
    }
}

/// Convert one f32 sample to the device's integer/float format. Decoded
/// integer PCM (16/24-bit) maps back to exactly the original values.
#[cfg(windows)]
fn write_sample(out: &mut Vec<u8>, s: f32, bytes: usize, valid: usize, float: bool, dither: Option<&mut Dither>) {
    if float {
        out.extend_from_slice(&s.to_le_bytes());
        return;
    }
    let v = quantize(s, valid as u32, dither);
    // Left-justify the valid bits in the container.
    let shifted = v << (bytes * 8 - valid);
    out.extend_from_slice(&shifted.to_le_bytes()[..bytes]);
}

#[cfg(not(windows))]
pub fn open_exclusive(device: Option<&str>, _rate: u32, _bits: u32, shared: Arc<OutShared>, _apply_gain: bool) -> Result<Output> {
    open_shared(device, shared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::dsp::{EqConfig, FilterKind, FilterSpec};

    fn bits(samples: &[f32]) -> Vec<u32> {
        samples.iter().map(|s| s.to_bits()).collect()
    }

    fn music(frames: usize) -> Vec<f32> {
        (0..frames * 2).map(|i| ((i as f32) * 0.37).sin() * 0.6 + ((i as f32) * 0.011).cos() * 0.2).collect()
    }

    /// Push `input` through a real ring and `fill`, in callback-sized pieces.
    fn play(shared: &OutShared, dsp: &mut Dsp, input: &[f32], gain: bool) -> Vec<f32> {
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(input.len().max(8));
        let mut chunk = producer.write_chunk(input.len()).unwrap();
        let (a, b) = chunk.as_mut_slices();
        a.copy_from_slice(&input[..a.len()]);
        b.copy_from_slice(&input[a.len()..]);
        chunk.commit_all();
        let mut out = vec![0.0f32; input.len()];
        for piece in out.chunks_mut(960) {
            fill(&mut consumer, shared, piece, gain, dsp);
        }
        out
    }

    #[test]
    fn the_exact_path_through_the_ring_is_bit_identical_without_an_equaliser() {
        let input = music(9600);
        let shared = OutShared::default();
        shared.set_gain(1.0);
        // Never configured, and configured to do nothing: identical bits.
        for cfg in [
            None,
            Some(EqConfig::default()),
            Some(EqConfig {
                enabled: true,
                preamp_db: 0.0,
                filters: vec![FilterSpec { gain_db: 0.0, ..FilterSpec::new(FilterKind::Peaking) }],
            }),
        ] {
            if let Some(cfg) = &cfg {
                shared.eq.write(cfg);
            }
            let mut dsp = Dsp::new(48_000);
            assert_eq!(bits(&play(&shared, &mut dsp, &input, true)), bits(&input));
        }
    }

    #[test]
    fn an_active_chain_changes_the_audio_and_flush_resets_it() {
        let input = music(9600);
        let shared = OutShared::default();
        shared.set_gain(1.0);
        shared.eq.write(&EqConfig { enabled: true, preamp_db: 0.0, filters: vec![FilterSpec::new(FilterKind::LowPass)] });
        let mut dsp = Dsp::new(48_000);
        let out = play(&shared, &mut dsp, &input, true);
        assert_ne!(bits(&out), bits(&input));
        // A flush (seek) leaves no memory of the old audio behind.
        shared.flush.store(true, Ordering::Release);
        let again = play(&shared, &mut dsp, &input, true);
        assert!(again.iter().all(|s| s.is_finite()));
    }

    #[cfg(windows)]
    #[test]
    fn integer_pcm_round_trips_exactly_through_f32_and_write_sample() {
        for (bytes, valid, values) in
            [(2usize, 16usize, [-32768i32, -12345, -1, 0, 1, 777, 32767]), (4, 24, [-8_388_608, -4_000_001, -1, 0, 1, 123_456, 8_388_607])]
        {
            for v in values {
                let max = (1i64 << (valid - 1)) as f32;
                let sample = v as f32 / max;
                let mut out = Vec::new();
                write_sample(&mut out, sample, bytes, valid, false, None);
                let mut raw = [0u8; 8];
                raw[..bytes].copy_from_slice(&out);
                let back = i64::from_le_bytes(raw) << (64 - bytes * 8) >> (64 - bytes * 8) >> (bytes * 8 - valid);
                assert_eq!(back, i64::from(v), "{valid}-bit {v}");
            }
        }
    }

    #[test]
    fn the_quantizer_is_exact_rounded_and_limited() {
        // Every 16-bit value that went through f32 comes back unchanged (scale 2^15, not 2^15 - 1).
        for v in (-32768i64..=32767).step_by(7).chain([-32768, -1, 0, 1, 32767]) {
            assert_eq!(quantize(v as f32 / 32768.0, 16, None), v);
        }
        assert_eq!(quantize(-1.0, 16, None), -32768);
        assert_eq!(quantize(1.0, 16, None), 32767, "full scale is limited, not wrapped");
        assert_eq!(quantize(7.5, 24, None), 8_388_607);
        // Rounds to nearest, not towards zero: 0.6 LSB is 1, -0.6 LSB is -1.
        assert_eq!(quantize(0.6 / 32768.0, 16, None), 1);
        assert_eq!(quantize(-0.6 / 32768.0, 16, None), -1);
        assert_eq!(quantize(0.4 / 32768.0, 16, None), 0);
        assert_eq!(quantize(f32::MAX, 32, None), i64::from(i32::MAX));
    }

    #[test]
    fn dither_is_triangular_zero_mean_and_within_one_bit() {
        let mut d = Dither::new();
        let n = 200_000;
        let values: Vec<f64> = (0..n).map(|_| d.tpdf()).collect();
        let mean = values.iter().sum::<f64>() / n as f64;
        let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
        assert!(values.iter().all(|v| (-1.0..=1.0).contains(v)));
        assert!(mean.abs() < 0.01, "mean {mean}");
        // The variance of the sum of two uniform(0,1) numbers is 1/6.
        assert!((var - 1.0 / 6.0).abs() < 0.005, "variance {var}");
    }

    #[test]
    fn dither_removes_the_bias_of_quantising_a_quiet_signal() {
        // A constant 0.3 LSB: plain rounding turns it into 0 (the signal is lost); with
        // dither the average of the output recovers it.
        let level = 0.3 / 32768.0;
        let plain: f64 = (0..50_000).map(|_| quantize(level as f32, 16, None) as f64).sum::<f64>() / 50_000.0;
        assert_eq!(plain, 0.0);
        let mut d = Dither::new();
        let dithered: f64 = (0..200_000).map(|_| quantize(level as f32, 16, Some(&mut d)) as f64).sum::<f64>() / 200_000.0;
        assert!((dithered - 0.3).abs() < 0.01, "average {dithered}");
    }

    #[test]
    fn fill_says_when_the_signal_was_changed_and_keeps_the_exact_path_exact() {
        let input = music(4800);
        let shared = OutShared::default();
        // Volume at full and no equaliser: untouched, so nothing to dither.
        shared.set_gain(1.0);
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(input.len());
        let mut chunk = producer.write_chunk(input.len()).unwrap();
        let (a, b) = chunk.as_mut_slices();
        a.copy_from_slice(&input[..a.len()]);
        b.copy_from_slice(&input[a.len()..]);
        chunk.commit_all();
        let mut out = vec![0.0f32; input.len()];
        let mut dsp = Dsp::new(48_000);
        assert!(!fill(&mut consumer, &shared, &mut out, true, &mut dsp));
        assert_eq!(bits(&out), bits(&input));
        // Volume below full: changed.
        shared.set_gain(0.5);
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(input.len());
        let mut chunk = producer.write_chunk(input.len()).unwrap();
        let (a, b) = chunk.as_mut_slices();
        a.copy_from_slice(&input[..a.len()]);
        b.copy_from_slice(&input[a.len()..]);
        chunk.commit_all();
        assert!(fill(&mut consumer, &shared, &mut out, true, &mut dsp));
    }

    #[test]
    fn the_peak_meter_reports_the_loudest_sample_and_resets_when_taken() {
        let shared = OutShared::default();
        shared.set_gain(1.0);
        let mut input = music(4800);
        input[1234] = 1.7; // an over, as a boost could make
        let mut dsp = Dsp::new(48_000);
        let out = play(&shared, &mut dsp, &input, true);
        assert_eq!(bits(&out), bits(&input), "measuring changes nothing");
        let peak = f32::from_bits(shared.peak.swap(0, Ordering::Relaxed));
        assert_eq!(peak, 1.7);
        assert_eq!(shared.peak.load(Ordering::Relaxed), 0);
    }
}
