//! Lyrics from several providers: TIDAL first, then LRCLIB (lrclib.net, an
//! open database of synced lyrics with a public API). Synced lyrics win over
//! plain text, whichever provider has them.

use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{parse_lrc, Lyrics, Tidal, Track};

const LRCLIB: &str = "https://lrclib.net/api";

fn from_lrclib(v: &Value) -> Option<Lyrics> {
    let synced = v.get("syncedLyrics").and_then(Value::as_str).map(parse_lrc).unwrap_or_default();
    let text = v.get("plainLyrics").and_then(Value::as_str).unwrap_or("").to_string();
    (!synced.is_empty() || !text.is_empty()).then(|| Lyrics { text, synced, provider: "LRCLIB".into() })
}

fn lrclib(t: &Tidal, track: &Track) -> Result<Lyrics> {
    let http = t.http();
    let artist = track.artists.first().map(|a| a.name.clone()).unwrap_or_default();
    let album = track.album.as_ref().map(|a| a.title.clone()).unwrap_or_default();
    // Exact match by title, artist, album and duration.
    let exact = http
        .get(format!("{LRCLIB}/get"))
        .timeout(Duration::from_secs(8))
        .query(&[
            ("track_name", track.title.as_str()),
            ("artist_name", artist.as_str()),
            ("album_name", album.as_str()),
            ("duration", &track.duration.to_string()),
        ])
        .send();
    if let Ok(resp) = exact {
        if resp.status().is_success() {
            if let Some(l) = resp.json::<Value>().ok().as_ref().and_then(from_lrclib) {
                return Ok(l);
            }
        }
    }
    // Looser search; prefer synced results with a close duration.
    let list: Value = http
        .get(format!("{LRCLIB}/search"))
        .timeout(Duration::from_secs(8))
        .query(&[("track_name", track.title.as_str()), ("artist_name", artist.as_str())])
        .send()?
        .json()?;
    let items = list.as_array().cloned().unwrap_or_default();
    let close = |v: &Value| {
        let d = v.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
        (d - track.duration as f64).abs() <= 3.0
    };
    items
        .iter()
        .filter(|v| close(v))
        .find(|v| v.get("syncedLyrics").and_then(Value::as_str).is_some_and(|s| !s.is_empty()))
        .or_else(|| items.iter().find(|v| close(v)))
        .and_then(from_lrclib)
        .ok_or_else(|| anyhow!("LRCLIB has no lyrics for this track"))
}

impl Tidal {
    /// Best lyrics available for `track` across providers.
    pub fn lyrics_any(&self, track: &Track) -> Result<Lyrics> {
        let tidal = self.lyrics(track.id).ok().filter(|l| !l.synced.is_empty() || !l.text.is_empty()).map(|mut l| {
            l.provider = if l.provider.is_empty() { "TIDAL".into() } else { format!("TIDAL · {}", l.provider) };
            l
        });
        if tidal.as_ref().is_some_and(|l| !l.synced.is_empty()) {
            return Ok(tidal.unwrap());
        }
        match lrclib(self, track) {
            Ok(l) if !l.synced.is_empty() => Ok(l),
            Ok(l) if tidal.is_none() => Ok(l),
            _ => tidal.ok_or_else(|| anyhow!("no lyrics")),
        }
    }
}
