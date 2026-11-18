//! The play queue, saved so it survives closing Abyss, a crash or a restart.
//!
//! `queue.json` in the config folder holds the tracks, which one was playing
//! and where. It is restored paused: nothing opens the audio device until the
//! user presses play. Local tracks whose file is gone are left out.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::tidal::Track;

/// Long queues are cut to this many tracks (around the current one).
const MAX_TRACKS: usize = 5000;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedQueue {
    pub tracks: Vec<Track>,
    /// The track that was playing (or about to play).
    pub index: usize,
    /// Seconds into that track.
    pub position: f64,
}

fn path() -> PathBuf {
    crate::config::config_dir().join("queue.json")
}

/// The saved queue, if any. `signed_in`: TIDAL tracks can play; otherwise
/// only local files are kept.
pub fn load(signed_in: bool) -> Option<SavedQueue> {
    let bytes = std::fs::read(path()).ok()?;
    let saved: SavedQueue = serde_json::from_slice(&bytes).ok()?;
    let saved = keep_playable(saved, signed_in, |p| p.is_file());
    (!saved.tracks.is_empty()).then_some(saved)
}

/// Drop what cannot play now and keep `index` on the same track (or the next one left).
fn keep_playable(saved: SavedQueue, signed_in: bool, exists: impl Fn(&std::path::Path) -> bool) -> SavedQueue {
    let mut index = 0;
    let mut position = 0.0;
    let mut tracks = Vec::with_capacity(saved.tracks.len());
    for (i, track) in saved.tracks.into_iter().enumerate() {
        let ok = match &track.local_path {
            Some(p) => exists(p),
            None => signed_in && track.streamable,
        };
        if i == saved.index {
            index = tracks.len();
            position = if ok { saved.position.max(0.0) } else { 0.0 };
        }
        if ok {
            tracks.push(track);
        }
    }
    if index >= tracks.len() {
        index = 0;
        position = 0.0;
    }
    SavedQueue { tracks, index, position }
}

/// Write the queue (or remove the file when it is empty). Atomic: a crash
/// while writing leaves the previous file.
pub fn save(tracks: &[Track], index: usize, position: f64) -> std::io::Result<()> {
    if tracks.is_empty() {
        return match std::fs::remove_file(path()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let (start, end) = window(tracks.len(), index);
    let saved = SavedQueue { tracks: tracks[start..end].to_vec(), index: index - start, position };
    let json = serde_json::to_vec(&saved).map_err(std::io::Error::other)?;
    let dir = crate::config::config_dir();
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join("queue.json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path())
}

/// The slice of at most `MAX_TRACKS` kept around `index` (a few behind, the rest ahead).
fn window(len: usize, index: usize) -> (usize, usize) {
    if len <= MAX_TRACKS {
        return (0, len);
    }
    let start = index.saturating_sub(100).min(len - MAX_TRACKS);
    (start, start + MAX_TRACKS)
}

/// A cheap fingerprint of the queue's order, to save only when it changed.
pub fn fingerprint(ids: impl Iterator<Item = u64>) -> u64 {
    ids.fold(0xcbf29ce484222325u64, |h, id| (h ^ id).wrapping_mul(0x100000001b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: u64, local: Option<&str>) -> Track {
        Track { id, title: format!("t{id}"), streamable: true, local_path: local.map(PathBuf::from), ..Default::default() }
    }

    #[test]
    fn tracks_round_trip_through_json() {
        let saved = SavedQueue { tracks: vec![track(1, None), track(2, Some("C:/m/a.flac"))], index: 1, position: 12.5 };
        let back: SavedQueue = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(back, saved);
    }

    #[test]
    fn signed_out_keeps_local_files_and_follows_the_current_track() {
        let saved = SavedQueue {
            tracks: vec![track(1, None), track(2, Some("a.flac")), track(3, None), track(4, Some("b.flac"))],
            index: 3,
            position: 40.0,
        };
        let kept = keep_playable(saved, false, |_| true);
        assert_eq!(kept.tracks.iter().map(|t| t.id).collect::<Vec<_>>(), vec![2, 4]);
        assert_eq!((kept.index, kept.position), (1, 40.0));
    }

    #[test]
    fn a_missing_current_track_starts_the_next_one_from_the_beginning() {
        let saved = SavedQueue { tracks: vec![track(1, Some("gone.flac")), track(2, Some("here.flac"))], index: 0, position: 90.0 };
        let kept = keep_playable(saved, true, |p| p.ends_with("here.flac"));
        assert_eq!(kept.tracks.len(), 1);
        assert_eq!((kept.index, kept.position), (0, 0.0));
    }

    #[test]
    fn long_queues_keep_a_window_around_the_current_track() {
        assert_eq!(window(10, 3), (0, 10));
        assert_eq!(window(MAX_TRACKS + 1000, 50), (0, MAX_TRACKS));
        assert_eq!(window(MAX_TRACKS + 1000, 600), (500, 500 + MAX_TRACKS));
        assert_eq!(window(MAX_TRACKS + 1000, MAX_TRACKS + 999), (1000, MAX_TRACKS + 1000));
    }

    #[test]
    fn fingerprint_changes_with_order() {
        assert_ne!(fingerprint([1, 2].into_iter()), fingerprint([2, 1].into_iter()));
    }
}
