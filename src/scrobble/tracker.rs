//! Decides when a listen counts as a scrobble. Pure state, no I/O.
//!
//! Last.fm rule: the track is longer than 30 seconds and was played for at
//! least half of its length or 4 minutes, whichever comes first.

use super::{Item, Meta};
use crate::tidal::Track;

/// Tracks of 30 seconds or less are never scrobbled.
pub const MIN_TRACK_SECS: u32 = 30;
/// Listening this long always counts, however long the track is.
pub const MAX_THRESHOLD_SECS: u32 = 240;

/// Whether `heard` seconds of a track of `duration` seconds is a scrobble.
pub fn eligible(duration: u32, heard: u32) -> bool {
    duration > MIN_TRACK_SECS && (heard.saturating_mul(2) >= duration || heard >= MAX_THRESHOLD_SECS)
}

impl Meta {
    /// Artist and title are required by Last.fm; a track without them is skipped.
    pub fn from_track(track: &Track) -> Option<Self> {
        let artist = track.artists.first().map(|a| a.name.trim().to_string()).filter(|a| !a.is_empty())?;
        let title = track.title.trim().to_string();
        if title.is_empty() {
            return None;
        }
        Some(Self {
            artist,
            track: title,
            album: track.album.as_ref().map(|a| a.title.trim().to_string()).unwrap_or_default(),
            duration: track.duration,
        })
    }
}

/// What the app should send.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// The track just started: update "now playing".
    NowPlaying(Meta),
    /// The listen counts: scrobble it.
    Scrobble(Item),
}

struct Current {
    key: u64,
    meta: Option<Meta>,
    start: u64,
    heard: f32,
    announced: bool,
    scrobbled: bool,
}

#[derive(Default)]
pub struct Tracker {
    current: Option<Current>,
}

impl Tracker {
    /// `track` is the engine key and track now playing, `audible` whether sound
    /// is actually coming out, `dt` the seconds since the last call and `now` the
    /// Unix time. Only audible seconds count, so pausing or seeking cannot
    /// inflate a listen.
    pub fn tick(&mut self, track: Option<(u64, &Track)>, audible: bool, dt: f32, now: u64) -> Vec<Event> {
        let key = track.map(|(key, _)| key);
        if self.current.as_ref().map(|c| c.key) != key {
            self.current =
                track.map(|(key, t)| Current { key, meta: Meta::from_track(t), start: now, heard: 0., announced: false, scrobbled: false });
        }
        let mut events = Vec::new();
        let Some(cur) = self.current.as_mut() else { return events };
        let Some(meta) = cur.meta.as_ref() else { return events };
        if !audible {
            return events;
        }
        if !cur.announced {
            cur.announced = true;
            cur.start = now;
            events.push(Event::NowPlaying(meta.clone()));
        }
        cur.heard += dt.max(0.);
        if !cur.scrobbled && eligible(meta.duration, cur.heard as u32) {
            cur.scrobbled = true;
            events.push(Event::Scrobble(Item { id: format!("live:{:x}:{:x}", cur.start, cur.key), ts: cur.start, meta: meta.clone() }));
        }
        events
    }

    /// A quality change restarts the engine under a new key without a new listen.
    pub fn rekey(&mut self, old: u64, new: u64) {
        if let Some(cur) = self.current.as_mut().filter(|c| c.key == old) {
            cur.key = new;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tidal::models::{AlbumRef, ArtistRef};

    fn track(duration: u32) -> Track {
        Track {
            id: 1,
            title: "Song".into(),
            version: None,
            duration,
            artists: vec![ArtistRef { id: 1, name: "Artist".into() }],
            album: Some(AlbumRef { id: 2, title: "Album".into(), cover: None }),
            explicit: false,
            track_number: 1,
            volume_number: 1,
            audio_quality: String::new(),
            tags: Vec::new(),
            streamable: true,
            added: None,
            bpm: None,
            local_path: None,
        }
    }

    #[test]
    fn threshold_edges() {
        assert!(!eligible(29, 29));
        assert!(!eligible(30, 30), "30 s or less is never scrobbled");
        assert!(!eligible(31, 15));
        assert!(eligible(31, 16), "half of a short track");
        assert!(!eligible(200, 99));
        assert!(eligible(200, 100));
        assert!(!eligible(1200, 239), "a long track needs 4 minutes");
        assert!(eligible(1200, 240));
        assert!(!eligible(0, 500), "unknown length is not scrobbled");
    }

    #[test]
    fn announces_once_and_scrobbles_once() {
        let t = track(100);
        let mut tracker = Tracker::default();
        let first = tracker.tick(Some((7, &t)), true, 1., 1_000);
        assert_eq!(first, vec![Event::NowPlaying(Meta::from_track(&t).unwrap())]);
        assert!(tracker.tick(Some((7, &t)), true, 48., 1_001).is_empty());
        let scrobble = tracker.tick(Some((7, &t)), true, 1., 1_049);
        assert_eq!(scrobble.len(), 1);
        match &scrobble[0] {
            Event::Scrobble(item) => {
                assert_eq!(item.ts, 1_000, "the listen is stamped with its start");
                assert_eq!(item.id, "live:3e8:7");
            }
            other => panic!("{other:?}"),
        }
        assert!(tracker.tick(Some((7, &t)), true, 60., 1_100).is_empty(), "never twice");
    }

    #[test]
    fn paused_time_does_not_count_and_does_not_announce() {
        let t = track(100);
        let mut tracker = Tracker::default();
        assert!(tracker.tick(Some((1, &t)), false, 500., 10).is_empty());
        assert!(!tracker.tick(Some((1, &t)), true, 1., 11).is_empty());
        assert!(tracker.tick(Some((1, &t)), false, 500., 12).is_empty());
    }

    #[test]
    fn a_new_key_starts_a_new_listen_and_rekey_keeps_it() {
        let t = track(100);
        let mut tracker = Tracker::default();
        tracker.tick(Some((1, &t)), true, 40., 0);
        tracker.rekey(1, 2);
        let events = tracker.tick(Some((2, &t)), true, 11., 40);
        assert!(matches!(events.as_slice(), [Event::Scrobble(_)]), "same listen continues after a quality restart: {events:?}");
        let again = tracker.tick(Some((3, &t)), true, 1., 60);
        assert!(matches!(again.as_slice(), [Event::NowPlaying(_)]), "a repeat has a new key");
    }

    #[test]
    fn tracks_without_artist_or_title_are_skipped() {
        let mut t = track(100);
        t.artists.clear();
        let mut tracker = Tracker::default();
        assert!(tracker.tick(Some((1, &t)), true, 100., 0).is_empty());
        let mut t = track(100);
        t.title = "  ".into();
        assert!(tracker.tick(Some((2, &t)), true, 100., 0).is_empty());
    }
}
