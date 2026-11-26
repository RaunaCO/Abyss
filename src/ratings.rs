//! Star ratings (0 to 5 in half-star steps) for tracks and albums, stored
//! locally: TIDAL has no rating API. Values are half stars, 1..=10 (0 = unrated).
//!
//! Three layers, so a rating is never lost and always shows at once:
//! 1. memory, updated first (what the stars draw);
//! 2. the SQLite database, the truth, plus a JSON mirror of it rewritten
//!    atomically after every successful save (read if the database cannot be);
//! 3. a journal of changes the database refused for now (busy, unavailable),
//!    replayed with the next change, on a timer and at start.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Result;
use rusqlite::{params, Transaction};
use serde::{Deserialize, Serialize};

use crate::{config::config_dir, storage};

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Data {
    tracks: HashMap<String, u8>,
    albums: HashMap<String, u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_both_formats_and_persists_changes_without_losing_other_ratings() -> Result<()> {
        for (legacy, album_rating) in
            [(r#"{"18446744073709551615":7}"#, 0), (r#"{"tracks":{"18446744073709551615":7},"albums":{"18446744073709551615":9}}"#, 9)]
        {
            let dir = tempfile::tempdir()?;
            let file = dir.path().join("ratings.json");
            std::fs::write(&file, legacy)?;
            let mut first = Ratings::load_at(dir.path())?;
            let mut second = Ratings::load_at(dir.path())?;
            assert_eq!(first.get(u64::MAX), 7);
            assert_eq!(first.get_album(u64::MAX), album_rating);
            first.set_at(dir.path(), "track", u64::MAX, 0)?;
            second.set_at(dir.path(), "album", 9, 5)?;
            let restored = Ratings::load_at(dir.path())?;
            assert_eq!(restored.get(u64::MAX), 0);
            assert_eq!(restored.get_album(u64::MAX), album_rating);
            assert_eq!(restored.get_album(9), 5);
            assert_eq!(std::fs::read_to_string(file)?, legacy);
        }
        Ok(())
    }

    #[test]
    fn malformed_source_survives_and_new_ratings_can_be_saved() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("ratings.json");
        std::fs::write(&file, "{\"tracks\":")?;
        let mut ratings = Ratings::default();
        ratings.set_at(dir.path(), "track", 1, 7)?;
        assert_eq!(ratings.get(1), 7);
        assert_eq!(std::fs::read_to_string(&file)?, "{\"tracks\":");
        std::fs::write(file, r#"{"1":8,"2":9}"#)?;
        let ratings = Ratings::load_at(dir.path())?;
        assert_eq!(ratings.get(1), 7);
        assert_eq!(ratings.get(2), 9);
        Ok(())
    }

    /// A folder where the database cannot be opened (its file name is a directory).
    fn broken_db() -> Result<tempfile::TempDir> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join(storage::FILE))?;
        Ok(dir)
    }

    #[test]
    fn a_refused_save_is_shown_journaled_and_replayed_into_the_database() -> Result<()> {
        let dir = broken_db()?;
        let mut ratings = Ratings::default();
        assert_eq!(ratings.set_at(dir.path(), "track", 4, 6)?, Saved::Queued);
        assert_eq!(ratings.set_at(dir.path(), "album", 5, 9)?, Saved::Queued);
        assert_eq!((ratings.get(4), ratings.get_album(5)), (6, 9), "shown at once");
        assert!(ratings.has_pending());
        // The database comes back: the next start replays the journal into it.
        std::fs::remove_dir(dir.path().join(storage::FILE))?;
        let restored = Ratings::load_at(dir.path())?;
        assert_eq!((restored.get(4), restored.get_album(5)), (6, 9));
        assert!(!restored.has_pending());
        assert!(!dir.path().join(PENDING).exists(), "journal cleared once stored");
        let again = Ratings::load_at(dir.path())?;
        assert_eq!((again.get(4), again.get_album(5)), (6, 9), "now in the database itself");
        Ok(())
    }

    #[test]
    fn a_retry_moves_the_journal_into_the_database_while_running() -> Result<()> {
        let dir = broken_db()?;
        let mut ratings = Ratings { dir: Some(dir.path().to_path_buf()), ..Ratings::default() };
        assert_eq!(ratings.set_at(dir.path(), "track", 3, 2)?, Saved::Queued);
        assert!(!ratings.retry_pending(), "still refused");
        std::fs::remove_dir(dir.path().join(storage::FILE))?;
        assert!(ratings.retry_pending());
        assert!(!ratings.has_pending());
        assert_eq!(Ratings::load_at(dir.path())?.get(3), 2);
        Ok(())
    }

    #[test]
    fn an_unreadable_database_falls_back_to_the_mirror() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut ratings = Ratings::load_at(dir.path())?;
        assert_eq!(ratings.set_at(dir.path(), "track", 1, 8)?, Saved::Stored);
        ratings.set_at(dir.path(), "album", 2, 4)?;
        assert!(dir.path().join(MIRROR).exists());
        // Lose the database: the mirror still has everything.
        std::fs::remove_file(dir.path().join(storage::FILE))?;
        std::fs::create_dir(dir.path().join(storage::FILE))?;
        let fallback = Ratings::load_at(dir.path())?;
        assert_eq!((fallback.get(1), fallback.get_album(2)), (8, 4));
        Ok(())
    }

    #[test]
    fn a_cleared_rating_stays_cleared_through_the_journal() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut ratings = Ratings::load_at(dir.path())?;
        ratings.set_at(dir.path(), "track", 7, 10)?;
        std::fs::remove_file(dir.path().join(storage::FILE))?;
        std::fs::create_dir(dir.path().join(storage::FILE))?;
        assert_eq!(ratings.set_at(dir.path(), "track", 7, 0)?, Saved::Queued);
        assert_eq!(ratings.get(7), 0);
        std::fs::remove_dir(dir.path().join(storage::FILE))?;
        // The database lost the 10 with its file and the journal says cleared.
        let restored = Ratings::load_at(dir.path())?;
        assert_eq!(restored.get(7), 0);
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Legacy {
    Groups(Data),
    Tracks(HashMap<String, u8>),
}

