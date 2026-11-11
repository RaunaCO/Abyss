//! Data types for the TIDAL catalogue, parsed leniently from JSON.

use serde_json::Value;

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}
fn opt_s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).filter(|x| !x.is_empty()).map(str::to_string)
}
fn u(v: &Value, k: &str) -> u64 {
    match v.get(k) {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
        Some(Value::String(x)) => x.parse().unwrap_or(0),
        _ => 0,
    }
}
fn tags(v: &Value) -> Vec<String> {
    v.get("mediaMetadata")
        .and_then(|m| m.get("tags"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Image URL from a TIDAL image id (`xxxxxxxx-xxxx-...`).
pub fn image_url(id: &str, size: u32) -> String {
    // Artwork of local files: a path, read from disk by the image loader.
    if id.starts_with("local:") || id.starts_with("http") {
        return id.to_string();
    }
    format!("https://resources.tidal.com/images/{}/{size}x{size}.jpg", id.replace('-', "/"))
}

/// Public TIDAL catalogue link.
pub fn share_url(kind: &str, id: &str) -> String {
    format!("https://tidal.com/browse/{kind}/{id}")
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArtistRef {
    pub id: u64,
    pub name: String,
}

impl ArtistRef {
    fn parse(v: &Value) -> Self {
        Self { id: u(v, "id"), name: s(v, "name") }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlbumRef {
    pub id: u64,
    pub title: String,
    pub cover: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: u64,
    pub title: String,
    pub version: Option<String>,
    pub duration: u32,
    pub artists: Vec<ArtistRef>,
    pub album: Option<AlbumRef>,
    pub explicit: bool,
    pub track_number: u32,
    pub volume_number: u32,
    pub audio_quality: String,
    pub tags: Vec<String>,
    pub streamable: bool,
    /// When it was added to the list it is shown in (ISO 8601).
    pub added: Option<String>,
    pub bpm: Option<u32>,
    pub local_path: Option<std::path::PathBuf>,
}

impl Track {
    /// Take the "date added" from the list entry that wraps the track.
    pub fn with_added(mut self, entry: &Value) -> Self {
        self.added = entry.get("created").or_else(|| entry.get("dateAdded")).and_then(Value::as_str).map(str::to_string);
        self
    }

    pub fn parse(v: &Value) -> Option<Self> {
        let id = u(v, "id");
        if id == 0 {
            return None;
        }
        let artists = v
            .get("artists")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(ArtistRef::parse).collect())
            .or_else(|| v.get("artist").map(|a| vec![ArtistRef::parse(a)]))
            .unwrap_or_default();
        let album =
            v.get("album").filter(|a| a.is_object()).map(|a| AlbumRef { id: u(a, "id"), title: s(a, "title"), cover: opt_s(a, "cover") });
        Some(Self {
            id,
            title: s(v, "title"),
            version: opt_s(v, "version"),
            duration: u(v, "duration") as u32,
            artists,
            album,
            explicit: v.get("explicit").and_then(Value::as_bool).unwrap_or(false),
            track_number: u(v, "trackNumber") as u32,
            volume_number: u(v, "volumeNumber").max(1) as u32,
            audio_quality: s(v, "audioQuality"),
            tags: tags(v),
            streamable: v.get("streamReady").and_then(Value::as_bool).unwrap_or(true)
                && v.get("allowStreaming").and_then(Value::as_bool).unwrap_or(true),
            added: None,
            local_path: None,
            bpm: v.get("bpm").and_then(Value::as_u64).filter(|b| *b > 0).map(|b| b as u32),
        })
    }

    pub fn is_hires(&self) -> bool {
        self.tags.iter().any(|t| t == "HIRES_LOSSLESS")
    }

    pub fn display_title(&self) -> String {
        match &self.version {
            Some(v) => format!("{} ({v})", self.title),
            None => self.title.clone(),
        }
    }

    pub fn artist_names(&self) -> String {
        self.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
    }

    pub fn cover(&self, size: u32) -> Option<String> {
        if self.local_path.is_some() {
            if let Some(cover) = crate::local::track_cover(self.id) {
                return Some(cover);
            }
        }
        self.album.as_ref()?.cover.as_deref().map(|c| image_url(c, size))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Album {
    pub id: u64,
    pub title: String,
    pub cover: Option<String>,
    pub artists: Vec<ArtistRef>,
    pub release_date: String,
    pub number_of_tracks: u32,
    pub duration: u32,
    pub kind: String,
    pub explicit: bool,
    pub tags: Vec<String>,
    pub copyright: String,
}

impl Album {
    pub fn parse(v: &Value) -> Option<Self> {
        let id = u(v, "id");
        if id == 0 {
            return None;
        }
        let artists = v
            .get("artists")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(ArtistRef::parse).collect())
            .or_else(|| v.get("artist").map(|a| vec![ArtistRef::parse(a)]))
            .unwrap_or_default();
        Some(Self {
            id,
            title: s(v, "title"),
            cover: opt_s(v, "cover"),
            artists,
            release_date: s(v, "releaseDate"),
            number_of_tracks: u(v, "numberOfTracks") as u32,
            duration: u(v, "duration") as u32,
            kind: s(v, "type"),
            explicit: v.get("explicit").and_then(Value::as_bool).unwrap_or(false),
            tags: tags(v),
            copyright: s(v, "copyright"),
        })
    }

    pub fn artist_names(&self) -> String {
        self.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
    }

    pub fn cover(&self, size: u32) -> Option<String> {
        self.cover.as_deref().map(|c| image_url(c, size))
    }

    pub fn year(&self) -> &str {
        self.release_date.get(..4).unwrap_or("")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Artist {
    pub id: u64,
    pub name: String,
    pub picture: Option<String>,
}

impl Artist {
    pub fn parse(v: &Value) -> Option<Self> {
        let id = u(v, "id");
        (id != 0).then(|| Self { id, name: s(v, "name"), picture: opt_s(v, "picture") })
    }

    pub fn picture(&self, size: u32) -> Option<String> {
        self.picture.as_deref().map(|c| image_url(c, size))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Playlist {
    pub uuid: String,
    pub title: String,
    pub description: String,
    pub image: Option<String>,
    pub square_image: Option<String>,
    pub number_of_tracks: u32,
    pub duration: u32,
    pub creator: String,
    pub creator_id: u64,
    pub kind: String,
}

impl Playlist {
    pub fn parse(v: &Value) -> Option<Self> {
        let uuid = s(v, "uuid");
        if uuid.is_empty() {
            return None;
        }
        let creator = v.get("creator").cloned().unwrap_or(Value::Null);
        let mut creator_name = s(&creator, "name");
        if creator_name.is_empty() {
            creator_name = if s(v, "type") == "EDITORIAL" { "TIDAL".into() } else { String::new() };
        }
        Some(Self {
            uuid,
            title: s(v, "title"),
            description: s(v, "description"),
            image: opt_s(v, "image"),
            square_image: opt_s(v, "squareImage"),
            number_of_tracks: u(v, "numberOfTracks") as u32,
            duration: u(v, "duration") as u32,
            creator: creator_name,
            creator_id: u(&creator, "id"),
            kind: s(v, "type"),
        })
    }

    pub fn cover(&self, size: u32) -> Option<String> {
        if let Some(url) = self.image.as_deref().filter(|i| i.starts_with("http")) {
            return Some(url.to_string());
        }
        if let Some(sq) = &self.square_image {
            let sz = [160, 320, 480, 640, 750, 1080].into_iter().find(|&x| x >= size).unwrap_or(1080);
            return Some(format!("https://resources.tidal.com/images/{}/{sz}x{sz}.jpg", sq.replace('-', "/")));
        }
        self.image.as_deref().map(|i| format!("https://resources.tidal.com/images/{}/480x320.jpg", i.replace('-', "/")))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mix {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub image: Option<String>,
}

impl Mix {
    pub fn parse(v: &Value) -> Option<Self> {
        let id = s(v, "id");
        if id.is_empty() {
            return None;
        }
        let images = v.get("images");
        let image = ["MEDIUM", "LARGE", "SMALL"].iter().find_map(|k| images?.get(*k)?.get("url")?.as_str().map(str::to_string));
        Some(Self { id, title: s(v, "title"), subtitle: s(v, "subTitle"), image })
    }
}

impl Mix {
    /// A mix as the home feed describes it (v2 API: text infos and image list).
    pub fn parse_feed(v: &Value) -> Option<Self> {
        let id = s(v, "id");
        if id.is_empty() {
            return None;
        }
        let text = |k: &str| v.get(k).and_then(|t| t.get("text")).and_then(Value::as_str).unwrap_or("").to_string();
        let images = v.get("mixImages").and_then(Value::as_array);
        let image = ["MEDIUM", "LARGE", "SMALL"].iter().find_map(|size| {
            images?.iter().find(|i| i.get("size").and_then(Value::as_str) == Some(*size))?.get("url")?.as_str().map(str::to_string)
        });
        Some(Self { id, title: text("titleTextInfo"), subtitle: text("subtitleTextInfo"), image })
    }
}

/// One card in a horizontal row (home page, artist page, ...).
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Album(Album),
    Playlist(Playlist),
    Artist(Artist),
    Mix(Mix),
    Track(Track),
    /// A link to an editorial page (genre, mood, decade...).
    Link(Link),
    /// A featured promotion with its category label.
    Promo(Promo),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub title: String,
    /// Page name for `/pages/{path}`.
    pub path: String,
    pub icon: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Promo {
    pub label: String,
    pub title: String,
    pub subtitle: String,
    pub image: Option<String>,
    pub kind: String,
    pub target: String,
}

impl Promo {
    pub fn image_url(&self) -> Option<String> {
        self.image.as_ref().map(|i| format!("https://resources.tidal.com/images/{}/550x400.jpg", i.replace('-', "/")))
    }
}

impl Item {
    pub fn parse_typed(kind: &str, v: &Value) -> Option<Self> {
        match kind {
            "ALBUM" | "ALBUM_LIST" => Album::parse(v).map(Item::Album),
            "PLAYLIST" | "PLAYLIST_LIST" | "USER_PLAYLIST" => Playlist::parse(v).map(Item::Playlist),
            "ARTIST" | "ARTIST_LIST" => Artist::parse(v).map(Item::Artist),
            "MIX" | "MIX_LIST" => Mix::parse(v).map(Item::Mix),
            "TRACK" | "TRACK_LIST" => Track::parse(v).map(Item::Track),
            _ => None,
        }
    }
}

/// How a row is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Cards,
    /// Rounded chips (genres, moods, decades).
    Chips,
    /// Plain links with an icon.
    Links,
    /// Big promotion cards with a category label.
    Featured,
    /// The 3x2 grid of quick links at the top of "For you".
    Shortcuts,
}

/// A titled row of items.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub title: String,
    pub items: Vec<Item>,
    pub kind: RowKind,
}

#[derive(Debug, Clone, Default)]
pub struct SearchResults {
    pub top: Option<Item>,
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
}

/// Where the audio of a track comes from.
#[derive(Debug, Clone)]
pub enum StreamSource {
    /// One progressive file (FLAC or MP4/AAC).
    Single(String),
    /// DASH: init segment followed by media segments, fragmented MP4.
    Segments(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub source: StreamSource,
    /// DASH only: seconds per media segment, used to jump straight to a position.
    pub seg_secs: Vec<f64>,
    pub codec: String,
    pub audio_quality: String,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<u32>,
    /// Loudness normalisation: gain in dB and sample peak (linear) of the track.
    pub replay_gain: Option<f32>,
    pub peak: Option<f32>,
    /// Fetches fresh signed links for the same stream when these expire.
    pub renew: Option<Renewer>,
}

/// Callback that resolves the same stream again (new signed URLs).
#[derive(Clone)]
pub struct Renewer(pub crate::audio::stream::Renew);

impl std::fmt::Debug for Renewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Renewer")
    }
}

impl StreamInfo {
    /// Text for the quality badge, e.g. "24-BIT 96 kHz FLAC".
    pub fn detail(&self) -> String {
        let mut parts = Vec::new();
        if let Some(b) = self.bit_depth {
            parts.push(crate::i18n::text("audio.bit_depth", &[b.to_string()]));
        }
        if let Some(r) = self.sample_rate {
            let khz = r as f32 / 1000.0;
            parts.push(if khz.fract() == 0.0 {
                crate::i18n::text("khz", &[format!("{khz:.0}")])
            } else {
                crate::i18n::text("khz", &[format!("{khz:.1}")])
            });
        }
        parts.push(self.codec.to_uppercase());
        parts.join(" ")
    }

    pub fn tier(&self) -> &'static str {
        match self.audio_quality.as_str() {
            "HI_RES_LOSSLESS" | "HI_RES" => "MAX",
            "LOSSLESS" => "HIGH",
            _ => "LOW",
        }
    }
}
