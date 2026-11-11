//! Streaming download with a blocking `Read + Seek` view, so decoding starts as
//! soon as the first bytes arrive.
//!
//! DASH streams are fetched with several parallel connections (appended in
//! order) into a [`SegmentWindow`]: every segment is stored on its own, the
//! init segment is kept for the whole track, segments the reader has moved
//! past are dropped (two are kept behind it), and downloads pause once they
//! are [`AHEAD`] segments in front of the reader. Memory for a track is
//! therefore bounded by the window, not by the length of the track. A stream
//! can be *restarted* at any segment, which makes seeking instant.
//!
//! Signed segment URLs expire (TIDAL's CDN: about an hour). When a segment is
//! refused with 401/403/410 the buffer asks its [`Renew`] callback for a fresh
//! manifest and continues at the same segment. Segments the reader needs again
//! after they were dropped are fetched back on demand. On slow connections the
//! window grows to [`AHEAD_SLOW`] segments.
//!
//! Single progressive files (`StreamSource::Single`) are downloaded whole into
//! one contiguous buffer, allocated once from the Content-Length.

use std::collections::VecDeque;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use reqwest::blocking::Client;

use crate::tidal::StreamSource;

/// At most this many segment downloads run at once.
const PARALLEL_SEGMENTS: usize = 4;
/// Media segments kept behind the one the reader is in (demuxers may step
/// back a little, e.g. to re-read a fragment header).
const BEHIND: usize = 2;
/// Media segments that may be downloaded ahead of the one the reader is in
/// (about 60 s with TIDAL's 4 s segments).
const AHEAD: usize = 15;
/// The window used once a segment took longer than half its play time to
/// arrive (about 2 minutes of audio).
const AHEAD_SLOW: usize = 30;
/// How long a segment keeps being retried through network errors, in units of
/// the base retry delay (400 ms in the app: about 2 minutes, during which the
/// player shows "buffering"). The delay doubles up to 12 units (~5 s).
const RETRY_SPAN: u32 = 300;
const RETRY_MAX_STEP: u32 = 12;
/// HTTP errors from the server (4xx other than expiry) are not network
/// trouble: give up after this many attempts.
const CLIENT_ERROR_TRIES: u32 = 3;

/// Asks the service for a fresh copy of the stream (new signed URLs).
pub type Renew = Arc<dyn Fn() -> Result<StreamSource, String> + Send + Sync>;

/// Errors that mean "this URL is no longer valid", not "the network failed".
fn is_expired(e: &str) -> bool {
    ["(401", "(403", "(410"].iter().any(|c| e.contains(c))
}

/// Fetches one URL completely. A trait so the window can be tested offline.
pub trait Fetch: Send + Sync {
    fn get(&self, url: &str) -> Result<Vec<u8>, String>;
}

struct HttpFetch(Client);

impl Fetch for HttpFetch {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let resp = self.0.get(url).timeout(Duration::from_secs(60)).send().map_err(|e| e.without_url().to_string())?;
        if !resp.status().is_success() {
            return Err(format!("download failed ({})", resp.status()));
        }
        resp.bytes().map(|x| x.to_vec()).map_err(|e| e.without_url().to_string())
    }
}

/// One downloaded DASH piece. Piece 0 is the init segment; piece `n >= 1` is
/// the `n`-th media segment of the current generation.
struct Piece {
    index: usize,
    /// Logical byte offset of the piece in the stream the reader sees.
    start: u64,
    data: Arc<[u8]>,
}

/// Where a logical byte position is right now.
enum Locate {
    Data {
        piece: usize,
        bytes: Arc<[u8]>,
        offset: usize,
    },
    /// Not downloaded yet.
    Pending,
    /// Was downloaded and has been dropped.
    Evicted,
}

/// Bounded storage of a DASH stream: the init segment plus a sliding window of
/// media segments around the reader.
#[derive(Default)]
struct SegmentWindow {
    init: Option<Arc<[u8]>>,
    segs: VecDeque<Piece>,
    /// Logical length of everything appended so far (dropped pieces included).
    end: u64,
    /// Index the next appended piece gets.
    next_piece: usize,
    /// Piece the reader is in.
    read_piece: usize,
    /// Logical start of every piece appended so far (to find dropped ones).
    starts: Vec<u64>,
    /// A dropped piece fetched again because the reader went back to it.
    recovered: Option<Piece>,
    /// Bytes currently held, and the most ever held at once.
    retained: usize,
    peak: usize,
}

impl SegmentWindow {
    fn push(&mut self, data: Arc<[u8]>) {
        let (index, start) = (self.next_piece, self.end);
        self.starts.push(start);
        self.end += data.len() as u64;
        self.next_piece += 1;
        self.retained += data.len();
        self.peak = self.peak.max(self.retained);
        if index == 0 {
            self.init = Some(data);
        } else {
            self.segs.push_back(Piece { index, start, data });
        }
    }

