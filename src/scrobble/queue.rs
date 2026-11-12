//! Persistent scrobble queue in the listening database (`listening.sqlite3`).
//!
//! Own tables only: `scrobble_queue` (waiting to be sent), `scrobbled` (done,
//! kept so a backfill never sends a listen twice) and `scrobble_meta`. The
//! `history` table (Replay) is only ever read.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Transaction};

use super::{tracker::eligible, Item, Meta};
use crate::{history::Entry, storage};

/// Last.fm accepts at most 50 scrobbles per call.
pub const BATCH: usize = 50;
/// A history entry within this many seconds of a listen already sent or queued
/// (same artist and title) is the same listen seen by the live tracker and by Replay.
const SAME_LISTEN_SECS: u64 = 300;
/// Rows written per transaction, so Replay saves are never blocked for long.
const CHUNK: usize = 1000;

/// Times (Unix seconds) of listens already sent or queued, by lowercase artist and title.
type Seen = HashMap<(String, String), Vec<u64>>;

fn prepare(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS scrobble_queue (
            id TEXT PRIMARY KEY NOT NULL,
            ts INTEGER NOT NULL,
            artist TEXT NOT NULL,
            track TEXT NOT NULL,
            album TEXT NOT NULL,
            duration INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS scrobble_queue_order ON scrobble_queue(ts, id);
        CREATE TABLE IF NOT EXISTS scrobbled (
            id TEXT PRIMARY KEY NOT NULL,
            ts INTEGER NOT NULL,
            artist TEXT NOT NULL,
            track TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS scrobble_meta (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL)",
    )?;
    Ok(())
}

fn key(artist: &str, track: &str) -> (String, String) {
    (artist.trim().to_lowercase(), track.trim().to_lowercase())
}

fn insert(tx: &Transaction<'_>, item: &Item) -> Result<bool> {
    let known: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM scrobbled WHERE id = ?1) OR EXISTS(SELECT 1 FROM scrobble_queue WHERE id = ?1)",
        [&item.id],
        |row| row.get(0),
    )?;
    if known {
        return Ok(false);
    }
    tx.execute(
        "INSERT INTO scrobble_queue(id, ts, artist, track, album, duration) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![item.id, item.ts, item.meta.artist, item.meta.track, item.meta.album, item.meta.duration],
    )?;
    Ok(true)
}

/// Queue one listen. Returns false if it was already queued or sent.
pub fn push(dir: &Path, item: &Item) -> Result<bool> {
    storage::transaction(dir, |tx| {
        prepare(tx)?;
        insert(tx, item)
    })
}

pub fn pending(dir: &Path) -> Result<usize> {
    storage::transaction(dir, |tx| {
        prepare(tx)?;
        Ok(tx.query_row("SELECT COUNT(*) FROM scrobble_queue", [], |row| row.get::<_, i64>(0))? as usize)
    })
}

