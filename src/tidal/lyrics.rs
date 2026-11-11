//! Local and network lyrics with a bounded, language-aware provider chain.

use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{parse_lrc, Lyrics, Tidal, Track};

const LRCLIB: &str = "https://lrclib.net/api";

fn from_lrclib(v: &Value) -> Option<Lyrics> {
    let synced = v.get("syncedLyrics").and_then(Value::as_str).map(parse_lrc).unwrap_or_default();
    let text = v.get("plainLyrics").and_then(Value::as_str).unwrap_or("").to_string();
    (!synced.is_empty() || !text.is_empty()).then(|| Lyrics { text, synced, provider: "LRCLIB".into(), roman: Vec::new() })
}

fn lrclib(t: &Tidal, track: &Track, deadline: Instant) -> Result<Lyrics> {
    let http = t.http();
    let artist = track.artists.first().map(|a| a.name.clone()).unwrap_or_default();
    let album = track.album.as_ref().map(|a| a.title.clone()).unwrap_or_default();
    let mut plain = None;
    // Exact match by title, artist, album and duration.
    let exact = http
        .get(format!("{LRCLIB}/get"))
        .timeout(remaining(deadline)?)
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
                if !l.synced.is_empty() {
                    return Ok(l);
                }
                plain = Some(l);
            }
        }
    }
    // Looser search; prefer synced results with a close duration.
    let search = (|| -> Result<Lyrics> {
        let list: Value = http
            .get(format!("{LRCLIB}/search"))
            .timeout(remaining(deadline)?)
            .query(&[("track_name", track.title.as_str()), ("artist_name", artist.as_str())])
            .send()?
            .error_for_status()?
            .json()?;
        let items = list.as_array().cloned().unwrap_or_default();
        let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
        let close = |v: &Value| {
            let d = v.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
            (d - track.duration as f64).abs() <= 3.0
                && norm(v.get("trackName").and_then(Value::as_str).unwrap_or("")) == norm(&track.title)
                && (artist.is_empty() || norm(v.get("artistName").and_then(Value::as_str).unwrap_or("")) == norm(&artist))
        };
        items
            .iter()
            .filter(|v| close(v))
            .find(|v| v.get("syncedLyrics").and_then(Value::as_str).is_some_and(|s| !s.is_empty()))
            .or_else(|| items.iter().find(|v| close(v)))
            .and_then(from_lrclib)
            .ok_or_else(|| anyhow!(crate::i18n::t("lyrics.lrclib_unavailable")))
    })();
    match search {
        Ok(l) if !l.synced.is_empty() => Ok(l),
        other => plain.map(Ok).unwrap_or(other),
    }
}

fn netease(t: &Tidal, track: &Track, deadline: Instant) -> Result<Lyrics> {
    let http = t.http();
    let artist = track.artists.first().map(|a| a.name.clone()).unwrap_or_default();
    let search: Value = http
        .get("https://music.163.com/api/search/get")
        .timeout(remaining(deadline)?)
        .header("Referer", "https://music.163.com")
        .query(&[("s", format!("{} {}", track.title, artist).as_str()), ("type", "1"), ("limit", "10"), ("offset", "0")])
        .send()?
        .json()?;
    let songs = search.get("result").and_then(|r| r.get("songs")).and_then(Value::as_array).cloned().unwrap_or_default();
    let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    let want = norm(&track.title);
    let want_artist = norm(&artist);
    // Same length (±3 s) and a title that contains, or is contained in, ours.
    let song = songs
        .iter()
        .find(|s| {
            let d = s.get("duration").and_then(Value::as_f64).unwrap_or(0.0) / 1000.0;
            let name = norm(s.get("name").and_then(Value::as_str).unwrap_or(""));
            let artist_matches = want_artist.is_empty()
                || s.get("artists").and_then(Value::as_array).is_some_and(|artists| {
                    artists.iter().any(|a| norm(a.get("name").and_then(Value::as_str).unwrap_or("")) == want_artist)
                });
            (d - track.duration as f64).abs() <= 3.0 && !name.is_empty() && name == want && artist_matches
        })
        .ok_or_else(|| anyhow!(crate::i18n::t("lyrics.netease_no_match")))?;
    let id = song.get("id").and_then(Value::as_u64).ok_or_else(|| anyhow!(crate::i18n::t("lyrics.netease_no_id")))?;
    let lyr: Value = http
        .get("https://music.163.com/api/song/lyric")
        .timeout(remaining(deadline)?)
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
        return Err(anyhow!(crate::i18n::t("lyrics.netease_no_synced")));
    }
    let text = synced.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join(
        "
",
    );
    Ok(Lyrics { text, synced, provider: "NetEase".into(), roman: Vec::new() })
}

