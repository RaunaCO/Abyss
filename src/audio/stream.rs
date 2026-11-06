//! Progressive download into memory with a blocking `Read + Seek` view, so
//! decoding starts as soon as the first bytes arrive.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use reqwest::blocking::Client;

use crate::tidal::StreamSource;

#[derive(Default)]
struct State {
    data: Vec<u8>,
    /// Known total size (progressive files with a Content-Length).
    total: Option<u64>,
    done: bool,
    error: Option<String>,
}

pub struct Buffer {
    state: Mutex<State>,
    ready: Condvar,
    cancelled: AtomicBool,
}

impl Buffer {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.ready.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn append(&self, chunk: &[u8]) {
        self.state.lock().data.extend_from_slice(chunk);
        self.ready.notify_all();
    }

    fn finish(&self, error: Option<String>) {
        let mut s = self.state.lock();
        s.done = true;
        s.error = error;
        drop(s);
        self.ready.notify_all();
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
}

/// Start downloading `source` on its own thread.
pub fn start(http: Client, source: StreamSource) -> Arc<Buffer> {
    let buf = Arc::new(Buffer { state: Mutex::new(State::default()), ready: Condvar::new(), cancelled: AtomicBool::new(false) });
    let b = buf.clone();
    std::thread::Builder::new()
        .name("download".into())
        .spawn(move || {
            let result = match source {
                StreamSource::Single(url) => fetch(&http, &url, &b, true),
                StreamSource::Segments(urls) => urls.iter().try_for_each(|u| fetch(&http, u, &b, false)),
            };
            b.finish(result.err().filter(|_| !b.is_cancelled()));
        })
        .expect("spawn download thread");
    buf
}

fn fetch(http: &Client, url: &str, buf: &Buffer, set_total: bool) -> Result<(), String> {
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
            if set_total {
                if let Some(len) = resp.content_length() {
                    buf.state.lock().total = Some(len);
                }
            }
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                if buf.is_cancelled() {
                    return Ok(());
                }
                let n = resp.read(&mut chunk).map_err(|e| e.to_string())?;
                if n == 0 {
                    return Ok(());
                }
                buf.append(&chunk[..n]);
            }
        })();
        match attempt {
            Ok(()) => return Ok(()),
            Err(e) if tries < 3 && !buf.is_cancelled() => {
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
