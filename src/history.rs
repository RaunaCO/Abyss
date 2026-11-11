//! Listening history recorded by Abyss, for Replay.
//!
//! TIDAL does not give this app the account's full listening history, so
//! Abyss keeps its own: every time a track plays here, the seconds actually
//! heard (paused and buffering time excluded) are stored with the track's
//! title, artists and album. The records stay in Abyss's local database.
//!
//! Replay derives everything from these entries; nothing is estimated.

use std::{
    collections::HashMap,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use rusqlite::{params, Transaction};
use serde::{Deserialize, Serialize};

use crate::tidal::Track;
use crate::{config::config_dir, storage};

/// Oldest entries are dropped beyond this (years of daily listening).
const MAX: usize = 60_000;
/// Listening shorter than this is not kept (a skipped track).
pub const MIN_KEEP_SECS: u32 = 5;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Entry {
    /// When the listening started (Unix seconds, UTC).
    pub ts: u64,
    /// Seconds actually heard.
    pub secs: u32,
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub artist_id: u64,
    pub album: String,
    pub album_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    /// Length of the track (s).
    pub dur: u32,
}

impl Entry {
    /// A "play": heard for at least 30 s, or half of a short track.
    pub fn counts_as_play(&self) -> bool {
        self.secs >= 30 || (self.dur > 0 && self.secs * 2 >= self.dur)
    }
}

fn prepare(tx: &Transaction<'_>, dir: &Path) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS history (
            recording TEXT PRIMARY KEY NOT NULL,
            started INTEGER NOT NULL,
            seconds INTEGER NOT NULL,
            entry TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS history_order ON history(started, recording)",
    )?;
    storage::import_json::<Vec<Entry>>(tx, dir, "history.json", |entries| {
        let mut insert = tx.prepare("INSERT INTO history(recording, started, seconds, entry) VALUES (?1, ?2, ?3, ?4)")?;
        for (i, entry) in entries.iter().enumerate() {
            insert.execute(params![format!("legacy:{i:016x}"), entry.ts, entry.secs, serde_json::to_string(entry)?])?;
        }
        Ok(())
    })
}

pub fn load() -> Result<Vec<Entry>> {
    load_at(&config_dir())
}

fn load_at(dir: &Path) -> Result<Vec<Entry>> {
    storage::transaction(dir, |tx| {
        prepare(tx, dir)?;
        let mut stmt = tx.prepare("SELECT entry FROM history ORDER BY started, recording")?;
        let mut rows = stmt.query([])?;
        let mut history = Vec::new();
        while let Some(row) = rows.next()? {
            history.push(serde_json::from_str(row.get_ref(0)?.as_str()?)?);
        }
        Ok(history)
    })
}

fn save_at<'a>(dir: &Path, entries: impl IntoIterator<Item = (&'a u128, &'a Entry)>) -> Result<()> {
    storage::transaction(dir, |tx| {
        prepare(tx, dir)?;
        let mut write = tx.prepare(
            "INSERT INTO history(recording, started, seconds, entry) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(recording) DO UPDATE SET seconds = excluded.seconds, entry = excluded.entry
             WHERE excluded.seconds > history.seconds",
        )?;
        for (id, entry) in entries {
            write.execute(params![format!("{id:032x}"), entry.ts, entry.secs, serde_json::to_string(entry)?])?;
        }
        tx.execute(
            "DELETE FROM history WHERE (started, recording) <= (
                SELECT started, recording FROM history ORDER BY started DESC, recording DESC LIMIT 1 OFFSET ?1
            )",
            [MAX],
        )?;
        Ok(())
    })
}

type Entries = HashMap<u128, Entry>;
pub type SaveBatch = Arc<Entries>;

