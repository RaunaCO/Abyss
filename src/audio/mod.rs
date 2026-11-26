//! Playback engine.
//!
//! Each track is decoded on its own worker thread (so slow downloads never
//! block the engine); the engine thread resamples if needed and feeds a ring
//! buffer that the real-time output drains. The next track is queued ahead of
//! time and starts on the very next sample: gapless playback.

pub mod decode;
pub mod dsp;
pub mod graphic;
pub mod output;
pub mod stream;

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use reqwest::blocking::Client;

use crate::tidal::{StreamInfo, StreamSource};
use decode::{Decoder, Resampler2};
use output::{OutShared, Output};
use stream::Buffer;

#[derive(Debug, Clone)]
pub struct OutputPrefs {
    pub device: Option<String>,
    pub exclusive: bool,
    /// Exclusive mode only: keep the volume at 100 % (bit-perfect).
    pub force_volume: bool,
    /// Loudness normalisation with TIDAL's per-track ReplayGain.
    pub normalize: bool,
}

#[derive(Clone)]
pub enum Source {
    Stream(StreamInfo),
    Local(std::path::PathBuf),
}

pub enum Cmd {
    /// Replace everything and play this track (from `start` seconds).
    Play {
        key: u64,
        source: Source,
        start: f64,
        paused: bool,
    },
    /// Track to continue with when the current one ends (replaces the old one).
    Queue {
        key: u64,
        source: Source,
    },
    ClearQueue,
    Pause,
    Resume,
    Seek(f64),
    Stop,
    Volume(f32),
    Output(OutputPrefs),
}

pub enum Event {
    /// The queue ran out and playback stopped at the end of `key`.
    Finished {
        key: u64,
    },
    Error {
        key: Option<u64>,
        message: String,
    },
}

/// What the UI reads on every frame.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub key: Option<u64>,
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    pub buffering: bool,
    /// Source format of the playing track: (bits, rate).
    pub source: Option<(u32, u32)>,
    pub output: String,
    pub output_rate: u32,
    #[cfg(feature = "ui-iced")]
    pub output_exclusive: bool,
    /// Downloaded fraction of the playing track (0..1).
    pub downloaded: f32,
}

#[derive(Clone)]
pub struct Player {
    tx: Sender<Cmd>,
    state: Arc<Mutex<EngineView>>,
    out: Arc<OutShared>,
}

/// A track's place on the output timeline.
#[derive(Clone)]
struct Mark {
    key: u64,
    start_frame: u64,
    offset: f64,
    duration: f64,
    source: Option<(u32, u32)>,
    buffer: Option<Arc<Buffer>>,
    playback_source: Source,
}

#[derive(Default)]
struct EngineView {
    marks: VecDeque<Mark>,
    output: String,
    output_rate: u32,
    #[cfg(feature = "ui-iced")]
    output_exclusive: bool,
    buffering: bool,
    /// Key of the track that should be shown while nothing is audible yet.
    loading: Option<u64>,
    /// While seeking nothing is audible yet; keep showing the target position,
    /// duration and format so the player bar does not blank out.
    seek_hint: Option<(f64, f64, Option<(u32, u32)>)>,
}

impl Player {
    pub fn new(http: Client, volume: f32, prefs: OutputPrefs) -> (Self, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let (ev_tx, ev_rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(EngineView::default()));
        let out = Arc::new(OutShared::default());
        out.set_gain(gain_for(volume));
        let st = state.clone();
        let o = out.clone();
        std::thread::Builder::new()
            .name("audio-engine".into())
            .spawn(move || Engine::new(http, rx, ev_tx, st, o, prefs, volume).run())
            .expect("spawn audio engine");
        (Self { tx, state, out }, ev_rx)
    }

    /// Loudness of the low / mid / high bands being played (for visuals).
    pub fn levels(&self) -> [f32; 3] {
        self.out.levels()
    }

    /// Publish the equaliser chain. Safe to call from the UI at any time: the
    /// output callback reads it without locking.
    pub fn set_eq(&self, config: &dsp::EqConfig) {
        self.out.eq.write(config);
    }

