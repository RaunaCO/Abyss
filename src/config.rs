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
            Quality::Low => crate::i18n::t("low"),
            Quality::High => crate::i18n::t("high"),
            Quality::Max => crate::i18n::t("max"),
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Quality::Low => crate::i18n::t("up_to_320_kbps_aac"),
            Quality::High => crate::i18n::t("up_to_16_bit_44_1_khz_flac"),
            Quality::Max => crate::i18n::t("up_to_24_bit_192_khz_flac"),
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
    /// Session tokens. In memory only in plaintext; on disk they are written
    /// as `protected_tokens` (DPAPI) when protection is available. These two
    /// fields are still read from older plaintext files and migrated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// DPAPI-encrypted `{access_token, refresh_token}`, base64 (see secure.rs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protected_tokens: Option<String>,
    /// Unix seconds.
    pub expires_at: u64,
    pub user_id: Option<u64>,
    pub country_code: String,
    pub username: Option<String>,
    pub user_picture: Option<String>,

    /// None follows the system UI language. Supported overrides are BCP 47 base codes.
    pub language: Option<String>,
    /// One-time welcome, repeatable without clearing this completion marker.
    pub welcome_complete: bool,
    /// Canonical onboarding choice: tidal or local. Saved tokens are preserved.
    pub preferred_service: String,
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
    /// Discord Rich Presence ("Listening to Abyss").
    pub discord: bool,
    /// Now Playing background: blurred cover (true) or flat colour (false).
    pub np_blur: bool,
    /// Light interface theme (dark is the default).
    pub light_theme: bool,
    /// Accent colour, 0xRRGGBB (Abyss's cyan by default; never the system's).
    pub accent: u32,
    /// Library: last category shown ("tracks", "albums", "artists") and the
    /// list (false) / grid (true) choice of each.
    pub lib_last: String,
    pub lib_grid: std::collections::HashMap<String, bool>,
    /// Account e-mail as TIDAL reports it (shown in the sidebar), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_email: Option<String>,
    /// Extra columns in track lists: BPM and ratings; and the Uploads tab on Home.
    pub audio_metadata: bool,
    pub show_ratings: bool,
    pub hide_uploads: bool,
    /// Show the Latin pronunciation under lyrics in Asian scripts.
    pub lyrics_romanize: bool,
    pub wide_side_panel: bool,
    pub side_panel_width: Option<f32>,
    /// Look up lyrics on third-party services (LRCLIB, NetEase, lyrics.ovh)
    /// when TIDAL has none. Sends the track title, artist, album and length.
    pub external_lyrics: bool,
    /// Application ID of the Discord application named Abyss.
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
            protected_tokens: None,
            expires_at: 0,
            user_id: None,
            country_code: "US".into(),
            username: None,
            user_picture: None,
            language: None,
            welcome_complete: false,
            preferred_service: "tidal".into(),
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
            light_theme: false,
            accent: crate::ui::theme::DEFAULT_ACCENT,
            lib_last: "tracks".into(),
            lib_grid: Default::default(),
            user_email: None,
            audio_metadata: true,
            show_ratings: true,
            hide_uploads: false,
            lyrics_romanize: true,
            wide_side_panel: false,
            side_panel_width: None,
            external_lyrics: true,
            discord_client_id: String::new(),
            // Replaced by whichever client the user signed in with.
            client_id: DEVICE_CLIENT_ID.into(),
            client_secret: DEVICE_CLIENT_SECRET.into(),
        }
    }
}

/// Public application identifier. No verified Abyss ID is bundled.
// The owner must register or rename the Discord application for Abyss.
// An empty ID keeps activity publishing inactive until one is configured.
pub const DISCORD_APP_ID: &str = "";

/// TIDAL device-authorization client (TV flow), retained for compatibility.
pub const DEVICE_CLIENT_ID: &str = "fX2JxdmntZWK0ixT";
pub const DEVICE_CLIENT_SECRET: &str = "1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg=";

pub fn config_dir() -> PathBuf {
    // Tests and side-by-side runs can use their own settings, ratings and
    // recents instead of the user's.
    if let Some(dir) = std::env::var_os("ABYSS_CONFIG_DIR").or_else(|| std::env::var_os("TIDE_CONFIG_DIR")) {
        return PathBuf::from(dir);
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("abyss")
}

pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ABYSS_CACHE_DIR").or_else(|| std::env::var_os("TIDE_CACHE_DIR")) {
        return PathBuf::from(dir);
    }
    dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".")).join("abyss")
}