    fn locate(&self, pos: u64) -> Locate {
        if let Some(init) = &self.init {
            if pos < init.len() as u64 {
                return Locate::Data { piece: 0, bytes: init.clone(), offset: pos as usize };
            }
        }
        if pos >= self.end {
            return Locate::Pending;
        }
        if let Some(p) = &self.recovered {
            if pos >= p.start && pos < p.start + p.data.len() as u64 {
                return Locate::Data { piece: p.index, bytes: p.data.clone(), offset: (pos - p.start) as usize };
            }
        }
        match self.segs.front() {
            Some(first) if pos >= first.start => self
                .segs
                .iter()
                .find(|p| pos < p.start + p.data.len() as u64)
                .map(|p| Locate::Data { piece: p.index, bytes: p.data.clone(), offset: (pos - p.start) as usize })
                .unwrap_or(Locate::Pending),
            _ => Locate::Evicted,
        }
    }

    /// Index of the (dropped) piece that contains `pos`.
    fn piece_at(&self, pos: u64) -> Option<usize> {
        match self.starts.binary_search(&pos) {
            Ok(i) => Some(i),
            Err(0) => None,
            Err(i) => Some(i - 1),
        }
    }

    /// The reader is now in `piece`: drop media pieces more than [`BEHIND`]
    /// before it. Returns whether downloads may now go further ahead.
    fn consume(&mut self, piece: usize) -> bool {
        let advanced = piece > self.read_piece;
        self.read_piece = piece;
        if self.recovered.as_ref().is_some_and(|p| p.index != piece) {
            self.recovered = None;
        }
        while let Some(front) = self.segs.front() {
            if front.index + BEHIND >= piece {
                break;
            }
            self.retained -= front.data.len();
            self.segs.pop_front();
        }
        advanced
    }

    /// Backpressure: may piece `index` be downloaded now?
    fn may_fetch(&self, index: usize, ahead: usize) -> bool {
        index <= self.read_piece + ahead
    }
}

enum Store {
    /// Single progressive file, kept whole.
    Bytes(Vec<u8>),
    /// DASH segments.
    Window(SegmentWindow),
}

impl Default for Store {
    fn default() -> Self {
        Store::Bytes(Vec::new())
    }
}

impl Store {
    fn len(&self) -> u64 {
        match self {
            Store::Bytes(d) => d.len() as u64,
            Store::Window(w) => w.end,
        }
    }
}

#[derive(Default)]
struct State {
    store: Store,
    /// Known total size (progressive files with a Content-Length).
    total: Option<u64>,
    done: bool,
    error: Option<String>,
    /// Bumped on every restart so stale download threads stop writing.
    generation: u64,
    /// Number of pieces already appended (segment downloads keep order).
    appended: usize,
    /// DASH: media segment the current generation started at.
    from_seg: usize,
}

pub struct Buffer {
    state: Mutex<State>,
    ready: Condvar,
    cancelled: AtomicBool,
    http: Client,
    fetcher: Arc<dyn Fetch>,
    retry_delay: Duration,
    source: StreamSource,
    seg_secs: Vec<f64>,
    /// DASH: current segment URLs (init first); replaced when they expire.
    urls: Mutex<Arc<Vec<String>>>,
    renew: Option<Renew>,
    /// When the URLs were last renewed (parallel workers renew only once).
    renewed: Mutex<Option<Instant>>,
    /// Current backpressure window (grows on slow connections).
    ahead: AtomicUsize,
}

impl Buffer {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.ready.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn live(&self, generation: u64) -> bool {
        !self.is_cancelled() && self.state.lock().generation == generation
    }

    pub fn total_len(&self) -> Option<u64> {
        let s = self.state.lock();
        if s.done {
            Some(s.store.len())
        } else {
            s.total
        }
    }

    /// Bytes downloaded so far (for DASH including segments already dropped),
    /// the total size when known, and whether the download is complete.
    pub fn downloaded(&self) -> (u64, Option<u64>, bool) {
        let s = self.state.lock();
        (s.store.len(), s.total, s.done)
    }

    /// DASH streams with known segment lengths can jump to any position.
    pub fn can_jump(&self) -> bool {
        matches!(self.source, StreamSource::Segments(_)) && !self.seg_secs.is_empty()
    }

    /// Segment index containing `secs` and the time at which it starts.
    pub fn segment_for(&self, secs: f64) -> Option<(usize, f64)> {
        if !self.can_jump() {
            return None;
        }
        let mut start = 0.0;
        for (i, d) in self.seg_secs.iter().enumerate() {
            if secs < start + d || i + 1 == self.seg_secs.len() {
                return Some((i, start));
            }
            start += d;
        }
        None
    }