fn lyrics_ovh(t: &Tidal, track: &Track, deadline: Instant) -> Result<Lyrics> {
    let artist = track.artists.first().map(|a| a.name.clone()).unwrap_or_default();
    let mut url = reqwest::Url::parse("https://api.lyrics.ovh/v1/")?;
    url.path_segments_mut().map_err(|_| anyhow!(crate::i18n::t("lyrics.ovh_unavailable")))?.pop_if_empty().push(&artist).push(&track.title);
    let v: Value = t.http().get(url).timeout(remaining(deadline)?).send()?.json()?;
    let text = v.get("lyrics").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if text.is_empty() {
        return Err(anyhow!(crate::i18n::t("lyrics.ovh_unavailable")));
    }
    Ok(Lyrics { text, synced: Vec::new(), provider: "lyrics.ovh".into(), roman: Vec::new() })
}

const BUDGET: Duration = Duration::from_secs(8);
const MISS_TTL: u64 = 15 * 60;
const MAX_MISSES: usize = 2048;

fn remaining(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .and_then(|d| d.checked_sub(Duration::from_millis(100)))
        .filter(|d| !d.is_zero())
        .ok_or_else(|| anyhow!(crate::i18n::t("lyrics.provider_timeout")))
}

fn from_text(text: String, provider: &str) -> Result<Lyrics> {
    let text = text.trim_start_matches('\u{feff}').trim().to_string();
    anyhow::ensure!(!text.is_empty(), crate::i18n::t("no_provider_has_lyrics_for_this_track"));
    let synced = parse_lrc(&text);
    let text = if synced.is_empty() { text } else { synced.iter().map(|(_, line)| line.as_str()).collect::<Vec<_>>().join("\n") };
    Ok(Lyrics { text, synced, provider: provider.into(), roman: Vec::new() })
}

fn sidecar(path: &Path) -> Result<Lyrics> {
    let mut data = Vec::new();
    fs::File::open(path.with_extension("lrc"))?.take(1024 * 1024 + 1).read_to_end(&mut data)?;
    anyhow::ensure!(data.len() <= 1024 * 1024, crate::i18n::t("lyrics.local_too_large"));
    let text = if data.starts_with(&[0xff, 0xfe]) || data.starts_with(&[0xfe, 0xff]) {
        let little = data[0] == 0xff;
        let words: Vec<_> = data[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| if little { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) })
            .collect();
        String::from_utf16(&words)?
    } else {
        String::from_utf8(data)?
    };
    from_text(text, crate::i18n::t("lyrics.sidecar"))
}

fn embedded(path: &Path) -> Result<Lyrics> {
    use lofty::{file::TaggedFileExt, tag::ItemKey};
    let file =
        lofty::probe::Probe::open(path)?.options(lofty::config::ParseOptions::new().read_properties(false).read_cover_art(false)).read()?;
    let mut plain = None;
    for tag in file.tags() {
        for key in [ItemKey::Lyrics, ItemKey::UnsyncLyrics] {
            for text in tag.get_strings(key) {
                if let Ok(l) = from_text(text.to_string(), crate::i18n::t("lyrics.embedded")) {
                    if !l.synced.is_empty() {
                        return Ok(l);
                    }
                    if plain.is_none() {
                        plain = Some(l);
                    }
                }
            }
        }
    }
    plain.ok_or_else(|| anyhow!(crate::i18n::t("no_provider_has_lyrics_for_this_track")))
}

fn cjk(track: &Track) -> bool {
    // There is no language field in service metadata. Script is a heuristic,
    // never the user's interface language. Ambiguous Latin titles use LRCLIB.
    track.title.chars().chain(track.artist_names().chars()).any(|c| {
        matches!(c as u32,
        0x3040..=0x30ff | 0x3400..=0x9fff | 0xac00..=0xd7af | 0x20000..=0x3134f)
    })
}

fn miss_key(track: &Track, external: bool) -> String {
    let mut hash = Sha256::new();
    hash.update(format!(
        "{}|{}|{}|{}|{}|{}",
        track.id,
        track.title,
        track.artist_names(),
        track.album.as_ref().map(|a| a.title.as_str()).unwrap_or(""),
        track.duration,
        external
    ));
    if let Some(path) = &track.local_path {
        for path in [path.clone(), path.with_extension("lrc")] {
            hash.update(path.to_string_lossy().as_bytes());
            if let Ok(meta) = fs::metadata(path) {
                hash.update(format!("{}|{:?}", meta.len(), meta.modified().ok()));
            }
        }
    }
    format!("{:x}", hash.finalize())
}

fn misses() -> &'static Mutex<HashMap<String, u64>> {
    static CACHE: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(
            fs::read(crate::config::config_dir().join("lyrics-missing.json"))
                .ok()
                .and_then(|d| serde_json::from_slice(&d).ok())
                .unwrap_or_default(),
        )
    })
}

fn remember_missing(key: String) {
    let mut cache = misses().lock().unwrap_or_else(|e| e.into_inner());
    let now = super::now();
    cache.retain(|_, at| now.saturating_sub(*at) < MISS_TTL);
    if cache.len() >= MAX_MISSES {
        if let Some(oldest) = cache.iter().min_by_key(|(_, at)| *at).map(|(k, _)| k.clone()) {
            cache.remove(&oldest);
        }
    }
    cache.insert(key, now);
    persist_misses(&cache);
}

