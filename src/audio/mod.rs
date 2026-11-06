//! Playback engine.
//!
//! Each track is decoded on its own worker thread (so slow downloads never
//! block the engine); the engine thread resamples if needed and feeds a ring
//! buffer that the real-time output drains. The next track is queued ahead of
//! time and starts on the very next sample: gapless playback.

pub mod decode;
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

pub enum Cmd {
    /// Replace everything and play this track (from `start` seconds).
    Play { key: u64, info: StreamInfo, start: f64, paused: bool },
    /// Track to continue with when the current one ends (replaces the old one).
    Queue { key: u64, info: StreamInfo },
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
    Finished { key: u64 },
    Error { key: Option<u64>, message: String },
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
    buffer: Arc<Buffer>,
}

#[derive(Default)]
struct EngineView {
    marks: VecDeque<Mark>,
    output: String,
    output_rate: u32,
    buffering: bool,
    /// Key of the track that should be shown while nothing is audible yet.
    loading: Option<u64>,
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
            ..Default::default()
        };
        if let Some(m) = v.marks.front() {
            let elapsed = played.saturating_sub(m.start_frame) as f64 / rate;
            snap.key = Some(m.key);
            snap.position = (m.offset + elapsed).min(if m.duration > 0.0 { m.duration } else { f64::MAX });
            snap.duration = m.duration;
            snap.source = m.source;
            let (have, total, done) = m.buffer.downloaded();
            snap.downloaded = if done { 1.0 } else { total.map(|t| have as f32 / t.max(1) as f32).unwrap_or(0.0) };
        } else if let Some(k) = v.loading {
            snap.key = Some(k);
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
    rx: Receiver<DecMsg>,
    seek_tx: Sender<f64>,
    buffer: Arc<Buffer>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.buffer.cancel();
    }
}

fn spawn_worker(http: &Client, info: &StreamInfo) -> Worker {
    let mp4 = matches!(info.source, StreamSource::Segments(_)) || !info.codec.eq_ignore_ascii_case("flac");
    let buffer = stream::start(http.clone(), info.source.clone());
    let (tx, rx) = mpsc::sync_channel::<DecMsg>(24);
    let (seek_tx, seek_rx) = mpsc::channel::<f64>();
    let buf = buffer.clone();
    std::thread::Builder::new()
        .name("decode".into())
        .spawn(move || decode_loop(buf, mp4, tx, seek_rx))
        .expect("spawn decoder");
    Worker { rx, seek_tx, buffer }
}