    /// Drop everything and download again from media segment `seg`
    /// (the init segment is always kept first). Only for DASH streams.
    pub fn restart_at(self: &Arc<Self>, seg: usize) {
        {
            let mut s = self.state.lock();
            s.generation += 1;
            match &mut s.store {
                Store::Bytes(d) => d.clear(),
                // Release the old window's memory right away.
                Store::Window(w) => *w = SegmentWindow::default(),
            }
            s.total = None;
            s.done = false;
            s.error = None;
            s.appended = 0;
            s.from_seg = seg;
        }
        self.ready.notify_all();
        self.begin(seg);
    }

    fn append(&self, generation: u64, chunk: &[u8]) {
        let mut s = self.state.lock();
        if s.generation == generation {
            if let Store::Bytes(d) = &mut s.store {
                d.extend_from_slice(chunk);
            }
        }
        drop(s);
        self.ready.notify_all();
    }

    fn finish(&self, generation: u64, error: Option<String>) {
        let mut s = self.state.lock();
        if s.generation != generation {
            return;
        }
        s.done = true;
        s.error = error;
        drop(s);
        self.ready.notify_all();
    }

    fn begin(self: &Arc<Self>, from_seg: usize) {
        let generation = self.state.lock().generation;
        match &self.source {
            StreamSource::Single(url) => {
                let (b, url) = (self.clone(), url.clone());
                std::thread::Builder::new()
                    .name("download".into())
                    .spawn(move || {
                        let r = fetch(&b.http, &url, &b, generation);
                        b.finish(generation, r.err().filter(|_| b.live(generation)));
                    })
                    .expect("spawn download thread");
            }
            StreamSource::Segments(urls) => {
                // The init segment, then media segments from `from_seg`.
                let pieces = urls.len().saturating_sub(from_seg).max(1);
                let next = Arc::new(AtomicUsize::new(0));
                for n in 0..PARALLEL_SEGMENTS.min(pieces) {
                    let (b, next) = (self.clone(), next.clone());
                    std::thread::Builder::new()
                        .name(format!("download-{n}"))
                        .spawn(move || segment_worker(b, generation, from_seg, pieces, next))
                        .expect("spawn download thread");
                }
            }
        }
    }

    /// URL of piece `i` of a generation that started at media segment `from_seg`.
    fn url_for(&self, from_seg: usize, i: usize) -> Option<String> {
        let urls = self.urls.lock().clone();
        urls.get(if i == 0 { 0 } else { from_seg + i }).cloned()
    }

    /// Replace expired URLs with fresh ones from the service. Several workers
    /// hit the expiry at once; only the first renews, the others reuse it.
    fn renew_urls(&self) -> Result<(), String> {
        let Some(renew) = &self.renew else { return Err("stream link expired".into()) };
        let mut last = self.renewed.lock();
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) {
            return Ok(());
        }
        let StreamSource::Segments(fresh) = renew()? else { return Err("stream format changed".into()) };
        if fresh.len() != self.urls.lock().len() {
            return Err("stream layout changed".into());
        }
        log::info!("stream links renewed");
        *self.urls.lock() = Arc::new(fresh);
        *last = Some(Instant::now());
        Ok(())
    }

    /// Length in seconds of one media segment (for the slow-network check).
    fn seg_len(&self) -> f64 {
        self.seg_secs.first().copied().filter(|d| *d > 0.0).unwrap_or(4.0)
    }

    #[cfg(test)]
    fn window_stats(&self) -> (usize, usize, Vec<usize>) {
        match &self.state.lock().store {
            Store::Window(w) => {
                let mut held: Vec<usize> = w.init.iter().map(|_| 0).collect();
                held.extend(w.segs.iter().map(|p| p.index));
                (w.retained, w.peak, held)
            }
            Store::Bytes(d) => (d.len(), d.capacity(), Vec::new()),
        }
    }
}

fn segment_worker(b: Arc<Buffer>, generation: u64, from_seg: usize, pieces: usize, next: Arc<AtomicUsize>) {
    loop {
        let i = next.fetch_add(1, Ordering::SeqCst);
        if i >= pieces || !b.live(generation) {
            return;
        }
        // Backpressure: wait until the reader is close enough to piece `i`.
        {
            let mut s = b.state.lock();
            while s.generation == generation && !b.is_cancelled() {
                match &s.store {
                    Store::Window(w) if !w.may_fetch(i, b.ahead.load(Ordering::Relaxed)) => {
                        b.ready.wait_for(&mut s, Duration::from_millis(100));
                    }
                    _ => break,
                }
            }
            if s.generation != generation || b.is_cancelled() {
                return;
            }
        }
        let t0 = Instant::now();
        let bytes = match get_piece(&b, generation, from_seg, i) {
            Ok(v) => v,
            Err(e) => {
                b.finish(generation, Some(e).filter(|_| b.live(generation)));
                return;
            }
        };
        // Append in order.
        let mut s = b.state.lock();
        while s.generation == generation && s.appended != i && !b.is_cancelled() {
            b.ready.wait_for(&mut s, Duration::from_millis(100));
        }
        if s.generation != generation || b.is_cancelled() {
            return;
        }
        if i > 0 && t0.elapsed().as_secs_f64() > b.seg_len() * 0.5 && b.ahead.load(Ordering::Relaxed) < AHEAD_SLOW {
            log::info!("slow connection: buffering further ahead");
            b.ahead.store(AHEAD_SLOW, Ordering::Relaxed);
        }
        if let Store::Window(w) = &mut s.store {
            w.push(Arc::from(bytes));
        }
        s.appended += 1;
        let last = s.appended == pieces;
        drop(s);
        b.ready.notify_all();
        if last {
            b.finish(generation, None);
        }
    }
}