fn prepare(tx: &Transaction<'_>, dir: &Path) -> Result<()> {
    // TEXT preserves the high bit used for local tracks and albums.
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS ratings (
            kind TEXT NOT NULL CHECK(kind IN ('track', 'album')),
            id TEXT NOT NULL,
            halves INTEGER NOT NULL CHECK(halves BETWEEN 1 AND 10),
            PRIMARY KEY (kind, id)
        )",
    )?;
    storage::import_json(tx, dir, "ratings.json", |legacy| {
        let Data { tracks, albums } = match legacy {
            Legacy::Groups(groups) => groups,
            Legacy::Tracks(tracks) => Data { tracks, albums: HashMap::new() },
        };
        let mut insert = tx.prepare("INSERT OR IGNORE INTO ratings(kind, id, halves) VALUES (?1, ?2, ?3)")?;
        for (kind, map) in [("track", tracks), ("album", albums)] {
            for (id, halves) in map {
                if let (Ok(id), 1..) = (id.parse::<u64>(), halves) {
                    insert.execute(params![kind, id.to_string(), halves.min(10)])?;
                }
            }
        }
        Ok(())
    })
}

/// The mirror: every rating, rewritten atomically after each successful save.
const MIRROR: &str = "ratings-mirror.json";
/// The journal: changes the database has not taken yet (0 = cleared).
const PENDING: &str = "ratings-pending.json";

#[derive(Default, Clone)]
pub struct Ratings {
    tracks: HashMap<u64, u8>,
    albums: HashMap<u64, u8>,
    /// Changes not yet in the database: (album?, id) -> half stars (0 = cleared).
    pending: HashMap<(bool, u64), u8>,
    /// Where they are stored (the profile's folder unless a test chose one).
    dir: Option<PathBuf>,
}

