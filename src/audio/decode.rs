//! Symphonia decoding to interleaved stereo f32, plus sinc resampling.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder as _, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use super::stream::{Buffer, Reader};

pub struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    pub sample_rate: u32,
    pub channels: usize,
    pub bits_per_sample: Option<u32>,
    /// Total length in seconds if known.
    pub duration: Option<f64>,
    buf: Option<SampleBuffer<f32>>,
}

impl Decoder {
    pub fn open(buffer: Arc<Buffer>, mp4: bool) -> Result<Self> {
        // Fragmented MP4 (DASH) is read front to back; FLAC/MP4 files seek.
        let seekable = !mp4 || buffer.total_len().is_some();
        let mss = MediaSourceStream::new(
            Box::new(Reader::new(buffer, seekable)),
            MediaSourceStreamOptions { buffer_len: 256 * 1024 },
        );
        let mut hint = Hint::new();
        hint.with_extension(if mp4 { "mp4" } else { "flac" });
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions { enable_gapless: true, ..Default::default() },
                &MetadataOptions::default(),
            )
            .context("unrecognised audio format")?;
        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow!("no audio track"))?;
        let params = track.codec_params.clone();
        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .context("unsupported codec")?;
        let sample_rate = params.sample_rate.ok_or_else(|| anyhow!("unknown sample rate"))?;
        let channels = params.channels.map(|c| c.count()).unwrap_or(2).max(1);
        let duration = match (params.n_frames, params.time_base) {
            (Some(n), Some(tb)) => {
                let t = tb.calc_time(n);
                Some(t.seconds as f64 + t.frac)
            }
            (Some(n), None) => Some(n as f64 / sample_rate as f64),
            _ => None,
        };
        Ok(Self {
            track_id: track.id,
            format,
            decoder,
            sample_rate,
            channels,
            bits_per_sample: params.bits_per_sample,
            duration,
            buf: None,
        })
    }

    /// Decode the next packet as interleaved stereo. `None` at the end.
    pub fn next_chunk(&mut self) -> Result<Option<Vec<f32>>> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(SymError::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(e) => return Err(anyhow!("demux error: {e}")),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(d) => d,
                Err(SymError::DecodeError(e)) => {
                    log::warn!("skipping bad packet: {e}");
                    continue;
                }
                Err(e) => return Err(anyhow!("decode error: {e}")),
            };
            let spec = *decoded.spec();
            let frames = decoded.capacity() as u64;
            let buf = match &mut self.buf {
                Some(b) if b.capacity() >= decoded.capacity() * spec.channels.count() => b,
                slot => slot.insert(SampleBuffer::new(frames, spec)),
            };
            buf.copy_interleaved_ref(decoded);
            let samples = buf.samples();
            let ch = spec.channels.count().max(1);
            let out = match ch {
                2 => samples.to_vec(),
                1 => samples.iter().flat_map(|&s| [s, s]).collect(),
                _ => samples.chunks_exact(ch).flat_map(|f| [f[0], f[1]]).collect(),
            };
            if out.is_empty() {
                continue;
            }
            return Ok(Some(out));
        }
    }

    /// Seek to `secs`. Returns the actual position reached.
    pub fn seek(&mut self, secs: f64) -> Result<f64> {
        let t = Time { seconds: secs.trunc() as u64, frac: secs.fract() };
        let to = self
            .format
            .seek(SeekMode::Accurate, SeekTo::Time { time: t, track_id: Some(self.track_id) })
            .map_err(|e| anyhow!("seek failed: {e}"))?;
        self.decoder.reset();
        Ok(to.actual_ts as f64 / self.sample_rate as f64)
    }
}

/// Band-limited sinc resampler for interleaved stereo.
pub struct Resampler2 {
    inner: SincFixedIn<f32>,
    pending: [Vec<f32>; 2],
    chunk: usize,
    /// Output frames still to drop (the filter's delay), so tracks stay gapless.
    skip: usize,
}

impl Resampler2 {
    pub fn new(from: u32, to: u32) -> Result<Self> {
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            oversampling_factor: 256,
            interpolation: SincInterpolationType::Cubic,
            window: WindowFunction::BlackmanHarris2,
        };
        let chunk = 1024;
        let inner = SincFixedIn::<f32>::new(to as f64 / from as f64, 1.0, params, chunk, 2)
            .map_err(|e| anyhow!("resampler: {e}"))?;
        let skip = inner.output_delay();
        Ok(Self { inner, pending: [Vec::new(), Vec::new()], chunk, skip })
    }

    /// Feed interleaved stereo; returns whatever output is ready.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        for f in input.chunks_exact(2) {
            self.pending[0].push(f[0]);
            self.pending[1].push(f[1]);
        }
        let mut out = Vec::new();
        while self.pending[0].len() >= self.chunk {
            let l: Vec<f32> = self.pending[0].drain(..self.chunk).collect();
            let r: Vec<f32> = self.pending[1].drain(..self.chunk).collect();
            if let Ok(res) = self.inner.process(&[l, r], None) {
                interleave(&res, &mut out);
            }
        }
        self.trim(out)
    }

    fn trim(&mut self, mut out: Vec<f32>) -> Vec<f32> {
        if self.skip > 0 {
            let n = (self.skip * 2).min(out.len());
            out.drain(..n);
            self.skip -= n / 2;
        }
        out
    }

    /// Flush the remaining input at the end of a track.
    pub fn finish(&mut self) -> Vec<f32> {
        let mut out = Vec::new();
        let l = std::mem::take(&mut self.pending[0]);
        let r = std::mem::take(&mut self.pending[1]);
        if !l.is_empty() {
            if let Ok(res) = self.inner.process_partial(Some(&[l, r]), None) {
                interleave(&res, &mut out);
            }
        }
        if let Ok(res) = self.inner.process_partial(None::<&[Vec<f32>]>, None) {
            interleave(&res, &mut out);
        }
        self.trim(out)
    }
}

fn interleave(chans: &[Vec<f32>], out: &mut Vec<f32>) {
    let n = chans[0].len().min(chans[1].len());
    out.reserve(n * 2);
    for i in 0..n {
        out.push(chans[0][i]);
        out.push(chans[1][i]);
    }
}