/// Download piece `i`, renewing expired links and retrying network errors
/// with a growing delay for up to [`RETRY_SPAN`] (about 2 minutes).
fn get_piece(b: &Buffer, generation: u64, from_seg: usize, i: usize) -> Result<Vec<u8>, String> {
    let (mut tries, mut renewals) = (0u32, 0u32);
    let deadline = Instant::now() + b.retry_delay * RETRY_SPAN;
    loop {
        let url = b.url_for(from_seg, i).ok_or("segment out of range")?;
        let err = match b.fetcher.get(&url) {
            Ok(v) => return Ok(v),
            Err(e) => e,
        };
        if !b.live(generation) {
            return Err(err);
        }
        if is_expired(&err) && renewals < 2 {
            renewals += 1;
            b.renew_urls()?;
            continue;
        }
        tries += 1;
        let client_error = err.contains("download failed (4");
        if (client_error && tries >= CLIENT_ERROR_TRIES) || Instant::now() >= deadline {
            return Err(err);
        }
        if tries == 1 || tries % 5 == 0 {
            log::warn!("retrying download (attempt {tries}): {err}");
        }
        std::thread::sleep((b.retry_delay * 2u32.pow((tries - 1).min(4))).min(b.retry_delay * RETRY_MAX_STEP));
    }
}

/// Start downloading `source`. `seg_secs` are the DASH segment lengths (may
/// be empty); `renew` fetches fresh links when the signed ones expire.
pub fn start(http: Client, source: StreamSource, seg_secs: Vec<f64>, renew: Option<Renew>) -> Arc<Buffer> {
    let fetcher: Arc<dyn Fetch> = Arc::new(HttpFetch(http.clone()));
    start_with(http, fetcher, Duration::from_millis(400), source, seg_secs, renew)
}

fn start_with(
    http: Client,
    fetcher: Arc<dyn Fetch>,
    retry_delay: Duration,
    source: StreamSource,
    seg_secs: Vec<f64>,
    renew: Option<Renew>,
) -> Arc<Buffer> {
    let (store, urls) = match &source {
        StreamSource::Single(_) => (Store::Bytes(Vec::new()), Vec::new()),
        StreamSource::Segments(u) => (Store::Window(SegmentWindow::default()), u.clone()),
    };
    let buf = Arc::new(Buffer {
        state: Mutex::new(State { store, ..State::default() }),
        ready: Condvar::new(),
        cancelled: AtomicBool::new(false),
        http,
        fetcher,
        retry_delay,
        source,
        seg_secs,
        urls: Mutex::new(Arc::new(urls)),
        renew,
        renewed: Mutex::new(None),
        ahead: AtomicUsize::new(AHEAD),
    });
    buf.begin(0);
    buf
}

fn fetch(http: &Client, url: &str, buf: &Buffer, generation: u64) -> Result<(), String> {
    let mut tries = 0;
    loop {
        tries += 1;
        let start_len = buf.state.lock().store.len() as usize;
        let attempt = (|| -> Result<(), String> {
            let mut resp = http.get(url).timeout(Duration::from_secs(120)).send().map_err(|e| e.without_url().to_string())?;
            if !resp.status().is_success() {
                return Err(format!("download failed ({})", resp.status()));
            }
            if let Some(len) = resp.content_length() {
                let mut s = buf.state.lock();
                s.total = Some(len);
                // One exact allocation instead of repeated doubling.
                if let Store::Bytes(d) = &mut s.store {
                    d.reserve_exact((len as usize).saturating_sub(d.len()));
                }
            }
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                if !buf.live(generation) {
                    return Ok(());
                }
                let n = resp.read(&mut chunk).map_err(|e| e.to_string())?;
                if n == 0 {
                    return Ok(());
                }
                buf.append(generation, &chunk[..n]);
            }
        })();
        match attempt {
            Ok(()) => return Ok(()),
            Err(e) if tries < 3 && buf.live(generation) => {
                log::warn!("retrying download: {e}");
                // Drop the partial piece and fetch it again.
                if let Store::Bytes(d) = &mut buf.state.lock().store {
                    d.truncate(start_len);
                }
                std::thread::sleep(Duration::from_millis(400 * tries));
            }
            Err(e) => return Err(e),
        }
    }
}