    /// Loudest sample of the left and the right channel sent to the device since
    /// the last call (linear, 0 when nothing played). Above 1.0 means that
    /// channel is over full scale.
    pub fn take_peaks(&self) -> [f32; 2] {
        self.out.peak.each_ref().map(|p| f32::from_bits(p.swap(0, Ordering::Relaxed)))
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn snapshot(&self) -> Snapshot {
        let played = self.out.played.load(Ordering::Acquire);
        let paused = self.out.paused.load(Ordering::Relaxed);
        let mut v = self.state.lock();
        // Drop marks of tracks that are completely behind us.
        while v.marks.len() > 1 && v.marks[1].start_frame <= played {
            v.marks.pop_front();
        }
        let rate = v.output_rate.max(1) as f64;
        let mut snap = Snapshot {
            paused,
            buffering: v.buffering,
            output: v.output.clone(),
            output_rate: v.output_rate,
            #[cfg(feature = "ui-iced")]
            output_exclusive: v.output_exclusive,
            ..Default::default()
        };
        if let Some(m) = v.marks.front() {
            let elapsed = played.saturating_sub(m.start_frame) as f64 / rate;
            snap.key = Some(m.key);
            snap.position = (m.offset + elapsed).min(if m.duration > 0.0 { m.duration } else { f64::MAX });
            snap.duration = m.duration;
            snap.source = m.source;
            snap.downloaded = m.buffer.as_ref().map_or(1.0, |buffer| {
                let (have, total, done) = buffer.downloaded();
                if done {
                    1.0
                } else {
                    total.map_or(0.0, |t| have as f32 / t.max(1) as f32)
                }
            });
        } else if let Some(k) = v.loading {
            snap.key = Some(k);
            if let Some((pos, dur, src)) = v.seek_hint {
                snap.position = pos;
                snap.duration = dur;
                snap.source = src;
            }
        }
        snap
    }
}

/// Perceptual volume curve (TIDAL's slider feels roughly cubic).
fn gain_for(volume: f32) -> f32 {
    let v = volume.clamp(0.0, 1.0);
    v * v * v
}

// ------------------------------------------------------------- decode worker

enum DecMsg {
    Format { rate: u32, bits: Option<u32>, duration: Option<f64> },
    Chunk(Vec<f32>),
    Seeked(f64),
    End,
    Error(String),
}

struct Worker {
    rx: Receiver<(u64, DecMsg)>,
    seek_tx: Sender<(u64, f64)>,
    buffer: Option<Arc<Buffer>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(buffer) = &self.buffer {
            buffer.cancel();
        }
    }
}

enum DecodeInput {
    Stream { buffer: Arc<Buffer>, mp4: bool },
    Local(std::path::PathBuf),
}

impl DecodeInput {
    fn open(&self) -> anyhow::Result<Decoder> {
        match self {
            Self::Stream { buffer, mp4 } => Decoder::open(buffer.clone(), *mp4),
            Self::Local(path) => Decoder::open_file(path),
        }
    }

    fn seek(&self, decoder: &mut Decoder, seconds: f64) -> anyhow::Result<f64> {
        match self {
            Self::Stream { buffer, mp4 } => do_seek(decoder, buffer, *mp4, seconds),
            Self::Local(_) => decoder.seek(seconds),
        }
    }

    fn cancelled(&self) -> bool {
        matches!(self, Self::Stream { buffer, .. } if buffer.is_cancelled())
    }
}

fn spawn_worker(http: &Client, source: Source) -> Worker {
    let (input, buffer) = match source {
        Source::Stream(info) => {
            let mp4 = matches!(info.source, StreamSource::Segments(_)) || !info.codec.eq_ignore_ascii_case("flac");
            let buffer = stream::start(http.clone(), info.source, info.seg_secs, info.renew.map(|r| r.0));
            (DecodeInput::Stream { buffer: buffer.clone(), mp4 }, Some(buffer))
        }
        Source::Local(path) => (DecodeInput::Local(path), None),
    };
    let (tx, rx) = mpsc::sync_channel(24);
    let (seek_tx, seek_rx) = mpsc::channel();
    std::thread::Builder::new().name("decode".into()).spawn(move || decode_loop(input, tx, seek_rx)).expect("spawn decoder");
    Worker { rx, seek_tx, buffer }
}