pub fn save_batch(batch: &SaveBatch) -> Result<()> {
    save_at(&config_dir(), batch.iter())
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Records the listening of the current track.
#[derive(Default)]
pub struct Recorder {
    /// History position, engine playback key and persistent recording id.
    open: Option<(usize, u64, u128)>,
    /// Fraction of a second heard but not yet counted.
    carry: f32,
    playing: bool,
    pending: Entries,
    in_flight: Option<SaveBatch>,
    last_attempt: Option<Instant>,
    failed: bool,
    urgent: bool,
}

impl Recorder {
    /// A seek keeps the engine key. A repeat of the same track gets a new key.
    pub fn tick(&mut self, list: &mut Vec<Entry>, track: Option<(u64, &Track)>, playing: bool, dt: f32) -> Option<SaveBatch> {
        self.step(list, track, playing, dt);
        let elapsed = self.last_attempt.map_or(Duration::MAX, |t| t.elapsed());
        let due = if self.failed { elapsed >= Duration::from_secs(5) } else { self.urgent || elapsed >= Duration::from_secs(30) };
        if self.in_flight.is_some() || self.pending.is_empty() || !due {
            return None;
        }
        let batch = Arc::new(std::mem::take(&mut self.pending));
        self.in_flight = Some(batch.clone());
        self.last_attempt = Some(Instant::now());
        self.urgent = false;
        Some(batch)
    }

    /// Retry failed writes, keeping any newer checkpoint for the same session.
    /// Returns true on the first failure, so the UI can notify once.
    pub fn saved(&mut self, ok: bool) -> bool {
        let Some(batch) = self.in_flight.take() else { return false };
        let first_failure = !ok && !self.failed;
        self.failed = !ok;
        if !ok {
            for (id, entry) in batch.iter() {
                self.pending.entry(*id).or_insert_with(|| entry.clone());
            }
        }
        first_failure
    }

    /// A quality change restarts the engine without starting a new listen.
    pub fn rekey(&mut self, old: u64, new: u64) {
        if let Some((_, key, _)) = &mut self.open {
            if *key == old {
                *key = new;
            }
        }
    }

    pub fn flush(&mut self) -> Result<()> {
        self.flush_at(&config_dir())
    }

    fn flush_at(&mut self, dir: &Path) -> Result<()> {
        if self.pending.is_empty() && self.in_flight.is_none() {
            return Ok(());
        }
        // A background commit and the final flush can race. Upserts only
        // advance the recorded seconds, so the result is independent of order.
        save_at(dir, self.in_flight.iter().flat_map(|batch| batch.iter()).chain(self.pending.iter()))?;
        self.pending.clear();
        self.in_flight = None;
        Ok(())
    }

    /// Record without touching the disk.
    fn step(&mut self, list: &mut Vec<Entry>, track: Option<(u64, &Track)>, playing: bool, dt: f32) {
        if self.open.map(|(_, key, _)| key) != track.map(|(key, _)| key) {
            self.close(list);
        }
        self.urgent |= self.playing && !playing;
        self.playing = playing;
        if let (Some((key, t)), true) = (track, playing) {
            if self.open.is_none() {
                let extra = (list.len() + 1).saturating_sub(MAX);
                list.drain(..extra);
                list.push(Entry {
                    ts: unix_now(),
                    secs: 0,
                    id: t.id,
                    title: t.display_title(),
                    artist: t.artist_names(),
                    artist_id: t.artists.first().map(|a| a.id).unwrap_or(0),
                    album: t.album.as_ref().map(|a| a.title.clone()).unwrap_or_default(),
                    album_id: t.album.as_ref().map(|a| a.id).unwrap_or(0),
                    cover: t.album.as_ref().and_then(|a| a.cover.clone()),
                    dur: t.duration,
                });
                self.open = Some((list.len() - 1, key, rand::random()));
                self.carry = 0.0;
            }
            if let Some((i, _, id)) = self.open {
                // Whole seconds only; the fraction carries to the next tick.
                self.carry += dt.clamp(0.0, 1.0);
                let whole = self.carry.floor();
                if whole >= 1.0 {
                    self.carry -= whole;
                    if let Some(e) = list.get_mut(i) {
                        let kept = e.secs >= MIN_KEEP_SECS;
                        e.secs += whole as u32;
                        if e.secs >= MIN_KEEP_SECS {
                            self.pending.insert(id, e.clone());
                            self.urgent |= !kept;
                        }
                    }
                }
            }
        }
    }

    /// The current track ended or changed: drop it if it was barely heard.
    fn close(&mut self, list: &mut Vec<Entry>) {
        if let Some((i, _, _)) = self.open.take() {
            if list.get(i).is_some_and(|e| e.secs < MIN_KEEP_SECS) && i + 1 == list.len() {
                list.pop();
            }
            self.urgent = true;
        }
    }
}

// ------------------------------------------------------------------ dates

/// Offset of local time from UTC (s), as Windows reports it now.
pub fn local_offset() -> i64 {
    #[cfg(windows)]
    {
        #[repr(C)]
        #[derive(Default)]
        struct SystemTime {
            year: u16,
            month: u16,
            dow: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            ms: u16,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetLocalTime(t: *mut SystemTime);
            fn GetSystemTime(t: *mut SystemTime);
        }
        let (mut l, mut u) = (SystemTime::default(), SystemTime::default());
        unsafe {
            GetLocalTime(&mut l);
            GetSystemTime(&mut u);
        }
        let secs = |t: &SystemTime| {
            days_from_civil(t.year as i64, t.month as u32, t.day as u32) * 86400
                + t.hour as i64 * 3600
                + t.minute as i64 * 60
                + t.second as i64
        };
        // Rounded to the quarter hour (both clocks are read a moment apart).
        ((secs(&l) - secs(&u)) as f64 / 900.0).round() as i64 * 900
    }
    #[cfg(not(windows))]
    0
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Civil date (year, month, day) of a day number since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Local day number and hour of a Unix timestamp.
pub fn local_day_hour(ts: u64, offset: i64) -> (i64, u32) {
    let t = ts as i64 + offset;
    (t.div_euclid(86400), (t.rem_euclid(86400) / 3600) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trip() {
        for z in [-1000, 0, 1, 59, 60, 365, 20_400, 20_765] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2026, 11, 8), 20_765);
    }

    #[test]
    fn play_threshold() {
        let e = |secs, dur| Entry {
            ts: 0,
            secs,
            id: 1,
            title: String::new(),
            artist: String::new(),
            artist_id: 0,
            album: String::new(),
            album_id: 0,
            cover: None,
            dur,
        };
        assert!(e(30, 300).counts_as_play());
        assert!(!e(29, 300).counts_as_play());
        assert!(e(20, 40).counts_as_play());
    }

    #[test]
    fn recorder_counts_only_playing_time() {
        let t = Track {
            id: 7,
            title: "T".into(),
            version: None,
            duration: 200,
            artists: vec![],
            album: None,
            explicit: false,
            track_number: 1,
            volume_number: 1,
            audio_quality: String::new(),
            tags: vec![],
            streamable: true,
            added: None,
            bpm: None,
            local_path: None,
        };
        let mut list = Vec::new();
        let mut r = Recorder::default();
        // 10 s playing, 5 s paused, 3 s playing.
        for _ in 0..50 {
            r.step(&mut list, Some((1, &t)), true, 0.2);
        }
        for _ in 0..25 {
            r.step(&mut list, Some((1, &t)), false, 0.2);
        }
        for _ in 0..15 {
            r.step(&mut list, Some((1, &t)), true, 0.2);
        }
        assert_eq!(list.len(), 1);
        assert!((12..=13).contains(&list[0].secs), "{}", list[0].secs);
    }

    #[test]
    fn migrates_once_and_never_overwrites_a_newer_checkpoint() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let legacy = Entry {
            ts: 100,
            secs: 42,
            id: 7,
            title: "Pista anterior".into(),
            artist: "Artista".into(),
            artist_id: 1,
            album: "Álbum".into(),
            album_id: 2,
            cover: None,
            dur: 200,
        };
        let bytes = serde_json::to_vec(&vec![legacy.clone()])?;
        std::fs::write(dir.path().join("history.json"), &bytes)?;
        assert_eq!(load_at(dir.path())?, vec![legacy.clone()]);

        let mut entry = Entry { ts: 200, secs: 5, ..legacy.clone() };
        save_at(dir.path(), [(&1, &entry)])?;
        entry.secs = 25;
        save_at(dir.path(), [(&1, &entry), (&2, &entry)])?;
        let old_checkpoint = Entry { secs: 6, ..entry.clone() };
        save_at(dir.path(), [(&1, &old_checkpoint)])?;

        assert_eq!(load_at(dir.path())?, vec![legacy, entry.clone(), entry]);
        assert_eq!(std::fs::read(dir.path().join("history.json"))?, bytes);
        Ok(())
    }

    #[test]
    fn failed_write_retries_the_latest_seconds_and_a_repeat_starts_a_new_entry() {
        let track = Track::parse(&serde_json::json!({"id": 1, "duration": 200})).unwrap();
        let mut history = Vec::new();
        let mut recorder = Recorder::default();
        for _ in 0..4 {
            assert!(recorder.tick(&mut history, Some((1, &track)), true, 1.).is_none());
        }
        let batch = recorder.tick(&mut history, Some((1, &track)), true, 1.).unwrap();
        let id = *batch.keys().next().unwrap();
        assert_eq!(batch[&id].secs, 5);
        assert!(recorder.tick(&mut history, Some((1, &track)), true, 1.).is_none());
        assert!(recorder.saved(false));
        recorder.last_attempt = Some(Instant::now() - Duration::from_secs(5));
        let retry = recorder.tick(&mut history, Some((1, &track)), false, 0.).unwrap();
        assert_eq!(retry[&id].secs, 6);
        recorder.saved(true);
        recorder.rekey(1, 2);
        recorder.step(&mut history, Some((2, &track)), true, 1.);
        recorder.step(&mut history, Some((3, &track)), true, 1.);
        assert_eq!(history.iter().map(|entry| entry.secs).collect::<Vec<_>>(), vec![7, 1]);
    }

    #[test]
    fn close_flushes_the_latest_seconds_during_a_checkpoint() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let track = Track::parse(&serde_json::json!({"id": 1, "duration": 200})).unwrap();
        let mut history = Vec::new();
        let mut recorder = Recorder::default();
        for _ in 0..4 {
            assert!(recorder.tick(&mut history, Some((1, &track)), true, 1.).is_none());
        }
        let in_flight = recorder.tick(&mut history, Some((1, &track)), true, 1.).unwrap();
        recorder.step(&mut history, Some((1, &track)), true, 1.);
        recorder.flush_at(dir.path())?;
        save_at(dir.path(), in_flight.iter())?;

        assert_eq!(load_at(dir.path())?, history);
        assert_eq!(history[0].secs, 6);
        assert!(recorder.pending.is_empty() && recorder.in_flight.is_none());
        Ok(())
    }
}