/// A reader over a [`Buffer`]; blocks until the requested bytes are there.
pub struct Reader {
    buf: Arc<Buffer>,
    pos: u64,
    seekable: bool,
}

impl Reader {
    /// `seekable: false` makes demuxers read strictly front to back (used for
    /// fragmented MP4 so playback starts without scanning the whole file).
    pub fn new(buf: Arc<Buffer>, seekable: bool) -> Self {
        Self { buf, pos: 0, seekable }
    }
}

impl Read for Reader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut s = self.buf.state.lock();
        loop {
            if self.buf.is_cancelled() {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            match &mut s.store {
                Store::Bytes(data) => {
                    let len = data.len() as u64;
                    if self.pos < len {
                        let n = ((len - self.pos) as usize).min(out.len());
                        let start = self.pos as usize;
                        out[..n].copy_from_slice(&data[start..start + n]);
                        self.pos += n as u64;
                        return Ok(n);
                    }
                }
                Store::Window(w) => match w.locate(self.pos) {
                    Locate::Data { piece, bytes, offset } => {
                        let advanced = w.consume(piece);
                        // Copy outside the lock; the Arc keeps the piece alive.
                        drop(s);
                        if advanced {
                            self.buf.ready.notify_all();
                        }
                        let n = (bytes.len() - offset).min(out.len());
                        out[..n].copy_from_slice(&bytes[offset..offset + n]);
                        self.pos += n as u64;
                        return Ok(n);
                    }
                    Locate::Evicted => {
                        // The demuxer went back past the window: fetch that
                        // piece again (outside the lock) and serve it.
                        let Some(piece) = w.piece_at(self.pos) else {
                            return Err(io::Error::other("stream position not found"));
                        };
                        let start = w.starts[piece];
                        let (generation, from_seg) = (s.generation, s.from_seg);
                        drop(s);
                        let bytes = get_piece(&self.buf, generation, from_seg, piece).map_err(io::Error::other)?;
                        s = self.buf.state.lock();
                        if s.generation != generation {
                            continue;
                        }
                        if let Store::Window(w) = &mut s.store {
                            w.recovered = Some(Piece { index: piece, start, data: Arc::from(bytes) });
                        }
                        continue;
                    }
                    Locate::Pending => {}
                },
            }
            if s.done {
                if let Some(e) = &s.error {
                    return Err(io::Error::other(e.clone()));
                }
                return Ok(0);
            }
            self.buf.ready.wait_for(&mut s, Duration::from_millis(250));
        }
    }
}

impl Seek for Reader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let new = match to {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) => {
                // Wait for the size to be known.
                let mut s = self.buf.state.lock();
                let total = loop {
                    if let Some(t) = s.total {
                        break t;
                    }
                    if s.done {
                        break s.store.len();
                    }
                    if self.buf.is_cancelled() {
                        return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
                    }
                    self.buf.ready.wait_for(&mut s, Duration::from_millis(250));
                };
                total as i64 + d
            }
        };
        if new < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = new as u64;
        Ok(self.pos)
    }
}

impl symphonia::core::io::MediaSource for Reader {
    fn is_seekable(&self) -> bool {
        self.seekable
    }

