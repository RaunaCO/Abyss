//! Progressive download into memory with a blocking `Read + Seek` view, so
//! decoding starts as soon as the first bytes arrive.
//!
//! DASH streams are fetched with several parallel connections (appended in
//! order) and can be *restarted* at any segment, which makes seeking to a
//! position that has not been downloaded yet instant.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use reqwest::blocking::Client;

use crate::tidal::StreamSource;

const PARALLEL_SEGMENTS: usize = 4;

#[derive(Default)]
struct State {
    data: Vec<u8>,
    /// Known total size (progressive files with a Content-Length).
    total: Option<u64>,
    done: bool,
    error: Option<String>,
    /// Bumped on every restart so stale download threads stop writing.
    generation: u64,
    /// Number of pieces already appended (segment downloads keep order).
    appended: usize,
}

pub struct Buffer {
    state: Mutex<State>,
    ready: Condvar,
    cancelled: AtomicBool,
    http: Client,
    source: StreamSource,
    seg_secs: Vec<f64>,
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
            Some(s.data.len() as u64)
        } else {
            s.total
        }
    }

    pub fn downloaded(&self) -> (u64, Option<u64>, bool) {
        let s = self.state.lock();
        (s.data.len() as u64, s.total, s.done)
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
            s.data.clear();
            s.total = None;
            s.done = false;
            s.error = None;
            s.appended = 0;
        }
        self.ready.notify_all();
        self.begin(seg);
    }

    fn append(&self, generation: u64, chunk: &[u8]) {
        let mut s = self.state.lock();
        if s.generation == generation {
            s.data.extend_from_slice(chunk);
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
                let mut list: Vec<String> = Vec::with_capacity(urls.len());
                list.extend(urls.first().cloned());
                list.extend(urls.iter().skip(1 + from_seg).cloned());
                let list = Arc::new(list);
                let next = Arc::new(AtomicUsize::new(0));
                for n in 0..PARALLEL_SEGMENTS.min(list.len()) {
                    let (b, list, next) = (self.clone(), list.clone(), next.clone());
                    std::thread::Builder::new()
                        .name(format!("download-{n}"))
                        .spawn(move || segment_worker(b, generation, list, next))
                        .expect("spawn download thread");
                }
            }
        }
    }
}

fn segment_worker(b: Arc<Buffer>, generation: u64, list: Arc<Vec<String>>, next: Arc<AtomicUsize>) {
    loop {
        let i = next.fetch_add(1, Ordering::SeqCst);
        if i >= list.len() || !b.live(generation) {
            return;
        }
        let bytes = match get_bytes(&b, generation, &list[i]) {
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
        s.data.extend_from_slice(&bytes);
        s.appended += 1;
        let last = s.appended == list.len();
        drop(s);
        b.ready.notify_all();
        if last {
            b.finish(generation, None);
        }
    }
}

fn get_bytes(b: &Buffer, generation: u64, url: &str) -> Result<Vec<u8>, String> {
    let mut tries = 0;
    loop {
        tries += 1;
        let r = (|| -> Result<Vec<u8>, String> {
            let resp = b.http.get(url).timeout(Duration::from_secs(60)).send().map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("download failed ({})", resp.status()));
            }
            resp.bytes().map(|x| x.to_vec()).map_err(|e| e.to_string())
        })();
        match r {
            Ok(v) => return Ok(v),
            Err(e) if tries < 3 && b.live(generation) => {
                log::warn!("retrying download: {e}");
                std::thread::sleep(Duration::from_millis(400 * tries));
            }
            Err(e) => return Err(e),
        }
    }
}

/// Start downloading `source`. `seg_secs` are the DASH segment lengths (may be empty).
pub fn start(http: Client, source: StreamSource, seg_secs: Vec<f64>) -> Arc<Buffer> {
    let buf = Arc::new(Buffer {
        state: Mutex::new(State::default()),
        ready: Condvar::new(),
        cancelled: AtomicBool::new(false),
        http,
        source,
        seg_secs,
    });
    buf.begin(0);
    buf
}

fn fetch(http: &Client, url: &str, buf: &Buffer, generation: u64) -> Result<(), String> {
    let mut tries = 0;
    loop {
        tries += 1;
        let start_len = buf.state.lock().data.len();
        let attempt = (|| -> Result<(), String> {
            let mut resp = http
                .get(url)
                .timeout(Duration::from_secs(120))
                .send()
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("download failed ({})", resp.status()));
            }
            if let Some(len) = resp.content_length() {
                buf.state.lock().total = Some(len);
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
                buf.state.lock().data.truncate(start_len);
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
            let len = s.data.len() as u64;
            if self.pos < len {
                let n = ((len - self.pos) as usize).min(out.len());
                let start = self.pos as usize;
                out[..n].copy_from_slice(&s.data[start..start + n]);
                self.pos += n as u64;
                return Ok(n);
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
                        break s.data.len() as u64;
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