/// Move `dec` to `t` seconds. DASH streams restart the download at the right
/// segment (instant); other sources use the demuxer's seek, falling back to
/// decoding from the start and discarding samples.
pub fn do_seek(dec: &mut Decoder, buf: &Arc<Buffer>, mp4: bool, t: f64) -> anyhow::Result<f64> {
    if let Some((seg, seg_start)) = buf.segment_for(t) {
        buf.restart_at(seg);
        let mut d = Decoder::open(buf.clone(), mp4)?;
        let skip = ((t - seg_start).max(0.0) * d.sample_rate as f64) as usize * 2;
        let mut done = 0usize;
        while done < skip {
            match d.next_chunk() {
                Ok(Some(c)) => done += c.len(),
                _ => break,
            }
        }
        let reached = seg_start + done as f64 / 2.0 / d.sample_rate as f64;
        *dec = d;
        return Ok(reached);
    }
    if mp4 {
        // A single-file MP4 (AAC) stream is read front to back: its demuxer can
        // move forward but not back ("packet out-of-bounds for a non-seekable
        // stream"). The whole file stays in `buf`, so reopen it from the first
        // byte, without downloading again, and move forward to `t`.
        let mut d = Decoder::open(buf.clone(), mp4)?;
        let reached = match d.seek(t) {
            Ok(at) => at,
            Err(e) => {
                log::debug!("seek after reopening failed, decoding up to it: {e:#}");
                d = Decoder::open(buf.clone(), mp4)?;
                skip_to(&mut d, t)
            }
        };
        *dec = d;
        return Ok(reached);
    }
    match dec.seek(t) {
        Ok(at) => Ok(at),
        Err(e) => {
            log::debug!("seek fallback: {e:#}");
            let mut d = Decoder::open(buf.clone(), mp4)?;
            let reached = skip_to(&mut d, t);
            *dec = d;
            Ok(reached)
        }
    }
}

/// Decode and discard from the start of a freshly opened `d` up to `t` seconds.
/// Returns where it stopped (earlier if the stream ends first).
fn skip_to(d: &mut Decoder, t: f64) -> f64 {
    let skip = (t.max(0.0) * d.sample_rate as f64) as usize * 2;
    let mut done = 0usize;
    while done < skip {
        match d.next_chunk() {
            Ok(Some(c)) => done += c.len(),
            _ => break,
        }
    }
    done as f64 / 2.0 / d.sample_rate.max(1) as f64
}

fn decode_loop(input: DecodeInput, tx: SyncSender<(u64, DecMsg)>, seek_rx: Receiver<(u64, f64)>) {
    let mut generation = 0;
    let mut dec = match input.open() {
        Ok(d) => d,
        Err(e) => {
            if !input.cancelled() {
                let _ = tx.send((generation, DecMsg::Error(format!("{e:#}"))));
            }
            return;
        }
    };
    if tx.send((generation, DecMsg::Format { rate: dec.sample_rate, bits: dec.bits_per_sample, duration: dec.duration })).is_err() {
        return;
    }
    let mut target = None;
    loop {
        // Finish the newest request and stamp every message with its generation.
        while let Ok(request) = seek_rx.try_recv() {
            target = Some(request);
        }
        if let Some((requested, position)) = target.take() {
            generation = requested;
            match input.seek(&mut dec, position) {
                Ok(reached) => {
                    if tx.send((generation, DecMsg::Seeked(reached))).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    if tx.send((generation, DecMsg::Error(format!("{error:#}")))).is_err() {
                        return;
                    }
                    let Ok(request) = seek_rx.recv() else { return };
                    target = Some(request);
                    continue;
                }
            }
        }
        match dec.next_chunk() {
            Ok(Some(chunk)) => {
                if tx.send((generation, DecMsg::Chunk(chunk))).is_err() {
                    return;
                }
            }
            Ok(None) => {
                if tx.send((generation, DecMsg::End)).is_err() {
                    return;
                }
                // Stay alive: the user may still seek back into this track.
                let Ok(request) = seek_rx.recv() else { return };
                target = Some(request);
            }
            Err(e) => {
                if !input.cancelled() {
                    let _ = tx.send((generation, DecMsg::Error(format!("{e:#}"))));
                }
                return;
            }
        }
    }
}

// --------------------------------------------------------------------- engine

