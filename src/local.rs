//! Index and transactional metadata editing for user-selected audio files.
//!
//! Imported files join the Library directly: each track keeps when it was
//! added, its track / disc number, year, album artist and embedded artwork
//! (copied once per album into the configuration folder), and tracks are
//! grouped into albums that open like any other album. Folders that were
//! added are remembered and scanned again at start-up for new files.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use symphonia::core::{
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::{MetadataOptions, MetadataRevision, StandardTagKey, StandardVisualKey},
    probe::Hint,
};

use crate::audio::decode;
use crate::tidal::{Album, AlbumRef, Artist, ArtistPage, ArtistRef, Track};

/// Everything with this bit set is a local id (tracks and albums), never a
/// provider id.
pub const LOCAL_BIT: u64 = 1 << 63;

pub fn is_local_id(id: u64) -> bool {
    id & LOCAL_BIT != 0
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct Entry {
    path: PathBuf,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    duration: u32,
    #[serde(default)]
    genre: Option<String>,
    /// When it was imported ("2026-11-08T10:00:00.000+0000", like TIDAL's).
    #[serde(default)]
    added: Option<String>,
    #[serde(default)]
    track: u32,
    #[serde(default)]
    disc: u32,
    #[serde(default)]
    year: Option<String>,
    #[serde(default)]
    album_artist: Option<String>,
    /// Artwork copied out of the file (shared by the album's tracks).
    #[serde(default)]
    cover: Option<PathBuf>,
}

/// Album-level facts of the tracks currently loaded (year, album artist),
/// which `Track` has no fields for. Keyed by track id.
/// (year, album artist)
type AlbumFacts = (Option<String>, Option<String>, Option<String>);
static EXTRA: Mutex<Option<HashMap<u64, AlbumFacts>>> = Mutex::new(None);
static COVERS: Mutex<Option<HashMap<u64, PathBuf>>> = Mutex::new(None);

pub fn track_cover(id: u64) -> Option<String> {
    COVERS.lock().ok()?.as_ref()?.get(&id).map(|p| format!("local:{}", p.to_string_lossy()))
}

fn hash64(text: &str) -> u64 {
    let hash = Sha256::digest(text.as_bytes());
    let mut id = [0; 8];
    id.copy_from_slice(&hash[..8]);
    u64::from_le_bytes(id) | LOCAL_BIT
}

fn key(s: &str) -> String {
    s.trim().to_lowercase()
}

/// Album identity: album title and album artist (or the track artist).
fn album_id(album: &str, artist: Option<&str>) -> u64 {
    hash64(&format!("album|{}|{}", key(album), key(artist.unwrap_or(""))))
}

fn artist_ref(name: String) -> ArtistRef {
    ArtistRef { id: hash64(&format!("artist|{}", key(&name))), name: name.trim().to_string() }
}

impl Entry {
    fn inspect(path: PathBuf, added: &str) -> Result<Self> {
        let tags = decode::inspect_file(&path)?;
        let title = tags.title.unwrap_or_else(|| {
            path.file_stem().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| crate::i18n::t("audio_file").into())
        });
        let extra = read_extra(&path);
        let mut entry = Self {
            title,
            artist: tags.artist,
            album: tags.album,
            duration: tags.duration.unwrap_or(0.).round() as u32,
            genre: extra.genre,
            added: Some(added.to_string()),
            track: extra.track,
            disc: extra.disc,
            year: extra.year,
            album_artist: extra.album_artist,
            cover: None,
            path,
        };
        if let Some((data, ext)) = extra.cover {
            entry.cover = save_cover(0, &data, ext);
        }
        // Prefer the writable primary tag over stale secondary ID3v1/APE tags.
        if let Ok(file) = lofty::read_from_path(&entry.path) {
            entry.primary_metadata(&file);
        }
        Ok(entry)
    }

    fn primary_metadata(&mut self, file: &lofty::file::TaggedFile) {
        use lofty::{
            file::TaggedFileExt,
            picture::PictureType,
            tag::{Accessor, ItemKey},
        };
        let Some(tag) = file.primary_tag() else { return };
        self.title = tag
            .title()
            .map(|s| s.into_owned())
            .unwrap_or_else(|| self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
        self.artist = tag.artist().map(|s| s.into_owned());
        self.album = tag.album().map(|s| s.into_owned());
        self.album_artist = tag.get_string(ItemKey::AlbumArtist).map(str::to_owned);
        self.genre = tag.genre().map(|s| s.into_owned());
        self.year = tag.get_string(ItemKey::RecordingDate).map(|s| s.chars().take(4).collect());
        self.track = tag.track().unwrap_or(0);
        self.disc = tag.disk().unwrap_or(0);
        self.cover =
            tag.pictures().iter().find(|p| p.pic_type() == PictureType::CoverFront).or_else(|| tag.pictures().first()).and_then(|p| {
                if p.data().len() > 16 * 1024 * 1024 {
                    return None;
                }
                let ext = match image::guess_format(p.data()).ok()? {
                    image::ImageFormat::Png => "png",
                    image::ImageFormat::Jpeg => "jpg",
                    _ => return None,
                };
                save_cover(0, p.data(), ext)
            });
    }

    fn into_track(self) -> Track {
        let id = hash64(&self.path.to_string_lossy());
        if let Ok(mut covers) = COVERS.lock() {
            let covers = covers.get_or_insert_with(HashMap::new);
            if let Some(path) = &self.cover {
                covers.insert(id, path.clone());
            } else {
                covers.remove(&id);
            }
        }
        let album = self.album.as_ref().map(|title| AlbumRef {
            id: album_id(title, self.album_artist.as_deref().or(self.artist.as_deref())),
            title: title.clone(),
            cover: self.cover.as_ref().map(|p| format!("local:{}", p.to_string_lossy())),
        });
        if let Ok(mut extra) = EXTRA.lock() {
            extra.get_or_insert_with(HashMap::new).insert(id, (self.year.clone(), self.album_artist.clone(), self.genre.clone()));
        }
        Track {
            id,
            title: self.title,
            version: None,
            duration: self.duration,
            artists: self.artist.into_iter().filter(|name| !name.trim().is_empty()).map(artist_ref).collect(),
            album,
            explicit: false,
            track_number: self.track,
            volume_number: self.disc,
            audio_quality: String::new(),
            tags: Vec::new(),
            streamable: true,
            added: self.added,
            bpm: None,
            local_path: Some(self.path),
        }
    }

    fn from_track(track: &Track) -> Result<Self> {
        let (year, album_artist, genre) = EXTRA.lock().ok().and_then(|e| e.as_ref()?.get(&track.id).cloned()).unwrap_or_default();
        Ok(Self {
            path: track.local_path.clone().ok_or_else(|| anyhow!(crate::i18n::t("the_local_library_only_accepts_files")))?,
            title: track.title.clone(),
            artist: track.artists.first().map(|a| a.name.clone()),
            album: track.album.as_ref().map(|a| a.title.clone()),
            duration: track.duration,
            added: track.added.clone(),
            track: track.track_number,
            disc: track.volume_number,
            year,
            genre,
            album_artist,
            cover: track_cover(track.id).as_deref().and_then(|c| c.strip_prefix("local:")).map(PathBuf::from).or_else(|| {
                track.album.as_ref().and_then(|a| a.cover.as_deref()).and_then(|c| c.strip_prefix("local:")).map(PathBuf::from)
            }),
        })
    }
}

#[derive(Default)]
struct Extra {
    genre: Option<String>,
    track: u32,
    disc: u32,
    year: Option<String>,
    album_artist: Option<String>,
    cover: Option<(Vec<u8>, &'static str)>,
}

/// Track / disc number, year, album artist and the front cover, read from the
/// file's tags (ID3, Vorbis comments, MP4 atoms).
fn read_extra(path: &Path) -> Extra {
    let mut extra = Extra::default();
    let Ok(file) = fs::File::open(path) else { return extra };
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let Ok(mut probed) = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default()) else {
        return extra;
    };
    if let Some(rev) = probed.metadata.get().as_ref().and_then(|m| m.current()) {
        read_revision(rev, &mut extra);
    }
    if let Some(rev) = probed.format.metadata().current() {
        read_revision(rev, &mut extra);
    }
    extra
}

fn number(text: &str) -> u32 {
    // "3", "3/12", "03 of 12".
    text.trim().split(|c: char| !c.is_ascii_digit()).next().and_then(|n| n.parse().ok()).unwrap_or(0)
}

fn read_revision(rev: &MetadataRevision, extra: &mut Extra) {
    for tag in rev.tags() {
        let value = tag.value.to_string();
        match tag.std_key {
            Some(StandardTagKey::Genre) if extra.genre.is_none() => extra.genre = Some(value.clone()),
            Some(StandardTagKey::TrackNumber) if extra.track == 0 => extra.track = number(&value),
            Some(StandardTagKey::DiscNumber) if extra.disc == 0 => extra.disc = number(&value),
            Some(StandardTagKey::Date | StandardTagKey::ReleaseDate | StandardTagKey::OriginalDate) if extra.year.is_none() => {
                let digits: String = value.chars().filter(char::is_ascii_digit).take(4).collect();
                if digits.len() == 4 {
                    extra.year = Some(digits);
                }
            }
            Some(StandardTagKey::AlbumArtist) if extra.album_artist.is_none() && !value.trim().is_empty() => {
                extra.album_artist = Some(value.trim().to_string());
            }
            _ => {}
        }
    }
    if extra.cover.is_none() {
        let visuals = rev.visuals();
        let front = visuals.iter().find(|v| v.usage == Some(StandardVisualKey::FrontCover)).or_else(|| visuals.first());
        if let Some(v) = front {
            let ext = match v.media_type.to_ascii_lowercase().as_str() {
                "image/png" => Some("png"),
                "image/jpeg" | "image/jpg" => Some("jpg"),
                _ => None,
            };
            // Ignore absurd sizes (corrupt tags).
            if let Some(ext) = ext.filter(|_| v.data.len() <= 16 * 1024 * 1024) {
                extra.cover = Some((v.data.to_vec(), ext));
            }
        }
    }
}

fn covers_dir() -> PathBuf {
    crate::config::config_dir().join("local-covers")
}

/// Copy an album's artwork once; later tracks of the same album reuse it.
fn save_cover(album: u64, data: &[u8], ext: &str) -> Option<PathBuf> {
    let dir = covers_dir();
    let path = dir.join(format!("{:016x}-{:x}.{ext}", album & !LOCAL_BIT, Sha256::digest(data)));
    if path.exists() {
        return Some(path);
    }
    fs::create_dir_all(&dir).ok()?;
    let temp = path.with_extension("tmp");
    let mut f = fs::File::create(&temp).ok()?;
    f.write_all(data).ok()?;
    drop(f);
    fs::rename(&temp, &path).ok()?;
    Some(path)
}

fn index() -> PathBuf {
    crate::config::config_dir().join("local-library.json")
}

fn folders_file() -> PathBuf {
    crate::config::config_dir().join("local-folders.json")
}

pub fn load() -> Result<Vec<Track>> {
    let json = match fs::read(index()) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).context(crate::i18n::t("could_not_read_the_local_library")),
    };
    let entries: Vec<Entry> = serde_json::from_slice(&json).context(crate::i18n::t("could_not_parse_the_local_library"))?;
    Ok(entries
        .into_iter()
        .map(|mut entry| {
            // The copied index may contain absolute artwork paths under the
            // former configuration root. Read the copied artwork in Abyss;
            // paths to the user's original music files remain unchanged.
            if let Some(cover) = entry.cover.as_ref() {
                let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
                let legacy = [base.join("tide").join("local-covers"), base.join("tidal-gpui").join("local-covers")];
                if legacy.iter().any(|dir| cover.parent() == Some(dir.as_path())) {
                    if let Some(name) = cover.file_name() {
                        let copied = covers_dir().join(name);
                        if copied.is_file() {
                            entry.cover = Some(copied);
                        }
                    }
                }
            }
            entry.into_track()
        })
        .collect())
}