fn decode_loop(buf: Arc<Buffer>, mp4: bool, tx: SyncSender<DecMsg>, seek_rx: Receiver<f64>) {
    let open = || Decoder::open(buf.clone(), mp4);
    let mut dec = match open() {
        Ok(d) => d,
        Err(e) => {
            if !buf.is_cancelled() {
                let _ = tx.send(DecMsg::Error(format!("{e:#}")));
            }
            return;
        }
    };
    if tx
        .send(DecMsg::Format { rate: dec.sample_rate, bits: dec.bits_per_sample, duration: dec.duration })
        .is_err()
    {
        return;
    }
    loop {
        // Take only the newest seek request.
        let mut target = None;
        while let Ok(t) = seek_rx.try_recv() {
            target = Some(t);
        }
        if let Some(t) = target {
            let reached = match dec.seek(t) {
                Ok(at) => at,
                Err(e) => {
                    // Not seekable (fragmented MP4 still downloading): decode
                    // from the start and throw samples away up to the target.
                    log::debug!("seek fallback: {e:#}");
                    match open() {
                        Ok(mut d) => {
                            let skip = (t * d.sample_rate as f64) as usize * 2;
                            let mut done = 0usize;
                            while done < skip {
                                match d.next_chunk() {
                                    Ok(Some(c)) => done += c.len(),
                                    _ => break,
                                }
                            }
                            dec = d;
                            done as f64 / 2.0 / dec.sample_rate as f64
                        }
                        Err(e) => {
                            let _ = tx.send(DecMsg::Error(format!("{e:#}")));
                            return;
                        }
                    }
                }
            };
            if tx.send(DecMsg::Seeked(reached)).is_err() {
                return;
            }
        }
        match dec.next_chunk() {
            Ok(Some(chunk)) => {
                if tx.send(DecMsg::Chunk(chunk)).is_err() {
                    return;
                }
            }
            Ok(None) => {
                if tx.send(DecMsg::End).is_err() {
                    return;
                }
                // Stay alive: the user may still seek back into this track.
                match seek_rx.recv() {
                    Ok(t) => {
                        let reached = dec.seek(t).unwrap_or(0.0);
                        if tx.send(DecMsg::Seeked(reached)).is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
            Err(e) => {
                if !buf.is_cancelled() {
                    let _ = tx.send(DecMsg::Error(format!("{e:#}")));
                }
                return;
            }
        }
    }
}

// --------------------------------------------------------------------- engine

struct Track {
    key: u64,
    info: StreamInfo,
    worker: Worker,
    rate: Option<u32>,
    bits: u32,
    duration: f64,
    resampler: Option<Resampler2>,
    /// Waiting for `Seeked`; chunks before it are stale.
    seeking: bool,
    /// Seconds into the track where the next pushed sample belongs.
    start_offset: f64,
    marked: bool,
    ended: bool,
    /// A chunk waiting for the output device to (re)open.
    held: Option<Vec<f32>>,
    /// Loudness normalisation gain applied to every sample.
    norm: f32,
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
            Cmd::Play { key, info, start, paused } => {
                self.stop_all();
                self.paused = paused;
                self.out_shared.paused.store(paused, Ordering::Relaxed);
                let mut t = self.make_track(key, info);
                if start > 0.5 {
                    let _ = t.worker.seek_tx.send(start);
                    t.seeking = true;
                }
                self.cur = Some(t);
                let mut v = self.view.lock();
                v.loading = Some(key);
                v.buffering = true;
            }
            Cmd::Queue { key, info } => {
                if self.next.as_ref().map(|n| n.key) != Some(key) {
                    self.next = Some(self.make_track(key, info));
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
                let exclusive_fixed = self.prefs.exclusive && self.prefs.force_volume;
                self.out_shared.set_gain(if exclusive_fixed { 1.0 } else { gain_for(v) });
            }
            Cmd::Output(prefs) => {
                self.prefs = prefs;
                let exclusive_fixed = self.prefs.exclusive && self.prefs.force_volume;
                self.out_shared.set_gain(if exclusive_fixed { 1.0 } else { gain_for(self.volume) });
                // Re-open at the current position.
                let pos = self.position_of_current();
                self.out = None;
                if let Some(mut t) = self.cur.take() {
                    self.reset_timeline();
                    t.resampler = None;
                    t.marked = false;
                    t.ended = false;
                    let _ = t.worker.seek_tx.send(pos);
                    t.seeking = true;
                    self.drain_stale(&mut t);
                    self.cur = Some(t);
                } else {
                    self.reset_timeline();
                }
            }
        }
    }

    /// Linear gain for loudness normalisation (1.0 = untouched).
    fn norm_gain(&self, info: &StreamInfo) -> f32 {
        let bit_perfect = self.prefs.exclusive && self.prefs.force_volume;
        if !self.prefs.normalize || bit_perfect {
            return 1.0;
        }
        let Some(db) = info.replay_gain else { return 1.0 };
        let mut g = 10f32.powf(db / 20.0);
        // Never push the track's peak past full scale.
        if let Some(peak) = info.peak.filter(|p| *p > 0.0) {
            g = g.min(1.0 / peak);
        }
        g.clamp(0.05, 4.0)
    }

    fn make_track(&self, key: u64, info: StreamInfo) -> Track {
        let worker = spawn_worker(&self.http, &info);
        let norm = self.norm_gain(&info);
        Track {
            norm,
            key,
            bits: info.bit_depth.unwrap_or(16),
            info,
            worker,
            rate: None,
            duration: 0.0,
            resampler: None,
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
        // If decoding already moved on to the next track, seek in the one
        // that is audible instead.
        let audible = self.snapshot_key();
        if audible.is_some() && audible != Some(t.key) {
            // Rare: within the last ~300 ms of a track. Just restart the next track.
            self.cur = Some(t);
            return;
        }
        self.flush_output();
        self.reset_timeline();
        let _ = t.worker.seek_tx.send(secs.max(0.0));
        t.seeking = true;
        t.ended = false;
        t.marked = false;
        t.resampler = None;
        self.drain_stale(&mut t);
        self.view.lock().loading = Some(t.key);
        self.view.lock().buffering = true;
        self.cur = Some(t);
    }

    fn snapshot_key(&self) -> Option<u64> {
        let played = self.out_shared.played.load(Ordering::Acquire);
        let v = self.view.lock();
        v.marks.iter().rev().find(|m| m.start_frame <= played).map(|m| m.key)
    }

    /// Throw away chunks the worker produced before the seek.
    fn drain_stale(&self, t: &mut Track) {
        while let Ok(msg) = t.worker.rx.try_recv() {
            match msg {
                DecMsg::Format { rate, bits, duration } => {
                    t.rate = Some(rate);
                    t.bits = bits.unwrap_or(t.bits);
                    t.duration = duration.unwrap_or(t.duration);
                }
                DecMsg::Seeked(at) => {
                    t.seeking = false;
                    t.start_offset = at;
                    break;
                }
                _ => {}
            }
        }
    }

    fn ensure_output(&mut self, rate: u32, bits: u32) -> OutState {
        let wanted_exclusive = self.prefs.exclusive;
        if let Some(o) = &self.out {
            if !wanted_exclusive || o.rate == rate || !o.exclusive {
                return OutState::Ready;
            }
            // Exclusive mode at a different rate: wait for the ring to drain.
            if self.out_shared.played.load(Ordering::Acquire) < self.pushed {
                return OutState::Wait;
            }
            self.out = None;
        }
        let opened = if wanted_exclusive {
            let gain = !self.prefs.force_volume;
            output::open_exclusive(self.prefs.device.as_deref(), rate, bits, self.out_shared.clone(), gain).or_else(|e| {
                let _ = self.ev.send(Event::Error {
                    key: None,
                    message: format!("Modo exclusivo no disponible ({e:#}); usando modo compartido"),
                });
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
                // Marks refer to the old output's timeline.
                let played = self.pushed;
                for m in v.marks.iter_mut() {
                    m.start_frame = played;
                }
                self.out = Some(o);
                OutState::Ready
            }
            Err(e) => {
                let _ = self.ev.send(Event::Error { key: None, message: format!("No se pudo abrir la salida de audio: {e:#}") });
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
        let msg = match held.map(|h| Ok(DecMsg::Chunk(h))).unwrap_or_else(|| t.worker.rx.try_recv()) {
            Ok(m) => m,
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
                if t.norm != 1.0 {
                    for s in &mut samples {
                        *s *= t.norm;
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