    fn byte_len(&self) -> Option<u64> {
        if self.seekable {
            self.buf.total_len()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::time::Instant;

    /// Offline stand-in for the CDN: `seg://<i>` returns deterministic bytes
    /// for piece `i`, optionally slowly or failing.
    struct FakeFetch {
        sizes: Vec<usize>,
        delay: Duration,
        fail_times: Mutex<HashMap<usize, usize>>,
        fail_always: Option<usize>,
        in_flight: AtomicUsize,
        max_in_flight: AtomicUsize,
        requests: Mutex<Vec<usize>>,
        /// Links older than this version answer 403 (expired signature).
        min_version: AtomicUsize,
        /// Every request fails until this moment (network outage).
        outage_until: Mutex<Option<Instant>>,
    }

    fn byte(piece: usize, j: usize) -> u8 {
        ((piece * 7919 + j * 31 + (j >> 8)) % 251) as u8
    }

    fn piece_bytes(piece: usize, len: usize) -> Vec<u8> {
        (0..len).map(|j| byte(piece, j)).collect()
    }

    impl Fetch for FakeFetch {
        fn get(&self, url: &str) -> Result<Vec<u8>, String> {
            let rest = url.trim_start_matches("seg://v");
            let (ver, i) = rest.split_once('/').unwrap();
            let (ver, i): (usize, usize) = (ver.parse().unwrap(), i.parse().unwrap());
            if ver < self.min_version.load(Ordering::SeqCst) {
                return Err("download failed (403 Forbidden)".into());
            }
            if self.outage_until.lock().is_some_and(|t| Instant::now() < t) {
                return Err("error sending request: connection reset".into());
            }
            self.requests.lock().push(i);
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            if self.fail_always == Some(i) {
                return Err("simulated permanent failure".into());
            }
            if let Some(left) = self.fail_times.lock().get_mut(&i) {
                if *left > 0 {
                    *left -= 1;
                    return Err("simulated network error".into());
                }
            }
            Ok(piece_bytes(i, self.sizes[i]))
        }
    }

    struct Setup {
        sizes: Vec<usize>,
        delay: Duration,
        fail_times: HashMap<usize, usize>,
        fail_always: Option<usize>,
        seg_secs: f64,
        retry: Duration,
    }

    fn urls(n: usize, ver: usize) -> Vec<String> {
        (0..n).map(|i| format!("seg://v{ver}/{i}")).collect()
    }

    impl Setup {
        /// `media` segments of about `size` bytes (varying) plus a small init segment.
        fn new(media: usize, size: usize) -> Self {
            let mut sizes = vec![619];
            sizes.extend((0..media).map(|i| size + (i * 37) % 101));
            Setup {
                sizes,
                delay: Duration::ZERO,
                fail_times: HashMap::new(),
                fail_always: None,
                seg_secs: 4.0,
                retry: Duration::from_millis(1),
            }
        }

        fn start(self) -> (Arc<Buffer>, Arc<FakeFetch>) {
            self.start_renewing(false)
        }

        /// `renewing`: links can be renewed; each renewal hands out the
        /// fake's current version.
        fn start_renewing(self, renewing: bool) -> (Arc<Buffer>, Arc<FakeFetch>) {
            let n = self.sizes.len();
            let secs = vec![self.seg_secs; n - 1];
            let fake = Arc::new(FakeFetch {
                sizes: self.sizes,
                delay: self.delay,
                fail_times: Mutex::new(self.fail_times),
                fail_always: self.fail_always,
                in_flight: AtomicUsize::new(0),
                max_in_flight: AtomicUsize::new(0),
                requests: Mutex::new(Vec::new()),
                min_version: AtomicUsize::new(0),
                outage_until: Mutex::new(None),
            });
            let renew: Option<Renew> = renewing.then(|| {
                let f = fake.clone();
                Arc::new(move || Ok(StreamSource::Segments(urls(n, f.min_version.load(Ordering::SeqCst))))) as Renew
            });
            let buf = start_with(Client::new(), fake.clone(), self.retry, StreamSource::Segments(urls(n, 0)), secs, renew);
            (buf, fake)
        }
    }

    /// Everything the decoder should see: init, then media from `media_from`.
    fn expected(fake: &FakeFetch, media_from: usize) -> Vec<u8> {
        let mut v = piece_bytes(0, fake.sizes[0]);
        for i in media_from..fake.sizes.len() {
            v.extend(piece_bytes(i, fake.sizes[i]));
        }
        v
    }

    /// Read to the end in odd-sized chunks on another thread, with a deadline
    /// so a deadlock fails the test instead of hanging it.
    fn read_all(buf: &Arc<Buffer>, pause: Duration) -> io::Result<Vec<u8>> {
        let b = buf.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut r = Reader::new(b, false);
            let mut out = Vec::new();
            let mut chunk = vec![0u8; 777];
            let res = loop {
                match r.read(&mut chunk) {
                    Ok(0) => break Ok(out),
                    Ok(n) => out.extend_from_slice(&chunk[..n]),
                    Err(e) => break Err(e),
                }
                if !pause.is_zero() {
                    std::thread::sleep(pause);
                }
            };
            let _ = tx.send(res);
        });
        rx.recv_timeout(Duration::from_secs(30)).expect("reader deadlocked")
    }