/// The oldest queued listens, up to `limit`.
pub fn take(dir: &Path, limit: usize) -> Result<Vec<Item>> {
    storage::transaction(dir, |tx| {
        prepare(tx)?;
        let mut statement = tx.prepare("SELECT id, ts, artist, track, album, duration FROM scrobble_queue ORDER BY ts, id LIMIT ?1")?;
        let rows = statement.query_map([limit as i64], |row| {
            Ok(Item {
                id: row.get(0)?,
                ts: row.get(1)?,
                meta: Meta { artist: row.get(2)?, track: row.get(3)?, album: row.get(4)?, duration: row.get(5)? },
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    })
}

/// Sent: leave the queue and remember it as done, in one transaction.
pub fn complete(dir: &Path, items: &[Item]) -> Result<()> {
    storage::transaction(dir, |tx| {
        prepare(tx)?;
        for item in items {
            tx.execute("DELETE FROM scrobble_queue WHERE id = ?1", [&item.id])?;
            tx.execute(
                "INSERT OR IGNORE INTO scrobbled(id, ts, artist, track) VALUES (?1, ?2, ?3, ?4)",
                params![item.id, item.ts, item.meta.artist, item.meta.track],
            )?;
        }
        Ok(())
    })
}

/// Will never be accepted: leave the queue without being marked as sent.
pub fn discard(dir: &Path, items: &[Item]) -> Result<()> {
    storage::transaction(dir, |tx| {
        prepare(tx)?;
        for item in items {
            tx.execute("DELETE FROM scrobble_queue WHERE id = ?1", [&item.id])?;
        }
        Ok(())
    })
}

/// Drop the history listens still waiting (a cancelled sync). Live listens stay.
pub fn discard_replay(dir: &Path) -> Result<()> {
    storage::transaction(dir, |tx| {
        prepare(tx)?;
        tx.execute("DELETE FROM scrobble_queue WHERE id LIKE 'replay:%'", [])?;
        Ok(())
    })
}

/// Remember which Last.fm account the queue belongs to. A different account
/// starts clean, so listens are never sent to the wrong profile.
pub fn set_account(dir: &Path, name: &str) -> Result<()> {
    storage::transaction(dir, |tx| {
        prepare(tx)?;
        let stored: Option<String> = tx.query_row("SELECT value FROM scrobble_meta WHERE key = 'account'", [], |row| row.get(0)).ok();
        if stored.as_deref().is_some_and(|old| !old.eq_ignore_ascii_case(name)) {
            tx.execute_batch("DELETE FROM scrobble_queue; DELETE FROM scrobbled")?;
        }
        tx.execute(
            "INSERT INTO scrobble_meta(key, value) VALUES ('account', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [name],
        )?;
        Ok(())
    })
}

/// Queue the saved Replay listening history that has not been sent yet.
/// Reads `history` without changing it. Returns how many listens were queued.
pub fn enqueue_history(dir: &Path) -> Result<usize> {
    // One short read of the history, then small write transactions.
    let history: Vec<(String, Entry)> = storage::transaction(dir, |tx| {
        let exists: bool =
            tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'history')", [], |row| row.get(0))?;
        if !exists {
            return Ok(Vec::new());
        }
        let mut statement = tx.prepare("SELECT recording, entry FROM history ORDER BY started, recording")?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (recording, json) = row?;
            // A damaged row is skipped, never repaired or removed.
            if let Ok(entry) = serde_json::from_str::<Entry>(&json) {
                out.push((recording, entry));
            }
        }
        Ok(out)
    })?;

    let (mut done, mut queued): (HashSet<String>, Seen) = storage::transaction(dir, |tx| {
        prepare(tx)?;
        let mut ids = HashSet::new();
        let mut times = Seen::new();
        for sql in ["SELECT id, ts, artist, track FROM scrobbled", "SELECT id, ts, artist, track FROM scrobble_queue"] {
            let mut statement = tx.prepare(sql)?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?))
            })?;
            for row in rows {
                let (id, ts, artist, track) = row?;
                ids.insert(id);
                times.entry(key(&artist, &track)).or_default().push(ts);
            }
        }
        Ok((ids, times))
    })?;

    let mut fresh: Vec<Item> = Vec::new();
    for (recording, entry) in history {
        if entry.artist.trim().is_empty() || entry.title.trim().is_empty() || !eligible(entry.dur, entry.secs) {
            continue;
        }
        let id = format!("replay:{recording}");
        let k = key(&entry.artist, &entry.title);
        let same = queued.get(&k).is_some_and(|times| times.iter().any(|t| t.abs_diff(entry.ts) <= SAME_LISTEN_SECS));
        if done.contains(&id) || same {
            continue;
        }
        done.insert(id.clone());
        queued.entry(k).or_default().push(entry.ts);
        fresh.push(Item {
            id,
            ts: entry.ts,
            meta: Meta { artist: entry.artist, track: entry.title, album: entry.album, duration: entry.dur },
        });
    }

    let mut count = 0;
    for chunk in fresh.chunks(CHUNK) {
        count += storage::transaction(dir, |tx| {
            prepare(tx)?;
            let mut n = 0;
            for item in chunk {
                n += usize::from(insert(tx, item)?);
            }
            Ok(n)
        })?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, ts: u64, artist: &str, track: &str) -> Item {
        Item { id: id.into(), ts, meta: Meta { artist: artist.into(), track: track.into(), album: "Al".into(), duration: 200 } }
    }

    /// Write rows the way `history.rs` does, so the tests read the real schema.
    fn seed_history(dir: &Path, entries: &[(&str, Entry)]) {
        storage::transaction(dir, |tx| {
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS history (recording TEXT PRIMARY KEY NOT NULL, started INTEGER NOT NULL, seconds INTEGER NOT NULL, entry TEXT NOT NULL)",
            )?;
            for (recording, entry) in entries {
                tx.execute(
                    "INSERT INTO history(recording, started, seconds, entry) VALUES (?1, ?2, ?3, ?4)",
                    params![recording, entry.ts, entry.secs, serde_json::to_string(entry)?],
                )?;
            }
            Ok(())
        })
        .unwrap();
    }

    fn entry(ts: u64, secs: u32, dur: u32, artist: &str, title: &str) -> Entry {
        Entry {
            ts,
            secs,
            id: 1,
            title: title.into(),
            artist: artist.into(),
            artist_id: 1,
            album: "Al".into(),
            album_id: 2,
            cover: None,
            dur,
        }
    }

    fn history_dump(dir: &Path) -> Vec<(String, u64, u32, String)> {
        storage::transaction(dir, |tx| {
            let mut s = tx.prepare("SELECT recording, started, seconds, entry FROM history ORDER BY recording")?;
            let rows = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
    }

    #[test]
    fn push_is_idempotent_and_complete_remembers_the_listen() {
        let dir = tempfile::tempdir().unwrap();
        let a = item("a", 10, "X", "Y");
        assert!(push(dir.path(), &a).unwrap());
        assert!(!push(dir.path(), &a).unwrap(), "already queued");
        assert_eq!(pending(dir.path()).unwrap(), 1);
        assert_eq!(take(dir.path(), 10).unwrap(), vec![a.clone()]);
        complete(dir.path(), std::slice::from_ref(&a)).unwrap();
        assert_eq!(pending(dir.path()).unwrap(), 0);
        assert!(!push(dir.path(), &a).unwrap(), "already sent");
        assert_eq!(pending(dir.path()).unwrap(), 0);
    }

    #[test]
    fn take_returns_oldest_first_and_respects_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        for (id, ts) in [("c", 30), ("a", 10), ("b", 20)] {
            push(dir.path(), &item(id, ts, "X", id)).unwrap();
        }
        let ids: Vec<String> = take(dir.path(), 2).unwrap().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn discard_removes_without_marking_as_sent() {
        let dir = tempfile::tempdir().unwrap();
        let a = item("a", 10, "X", "Y");
        push(dir.path(), &a).unwrap();
        discard(dir.path(), std::slice::from_ref(&a)).unwrap();
        assert_eq!(pending(dir.path()).unwrap(), 0);
        assert!(push(dir.path(), &a).unwrap(), "a discarded item may be queued again");
    }

    #[test]
    fn backfill_queues_only_eligible_listens_and_never_changes_history() {
        let dir = tempfile::tempdir().unwrap();
        seed_history(
            dir.path(),
            &[
                ("r1", entry(1_000, 120, 200, "A", "One")),
                ("r2", entry(2_000, 10, 200, "A", "Skipped")),
                ("r3", entry(3_000, 100, 0, "A", "NoLength")),
                ("r4", entry(4_000, 100, 200, "", "NoArtist")),
                ("r5", entry(5_000, 100, 200, "B", "Two")),
            ],
        );
        let before = history_dump(dir.path());
        assert_eq!(enqueue_history(dir.path()).unwrap(), 2);
        assert_eq!(history_dump(dir.path()), before, "history is read only");
        let ids: Vec<String> = take(dir.path(), 10).unwrap().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, ["replay:r1", "replay:r5"]);
        // Running it again, before or after sending, never duplicates.
        assert_eq!(enqueue_history(dir.path()).unwrap(), 0);
        complete(dir.path(), &take(dir.path(), 10).unwrap()).unwrap();
        assert_eq!(enqueue_history(dir.path()).unwrap(), 0);
        assert_eq!(pending(dir.path()).unwrap(), 0);
        assert_eq!(history_dump(dir.path()), before);
    }

    #[test]
    fn a_listen_already_scrobbled_live_is_not_sent_again_from_history() {
        let dir = tempfile::tempdir().unwrap();
        let live = item("live:3e8:1", 1_000, "Artist", "Song");
        push(dir.path(), &live).unwrap();
        complete(dir.path(), &[live]).unwrap();
        seed_history(
            dir.path(),
            &[
                ("same", entry(1_004, 150, 200, "artist", "SONG")),
                ("later", entry(1_000 + SAME_LISTEN_SECS + 1, 150, 200, "Artist", "Song")),
            ],
        );
        assert_eq!(enqueue_history(dir.path()).unwrap(), 1);
        assert_eq!(take(dir.path(), 10).unwrap()[0].id, "replay:later");
    }

    #[test]
    fn backfill_without_a_history_table_or_with_damaged_rows() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(enqueue_history(dir.path()).unwrap(), 0);
        seed_history(dir.path(), &[("ok", entry(1, 120, 200, "A", "T"))]);
        storage::transaction(dir.path(), |tx| {
            tx.execute("INSERT INTO history(recording, started, seconds, entry) VALUES ('bad', 2, 2, '{not json')", [])?;
            Ok(())
        })
        .unwrap();
        assert_eq!(enqueue_history(dir.path()).unwrap(), 1);
        assert_eq!(history_dump(dir.path()).len(), 2, "the damaged row stays untouched");
    }

    #[test]
    fn a_different_account_starts_with_an_empty_queue() {
        let dir = tempfile::tempdir().unwrap();
        set_account(dir.path(), "Rauna").unwrap();
        push(dir.path(), &item("a", 1, "X", "Y")).unwrap();
        set_account(dir.path(), "rauna").unwrap();
        assert_eq!(pending(dir.path()).unwrap(), 1, "same account, any case");
        set_account(dir.path(), "someone-else").unwrap();
        assert_eq!(pending(dir.path()).unwrap(), 0);
    }
}