struct Track {
    key: u64,
    worker: Worker,
    rate: Option<u32>,
    bits: u32,
    duration: f64,
    resampler: Option<Resampler2>,
    generation: u64,
    seeking: bool,
    /// Seconds into the track where the next pushed sample belongs.
    start_offset: f64,
    marked: bool,
    ended: bool,
    /// A chunk waiting for the output device to (re)open.
    held: Option<Vec<f32>>,
    /// Cached ReplayGain, read only when normalization is enabled.
    normal_gain: Option<f32>,
    source: Source,
}

impl Track {
    fn request_seek(&mut self, position: f64) {
        self.generation = self.generation.wrapping_add(1);
        let _ = self.worker.seek_tx.send((self.generation, position));
        self.seeking = true;
        self.ended = false;
        self.marked = false;
        self.resampler = None;
        self.held = None;
    }

    fn normalization_gain(&mut self) -> f32 {
        let source = &self.source;
        *self.normal_gain.get_or_insert_with(|| {
            let (gain, peak) = match source {
                Source::Stream(info) => (info.replay_gain, info.peak),
                Source::Local(path) => local_replay_gain(path),
            };
            normalisation(gain, peak)
        })
    }
}

enum OutState {
    Ready,
    Wait,
    Failed,
}

struct Engine {
    http: Client,
    rx: Receiver<Cmd>,
    ev: Sender<Event>,
    view: Arc<Mutex<EngineView>>,
    out_shared: Arc<OutShared>,
    prefs: OutputPrefs,
    volume: f32,
    out: Option<Output>,
    /// Frames pushed into the ring since the output was opened.
    pushed: u64,
    pending: Vec<f32>,
    pending_pos: usize,
    cur: Option<Track>,
    next: Option<Track>,
    /// Paused by the user (kept across output re-opens).
    paused: bool,
}

impl Engine {
    fn new(
        http: Client,
        rx: Receiver<Cmd>,
        ev: Sender<Event>,
        view: Arc<Mutex<EngineView>>,
        out_shared: Arc<OutShared>,
        prefs: OutputPrefs,
        volume: f32,
    ) -> Self {
        Self {
            http,
            rx,
            ev,
            view,
            out_shared,
            prefs,
            volume,
            out: None,
            pushed: 0,
            pending: Vec::new(),
            pending_pos: 0,
            cur: None,
            next: None,
            paused: false,
        }
    }