    fn wait_until(mut cond: impl FnMut() -> bool) -> bool {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(10) {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// Logical offset where piece `i` starts (before any restart).
    fn offset_of(fake: &FakeFetch, i: usize) -> u64 {
        fake.sizes[..i].iter().map(|&s| s as u64).sum()
    }

    /// Advance `r` to logical position `target`, reading in small chunks.
    fn read_to(r: &mut Reader, target: u64) {
        let mut sink = vec![0u8; 333];
        while r.pos < target {
            let n = (target - r.pos).min(sink.len() as u64) as usize;
            let got = r.read(&mut sink[..n]).unwrap();
            assert!(got > 0, "unexpected end of stream");
        }
    }

    #[test]
    fn delivers_exact_bytes_in_order() {
        let mut s = Setup::new(60, 3000);
        s.delay = Duration::from_millis(2);
        let (buf, fake) = s.start();
        let got = read_all(&buf, Duration::ZERO).unwrap();
        assert_eq!(got.len(), expected(&fake, 1).len());
        assert!(got == expected(&fake, 1), "bytes differ from the source");
        assert_eq!(buf.total_len(), Some(got.len() as u64));
    }

    #[test]
    fn memory_stays_bounded() {
        // 200 x 64 KiB = 12.5 MiB of stream; the window may hold only
        // init + BEHIND + current + AHEAD segments.
        let (buf, fake) = Setup::new(200, 64 * 1024).start();
        let got = read_all(&buf, Duration::from_micros(200)).unwrap();
        assert!(got == expected(&fake, 1));
        let (_, peak, _) = buf.window_stats();
        let largest = *fake.sizes[1..].iter().max().unwrap();
        let bound = fake.sizes[0] + (BEHIND + 1 + AHEAD) * largest;
        let total: usize = fake.sizes.iter().sum();
        assert!(peak <= bound, "peak {peak} > bound {bound}");
        assert!(peak * 10 < total, "peak {peak} is not far below the stream size {total}");
    }

    #[test]
    fn evicts_consumed_segments_keeping_two_behind() {
        let (buf, fake) = Setup::new(80, 2000).start();
        let mut r = Reader::new(buf.clone(), false);
        read_to(&mut r, offset_of(&fake, 10) + 500);
        assert!(wait_until(|| buf.window_stats().2.last() == Some(&(10 + AHEAD))));
        let (_, _, held) = buf.window_stats();
        assert_eq!(held[0], 0, "init segment must stay");
        assert_eq!(held[1], 10 - BEHIND, "oldest kept media piece");
        assert_eq!(*held.last().unwrap(), 10 + AHEAD, "downloads stop AHEAD pieces in front");
        assert_eq!(held.len(), 1 + BEHIND + 1 + AHEAD);
    }

    #[test]
    fn needed_segments_stay_readable() {
        let (buf, fake) = Setup::new(40, 1500).start();
        let mut r = Reader::new(buf.clone(), false);
        read_to(&mut r, offset_of(&fake, 10) + 10);
        // Two pieces behind are still there, byte-exact.
        let mut b = vec![0u8; 100];
        r.seek(SeekFrom::Start(offset_of(&fake, 8))).unwrap();
        r.read_exact(&mut b).unwrap();
        assert_eq!(b, piece_bytes(8, 100));
        // The init segment is always there.
        r.seek(SeekFrom::Start(0)).unwrap();
        r.read_exact(&mut b).unwrap();
        assert_eq!(b, piece_bytes(0, 100));
        // Older pieces were dropped; going back fetches them again, byte-exact.
        let before = fake.requests.lock().iter().filter(|&&i| i == 3).count();
        r.seek(SeekFrom::Start(offset_of(&fake, 3) + 7)).unwrap();
        r.read_exact(&mut b).unwrap();
        assert_eq!(b, piece_bytes(3, 107)[7..]);
        assert_eq!(fake.requests.lock().iter().filter(|&&i| i == 3).count(), before + 1);
        // And reading on from there continues correctly into the window.
        let mut rest = vec![0u8; 3000];
        r.read_exact(&mut rest).unwrap();
        // (pieces 4..6 were dropped too, so this also fetches several back in a row)
        let mut want = piece_bytes(3, fake.sizes[3])[107..].to_vec();
        for i in 4..=6 {
            want.extend(piece_bytes(i, fake.sizes[i]));
        }
        assert_eq!(rest[..], want[..3000]);
    }

    #[test]
    fn expired_links_are_renewed_mid_track() {
        let (buf, fake) = Setup::new(60, 2000).start_renewing(true);
        let mut r = Reader::new(buf.clone(), false);
        read_to(&mut r, offset_of(&fake, 10));
        // Every link handed out so far expires now.
        fake.min_version.store(1, Ordering::SeqCst);
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).unwrap();
        let mut all = expected(&fake, 1)[..offset_of(&fake, 10) as usize].to_vec();
        all.extend(rest);
        assert!(all == expected(&fake, 1), "bytes differ after renewal");
        assert!(buf.urls.lock()[5].starts_with("seg://v1/"), "links were not renewed");
    }

    #[test]
    fn expired_links_without_renewal_fail_cleanly() {
        let (buf, fake) = Setup::new(40, 2000).start();
        fake.min_version.store(1, Ordering::SeqCst);
        let err = read_all(&buf, Duration::ZERO).unwrap_err();
        assert!(err.to_string().contains("expired"), "{err}");
    }

    #[test]
    fn rides_out_a_network_outage() {
        let mut s = Setup::new(40, 2000);
        s.retry = Duration::from_millis(40);
        let (buf, fake) = s.start();
        let mut r = Reader::new(buf.clone(), false);
        read_to(&mut r, offset_of(&fake, 2));
        // 0.5 s with no network at all; backoff 40+80+160+320 ms covers it.
        *fake.outage_until.lock() = Some(Instant::now() + Duration::from_millis(500));
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).unwrap();
        assert_eq!(offset_of(&fake, 2) as usize + rest.len(), expected(&fake, 1).len());
    }

