//! Star ratings (0 to 5 in half-star steps) for tracks and albums, stored
//! locally: TIDAL has no rating API. Values are half stars, 1..=10 (0 = unrated).

use std::{collections::HashMap, path::Path};

use anyhow::Result;
use rusqlite::{params, Transaction};
use serde::Deserialize;

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
    fn malformed_source_survives_and_failed_change_does_not_update_stars() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("ratings.json");
        std::fs::write(&file, "{\"tracks\":")?;
        let mut ratings = Ratings::default();
        assert!(ratings.set_at(dir.path(), "track", 1, 7).is_err());
        assert_eq!(ratings.get(1), 0);
        assert_eq!(std::fs::read_to_string(&file)?, "{\"tracks\":");
        std::fs::write(file, r#"{"1":8}"#)?;
        assert_eq!(Ratings::load_at(dir.path())?.get(1), 8);
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Legacy {
    Groups(Data),
    Tracks(HashMap<String, u8>),
}

#[derive(Default, Clone)]
pub struct Ratings {
    tracks: HashMap<u64, u8>,
    albums: HashMap<u64, u8>,
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

impl Ratings {
    pub fn load() -> Result<Self> {
        Self::load_at(&config_dir())
    }

    fn load_at(dir: &Path) -> Result<Self> {
        storage::transaction(dir, |tx| {
            prepare(tx, dir)?;
            let mut stmt = tx.prepare("SELECT kind, id, halves FROM ratings")?;
            let mut rows = stmt.query([])?;
            let mut ratings = Self::default();
            while let Some(row) = rows.next()? {
                let kind: String = row.get(0)?;
                let id = row.get::<_, String>(1)?.parse()?;
                ratings.map(&kind).insert(id, row.get(2)?);
            }
            Ok(ratings)
        })
    }

    /// Half stars, 0..=10.
    pub fn get(&self, track: u64) -> u8 {
        self.tracks.get(&track).copied().unwrap_or(0)
    }

    pub fn get_album(&self, album: u64) -> u8 {
        self.albums.get(&album).copied().unwrap_or(0)
    }

    pub fn set(&mut self, track: u64, halves: u8) -> Result<()> {
        self.set_at(&config_dir(), "track", track, halves)
    }

    pub fn set_album(&mut self, album: u64, halves: u8) -> Result<()> {
        self.set_at(&config_dir(), "album", album, halves)
    }

    fn set_at(&mut self, dir: &Path, kind: &str, id: u64, halves: u8) -> Result<()> {
        let halves = halves.min(10);
        storage::transaction(dir, |tx| {
            prepare(tx, dir)?;
            let id = id.to_string();
            if halves == 0 {
                tx.execute("DELETE FROM ratings WHERE kind = ?1 AND id = ?2", params![kind, id])?;
            } else {
                tx.execute(
                    "INSERT INTO ratings(kind, id, halves) VALUES (?1, ?2, ?3)
                    ON CONFLICT(kind, id) DO UPDATE SET halves = excluded.halves WHERE halves <> excluded.halves",
                    params![kind, id, halves],
                )?;
            }
            Ok(())
        })?;
        // Show the new value only after it has been committed.
        let map = self.map(kind);
        if halves == 0 {
            map.remove(&id);
        } else {
            map.insert(id, halves);
        }
        Ok(())
    }

    fn map(&mut self, kind: &str) -> &mut HashMap<u64, u8> {
        if kind == "album" {
            &mut self.albums
        } else {
            &mut self.tracks
        }
    }
}
