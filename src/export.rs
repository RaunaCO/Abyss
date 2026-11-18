//! "Export my data": a folder the user can open in a spreadsheet or take to
//! another computer or player. Nothing leaves the PC; no tokens are written.
//!
//! - `listening-history.csv`: every Replay entry (time, seconds heard, track).
//! - `ratings.csv`: track and album stars, with names when Abyss knows them.
//! - `settings.json`: the settings without the TIDAL session, the Last.fm key or
//!   the account e-mail.
//! - `queue.json`: the current play queue, when there is one.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::Settings;
use crate::history::Entry;

/// Write the export into a new folder inside `parent` and return its path.
pub fn export_all(parent: &Path, settings: &Settings) -> Result<PathBuf> {
    let history = crate::history::load().unwrap_or_default();
    let ratings = crate::ratings::Ratings::load().unwrap_or_default();
    let stamp = {
        let offset = crate::history::local_offset();
        let now = crate::history::unix_now() as i64 + offset;
        let (y, m, d) = crate::history::civil_from_days(now.div_euclid(86_400));
        let secs = now.rem_euclid(86_400);
        format!("{y:04}-{m:02}-{d:02} {:02}{:02}", secs / 3600, secs % 3600 / 60)
    };
    let dir = unique_dir(parent, &format!("Abyss export {stamp}"));
    std::fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
    std::fs::write(dir.join("listening-history.csv"), history_csv(&history))?;
    std::fs::write(dir.join("ratings.csv"), ratings_csv(&ratings.all(), &history))?;
    std::fs::write(dir.join("settings.json"), settings_json(settings)?)?;
    let queue = crate::config::config_dir().join("queue.json");
    if queue.is_file() {
        let _ = std::fs::copy(queue, dir.join("queue.json"));
    }
    Ok(dir)
}

fn unique_dir(parent: &Path, name: &str) -> PathBuf {
    let first = parent.join(name);
    if !first.exists() {
        return first;
    }
    (2..).map(|n| parent.join(format!("{name} ({n})"))).find(|p| !p.exists()).unwrap_or(first)
}

/// A CSV field, quoted when needed (RFC 4180). Leading `= + - @` get a quote
/// mark so a spreadsheet does not run a title as a formula.
fn field(text: &str) -> String {
    let guarded = if text.starts_with(['=', '+', '-', '@']) { format!("'{text}") } else { text.to_string() };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

fn iso_utc(ts: u64) -> String {
    let (y, m, d) = crate::history::civil_from_days((ts / 86_400) as i64);
    let s = ts % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", s / 3600, s % 3600 / 60, s % 60)
}

fn history_csv(entries: &[Entry]) -> String {
    let mut out = String::from("started_utc,seconds_heard,track_length,counts_as_play,title,artist,album,track_id,album_id,artist_id\r\n");
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.ts);
    for e in sorted {
        let _ = write!(
            out,
            "{},{},{},{},{},{},{},{},{},{}\r\n",
            iso_utc(e.ts),
            e.secs,
            e.dur,
            e.counts_as_play(),
            field(&e.title),
            field(&e.artist),
            field(&e.album),
            e.id,
            e.album_id,
            e.artist_id
        );
    }
    out
}

fn ratings_csv(ratings: &[(&'static str, u64, u8)], history: &[Entry]) -> String {
    // Names from the listening history (the ratings store keeps only ids).
    let mut tracks: HashMap<u64, (&str, &str, &str)> = HashMap::new();
    let mut albums: HashMap<u64, (&str, &str)> = HashMap::new();
    for e in history {
        tracks.insert(e.id, (&e.title, &e.artist, &e.album));
        albums.insert(e.album_id, (&e.album, &e.artist));
    }
    let mut out = String::from("kind,id,stars,title,artist,album\r\n");
    for &(kind, id, halves) in ratings {
        let (title, artist, album) = match kind {
            "track" => tracks.get(&id).copied().unwrap_or_default(),
            _ => albums.get(&id).map(|(album, artist)| ("", *artist, *album)).unwrap_or_default(),
        };
        let stars = halves as f32 / 2.0;
        let _ = write!(out, "{kind},{id},{stars},{},{},{}\r\n", field(title), field(artist), field(album));
    }
    out
}

/// The settings as JSON without anything that signs in or identifies the account.
fn settings_json(settings: &Settings) -> Result<Vec<u8>> {
    let mut s = settings.clone();
    s.access_token = None;
    s.refresh_token = None;
    s.protected_tokens = None;
    s.lastfm_session = None;
    s.lastfm_protected_session = None;
    s.user_email = None;
    Ok(serde_json::to_vec_pretty(&s)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ts: u64, title: &str) -> Entry {
        Entry {
            ts,
            secs: 200,
            id: 7,
            title: title.into(),
            artist: "Fairuz".into(),
            artist_id: 1,
            album: "Best, Vol. 1".into(),
            album_id: 9,
            cover: None,
            dur: 240,
        }
    }

    #[test]
    fn csv_fields_are_quoted_and_formulas_disarmed() {
        assert_eq!(field("plain"), "plain");
        assert_eq!(field("a, b"), "\"a, b\"");
        assert_eq!(field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(field("=HYPERLINK(1)"), "'=HYPERLINK(1)");
    }

    #[test]
    fn history_and_ratings_have_names_and_utc_times() {
        let history = vec![entry(86_400 + 3_661, "Li Beirut")];
        let csv = history_csv(&history);
        assert!(csv.contains("1970-01-02T01:01:01Z,200,240,true,Li Beirut,Fairuz,\"Best, Vol. 1\",7,9,1"), "{csv}");
        let csv = ratings_csv(&[("album", 9, 9), ("track", 7, 10)], &history);
        assert!(csv.contains("album,9,4.5,,Fairuz,\"Best, Vol. 1\""), "{csv}");
        assert!(csv.contains("track,7,5,Li Beirut,Fairuz,\"Best, Vol. 1\""), "{csv}");
    }

    #[test]
    fn exported_settings_carry_no_session() {
        let s = Settings {
            access_token: Some("AT-x".into()),
            refresh_token: Some("RT-y".into()),
            lastfm_session: Some("LFM-z".into()),
            user_email: Some("me@example.com".into()),
            volume: 0.4,
            ..Default::default()
        };
        let json = String::from_utf8(settings_json(&s).unwrap()).unwrap();
        for secret in ["AT-x", "RT-y", "LFM-z", "me@example.com"] {
            assert!(!json.contains(secret), "{secret} leaked");
        }
        assert!(json.contains("\"volume\": 0.4"));
    }
}
