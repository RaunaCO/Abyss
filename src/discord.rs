//! Discord Rich Presence, shown like Spotify's: a "Listening to TIDE" card
//! with the track title, the artist, the album cover, the album name on hover,
//! a progress bar and a button that opens the track.
//!
//! Talks to the local Discord client over its IPC named pipe
//! (`\\.\pipe\discord-ipc-N`), so nothing is sent anywhere else. It needs the
//! Application ID of a Discord application named "TIDE" (that name is what
//! Discord shows after "Listening to").

use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// What is playing, in the terms Discord needs.
#[derive(Clone, PartialEq)]
pub struct Activity {
    pub track_id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: Option<String>,
    /// Unix seconds when the track would have started at position 0.
    pub start: i64,
    pub end: i64,
}

enum Msg {
    Set(Activity),
    Clear,
    ClientId(String),
    Quit,
}

/// UI-side handle: diffs the player state and sends only real changes.
pub struct Discord {
    tx: Sender<Msg>,
    enabled: bool,
    last: Option<(u64, bool)>,
    last_start: i64,
}

impl Discord {
    pub fn new(client_id: String, enabled: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new().name("discord".into()).spawn(move || worker(client_id, rx)).ok();
        Self { tx, enabled, last: None, last_start: 0 }
    }

    pub fn set_client_id(&mut self, id: String) {
        let _ = self.tx.send(Msg::ClientId(id));
        self.last = None;
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            let _ = self.tx.send(Msg::Clear);
        }
        self.last = None;
    }

    /// Call on every UI tick. `now` is the playing track with its duration.
    pub fn update(&mut self, now: Option<(&crate::tidal::Track, f64)>, playing: bool, position: f64) {
        if !self.enabled {
            return;
        }
        let Some((track, duration)) = now else {
            if self.last.take().is_some() {
                let _ = self.tx.send(Msg::Clear);
            }
            return;
        };
        let unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let start = unix - position as i64;
        let key = (track.id, playing);
        // A jump of the start time means the user sought.
        let sought = self.last.is_some_and(|(id, p)| id == track.id && p == playing && playing && (start - self.last_start).abs() > 3);
        if self.last == Some(key) && !sought {
            return;
        }
        self.last = Some(key);
        self.last_start = start;
        if !playing {
            // Like Spotify, nothing is shown while paused.
            let _ = self.tx.send(Msg::Clear);
            return;
        }
        let dur = if duration > 0.0 { duration } else { track.duration as f64 };
        let _ = self.tx.send(Msg::Set(Activity {
            track_id: track.id,
            title: track.display_title(),
            artist: track.artist_names(),
            album: track.album.as_ref().map(|a| a.title.clone()).unwrap_or_default(),
            cover: track.cover(640),
            start,
            end: start + dur as i64,
        }));
    }
}

impl Drop for Discord {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Clear);
        let _ = self.tx.send(Msg::Quit);
    }
}

// ------------------------------------------------------------------ worker

struct Conn {
    pipe: std::fs::File,
    nonce: u64,
}

fn frame(op: u32, payload: &Value) -> Vec<u8> {
    let body = payload.to_string().into_bytes();
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

fn read_frame(pipe: &mut std::fs::File) -> std::io::Result<(u32, Vec<u8>)> {
    let mut head = [0u8; 8];
    pipe.read_exact(&mut head)?;
    let op = u32::from_le_bytes(head[..4].try_into().unwrap());
    let len = u32::from_le_bytes(head[4..].try_into().unwrap()) as usize;
    let mut body = vec![0u8; len.min(1 << 20)];
    pipe.read_exact(&mut body)?;
    Ok((op, body))
}

fn connect(client_id: &str) -> Option<Conn> {
    for n in 0..10 {
        let path = format!(r"\\.\pipe\discord-ipc-{n}");
        let Ok(mut pipe) = std::fs::OpenOptions::new().read(true).write(true).open(&path) else { continue };
        if pipe.write_all(&frame(0, &json!({ "v": 1, "client_id": client_id }))).is_err() {
            continue;
        }
        match read_frame(&mut pipe) {
            Ok((1, body)) if String::from_utf8_lossy(&body).contains("READY") => {
                log::info!("discord: connected on {path}");
                return Some(Conn { pipe, nonce: 0 });
            }
            _ => continue,
        }
    }
    None
}

impl Conn {
    fn set_activity(&mut self, activity: Option<&Activity>) -> std::io::Result<()> {
        self.nonce += 1;
        let activity_json = activity.map(|a| {
            let mut act = json!({
                // 2 = "Listening to", the same card Spotify gets.
                "type": 2,
                "details": a.title,
                "state": a.artist,
                "timestamps": { "start": a.start, "end": a.end },
                "buttons": [{ "label": "Escuchar en TIDAL", "url": format!("https://tidal.com/browse/track/{}", a.track_id) }],
            });
            let mut assets = serde_json::Map::new();
            if let Some(c) = &a.cover {
                assets.insert("large_image".into(), json!(c));
            }
            if !a.album.is_empty() {
                assets.insert("large_text".into(), json!(a.album));
            }
            act["assets"] = Value::Object(assets);
            act
        });
        let msg = json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": std::process::id(), "activity": activity_json },
            "nonce": self.nonce.to_string(),
        });
        self.pipe.write_all(&frame(1, &msg))?;
        // Read the reply so the pipe never fills up.
        let (_, body) = read_frame(&mut self.pipe)?;
        if String::from_utf8_lossy(&body).contains("\"ERROR\"") {
            log::warn!("discord: {}", String::from_utf8_lossy(&body));
        }
        Ok(())
    }
}

fn worker(mut client_id: String, rx: Receiver<Msg>) {
    let mut conn: Option<Conn> = None;
    let mut wanted: Option<Activity> = None;
    // Discord allows about 5 updates per 20 s.
    let mut last_send = std::time::Instant::now() - Duration::from_secs(10);
    let mut dirty = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Msg::Set(a)) => {
                wanted = Some(a);
                dirty = true;
            }
            Ok(Msg::Clear) => {
                wanted = None;
                dirty = true;
            }
            Ok(Msg::ClientId(id)) => {
                client_id = id;
                conn = None;
                dirty = true;
            }
            Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(c) = conn.as_mut() {
                    let _ = c.set_activity(None);
                }
                return;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if client_id.trim().is_empty() || !(dirty || (conn.is_none() && wanted.is_some())) {
            continue;
        }
        if last_send.elapsed() < Duration::from_millis(1500) {
            continue;
        }
        if conn.is_none() && wanted.is_some() {
            conn = connect(client_id.trim());
        }
        if let Some(c) = conn.as_mut() {
            match c.set_activity(wanted.as_ref()) {
                Ok(()) => {
                    dirty = false;
                    last_send = std::time::Instant::now();
                }
                Err(e) => {
                    log::debug!("discord: connection lost: {e}");
                    conn = None;
                }
            }
        } else if wanted.is_none() {
            dirty = false;
        }
    }
}