    #[test]
    fn rides_out_an_outage_longer_than_the_old_retry_limit() {
        // 0.8.0 gave up after 6 attempts (31 ms here); now retries continue
        // for RETRY_SPAN (300 ms here) while the reader waits.
        let (buf, fake) = Setup::new(30, 2000).start();
        let mut r = Reader::new(buf.clone(), false);
        read_to(&mut r, offset_of(&fake, 2));
        *fake.outage_until.lock() = Some(Instant::now() + Duration::from_millis(150));
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).unwrap();
        assert_eq!(offset_of(&fake, 2) as usize + rest.len(), expected(&fake, 1).len());
    }

    #[test]
    fn slow_network_widens_the_window() {
        // Segments of 20 ms of "audio" that take 25 ms to arrive.
        let mut s = Setup::new(100, 1000);
        s.seg_secs = 0.02;
        s.delay = Duration::from_millis(25);
        let (buf, fake) = s.start();
        assert!(wait_until(|| fake.requests.lock().len() == 1 + AHEAD_SLOW));
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(fake.requests.lock().len(), 1 + AHEAD_SLOW);
        assert_eq!(buf.ahead.load(Ordering::Relaxed), AHEAD_SLOW);
    }

    #[test]
    fn backpressure_with_fast_network() {
        let (buf, fake) = Setup::new(100, 4096).start();
        // Nobody reads: downloads must stop at init + AHEAD pieces.
        assert!(wait_until(|| fake.requests.lock().len() == 1 + AHEAD));
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(fake.requests.lock().len(), 1 + AHEAD, "downloaded past the window");
        // Reading into piece 5 lets exactly 5 more pieces in.
        let mut r = Reader::new(buf.clone(), false);
        read_to(&mut r, offset_of(&fake, 5) + 1);
        assert!(wait_until(|| fake.requests.lock().len() == 1 + 5 + AHEAD));
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(fake.requests.lock().len(), 1 + 5 + AHEAD);
    }

    #[test]
    fn slow_network_parallel_and_correct() {
        let mut s = Setup::new(48, 8000);
        s.delay = Duration::from_millis(25);
        let (buf, fake) = s.start();
        let got = read_all(&buf, Duration::ZERO).unwrap();
        assert!(got == expected(&fake, 1));
        let max = fake.max_in_flight.load(Ordering::SeqCst);
        assert!(max <= PARALLEL_SEGMENTS, "{max} downloads at once");
        assert!(max >= 2, "downloads should overlap on a slow network");
        // Every piece fetched exactly once.
        let mut req = fake.requests.lock().clone();
        req.sort();
        assert_eq!(req, (0..fake.sizes.len()).collect::<Vec<_>>());
    }

    #[test]
    fn cancel_stops_workers_and_frees_memory() {
        let mut s = Setup::new(200, 16 * 1024);
        s.delay = Duration::from_millis(5);
        let (buf, fake) = s.start();
        assert!(wait_until(|| fake.requests.lock().len() > 4));
        let weak = Arc::downgrade(&buf);
        let mut r = Reader::new(buf.clone(), false);
        buf.cancel();
        let mut b = [0u8; 16];
        assert_eq!(r.read(&mut b).unwrap_err().kind(), io::ErrorKind::Interrupted);
        drop(r);
        drop(buf);
        // Worker threads drop their handles; then the window is freed.
        assert!(wait_until(|| weak.upgrade().is_none()), "buffer still alive after cancel");
        let n = fake.requests.lock().len();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(fake.requests.lock().len(), n, "downloads continued after cancel");
    }

    #[test]
    fn transient_errors_are_retried() {
        let mut s = Setup::new(30, 2000);
        s.fail_times.insert(5, 2);
        s.fail_times.insert(0, 1);
        let (buf, fake) = s.start();
        let got = read_all(&buf, Duration::ZERO).unwrap();
        assert!(got == expected(&fake, 1));
        assert_eq!(fake.requests.lock().iter().filter(|&&i| i == 5).count(), 3);
    }

    #[test]
    fn permanent_error_reaches_the_reader() {
        let mut s = Setup::new(30, 2000);
        s.fail_always = Some(7);
        let (buf, _fake) = s.start();
        let err = read_all(&buf, Duration::ZERO).unwrap_err();
        assert!(err.to_string().contains("permanent"), "{err}");
    }

    #[test]
    fn restart_drops_old_window_and_serves_new_position() {
        let (buf, fake) = Setup::new(80, 3000).start();
        let mut r = Reader::new(buf.clone(), false);
        read_to(&mut r, offset_of(&fake, 20));
        // Hold new requests at the fake CDN while inspecting the reset. The
        // new workers may otherwise have refilled the window before we read it.
        let fetch_gate = fake.outage_until.lock();
        buf.restart_at(30);
        assert_eq!(buf.window_stats().0, 0, "old window not released");
        drop(fetch_gate);
        // A new reader (as after a decoder reopen) sees init + media 31..
        let got = read_all(&buf, Duration::ZERO).unwrap();
        assert!(got == expected(&fake, 31));
    }
}
