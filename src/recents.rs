//! Albums played in TIDE, newest first. TIDAL only learns what was played
//! through its own playback reports, so the home shortcuts would never change
//! after listening here; this local list feeds "Escuchado recientemente".

use serde::{Deserialize, Serialize};

use crate::config::config_dir;
use crate::tidal::{Album, ArtistRef, Track};

const MAX: usize = 24;

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct RecentAlbum {
    pub id: u64,
    pub title: String,
    pub cover: Option<String>,
    pub artist_id: u64,
    pub artist: String,
}

fn path() -> std::path::PathBuf {
    config_dir().join("recent.json")
}

pub fn load() -> Vec<RecentAlbum> {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(list: &[RecentAlbum]) {
    if let Ok(json) = serde_json::to_string(list) {
        let _ = std::fs::create_dir_all(config_dir());
        let _ = std::fs::write(path(), json);
    }
}

/// Put the track's album at the front. Returns true when the list changed.
pub fn record(list: &mut Vec<RecentAlbum>, track: &Track) -> bool {
    let Some(album) = &track.album else { return false };
    if list.first().map(|r| r.id) == Some(album.id) {
        return false;
    }
    list.retain(|r| r.id != album.id);
    list.insert(
        0,
        RecentAlbum {
            id: album.id,
            title: album.title.clone(),
            cover: album.cover.clone(),
            artist_id: track.artists.first().map(|a| a.id).unwrap_or(0),
            artist: track.artists.first().map(|a| a.name.clone()).unwrap_or_default(),
        },
    );
    list.truncate(MAX);
    save(list);
    true
}

impl RecentAlbum {
    pub fn to_album(&self) -> Album {
        Album {
            id: self.id,
            title: self.title.clone(),
            cover: self.cover.clone(),
            artists: vec![ArtistRef { id: self.artist_id, name: self.artist.clone() }],
            release_date: String::new(),
            number_of_tracks: 0,
            duration: 0,
            kind: "ALBUM".into(),
            explicit: false,
            tags: Vec::new(),
            copyright: String::new(),
        }
    }
}
