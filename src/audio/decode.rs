//! Symphonia decoding to interleaved stereo f32, plus sinc resampling.

use std::{fs::File, path::Path, sync::Arc};

use anyhow::{anyhow, Context, Result};
use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey};
use symphonia::core::probe::{Hint, ProbeResult};
use symphonia::core::units::{Time, TimeBase};

use super::stream::{Buffer, Reader};

pub struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    pub sample_rate: u32,
    pub channels: usize,
    pub bits_per_sample: Option<u32>,
    /// Total length in seconds if known.
    pub duration: Option<f64>,
    buf: Option<SampleBuffer<f32>>,
}

#[derive(Default)]
pub struct FileMetadata {
    pub title: Option<String>,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub duration: Option<f64>,
}

impl FileMetadata {
    fn read_tags(&mut self, revision: &MetadataRevision) {
        for tag in revision.tags() {
            let slot = match tag.std_key {
                Some(StandardTagKey::TrackTitle) => &mut self.title,
                Some(StandardTagKey::Album) => &mut self.album,
                Some(StandardTagKey::Artist) => &mut self.artist,
                _ => continue,
            };
            if slot.is_none() {
                let value = tag.value.to_string();
                let value = value.trim();
                if !value.is_empty() {
                    *slot = Some(value.to_owned());
                }
            }
        }
    }
}

fn probe(source: MediaSourceStream, hint: &Hint) -> Result<ProbeResult> {
    symphonia::default::get_probe()
        .format(hint, source, &FormatOptions { enable_gapless: true, ..Default::default() }, &MetadataOptions::default())
        .context("Formato de audio no reconocido")
}

fn probe_file(path: &Path) -> Result<ProbeResult> {
    let file = File::open(path).context("No se pudo abrir el archivo de audio")?;
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(&extension.to_ascii_lowercase());
    }
    probe(MediaSourceStream::new(Box::new(file), Default::default()), &hint)
}

pub fn inspect_file(path: &Path) -> Result<FileMetadata> {
    let mut probed = probe_file(path)?;
    let mut tags = FileMetadata::default();
    if let Some(metadata) = probed.metadata.get() {
        if let Some(revision) = metadata.current() {
            tags.read_tags(revision);
        }
    }
    if let Some(revision) = probed.format.metadata().current() {
        tags.read_tags(revision);
    }
    tags.duration = Decoder::from_format(probed.format)?.duration;
    Ok(tags)
}

impl Decoder {
    pub fn open(buffer: Arc<Buffer>, mp4: bool) -> Result<Self> {
        // Fragmented MP4 (DASH) is read front to back; FLAC/MP4 files seek.
        let seekable = !mp4 || buffer.total_len().is_some();
        let mss = MediaSourceStream::new(Box::new(Reader::new(buffer, seekable)), MediaSourceStreamOptions { buffer_len: 256 * 1024 });
        let mut hint = Hint::new();
        hint.with_extension(if mp4 { "mp4" } else { "flac" });
        Self::from_format(probe(mss, &hint)?.format)
    }

    pub fn open_file(path: &Path) -> Result<Self> {
        Self::from_format(probe_file(path)?.format)
    }

    fn from_format(format: Box<dyn FormatReader>) -> Result<Self> {
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
            .ok_or_else(|| anyhow!("El archivo no contiene una pista de audio compatible"))?;
        let params = track.codec_params.clone();
        let decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).context("unsupported codec")?;
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
            time_base: params.time_base,
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
        Ok(self.time_base.map_or(to.actual_ts as f64 / self.sample_rate as f64, |base| {
            let time = base.calc_time(to.actual_ts);
            time.seconds as f64 + time.frac
        }))
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
        let inner = SincFixedIn::<f32>::new(to as f64 / from as f64, 1.0, params, chunk, 2).map_err(|e| anyhow!("resampler: {e}"))?;
        let skip = inner.output_delay();
        Ok(Self { inner, pending: [Vec::new(), Vec::new()], chunk, skip })
    }

    /// Feed interleaved stereo; returns whatever output is ready.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        for f in input.as_chunks::<2>().0 {
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