/// Run before loading any stores or installing the panic hook. Stage the entire
/// tree, then publish it with one directory rename. Existing Abyss data wins.
/// Originals and incomplete copies are retained, including on failure.
pub fn prepare_config() -> std::io::Result<()> {
    if std::env::var_os("ABYSS_CONFIG_DIR").or_else(|| std::env::var_os("TIDE_CONFIG_DIR")).is_some() {
        return Ok(());
    }
    let destination = config_dir();
    if destination.exists() {
        return Ok(());
    }
    let base = destination.parent().unwrap();
    let source = [base.join("tide"), base.join("tidal-gpui")].into_iter().find(|p| p.is_dir());
    let Some(source) = source else { return Ok(()) };
    let staging = base.join(format!(".abyss-import-{}-{:016x}", std::process::id(), rand::random::<u64>()));
    std::fs::create_dir(&staging)?;
    copy_config_tree(&source, &staging)?;
    match std::fs::rename(&staging, &destination) {
        Ok(()) => Ok(()),
        // A concurrent startup may have finished its own copy first.
        Err(_) if destination.is_dir() => Ok(()),
        Err(error) => Err(error),
    }
}

fn copy_config_tree(source: &std::path::Path, destination: &std::path::Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        // Do not follow links out of the source tree or overwrite any files.
        if kind.is_symlink() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, crate::i18n::t("migration.symbolic_link")));
        }
        if kind.is_dir() {
            std::fs::create_dir(&to)?;
            copy_config_tree(&from, &to)?;
        } else if kind.is_file() {
            let mut input = std::fs::File::open(&from)?;
            let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&to)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
        } else {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, crate::i18n::t("migration.unsupported_entry")));
        }
    }
    Ok(())
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

#[derive(Serialize, Deserialize)]
struct Tokens {
    access_token: Option<String>,
    refresh_token: Option<String>,
}

impl Settings {
    pub fn load() -> Self {
        let mut s: Settings = std::fs::read(settings_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if let Some(blob) = s.protected_tokens.take() {
            let tokens = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, blob.as_bytes())
                .ok()
                .and_then(|b| crate::secure::unprotect(&b))
                .and_then(|b| serde_json::from_slice::<Tokens>(&b).ok());
            match tokens {
                Some(t) => {
                    s.access_token = t.access_token;
                    s.refresh_token = t.refresh_token;
                }
                // Another user / machine, or a damaged file: sign in again.
                None => log::warn!("stored session could not be decrypted; please sign in again"),
            }
        } else if s.access_token.is_some() || s.refresh_token.is_some() {
            // Plaintext tokens from an older version: rewrite them protected.
            s.save();
        }
        crate::i18n::set_language(s.language.as_deref());
        s
    }

    /// The JSON written to disk: tokens protected when possible.
    fn to_disk_json(&self) -> Option<Vec<u8>> {
        let mut disk = self.clone();
        if disk.access_token.is_some() || disk.refresh_token.is_some() {
            let tokens = Tokens { access_token: disk.access_token.take(), refresh_token: disk.refresh_token.take() };
            match serde_json::to_vec(&tokens).ok().and_then(|b| crate::secure::protect(&b)) {
                Some(blob) => disk.protected_tokens = Some(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, blob)),
                // No protection available on this platform: keep the previous behaviour.
                None => {
                    disk.access_token = tokens.access_token;
                    disk.refresh_token = tokens.refresh_token;
                }
            }
        }
        serde_json::to_vec_pretty(&disk).ok()
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        let tmp = dir.join("settings.json.tmp");
        if let Some(json) = self.to_disk_json() {
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, settings_path());
            }
        }
    }

    pub fn signed_in(&self) -> bool {
        self.refresh_token.is_some() && self.user_id.is_some()
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn tokens_never_reach_disk_in_plaintext() {
        let s = Settings { access_token: Some("AT-secret-1".into()), refresh_token: Some("RT-secret-2".into()), ..Default::default() };
        let json = String::from_utf8(s.to_disk_json().unwrap()).unwrap();
        assert!(!json.contains("AT-secret-1") && !json.contains("RT-secret-2"));
        assert!(json.contains("protected_tokens"));
        // And they come back.
        let mut back: Settings = serde_json::from_str(&json).unwrap();
        let blob = back.protected_tokens.take().unwrap();
        let raw = crate::secure::unprotect(&base64::Engine::decode(&base64::engine::general_purpose::STANDARD, blob).unwrap()).unwrap();
        let t: Tokens = serde_json::from_slice(&raw).unwrap();
        assert_eq!(t.refresh_token.as_deref(), Some("RT-secret-2"));
    }
}
