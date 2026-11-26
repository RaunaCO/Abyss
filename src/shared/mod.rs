//! State and calculations shared by every interface (GPUI reference and Iced).
//! Nothing here depends on a UI toolkit.

pub mod eqgraph;
pub mod eqmeter;
pub mod lyrics;
pub mod queue;
pub mod replay;
pub mod zoom;

use crate::tidal::Track;

/// Abyss's original cyan accent.
pub const DEFAULT_ACCENT: u32 = 0x33ffee;

/// A page of the main panel.
#[derive(Debug, Clone, PartialEq)]
pub enum Page {
    Home,
    Explore,
    /// An editorial page (genre, mood, decade...): (page name, title).
    Browse(String, String),
    Search(String),
    Album(u64),
    Artist(u64),
    Playlist(String),
    Mix(String),
    MyPlaylists,
    MyAlbums,
    MyTracks,
    MyArtists,
    MyMixes,
    /// Albums played recently in Abyss.
    Recent,
    /// Personal listening statistics.
    Replay,
    /// "Mixes & novedades": the user's mixes, Staff Picks and Uploads.
    Discover,
    Settings,
}

/// Something loaded in the background.
#[derive(Default)]
pub enum Load<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(t) => Some(t),
            _ => None,
        }
    }
    pub fn is_idle(&self) -> bool {
        matches!(self, Load::Idle)
    }
}

pub fn into_load<T>(r: anyhow::Result<T>) -> Load<T> {
    match r {
        Ok(v) => Load::Ready(v),
        Err(e) => Load::Failed(format!("{e:#}")),
    }
}

/// One entry of the play queue. `uid` tells two copies of a track apart.
#[derive(Clone, Debug)]
pub struct QItem {
    pub uid: u64,
    pub track: Track,
}

/// Nanoseconds since the epoch (seeds, stamps).
pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
}