    fn run(mut self) {
        loop {
            // Commands: block briefly when idle, otherwise just poll.
            let idle = self.cur.is_none();
            let cmd = if idle {
                match self.rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            } else {
                match self.rx.try_recv() {
                    Ok(c) => Some(c),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            };
            if let Some(cmd) = cmd {
                self.handle(cmd);
                continue;
            }
            if self.out_shared.failed.swap(false, Ordering::Relaxed) {
                self.recover_output();
            }
            if !self.pump() {
                std::thread::sleep(Duration::from_millis(4));
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play { key, source, start, paused } => {
                self.stop_all();
                self.paused = paused;
                self.out_shared.paused.store(paused, Ordering::Relaxed);
                let mut t = self.make_track(key, source);
                if start > 0.5 {
                    t.request_seek(start);
                }
                self.cur = Some(t);
                let mut v = self.view.lock();
                v.loading = Some(key);
                v.buffering = true;
            }
            Cmd::Queue { key, source } => {
                if self.next.as_ref().map(|n| n.key) != Some(key) {
                    self.next = Some(self.make_track(key, source));
                }
            }
            Cmd::ClearQueue => self.next = None,
            Cmd::Pause => {
                self.paused = true;
                self.out_shared.paused.store(true, Ordering::Relaxed);
            }
            Cmd::Resume => {
                self.paused = false;
                self.out_shared.paused.store(false, Ordering::Relaxed);
            }
            Cmd::Seek(secs) => self.seek(secs),
            Cmd::Stop => {
                self.stop_all();
                self.next = None;
            }
            Cmd::Volume(v) => {
                self.volume = v;
                self.out_shared.set_gain(gain_for(v));
            }
            Cmd::Output(prefs) => {
                self.prefs = prefs;
                self.out_shared.set_gain(gain_for(self.volume));
                // Re-open at the current position.
                let pos = self.position_of_current();
                self.close_output();
                if self.cur.is_some() {
                    self.seek(pos);
                } else {
                    self.reset_timeline();
                }
            }
        }
    }

    fn make_track(&self, key: u64, source: Source) -> Track {
        let bits = match &source {
            Source::Stream(info) => info.bit_depth.unwrap_or(16),
            Source::Local(_) => 16,
        };
        let worker = spawn_worker(&self.http, source.clone());
        Track {
            source,
            normal_gain: None,
            key,
            bits,
            worker,
            rate: None,
            duration: 0.0,
            resampler: None,
            generation: 0,
            seeking: false,
            start_offset: 0.0,
            marked: false,
            ended: false,
            held: None,
        }
    }

    fn stop_all(&mut self) {
        self.cur = None;
        self.flush_output();
        self.reset_timeline();
    }

    fn reset_timeline(&mut self) {
        self.pending.clear();
        self.pending_pos = 0;
        let mut v = self.view.lock();
        v.marks.clear();
        v.loading = None;
        v.seek_hint = None;
        v.buffering = false;
    }

    /// Discard everything queued in the output ring.
    fn flush_output(&mut self) {
        if self.out.is_some() {
            self.out_shared.flush.store(true, Ordering::Release);
            for _ in 0..100 {
                if !self.out_shared.flush.load(Ordering::Acquire) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            self.out_shared.flush.store(false, Ordering::Release);
            self.pushed = self.out_shared.played.load(Ordering::Acquire);
        }
    }

    fn position_of_current(&self) -> f64 {
        let played = self.out_shared.played.load(Ordering::Acquire);
        let v = self.view.lock();
        let rate = v.output_rate.max(1) as f64;
        v.marks
            .iter()
            .rev()
            .find(|m| m.start_frame <= played)
            .map(|m| m.offset + played.saturating_sub(m.start_frame) as f64 / rate)
            .unwrap_or(0.0)
    }

    fn seek(&mut self, secs: f64) {
        let Some(mut t) = self.cur.take() else { return };
        let played = self.out_shared.played.load(Ordering::Acquire);
        let audible = self.view.lock().marks.iter().rev().find(|m| m.start_frame <= played).cloned();
        if let Some(mark) = &audible {
            if mark.key != t.key {
                let previous = self.make_track(mark.key, mark.playback_source.clone());
                t.request_seek(0.0);
                self.next = Some(t);
                t = previous;
            }
        }
        let hint = audible.map(|m| (m.duration, m.source));
        self.flush_output();
        self.reset_timeline();
        if let Some((dur, src)) = hint {
            self.view.lock().seek_hint = Some((secs.max(0.0), dur, src));
        }
        t.request_seek(secs.max(0.0));
        self.view.lock().loading = Some(t.key);
        self.view.lock().buffering = true;
        self.cur = Some(t);
    }

    fn close_output(&mut self) {
        {
            let mut view = self.view.lock();
            view.output.clear();
            view.output_rate = 0;
            #[cfg(feature = "ui-iced")]
            {
                view.output_exclusive = false;
            }
        }
        self.out = None;
    }

    fn ensure_output(&mut self, rate: u32, bits: u32) -> OutState {
        let wanted_exclusive = self.prefs.exclusive;
        if let Some(o) = &self.out {
            if !wanted_exclusive || o.exclusive_source_bits.is_none() || (o.rate == rate && o.exclusive_source_bits == Some(bits)) {
                return OutState::Ready;
            }
            // Exclusive mode at a different rate: wait for the ring to drain.
            if self.out_shared.played.load(Ordering::Acquire) < self.pushed {
                return OutState::Wait;
            }
            self.close_output();
        }
        let opened = if wanted_exclusive {
            let gain = !self.prefs.force_volume;
            output::open_exclusive(self.prefs.device.as_deref(), rate, bits, self.out_shared.clone(), gain).or_else(|e| {
                let _ = self
                    .ev
                    .send(Event::Error { key: None, message: crate::i18n::text("audio.exclusive_unavailable", &[format!("{e:#}")]) });
                output::open_shared(self.prefs.device.as_deref(), self.out_shared.clone())
            })
        } else {
            output::open_shared(self.prefs.device.as_deref(), self.out_shared.clone())
        };
        match opened {
            Ok(o) => {
                self.pushed = self.out_shared.played.load(Ordering::Acquire);
                let mut v = self.view.lock();
                v.output = o.description.clone();
                v.output_rate = o.rate;
                #[cfg(feature = "ui-iced")]
                {
                    v.output_exclusive = o.exclusive_source_bits.is_some();
                }
                // Marks refer to the old output's timeline.
                let played = self.pushed;
                for m in v.marks.iter_mut() {
                    m.start_frame = played;
                }
                self.out = Some(o);
                OutState::Ready
            }
            Err(e) => {
                let _ = self.ev.send(Event::Error { key: None, message: crate::i18n::text("audio.output_failed", &[format!("{e:#}")]) });
                OutState::Failed
            }
        }
    }

    fn recover_output(&mut self) {
        log::warn!("output failed; reopening");
        let prefs = self.prefs.clone();
        self.handle(Cmd::Output(prefs));
    }

    /// Move audio along. Returns true if work was done.
    fn pump(&mut self) -> bool {
        // 1. Push leftovers into the ring.
        if self.pending_pos < self.pending.len() {
            let Some(out) = self.out.as_mut() else { return false };
            let free = out.producer.slots();
            let left = self.pending.len() - self.pending_pos;
            let n = free.min(left);
            let n = n - n % 2;
            if n == 0 {
                return false;
            }
            if let Ok(mut chunk) = out.producer.write_chunk(n) {
                let (a, b) = chunk.as_mut_slices();
                let src = &self.pending[self.pending_pos..self.pending_pos + n];
                a.copy_from_slice(&src[..a.len()]);
                b.copy_from_slice(&src[a.len()..]);
                chunk.commit_all();
            }
            self.pending_pos += n;
            self.pushed += (n / 2) as u64;
            if self.pending_pos >= self.pending.len() {
                self.pending.clear();
                self.pending_pos = 0;
            }
            return true;
        }

        let Some(mut t) = self.cur.take() else { return false };

        if t.ended {
            // Everything of this track is in the ring; continue with the next.
            if let Some(next) = self.next.take() {
                self.cur = Some(next);
                return true;
            }
            let played = self.out_shared.played.load(Ordering::Acquire);
            if played >= self.pushed {
                let _ = self.ev.send(Event::Finished { key: t.key });
                self.reset_timeline();
                return false;
            }
            self.cur = Some(t);
            return false;
        }

        let held = t.held.take();
        let msg = match held.map(|h| Ok((t.generation, DecMsg::Chunk(h)))).unwrap_or_else(|| t.worker.rx.try_recv()) {
            Ok((generation, message)) => {
                if generation != t.generation
                    && !matches!(message, DecMsg::Format { .. })
                    && !(t.rate.is_none() && matches!(message, DecMsg::Error(_)))
                {
                    self.cur = Some(t);
                    return true;
                }
                message
            }
            Err(TryRecvError::Empty) => {
                self.view.lock().buffering = !t.marked || self.out_shared.played.load(Ordering::Acquire) >= self.pushed;
                self.cur = Some(t);
                return false;
            }
            Err(TryRecvError::Disconnected) => {
                t.ended = true;
                self.cur = Some(t);
                return true;
            }
        };

        match msg {
            DecMsg::Format { rate, bits, duration } => {
                t.rate = Some(rate);
                if let Some(b) = bits {
                    t.bits = b;
                }
                t.duration = duration.unwrap_or(0.0);
            }
            DecMsg::Seeked(at) => {
                t.seeking = false;
                t.start_offset = at;
            }
            DecMsg::Chunk(samples) => {
                if t.seeking {
                    self.cur = Some(t);
                    return true;
                }
                let rate = t.rate.unwrap_or(44_100);
                match self.ensure_output(rate, t.bits) {
                    OutState::Ready => {}
                    OutState::Wait => {
                        t.held = Some(samples);
                        self.cur = Some(t);
                        return false;
                    }
                    OutState::Failed => {
                        self.cur = None;
                        self.reset_timeline();
                        return false;
                    }
                }
                let out_rate = self.out.as_ref().map(|o| o.rate).unwrap_or(rate);
                if out_rate != rate && t.resampler.is_none() {
                    match Resampler2::new(rate, out_rate) {
                        Ok(r) => t.resampler = Some(r),
                        Err(e) => log::error!("{e:#}"),
                    }
                }
                let mut samples = samples;
                let bit_perfect = self.prefs.force_volume && self.out.as_ref().is_some_and(|o| o.exclusive_source_bits.is_some());
                if self.prefs.normalize && !bit_perfect {
                    let gain = t.normalization_gain();
                    if gain != 1.0 {
                        for sample in &mut samples {
                            *sample *= gain;
                        }
                    }
                }
                let data = match &mut t.resampler {
                    Some(r) => r.process(&samples),
                    None => samples,
                };
                if !t.marked {
                    t.marked = true;
                    let mark = Mark {
                        key: t.key,
                        start_frame: self.pushed + (self.pending.len() - self.pending_pos) as u64 / 2,
                        offset: t.start_offset,
                        duration: if t.duration > 0.0 { t.duration } else { 0.0 },
                        source: Some((t.bits, rate)),
                        buffer: t.worker.buffer.clone(),
                        playback_source: t.source.clone(),
                    };
                    let mut v = self.view.lock();
                    v.marks.push_back(mark);
                    v.loading = None;
                    v.buffering = false;
                }
                self.pending.extend_from_slice(&data);
            }
            DecMsg::End => {
                if let Some(r) = &mut t.resampler {
                    let tail = r.finish();
                    self.pending.extend_from_slice(&tail);
                }
                t.ended = true;
            }
            DecMsg::Error(message) => {
                let _ = self.ev.send(Event::Error { key: Some(t.key), message });
                t.ended = true;
            }
        }
        self.cur = Some(t);
        true
    }
}

/// Linear gain from a ReplayGain value in dB, never pushing the sample peak past
/// full scale. No value: 1.0 (untouched).
fn normalisation(replay_gain: Option<f32>, peak: Option<f32>) -> f32 {
    let Some(db) = replay_gain.filter(|d| d.is_finite()) else { return 1.0 };
    let mut g = 10f32.powf(db / 20.0);
    if let Some(peak) = peak.filter(|p| p.is_finite() && *p > 0.0) {
        g = g.min(1.0 / peak);
    }
    g.clamp(0.05, 4.0)
}

/// Track gain (dB) and peak (linear) from a local file's ReplayGain tags
/// (`REPLAYGAIN_TRACK_GAIN` / `_PEAK`, as foobar2000, MusicBee or loudgain write them).
fn local_replay_gain(path: &std::path::Path) -> (Option<f32>, Option<f32>) {
    use lofty::{file::TaggedFileExt, tag::ItemKey};
    let Ok(file) = lofty::read_from_path(path) else { return (None, None) };
    let read = |key: ItemKey| file.tags().iter().find_map(|t| t.get_string(key).and_then(parse_replay_gain));
    (read(ItemKey::ReplayGainTrackGain), read(ItemKey::ReplayGainTrackPeak))
}

/// "-6.52 dB", "+1.2dB" or "0.988" to a number.
fn parse_replay_gain(text: &str) -> Option<f32> {
    let t = text.trim();
    let t = t.strip_suffix("dB").or_else(|| t.strip_suffix("db")).or_else(|| t.strip_suffix("DB")).unwrap_or(t);
    t.trim().trim_start_matches('+').parse::<f32>().ok().filter(|v| v.is_finite())
}

#[cfg(test)]
mod replay_gain_tests {
    use super::*;

    #[test]
    fn parses_the_usual_tag_spellings() {
        assert_eq!(parse_replay_gain("-6.52 dB"), Some(-6.52));
        assert_eq!(parse_replay_gain("+1.20dB"), Some(1.2));
        assert_eq!(parse_replay_gain(" 0.988 "), Some(0.988));
        assert_eq!(parse_replay_gain("loud"), None);
        assert_eq!(parse_replay_gain("NaN dB"), None);
    }

    #[test]
    fn gain_never_lifts_the_peak_past_full_scale() {
        assert_eq!(normalisation(None, Some(0.5)), 1.0);
        assert!((normalisation(Some(-6.0), None) - 0.501).abs() < 0.001);
        // +6 dB would be about 2.0, but a 0.8 peak only allows 1.25.
        assert!((normalisation(Some(6.0), Some(0.8)) - 1.25).abs() < 0.001);
    }
}
