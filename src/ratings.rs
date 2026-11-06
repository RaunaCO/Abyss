//! Star ratings (0 to 5 in half-star steps) for tracks and albums, stored
//! locally: TIDAL has no rating API. Values are half stars, 1..=10 (0 = unrated).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::config::config_dir;

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Data {
    tracks: HashMap<String, u8>,
    albums: HashMap<String, u8>,
}

#[derive(Default, Clone)]
pub struct Ratings {
    tracks: HashMap<u64, u8>,
    albums: HashMap<u64, u8>,
}

fn path() -> std::path::PathBuf {
    config_dir().join("ratings.json")
}

fn ids(m: HashMap<String, u8>) -> HashMap<u64, u8> {
    m.into_iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.min(10)))).collect()
}

impl Ratings {
    pub fn load() -> Self {
        let Ok(text) = std::fs::read_to_string(path()) else { return Self::default() };
        // Current format, or the first one: a flat map of track ratings.
        if let Ok(d) = serde_json::from_str::<Data>(&text) {
            if !d.tracks.is_empty() || !d.albums.is_empty() || text.contains("\"tracks\"") {
                return Self { tracks: ids(d.tracks), albums: ids(d.albums) };
            }
        }
        match serde_json::from_str::<HashMap<String, u8>>(&text) {
            Ok(flat) => Self { tracks: ids(flat), albums: HashMap::new() },
            Err(_) => Self::default(),
        }
    }

    /// Half stars, 0..=10.
    pub fn get(&self, track: u64) -> u8 {
        self.tracks.get(&track).copied().unwrap_or(0)
    }

    pub fn get_album(&self, album: u64) -> u8 {
        self.albums.get(&album).copied().unwrap_or(0)
    }

    pub fn set(&mut self, track: u64, halves: u8) {
        Self::put(&mut self.tracks, track, halves);
        self.save();
    }

    pub fn set_album(&mut self, album: u64, halves: u8) {
        Self::put(&mut self.albums, album, halves);
        self.save();
    }

    fn put(map: &mut HashMap<u64, u8>, id: u64, halves: u8) {
        if halves == 0 {
            map.remove(&id);
        } else {
            map.insert(id, halves.min(10));
        }
    }

    fn save(&self) {
        let as_text = |m: &HashMap<u64, u8>| m.iter().map(|(k, v)| (k.to_string(), *v)).collect::<HashMap<_, _>>();
        let data = Data { tracks: as_text(&self.tracks), albums: as_text(&self.albums) };
        if let Ok(json) = serde_json::to_string(&data) {
            let _ = std::fs::create_dir_all(config_dir());
            let _ = std::fs::write(path(), json);
        }
    }
}
