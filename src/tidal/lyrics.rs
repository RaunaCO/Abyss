//! Lyrics from several providers, tried in order: TIDAL, LRCLIB (open database
//! of synced lyrics), NetEase Cloud Music (large synced catalogue, strong for
//! Asian music) and lyrics.ovh (plain text). Synced lyrics win over plain text,
//! whichever provider has them.

use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{parse_lrc, Lyrics, Tidal, Track};

const LRCLIB: &str = "https://lrclib.net/api";

fn from_lrclib(v: &Value) -> Option<Lyrics> {
    let synced = v.get("syncedLyrics").and_then(Value::as_str).map(parse_lrc).unwrap_or_default();
    let text = v.get("plainLyrics").and_then(Value::as_str).unwrap_or("").to_string();
    (!synced.is_empty() || !text.is_empty()).then(|| Lyrics { text, synced, provider: "LRCLIB".into(), roman: Vec::new() })
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

fn netease(t: &Tidal, track: &Track) -> Result<Lyrics> {
    let http = t.http();
    let artist = track.artists.first().map(|a| a.name.clone()).unwrap_or_default();
    let search: Value = http
        .get("https://music.163.com/api/search/get")
        .timeout(Duration::from_secs(8))
        .header("Referer", "https://music.163.com")
        .query(&[("s", format!("{} {}", track.title, artist).as_str()), ("type", "1"), ("limit", "10"), ("offset", "0")])
        .send()?
        .json()?;
    let songs = search.get("result").and_then(|r| r.get("songs")).and_then(Value::as_array).cloned().unwrap_or_default();
    let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    let want = norm(&track.title);
    // Same length (±3 s) and a title that contains, or is contained in, ours.
    let song = songs
        .iter()
        .find(|s| {
            let d = s.get("duration").and_then(Value::as_f64).unwrap_or(0.0) / 1000.0;
            let name = norm(s.get("name").and_then(Value::as_str).unwrap_or(""));
            (d - track.duration as f64).abs() <= 3.0 && !name.is_empty() && (name.contains(&want) || want.contains(&name))
        })
        .ok_or_else(|| anyhow!("NetEase: no match"))?;
    let id = song.get("id").and_then(Value::as_u64).ok_or_else(|| anyhow!("NetEase: no id"))?;
    let lyr: Value = http
        .get("https://music.163.com/api/song/lyric")
        .timeout(Duration::from_secs(8))
        .header("Referer", "https://music.163.com")
        .query(&[("id", id.to_string().as_str()), ("lv", "1"), ("kv", "1"), ("tv", "-1")])
        .send()?
        .json()?;
    let lrc = lyr.get("lrc").and_then(|l| l.get("lyric")).and_then(Value::as_str).unwrap_or("");
    // Drop the credit lines NetEase puts first ("作词 : …", "Composer : …").
    let credit = |line: &str| {
        let l = line.split(']').next_back().unwrap_or("").trim();
        ["作词", "作曲", "编曲", "制作", "混音", "母带", "Lyricist", "Composer", "Arranger", "Producer"]
            .iter()
            .any(|k| l.starts_with(k) && (l.contains(':') || l.contains('：')))
    };
    let cleaned: String = lrc.lines().filter(|l| !credit(l)).collect::<Vec<_>>().join(
        "
",
    );
    let synced = parse_lrc(&cleaned);
    if synced.is_empty() {
        return Err(anyhow!("NetEase: no synced lyrics"));
    }
    let text = synced.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join(
        "
",
    );
    Ok(Lyrics { text, synced, provider: "NetEase".into(), roman: Vec::new() })
}

fn lyrics_ovh(t: &Tidal, track: &Track) -> Result<Lyrics> {
    let artist = track.artists.first().map(|a| a.name.clone()).unwrap_or_default();
    let enc = |s: &str| s.replace('/', " ");
    let v: Value = t
        .http()
        .get(format!("https://api.lyrics.ovh/v1/{}/{}", enc(&artist), enc(&track.title)))
        .timeout(Duration::from_secs(8))
        .send()?
        .json()?;
    let text = v.get("lyrics").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if text.is_empty() {
        return Err(anyhow!("lyrics.ovh: none"));
    }
    Ok(Lyrics { text, synced: Vec::new(), provider: "lyrics.ovh".into(), roman: Vec::new() })
}

impl Tidal {
    /// Best lyrics available for `track` across providers: the first synced
    /// lyrics found, otherwise the first plain text.
    pub fn lyrics_any(&self, track: &Track) -> Result<Lyrics> {
        let mut l = self.lyrics_best(track)?;
        let lines: Vec<String> = l.synced.iter().map(|(_, t)| t.clone()).collect();
        l.roman = crate::romanize::lines(&lines).unwrap_or_default();
        Ok(l)
    }

    fn lyrics_best(&self, track: &Track) -> Result<Lyrics> {
        let mut plain: Option<Lyrics> = None;
        let tidal = self.lyrics(track.id).ok().filter(|l| !l.synced.is_empty() || !l.text.is_empty()).map(|mut l| {
            l.provider = if l.provider.is_empty() { "TIDAL".into() } else { format!("TIDAL · {}", l.provider) };
            l
        });
        let providers: [&dyn Fn() -> Result<Lyrics>; 3] = [&|| lrclib(self, track), &|| netease(self, track), &|| lyrics_ovh(self, track)];
        // Third-party lookups are optional (they disclose what is playing).
        let external = if self.settings.lock().external_lyrics { &providers[..] } else { &providers[..0] };
        for l in std::iter::once(tidal.ok_or_else(|| anyhow!("none"))).chain(external.iter().map(|p| p())) {
            match l {
                Ok(l) if !l.synced.is_empty() => return Ok(l),
                Ok(l) if plain.is_none() && !l.text.is_empty() => plain = Some(l),
                Ok(_) => {}
                Err(e) => log::debug!("lyrics: {e:#}"),
            }
        }
        plain.ok_or_else(|| anyhow!("no lyrics"))
    }
}