pub fn save(tracks: &[Track]) -> Result<()> {
    let path = index();
    let dir = path.parent().context(crate::i18n::t("local_library_folder_not_found"))?;
    let entries = tracks.iter().map(Entry::from_track).collect::<Result<Vec<_>>>()?;
    let json = serde_json::to_vec(&entries)?;
    fs::create_dir_all(dir)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, json)?;
    fs::rename(temp, path).context(crate::i18n::t("local.save_failed_brief"))
}

/// Folders added with "Añadir carpeta" (scanned again at start-up).
pub fn folders() -> Vec<PathBuf> {
    fs::read(folders_file()).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
}

pub fn remember_folders(new: &[PathBuf]) {
    let mut list = folders();
    for f in new {
        if let Ok(f) = fs::canonicalize(f) {
            if !list.contains(&f) {
                list.push(f);
            }
        }
    }
    if let Ok(json) = serde_json::to_vec(&list) {
        let _ = fs::create_dir_all(crate::config::config_dir());
        let _ = fs::write(folders_file(), json);
    }
}

fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "flac" | "mp3" | "wav" | "m4a" | "mp4"))
}

fn linked(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 && meta.is_dir() {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

/// The current time as TIDAL writes "date added" (UTC).
pub fn now_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (y, m, d) = crate::history::civil_from_days(secs.div_euclid(86400));
    let s = secs.rem_euclid(86400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000+0000", s / 3600, s % 3600 / 60, s % 60)
}

#[derive(Default)]
pub struct Imported {
    pub tracks: Vec<Track>,
    pub duplicates: usize,
    pub rejected: usize,
}

/// Import files, or (with `directories`) everything supported inside folders.
/// Mixed lists (drag and drop) work too: folders are walked, files checked.
pub fn import(paths: Vec<PathBuf>, directories: bool, mut known: HashSet<PathBuf>) -> Imported {
    let mut result = Imported::default();
    let added = now_stamp();
    let mut pending = paths;
    while let Some(path) = pending.pop() {
        let Ok(meta) = fs::symlink_metadata(&path) else {
            result.rejected += 1;
            continue;
        };
        if linked(&meta) {
            if !directories {
                result.rejected += 1;
            }
            continue;
        }
        if meta.is_dir() {
            match fs::read_dir(&path) {
                Ok(entries) => pending.extend(entries.filter_map(|e| e.ok().map(|e| e.path()))),
                Err(_) => result.rejected += 1,
            }
        } else if meta.is_file() && supported(&path) {
            match fs::canonicalize(path) {
                Ok(path) if known.insert(path.clone()) => match Entry::inspect(path, &added) {
                    Ok(entry) => result.tracks.push(entry.into_track()),
                    Err(_) => result.rejected += 1,
                },
                Ok(_) => result.duplicates += 1,
                Err(_) => result.rejected += 1,
            }
        } else if !directories {
            result.rejected += 1;
        }
    }
    result.tracks.sort_by(|a, b| {
        let album = |t: &Track| t.album.as_ref().map(|a| key(&a.title)).unwrap_or_default();
        album(a).cmp(&album(b)).then(a.volume_number.cmp(&b.volume_number)).then(a.track_number.cmp(&b.track_number))
    });
    result
}

/// Albums made from the local tracks (tracks without an album tag are left
/// out), each with its tracks in disc / track order.
pub fn albums(tracks: &[Track]) -> Vec<(Album, Vec<Track>)> {
    let extra = EXTRA.lock().ok().and_then(|e| e.clone()).unwrap_or_default();
    let mut groups: HashMap<u64, Vec<Track>> = HashMap::new();
    let mut order: Vec<u64> = Vec::new();
    for t in tracks {
        let Some(a) = &t.album else { continue };
        groups.entry(a.id).or_insert_with(|| {
            order.push(a.id);
            Vec::new()
        });
        if let Some(g) = groups.get_mut(&a.id) {
            g.push(t.clone());
        }
    }
    order
        .into_iter()
        .filter_map(|id| {
            let mut list = groups.remove(&id)?;
            list.sort_by(|a, b| {
                a.volume_number.cmp(&b.volume_number).then(a.track_number.cmp(&b.track_number)).then(a.title.cmp(&b.title))
            });
            let first = list.first()?;
            let (year, album_artist, _) = list.iter().find_map(|t| extra.get(&t.id).cloned()).unwrap_or_default();
            let artist = album_artist.or_else(|| first.artists.first().map(|a| a.name.clone())).unwrap_or_default();
            let a = first.album.clone()?;
            let album = Album {
                id,
                title: a.title,
                cover: a.cover.or_else(|| list.iter().find_map(|t| t.album.as_ref()?.cover.clone())),
                artists: if artist.trim().is_empty() { Vec::new() } else { vec![artist_ref(artist)] },
                release_date: year.unwrap_or_default(),
                number_of_tracks: list.len() as u32,
                duration: list.iter().map(|t| t.duration).sum(),
                kind: "LOCAL".into(),
                explicit: false,
                tags: Vec::new(),
                copyright: String::new(),
            };
            Some((album, list))
        })
        .collect()
}

/// Artist and album-artist tags share stable local identities and ordinary pages.
pub fn artists(tracks: &[Track], albums: &[Album]) -> Vec<ArtistPage> {
    let albums: HashMap<_, _> = albums.iter().map(|a| (a.id, a)).collect();
    let mut pages: HashMap<u64, ArtistPage> = HashMap::new();
    let mut order = Vec::new();
    let mut seen_albums = HashSet::new();
    for track in tracks {
        let album = track.album.as_ref().and_then(|a| albums.get(&a.id).copied());
        let mut credits = HashSet::new();
        for artist in track.artists.iter().chain(album.into_iter().flat_map(|a| &a.artists)) {
            if !credits.insert(artist.id) {
                continue;
            }
            let page = pages.entry(artist.id).or_insert_with(|| {
                order.push(artist.id);
                ArtistPage {
                    artist: Artist { id: artist.id, name: artist.name.clone(), picture: None },
                    top_tracks: Vec::new(),
                    albums: Vec::new(),
                    singles: Vec::new(),
                    appears_on: Vec::new(),
                    similar: Vec::new(),
                    bio: None,
                }
            });
            if page.artist.picture.is_none() {
                page.artist.picture = track.cover(320);
            }
            page.top_tracks.push(track.clone());
            if let Some(album) = album {
                if seen_albums.insert((artist.id, album.id)) {
                    page.albums.push(album.clone());
                }
            }
        }
    }
    order.into_iter().filter_map(|id| pages.remove(&id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_from_tags() {
        assert_eq!(number("3/12"), 3);
        assert_eq!(number(" 07 "), 7);
        assert_eq!(number("x"), 0);
    }

    #[test]
    fn local_ids_are_marked() {
        assert!(is_local_id(album_id("Album", Some("Artist"))));
        assert_eq!(album_id("Album ", Some("ARTIST")), album_id("album", Some("artist")));
    }

    #[test]
    fn stamp_looks_like_tidal() {
        let s = now_stamp();
        assert_eq!(s.len(), "2026-11-08T10:00:00.000+0000".len());
        assert!(s.ends_with("+0000"));
    }
}

#[derive(Clone, Default)]
pub struct Edit {
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub genre: String,
    pub year: String,
    pub track: String,
    pub disc: String,
    /// None keeps artwork; Some(empty) removes it.
    pub cover: Option<PathBuf>,
}

impl Edit {
    pub fn read(track: &Track) -> Result<Self> {
        let path = track.local_path.as_ref().context(crate::i18n::t("local.unsupported_edit"))?;
        // WAV uses its index overrides; taggable formats use the actual file,
        // including fields absent from indexes created by older releases.
        if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
            return Ok(Self::of(track));
        }
        let fresh = Entry::inspect(path.clone(), track.added.as_deref().unwrap_or(""))?.into_track();
        Ok(Self::of(&fresh))
    }

    pub fn of(track: &Track) -> Self {
        let (year, album_artist, genre) = EXTRA.lock().ok().and_then(|e| e.as_ref()?.get(&track.id).cloned()).unwrap_or_default();
        Self {
            title: track.title.clone(),
            artist: track.artist_names(),
            album: track.album.as_ref().map(|a| a.title.clone()).unwrap_or_default(),
            album_artist: album_artist.unwrap_or_default(),
            genre: genre.unwrap_or_default(),
            year: year.unwrap_or_default(),
            track: track.track_number.to_string(),
            disc: track.volume_number.to_string(),
            cover: None,
        }
    }
}

fn backup_dir(path: &Path) -> PathBuf {
    crate::config::config_dir().join("local-originals").join(format!("{:016x}", hash64(&path.to_string_lossy())))
}

pub fn has_original(track: &Track) -> bool {
    track.local_path.as_ref().is_some_and(|p| backup_dir(p).join("entry.json").is_file())
}

/// Stop watching; imported files and the folder itself stay intact.
pub fn forget_folder(path: &Path) -> Result<()> {
    let list: Vec<_> = folders().into_iter().filter(|p| p != path).collect();
    let mut temp = tempfile::NamedTempFile::new_in(crate::config::config_dir())?;
    temp.write_all(&serde_json::to_vec(&list)?)?;
    temp.as_file().sync_all()?;
    temp.persist(folders_file()).map_err(|e| e.error)?;
    Ok(())
}

fn exclusive_file(path: &Path) -> Result<fs::File> {
    if fs::metadata(path)?.permissions().readonly() {
        return Err(anyhow!(crate::i18n::t("local.write_blocked")));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Deny readers/writers, allow atomic replacement while held open.
        options.share_mode(4);
    }
    options.open(path).context(crate::i18n::t("local.write_blocked"))
}

fn original(entry: &Entry, source: Option<&mut fs::File>) -> Result<()> {
    use std::io::{Seek, SeekFrom};
    let dir = backup_dir(&entry.path);
    fs::create_dir_all(&dir)?;
    if dir.join("entry.json").exists() {
        return Ok(());
    }
    if let Some(source) = source {
        if !dir.join("audio").exists() {
            let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
            source.seek(SeekFrom::Start(0))?;
            std::io::copy(source, &mut temp)?;
            temp.as_file().sync_all()?;
            temp.persist_noclobber(dir.join("audio")).map_err(|e| e.error)?;
        }
    }
    let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
    temp.write_all(&serde_json::to_vec(entry)?)?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(dir.join("entry.json")).map_err(|e| e.error)?;
    Ok(())
}

/// All writes go to a sibling temporary, validated and flushed before replacement.
pub fn edit(track: &Track, edit: Option<Edit>) -> Result<Track> {
    use lofty::{
        config::WriteOptions,
        file::{AudioFile, TaggedFileExt},
        picture::{Picture, PictureType},
        tag::{ItemKey, Tag},
    };
    use std::io::{Seek, SeekFrom};
    let entry = Entry::from_track(track)?;
    let path = &entry.path;
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    let wav = ext == "wav";
    anyhow::ensure!(matches!(ext.as_str(), "mp3" | "flac" | "m4a" | "mp4" | "wav"), crate::i18n::t("local.unsupported_edit"));
    let parse = |v: &str| -> Result<u32> {
        if v.trim().is_empty() {
            return Ok(0);
        }
        v.trim().parse().context(crate::i18n::t("local.invalid_number"))
    };
    if let Some(e) = &edit {
        parse(&e.track)?;
        parse(&e.disc)?;
        parse(&e.year)?;
    }
    let mut updated;
    if wav {
        original(&entry, None)?;
        updated = if edit.is_none() { serde_json::from_slice(&fs::read(backup_dir(path).join("entry.json"))?)? } else { entry.clone() };
    } else {
        let mut source = exclusive_file(path)?;
        let mut temp = tempfile::Builder::new()
            .suffix(&format!(".{ext}"))
            .tempfile_in(path.parent().context(crate::i18n::t("local.write_blocked"))?)?;
        if edit.is_none() {
            let mut backup = fs::File::open(backup_dir(path).join("audio"))?;
            std::io::copy(&mut backup, &mut temp)?;
        } else {
            std::io::copy(&mut source, &mut temp)?;
            temp.seek(SeekFrom::Start(0))?;
            let mut tagged = lofty::read_from_path(temp.path())?;
            let tag_type = tagged.primary_tag_type();
            if tagged.primary_tag().is_none() {
                tagged.insert_tag(Tag::new(tag_type));
            }
            let tag = tagged.primary_tag_mut().context(crate::i18n::t("local.unsupported_edit"))?;
            let e = edit.as_ref().unwrap();
            for (key, value) in [
                (ItemKey::TrackTitle, &e.title),
                (ItemKey::TrackArtist, &e.artist),
                (ItemKey::AlbumArtist, &e.album_artist),
                (ItemKey::AlbumTitle, &e.album),
                (ItemKey::Genre, &e.genre),
                (ItemKey::RecordingDate, &e.year),
                (ItemKey::TrackNumber, &e.track),
                (ItemKey::DiscNumber, &e.disc),
            ] {
                tag.remove_key(key);
                if !value.trim().is_empty() {
                    tag.insert_text(key, value.trim().into());
                }
            }
            if let Some(cover) = &e.cover {
                tag.remove_picture_type(PictureType::CoverFront);
                if !cover.as_os_str().is_empty() {
                    anyhow::ensure!(fs::metadata(cover)?.len() <= 16 * 1024 * 1024, crate::i18n::t("local.invalid_cover"));
                    let data = fs::read(cover)?;
                    anyhow::ensure!(
                        matches!(image::guess_format(&data)?, image::ImageFormat::Png | image::ImageFormat::Jpeg),
                        crate::i18n::t("local.invalid_cover")
                    );
                    let mut reader = std::io::Cursor::new(data);
                    let mut picture = Picture::from_reader(&mut reader)?;
                    picture.set_pic_type(PictureType::CoverFront);
                    tag.push_picture(picture);
                } else {
                    while tag.picture_count() > 0 {
                        tag.remove_picture(0);
                    }
                }
            }
            temp.seek(SeekFrom::Start(0))?;
            tagged.save_to(temp.as_file_mut(), WriteOptions::default())?;
        }
        temp.as_file().sync_all()?;
        let verified = lofty::read_from_path(temp.path())?;
        updated = Entry::inspect(temp.path().to_path_buf(), entry.added.as_deref().unwrap_or(""))?;
        updated.path = path.clone();
        updated.primary_metadata(&verified);
        if edit.is_none() && verified.primary_tag().is_none() {
            updated.title = entry.title.clone();
        }
        if edit.as_ref().and_then(|e| e.cover.as_ref()).is_some_and(|p| !p.as_os_str().is_empty()) {
            anyhow::ensure!(updated.cover.is_some(), crate::i18n::t("local.invalid_cover"));
        }
        fs::set_permissions(temp.path(), source.metadata()?.permissions())?;
        original(&entry, Some(&mut source))?;
        temp.persist(path).map_err(|e| e.error).context(crate::i18n::t("local.write_blocked"))?;
    }
    if wav {
        if let Some(e) = edit {
            updated.title = e.title.trim().into();
            let optional = |s: String| (!s.trim().is_empty()).then(|| s.trim().to_string());
            updated.artist = optional(e.artist);
            updated.album = optional(e.album);
            updated.album_artist = optional(e.album_artist);
            updated.genre = optional(e.genre);
            updated.year = optional(e.year);
            updated.track = parse(&e.track)?;
            updated.disc = parse(&e.disc)?;
            if let Some(cover) = e.cover {
                updated.cover = if cover.as_os_str().is_empty() {
                    None
                } else {
                    let data = fs::read(&cover)?;
                    anyhow::ensure!(data.len() <= 16 * 1024 * 1024, crate::i18n::t("local.invalid_cover"));
                    let format = image::guess_format(&data)?;
                    let ext = match format {
                        image::ImageFormat::Png => "png",
                        image::ImageFormat::Jpeg => "jpg",
                        _ => return Err(anyhow!(crate::i18n::t("local.invalid_cover"))),
                    };
                    Some(save_cover(hash64(&path.to_string_lossy()), &data, ext).context(crate::i18n::t("local.invalid_cover"))?)
                };
            }
        }
    }
    Ok(updated.into_track())
}
