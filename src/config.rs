//! Persistent settings: tokens, playback preferences, window state.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Streaming quality, using the same three tiers as the official app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Quality {
    /// AAC 320 kbps.
    Low,
    /// FLAC 16-bit / 44.1 kHz.
    High,
    /// FLAC up to 24-bit / 192 kHz.
    #[default]
    Max,
}

impl Quality {
    pub fn label(self) -> &'static str {
        match self {
            Quality::Low => "Low",
            Quality::High => "High",
            Quality::Max => "Max",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Quality::Low => "Hasta 320 kbps (AAC)",
            Quality::High => "Hasta 16-bit, 44.1 kHz (FLAC)",
            Quality::Max => "Hasta 24-bit, 192 kHz (FLAC)",
        }
    }

    /// Order in which `audioquality` values are tried for this tier.
    pub fn api_values(self) -> &'static [&'static str] {
        match self {
            Quality::Max => &["HI_RES_LOSSLESS", "LOSSLESS", "HIGH"],
            Quality::High => &["LOSSLESS", "HIGH"],
            Quality::Low => &["HIGH", "LOW"],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    /// Unix seconds.
    pub expires_at: u64,
    pub user_id: Option<u64>,
    pub country_code: String,
    pub username: Option<String>,
    pub user_picture: Option<String>,

    pub quality: Quality,
    /// WASAPI exclusive mode: bit-perfect output at the track's own sample rate.
    pub exclusive_mode: bool,
    /// Do not touch the samples in exclusive mode (volume control disabled).
    pub force_volume: bool,
    /// Loudness normalisation (TIDAL ReplayGain).
    pub normalize: bool,
    /// Keep playing similar tracks when the queue ends.
    pub autoplay: bool,
    /// Narrow icon-only sidebar.
    pub sidebar_collapsed: bool,
    /// UI zoom (1.0 = 100 %).
    pub zoom: f32,
    /// Collection sort per section: (key, descending).
    pub sorts: std::collections::HashMap<String, (String, bool)>,
    /// Output device name; `None` = system default.
    pub output_device: Option<String>,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub recent_searches: Vec<String>,
    /// Discord Rich Presence ("Listening to TIDE"), like Spotify's.
    pub discord: bool,
    /// Now Playing background: blurred cover (true) or flat colour (false).
    pub np_blur: bool,
    /// Extra columns in track lists: BPM and ratings; and the Uploads tab on Home.
    pub audio_metadata: bool,
    pub show_ratings: bool,
    pub hide_uploads: bool,
    /// Show the Latin pronunciation under lyrics in Asian scripts.
    pub lyrics_romanize: bool,
    /// Application ID of the Discord application named TIDE.
    pub discord_client_id: String,

    /// OAuth client used for the device login. Can be replaced in settings.json.
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            access_token: None,
            refresh_token: None,
            expires_at: 0,
            user_id: None,
            country_code: "US".into(),
            username: None,
            user_picture: None,
            quality: Quality::Max,
            exclusive_mode: false,
            force_volume: false,
            normalize: true,
            autoplay: true,
            sidebar_collapsed: true,
            zoom: 1.0,
            sorts: Default::default(),
            output_device: None,
            volume: 0.8,
            shuffle: false,
            repeat: Repeat::Off,
            recent_searches: Vec::new(),
            discord: true,
            np_blur: true,
            audio_metadata: true,
            show_ratings: true,
            hide_uploads: false,
            lyrics_romanize: true,
            discord_client_id: String::new(),
            // Replaced by whichever client the user signed in with.
            client_id: DEVICE_CLIENT_ID.into(),
            client_secret: DEVICE_CLIENT_SECRET.into(),
        }
    }
}

/// TIDAL's device-authorization client (TV / limited-input flow). Its
/// sessions get FLAC only for hi-res tracks; 16-bit tracks come as AAC.
/// Application ID of TIDE's Discord application (registered once by the project,
/// so users need no setup). Public identifier, not a secret.
pub const DISCORD_APP_ID: &str = "1568272471487025294";

pub const DEVICE_CLIENT_ID: &str = "fX2JxdmntZWK0ixT";
pub const DEVICE_CLIENT_SECRET: &str = "1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg=";

pub fn config_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("tide");
    // One-time migration from the project's former name (tidal-gpui).
    let old = base.join("tidal-gpui");
    if !dir.exists() && old.exists() {
        let _ = std::fs::rename(&old, &dir);
    }
    dir
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".")).join("tide")
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read(settings_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        let tmp = dir.join("settings.json.tmp");
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, settings_path());
            }
        }
    }

    pub fn signed_in(&self) -> bool {
        self.refresh_token.is_some() && self.user_id.is_some()
    }
}
