//! Audio output: shared mode through cpal (system mixer rate, sinc resampled
//! upstream) or WASAPI exclusive mode at the track's own rate (bit-perfect).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

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

/// Pull up to `out.len()` samples, honouring flush/pause/gain. Returns false
/// when the output should write silence instead.
fn fill(cons: &mut rtrb::Consumer<f32>, shared: &OutShared, out: &mut [f32], apply_gain: bool) {
    if shared.flush.load(Ordering::Acquire) {
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
        return;
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
    // (Band analysis for the backgrounds is no longer used: they drift on their own.)
    if apply_gain {
        let g = shared.gain();
        if g != 1.0 {
            for s in &mut out[..n] {
                *s *= g;
            }
        }
    }
    shared.played.fetch_add((n / 2) as u64, Ordering::Release);
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
    let mut render = move |frames: usize, write: &mut dyn FnMut(usize, f32)| {
        scratch.resize(frames * 2, 0.0);
        fill(&mut cons, &sh, &mut scratch, true);
        for f in 0..frames {
            for c in 0..channels {
                // Stereo to the first two channels, silence on the others.
                let v = if c < 2 { scratch[f * 2 + c] } else { 0.0 };
                write(f * channels + c, v);
            }
        }
    };
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => dev.build_output_stream(
            &stream_cfg,
            move |data: &mut [f32], _| render(data.len() / channels, &mut |i, v| data[i] = v),
            on_err,
            None,
        )?,
        cpal::SampleFormat::I16 => dev.build_output_stream(
            &stream_cfg,
            move |data: &mut [i16], _| render(data.len() / channels, &mut |i, v| data[i] = (v.clamp(-1.0, 1.0) * 32767.0) as i16),
            on_err,
            None,
        )?,
        cpal::SampleFormat::I32 => dev.build_output_stream(
            &stream_cfg,
            move |data: &mut [i32], _| {
                render(data.len() / channels, &mut |i, v| data[i] = (v.clamp(-1.0, 1.0) as f64 * 2147483647.0) as i32)
            },
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
            ac.start_stream().map_err(|e| e.to_string())?;
            while !stop2.load(Ordering::Relaxed) {
                let frames = ac.get_available_space_in_frames().map_err(|e| e.to_string())? as usize;
                if frames > 0 {
                    scratch.resize(frames * 2, 0.0);
                    fill(&mut cons, &shared, &mut scratch, apply_gain);
                    bytes.clear();
                    bytes.reserve(frames * block);
                    for &s in &scratch {
                        write_sample(&mut bytes, s, bytes_per_sample, valid, float);
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
fn write_sample(out: &mut Vec<u8>, s: f32, bytes: usize, valid: usize, float: bool) {
    if float {
        out.extend_from_slice(&s.to_le_bytes());
        return;
    }
    let s = s.clamp(-1.0, 1.0) as f64;
    let max = (1i64 << (valid - 1)) as f64;
    let v = (s * max).round().clamp(-max, max - 1.0) as i64;
    // Left-justify the valid bits in the container.
    let shifted = v << (bytes * 8 - valid);
    out.extend_from_slice(&shifted.to_le_bytes()[..bytes]);
}

#[cfg(not(windows))]
pub fn open_exclusive(device: Option<&str>, _rate: u32, _bits: u32, shared: Arc<OutShared>, _apply_gain: bool) -> Result<Output> {
    open_shared(device, shared)
}