/// How a change was kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saved {
    /// In the database (and the mirror).
    Stored,
    /// The database refused it for now: kept in the journal, retried later.
    Queued,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Snapshot {
    tracks: HashMap<String, u8>,
    albums: HashMap<String, u8>,
}

fn read_snapshot(path: &Path) -> Option<Snapshot> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn kind_name(album: bool) -> &'static str {
    if album {
        "album"
    } else {
        "track"
    }
}

/// Writes `changes` in one transaction: all of them or none.
fn store(dir: &Path, changes: &[((bool, u64), u8)]) -> Result<()> {
    storage::transaction(dir, |tx| {
        prepare(tx, dir)?;
        for &((album, id), halves) in changes {
            let id = id.to_string();
            if halves == 0 {
                tx.execute("DELETE FROM ratings WHERE kind = ?1 AND id = ?2", params![kind_name(album), id])?;
            } else {
                tx.execute(
                    "INSERT INTO ratings(kind, id, halves) VALUES (?1, ?2, ?3)
                    ON CONFLICT(kind, id) DO UPDATE SET halves = excluded.halves WHERE halves <> excluded.halves",
                    params![kind_name(album), id, halves],
                )?;
            }
        }
        Ok(())
    })
}

impl Ratings {
    pub fn load() -> Result<Self> {
        Self::load_at(&config_dir())
    }

    /// The database first, or the mirror if it cannot be read; then the
    /// journal on top (the newest changes), flushed to the database when it can.
    fn load_at(dir: &Path) -> Result<Self> {
        let from_db = storage::transaction(dir, |tx| {
            prepare(tx, dir)?;
            let mut stmt = tx.prepare("SELECT kind, id, halves FROM ratings")?;
            let mut rows = stmt.query([])?;
            let mut ratings = Self::default();
            while let Some(row) = rows.next()? {
                let kind: String = row.get(0)?;
                let id = row.get::<_, String>(1)?.parse()?;
                ratings.map(kind == "album").insert(id, row.get(2)?);
            }
            Ok(ratings)
        });
        let db_ok = from_db.is_ok();
        let mut ratings = from_db.unwrap_or_else(|error| {
            log::warn!("ratings database unreadable, using the mirror: {error:#}");
            let mut ratings = Self::default();
            if let Some(mirror) = read_snapshot(&dir.join(MIRROR)) {
                ratings.absorb(&mirror, false);
            }
            ratings
        });
        ratings.dir = Some(dir.to_path_buf());
        if let Some(journal) = read_snapshot(&dir.join(PENDING)) {
            ratings.absorb(&journal, true);
        }
        if db_ok && !ratings.retry_pending() {
            log::warn!("ratings: {} change(s) still wait for the database", ratings.pending.len());
        }
        if db_ok && ratings.pending.is_empty() {
            ratings.write_mirror(dir);
        }
        Ok(ratings)
    }

    /// Applies a stored snapshot; as `pending`, its entries also wait for the database.
    fn absorb(&mut self, snapshot: &Snapshot, pending: bool) {
        for (album, map) in [(false, &snapshot.tracks), (true, &snapshot.albums)] {
            for (id, &halves) in map {
                let Ok(id) = id.parse::<u64>() else { continue };
                let halves = halves.min(10);
                if halves == 0 {
                    self.map(album).remove(&id);
                } else {
                    self.map(album).insert(id, halves);
                }
                if pending {
                    self.pending.insert((album, id), halves);
                }
            }
        }
    }

    fn dir(&self) -> PathBuf {
        self.dir.clone().unwrap_or_else(config_dir)
    }