fn persist_misses(cache: &HashMap<String, u64>) {
    if let Ok(json) = serde_json::to_vec(cache) {
        let dir = crate::config::config_dir();
        let result = (|| -> Result<()> {
            use std::io::Write;
            fs::create_dir_all(&dir)?;
            let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
            temp.write_all(&json)?;
            temp.as_file().sync_all()?;
            temp.persist(dir.join("lyrics-missing.json")).map_err(|e| e.error)?;
            Ok(())
        })();
        if let Err(e) = result {
            log::debug!("lyrics miss cache: {e:#}");
        }
    }
}

pub fn forget_missing(track: &Track) {
    let mut cache = misses().lock().unwrap_or_else(|e| e.into_inner());
    cache.remove(&miss_key(track, true));
    cache.remove(&miss_key(track, false));
    persist_misses(&cache);
}

// Bound abandoned local reads if an unavailable network drive stalls the OS.
static LOCAL_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct LocalRead;
impl Drop for LocalRead {
    fn drop(&mut self) {
        LOCAL_READS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[derive(Clone, Copy)]
enum Provider {
    Embedded,
    Sidecar,
    Lrclib,
    Service,
    Netease,
    Ovh,
}

impl Tidal {
    pub fn lyrics_any(&self, track: &Track) -> Result<Lyrics> {
        let external = self.settings.lock().external_lyrics;
        let key = miss_key(track, external);
        let missing =
            misses().lock().unwrap_or_else(|e| e.into_inner()).get(&key).is_some_and(|at| super::now().saturating_sub(*at) < MISS_TTL);
        if missing {
            return Err(anyhow!(crate::i18n::t("no_provider_has_lyrics_for_this_track")));
        }
        let mut l = match self.lyrics_best(track, external) {
            Ok(l) => l,
            Err(e) => {
                if self.settings.lock().external_lyrics == external {
                    remember_missing(key);
                }
                return Err(e);
            }
        };
        let lines: Vec<String> = if l.synced.is_empty() {
            l.text.lines().map(str::to_owned).collect()
        } else {
            l.synced.iter().map(|(_, t)| t.clone()).collect()
        };
        l.roman = crate::romanize::lines(&lines).unwrap_or_default();
        Ok(l)
    }

    fn lyrics_best(&self, track: &Track, external: bool) -> Result<Lyrics> {
        use Provider::*;
        let mut providers = Vec::new();
        if track.local_path.is_some() {
            providers.extend([Embedded, Sidecar]);
        }
        if external {
            if cjk(track) {
                providers.extend([Netease, Lrclib]);
            } else {
                providers.push(Lrclib);
            }
        }
        if track.local_path.is_none() {
            providers.push(Service);
        }
        if external {
            if !cjk(track) {
                providers.push(Netease);
            }
            providers.push(Ovh);
        }
        let mut plain = None;
        for provider in providers {
            if matches!(provider, Lrclib | Netease | Ovh) && !self.settings.lock().external_lyrics {
                continue;
            }
            let permit = if matches!(provider, Embedded | Sidecar) {
                let count = LOCAL_READS.load(std::sync::atomic::Ordering::Acquire);
                if count >= 2
                    || LOCAL_READS
                        .compare_exchange(count, count + 1, std::sync::atomic::Ordering::AcqRel, std::sync::atomic::Ordering::Acquire)
                        .is_err()
                {
                    continue;
                }
                Some(LocalRead)
            } else {
                None
            };
            let client = self.clone();
            let track = track.clone();
            let deadline = Instant::now() + BUDGET;
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            // The caller also has an 8 s budget for local I/O. Network requests
            // share the provider's deadline, including search + lyric retrieval.
            let spawn = std::thread::Builder::new().name("lyrics-provider".into()).spawn(move || {
                let _permit = permit;
                let result = match provider {
                    Embedded => embedded(track.local_path.as_deref().unwrap()),
                    Sidecar => sidecar(track.local_path.as_deref().unwrap()),
                    Lrclib => lrclib(&client, &track, deadline),
                    Service => client.lyrics(track.id).map(|mut l| {
                        l.provider = if l.provider.is_empty() { "TIDAL".into() } else { format!("TIDAL ? {}", l.provider) };
                        l
                    }),
                    Netease => netease(&client, &track, deadline),
                    Ovh => lyrics_ovh(&client, &track, deadline),
                };
                let _ = tx.send(result);
            });
            if let Err(e) = spawn {
                log::debug!("lyrics worker: {e}");
                continue;
            }
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(Ok(l)) if !l.synced.is_empty() => return Ok(l),
                Ok(Ok(l)) if plain.is_none() && !l.text.trim().is_empty() => plain = Some(l),
                Ok(Err(e)) => log::debug!("lyrics: {e:#}"),
                Err(e) => log::debug!("lyrics provider deadline: {e}"),
                _ => {}
            }
        }
        plain.ok_or_else(|| anyhow!(crate::i18n::t("no_provider_has_lyrics_for_this_track")))
    }
}