    /// Every rating as ("track" or "album", id, half stars), sorted, for exporting.
    pub fn all(&self) -> Vec<(&'static str, u64, u8)> {
        let mut out: Vec<_> =
            self.tracks.iter().map(|(id, h)| ("track", *id, *h)).chain(self.albums.iter().map(|(id, h)| ("album", *id, *h))).collect();
        out.sort();
        out
    }

    /// Half stars, 0..=10.
    pub fn get(&self, track: u64) -> u8 {
        self.tracks.get(&track).copied().unwrap_or(0)
    }

    pub fn get_album(&self, album: u64) -> u8 {
        self.albums.get(&album).copied().unwrap_or(0)
    }

    /// Whether some changes still wait for the database.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn set(&mut self, track: u64, halves: u8) -> Result<Saved> {
        let dir = self.dir();
        self.set_at(&dir, "track", track, halves)
    }

    pub fn set_album(&mut self, album: u64, halves: u8) -> Result<Saved> {
        let dir = self.dir();
        self.set_at(&dir, "album", album, halves)
    }

    /// Shown at once (memory), then kept by the first layer that takes it: the
    /// database (with whatever was waiting, mirror refreshed), else the
    /// journal. Fails only if neither can be written; the value still shows
    /// and is retried with the next change or tick.
    fn set_at(&mut self, dir: &Path, kind: &str, id: u64, halves: u8) -> Result<Saved> {
        let halves = halves.min(10);
        let album = kind == "album";
        if halves == 0 {
            self.map(album).remove(&id);
        } else {
            self.map(album).insert(id, halves);
        }
        self.pending.insert((album, id), halves);
        if self.flush(dir).is_ok() {
            return Ok(Saved::Stored);
        }
        self.write_journal(dir)?;
        Ok(Saved::Queued)
    }

    /// Tries to move the waiting changes into the database (called on a
    /// timer); true when nothing waits any more.
    pub fn retry_pending(&mut self) -> bool {
        if self.pending.is_empty() {
            return true;
        }
        let dir = self.dir();
        self.flush(&dir).is_ok()
    }

    fn flush(&mut self, dir: &Path) -> Result<()> {
        let changes: Vec<_> = self.pending.iter().map(|(k, v)| (*k, *v)).collect();
        if let Err(error) = store(dir, &changes) {
            log::warn!("ratings: database busy or unavailable, {} change(s) kept in the journal: {error:#}", changes.len());
            return Err(error);
        }
        self.pending.clear();
        let _ = std::fs::remove_file(dir.join(PENDING));
        self.write_mirror(dir);
        Ok(())
    }

    fn snapshot(&self, pending_only: bool) -> Snapshot {
        let mut snapshot = Snapshot::default();
        if pending_only {
            for (&(album, id), &halves) in &self.pending {
                let map = if album { &mut snapshot.albums } else { &mut snapshot.tracks };
                map.insert(id.to_string(), halves);
            }
        } else {
            snapshot.tracks = self.tracks.iter().map(|(id, h)| (id.to_string(), *h)).collect();
            snapshot.albums = self.albums.iter().map(|(id, h)| (id.to_string(), *h)).collect();
        }
        snapshot
    }

    fn write_journal(&self, dir: &Path) -> Result<()> {
        let bytes = serde_json::to_vec(&self.snapshot(true))?;
        storage::atomic_write(&dir.join(PENDING), &bytes)?;
        Ok(())
    }

    /// Best effort: the database already holds the truth.
    fn write_mirror(&self, dir: &Path) {
        match serde_json::to_vec(&self.snapshot(false)) {
            Ok(bytes) => {
                if let Err(error) = storage::atomic_write(&dir.join(MIRROR), &bytes) {
                    log::debug!("could not refresh the ratings mirror: {error}");
                }
            }
            Err(error) => log::debug!("could not encode the ratings mirror: {error}"),
        }
    }

    fn map(&mut self, album: bool) -> &mut HashMap<u64, u8> {
        if album {
            &mut self.albums
        } else {
            &mut self.tracks
        }
    }
}
