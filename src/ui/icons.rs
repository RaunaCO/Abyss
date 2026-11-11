//! Abyss's own icon family, embedded as SVG.
//!
//! Drawn on a 24 px grid (live area 3..21) with one consistent language: a
//! 1.7 stroke with round caps and joins, geometric shapes, generous negative
//! space. Solid glyphs (play, pause, skip, filled heart and star) are filled
//! AND stroked, so their corners are softened like the outlines. Window
//! controls use a hairline (1 px) stroke. Every glyph is a single colour; the
//! colour comes from the element (`text_color`). Original artwork, part of
//! Abyss (MIT).

use super::sc;
use std::borrow::Cow;

use gpui::{px, svg, AssetSource, Rgba, SharedString, Styled, Svg};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Play,
    Pause,
    PlayRound,
    ChevUp,
    PauseRound,
    Next,
    Prev,
    Shuffle,
    Repeat,
    RepeatOne,
    Heart,
    HeartFilled,
    Star,
    Collapse,
    StarHalf,
    StarOutline,
    Search,
    Home,
    Explore,
    VolumeUp,
    VolumeOff,
    Queue,
    Back,
    Forward,
    Settings,
    Sun,
    Moon,
    Playlist,
    Album,
    Track,
    Artist,
    Mix,
    Lyrics,
    More,
    Close,
    Explicit,
    Clock,
    Radio,
    Menu,
    Plus,
    ChevDown,
    Edit,
    Link,
    Calendar,
    Trophy,
    VideoBox,
    WinMin,
    WinMax,
    WinRestore,
    WinClose,
    Fullscreen,
    ExitFullscreen,
    Sync,
    Library,
    SignOut,
    Grid,
    List,
    Stats,
    Upload,
}

impl Icon {
    /// Every icon, for the diagnostic sheet (`TIDE_ICON_SHEET`).
    pub const ALL: [Icon; 59] = [
        Icon::Play,
        Icon::Pause,
        Icon::PlayRound,
        Icon::ChevUp,
        Icon::PauseRound,
        Icon::Next,
        Icon::Prev,
        Icon::Shuffle,
        Icon::Repeat,
        Icon::RepeatOne,
        Icon::Heart,
        Icon::HeartFilled,
        Icon::Star,
        Icon::Collapse,
        Icon::StarHalf,
        Icon::StarOutline,
        Icon::Search,
        Icon::Home,
        Icon::Explore,
        Icon::VolumeUp,
        Icon::VolumeOff,
        Icon::Queue,
        Icon::Back,
        Icon::Forward,
        Icon::Settings,
        Icon::Sun,
        Icon::Moon,
        Icon::Playlist,
        Icon::Album,
        Icon::Track,
        Icon::Artist,
        Icon::Mix,
        Icon::Lyrics,
        Icon::More,
        Icon::Close,
        Icon::Explicit,
        Icon::Clock,
        Icon::Radio,
        Icon::Menu,
        Icon::Plus,
        Icon::ChevDown,
        Icon::Edit,
        Icon::Link,
        Icon::Calendar,
        Icon::Trophy,
        Icon::VideoBox,
        Icon::WinMin,
        Icon::WinMax,
        Icon::WinRestore,
        Icon::WinClose,
        Icon::Fullscreen,
        Icon::ExitFullscreen,
        Icon::Sync,
        Icon::Library,
        Icon::SignOut,
        Icon::Grid,
        Icon::List,
        Icon::Stats,
        Icon::Upload,
    ];

    fn name(self) -> &'static str {
        match self {
            Icon::Play => "play",
            Icon::Pause => "pause",
            Icon::PlayRound => "play_round",
            Icon::ChevUp => "chev_up",
            Icon::PauseRound => "pause_round",
            Icon::Next => "next",
            Icon::Prev => "prev",
            Icon::Shuffle => "shuffle",
            Icon::Repeat => "repeat",
            Icon::RepeatOne => "repeat_one",
            Icon::Heart => "heart",
            Icon::HeartFilled => "heart_filled",
            Icon::Star => "star",
            Icon::Collapse => "collapse",
            Icon::StarHalf => "star_half",
            Icon::StarOutline => "star_outline",
            Icon::Search => "search",
            Icon::Home => "home",
            Icon::Explore => "explore",
            Icon::VolumeUp => "volume_up",
            Icon::VolumeOff => "volume_off",
            Icon::Queue => "queue",
            Icon::Back => "back",
            Icon::Forward => "forward",
            Icon::Settings => "settings",
            Icon::Sun => "sun",
            Icon::Moon => "moon",
            Icon::Playlist => "playlist",
            Icon::Album => "album",
            Icon::Track => "track",
            Icon::Artist => "artist",
            Icon::Mix => "mix",
            Icon::Lyrics => "lyrics",
            Icon::More => "more",
            Icon::Close => "close",
            Icon::Explicit => "explicit",
            Icon::Clock => "clock",
            Icon::Radio => "radio",
            Icon::Menu => "menu",
            Icon::ChevDown => "chev_down",
            Icon::Edit => "edit",
            Icon::Calendar => "calendar",
            Icon::Trophy => "trophy",
            Icon::VideoBox => "video_box",
            Icon::Link => "link",
            Icon::Plus => "plus",
            Icon::WinMin => "win_min",
            Icon::WinMax => "win_max",
            Icon::WinRestore => "win_restore",
            Icon::WinClose => "win_close",
            Icon::Fullscreen => "fullscreen",
            Icon::ExitFullscreen => "exit_fullscreen",
            Icon::Sync => "sync",
            Icon::Library => "library",
            Icon::SignOut => "sign_out",
            Icon::Grid => "grid",
            Icon::List => "list",
            Icon::Stats => "stats",
            Icon::Upload => "upload",
        }
    }

    /// The icon in a fixed `size` box, drawn scaled by `scale` around its centre.
    /// The scale is a paint transform, so it changes smoothly (no whole-pixel
    /// steps) and never moves the layout around it.
    pub fn el_scaled(self, size: f32, color: Rgba, scale: f32) -> Svg {
        let el = self.el(size, color);
        if (scale - 1.0).abs() < 0.001 {
            return el;
        }
        el.with_transformation(gpui::Transformation::scale(gpui::size(scale, scale)))
    }

    pub fn el(self, size: f32, color: Rgba) -> Svg {
        // Whole device pixels: crisp edges, and the rasterised glyph is reused.
        let px_size = (size * super::zoom()).round().max(1.0);
        svg().path(SharedString::from(format!("icons/{}.svg", self.name()))).size(px(px_size)).flex_none().text_color(color)
    }
}

fn markup(name: &str) -> Option<&'static str> {
    Some(match name {
        "play" => r##"<path fill="#fff" d="M8 5.6v12.8a.6.6 0 0 0 .9.5l10.2-6.4a.6.6 0 0 0 0-1L8.9 5.1a.6.6 0 0 0-.9.5z"/>"##,
        "play_round" => r##"<path fill="#fff" d="M8.6 6.2v11.6a.6.6 0 0 0 .9.5l9.2-5.8a.6.6 0 0 0 0-1L9.5 5.7a.6.6 0 0 0-.9.5z"/>"##,
        "pause" => {
            r##"<rect fill="#fff" stroke="none" x="6.2" y="5" width="3.9" height="14" rx="1.2"/><rect fill="#fff" stroke="none" x="13.9" y="5" width="3.9" height="14" rx="1.2"/>"##
        }
        "pause_round" => {
            r##"<rect fill="#fff" stroke="none" x="6.4" y="5.4" width="3.8" height="13.2" rx="1.6"/><rect fill="#fff" stroke="none" x="13.8" y="5.4" width="3.8" height="13.2" rx="1.6"/>"##
        }
        "next" => {
            r##"<path fill="#fff" d="M6 6.7v10.6a.6.6 0 0 0 .9.5l8-5.3a.6.6 0 0 0 0-1l-8-5.3a.6.6 0 0 0-.9.5z"/><path d="M18 6v12"/>"##
        }
        "prev" => {
            r##"<path fill="#fff" d="M18 6.7v10.6a.6.6 0 0 1-.9.5l-8-5.3a.6.6 0 0 1 0-1l8-5.3a.6.6 0 0 1 .9.5z"/><path d="M6 6v12"/>"##
        }
        "shuffle" => {
            r##"<path d="M3.8 7.2h2.9c1.5 0 2.9.8 3.7 2l3.2 5.6c.8 1.2 2.2 2 3.7 2H20"/><path d="M3.8 16.8h2.9c1.5 0 2.9-.8 3.7-2"/><path d="M13.6 9.2c.8-1.2 2.2-2 3.7-2H20"/><path d="M17.6 4.8L20 7.2l-2.4 2.4M17.6 14.4l2.4 2.4-2.4 2.4"/>"##
        }
        "repeat" => {
            r##"<path d="M4.5 11.2V10a3.2 3.2 0 0 1 3.2-3.2h11.6"/><path d="M16.8 4.3l2.5 2.5-2.5 2.5"/><path d="M19.5 12.8V14a3.2 3.2 0 0 1-3.2 3.2H4.7"/><path d="M7.2 19.7l-2.5-2.5 2.5-2.5"/>"##
        }
        "repeat_one" => {
            r##"<path d="M4.5 11.2V10a3.2 3.2 0 0 1 3.2-3.2h11.6"/><path d="M16.8 4.3l2.5 2.5-2.5 2.5"/><path d="M19.5 12.8V14a3.2 3.2 0 0 1-3.2 3.2H4.7"/><path d="M7.2 19.7l-2.5-2.5 2.5-2.5"/><path d="M11 10.6l1.4-.9v4.6"/>"##
        }
        "heart" => {
            r##"<path d="M12 19.6c-.9-.5-7.8-4.6-7.8-9.9C4.2 7 6.1 5 8.5 5c1.5 0 2.7.7 3.5 1.9C12.8 5.7 14 5 15.5 5c2.4 0 4.3 2 4.3 4.7 0 5.3-6.9 9.4-7.8 9.9z"/>"##
        }
        "heart_filled" => {
            r##"<path fill="#fff" d="M12 19.6c-.9-.5-7.8-4.6-7.8-9.9C4.2 7 6.1 5 8.5 5c1.5 0 2.7.7 3.5 1.9C12.8 5.7 14 5 15.5 5c2.4 0 4.3 2 4.3 4.7 0 5.3-6.9 9.4-7.8 9.9z"/>"##
        }
        "star" => r##"<path fill="#fff" d="M12 4.3l2.3 4.6 5.1.8-3.7 3.6.9 5.1-4.6-2.4-4.6 2.4.9-5.1-3.7-3.6 5.1-.8z"/>"##,
        "star_outline" => r##"<path d="M12 4.3l2.3 4.6 5.1.8-3.7 3.6.9 5.1-4.6-2.4-4.6 2.4.9-5.1-3.7-3.6 5.1-.8z"/>"##,
        "star_half" => {
            r##"<path d="M12 4.3l2.3 4.6 5.1.8-3.7 3.6.9 5.1-4.6-2.4-4.6 2.4.9-5.1-3.7-3.6 5.1-.8z"/><path fill="#fff" d="M12 4.3v11.7l-4.6 2.4.9-5.1-3.7-3.6 5.1-.8z"/>"##
        }
        "collapse" => r##"<path d="M5.5 5v14"/><path d="M15.5 7l-5 5 5 5"/>"##,
        "chev_up" => r##"<path d="M7 14.5l5-5 5 5"/>"##,
        "chev_down" => r##"<path d="M7 9.5l5 5 5-5"/>"##,
        "back" => r##"<path d="M14.5 6.5L9 12l5.5 5.5"/>"##,
        "forward" => r##"<path d="M9.5 6.5L15 12l-5.5 5.5"/>"##,
        "search" => r##"<circle cx="10.8" cy="10.8" r="6.1"/><path d="M15.3 15.3l4.2 4.2"/>"##,
        "home" => r##"<path d="M4.6 10.4L12 4.7l7.4 5.7v7.7a1.6 1.6 0 0 1-1.6 1.6h-3.4v-5.2H9.6v5.2H6.2a1.6 1.6 0 0 1-1.6-1.6z"/>"##,
        "explore" => r##"<circle cx="12" cy="12" r="8.2"/><path fill="#fff" d="M15.2 8.8l-1.9 4.5-4.5 1.9 1.9-4.5z"/>"##,
        "volume_up" => {
            r##"<path fill="#fff" d="M4.6 9.6h2.7l3.8-3.3v11.4l-3.8-3.3H4.6z"/><path d="M14.6 9.3a3.9 3.9 0 0 1 0 5.4"/><path d="M17.3 6.7a7.6 7.6 0 0 1 0 10.6"/>"##
        }
        "volume_off" => {
            r##"<path fill="#fff" d="M4.6 9.6h2.7l3.8-3.3v11.4l-3.8-3.3H4.6z"/><path d="M15.3 9.7l4.6 4.6M19.9 9.7l-4.6 4.6"/>"##
        }
        "queue" => r##"<path d="M4.5 6.8h11M4.5 11.6h11M4.5 16.4h6"/><path fill="#fff" d="M15.4 14.2v5.2l4.1-2.6z"/>"##,
        "sun" => {
            r##"<circle cx="12" cy="12" r="4"/><path d="M12 2.8v2m0 14.4v2M2.8 12h2m14.4 0h2M5.5 5.5l1.4 1.4m10.2 10.2 1.4 1.4m0-13-1.4 1.4M6.9 17.1l-1.4 1.4"/>"##
        }
        "moon" => r##"<path d="M20.6 14A8.8 8.8 0 0 1 10 3.4 8.8 8.8 0 1 0 20.6 14Z"/>"##,
        "settings" => {
            r##"<path d="M4.5 8h8.7M17.7 8h1.8M4.5 16h1.8M10.8 16h8.7"/><circle cx="15.5" cy="8" r="2.2"/><circle cx="8.5" cy="16" r="2.2"/>"##
        }
        "playlist" => {
            r##"<path d="M4.5 6.6h11M4.5 11.2h11M4.5 15.8h5.6"/><circle cx="15.6" cy="17.4" r="2.1"/><path d="M17.7 17.4V10.6l2.1.9"/>"##
        }
        "album" => r##"<circle cx="12" cy="12" r="8.2"/><circle cx="12" cy="12" r="2.3"/>"##,
        "track" => r##"<circle cx="8.4" cy="17" r="2.6"/><path d="M11 17V5.4l6.6 1.8"/>"##,
        "artist" => r##"<circle cx="12" cy="8.6" r="3.6"/><path d="M5.4 19.6c.7-3.3 3.4-5.6 6.6-5.6s5.9 2.3 6.6 5.6"/>"##,
        "mix" => r##"<path d="M4 10.2v3.6M8 7v10M12 4.6v14.8M16 8v8M20 10.2v3.6"/>"##,
        "lyrics" => {
            r##"<path d="M5.4 5.4h13.2a1.6 1.6 0 0 1 1.6 1.6v8.4a1.6 1.6 0 0 1-1.6 1.6H10.2l-3.9 3v-3h-.9a1.6 1.6 0 0 1-1.6-1.6V7a1.6 1.6 0 0 1 1.6-1.6z"/><path d="M8.2 9.4h7.6M8.2 12.8h4.8"/>"##
        }
        "more" => {
            r##"<circle fill="#fff" stroke="none" cx="5.8" cy="12" r="1.6"/><circle fill="#fff" stroke="none" cx="12" cy="12" r="1.6"/><circle fill="#fff" stroke="none" cx="18.2" cy="12" r="1.6"/>"##
        }
        "close" => r##"<path d="M6.6 6.6l10.8 10.8M17.4 6.6L6.6 17.4"/>"##,
        "explicit" => r##"<rect x="4.4" y="4.4" width="15.2" height="15.2" rx="3.2"/><path d="M14.2 8.4H10v7.2h4.2M10 12h3.6"/>"##,
        "clock" => r##"<circle cx="12" cy="12" r="8.2"/><path d="M12 7.4V12l3.1 2"/>"##,
        "radio" => {
            r##"<circle fill="#fff" stroke="none" cx="12" cy="12" r="1.9"/><path d="M8.6 15.4a4.8 4.8 0 0 1 0-6.8M15.4 8.6a4.8 4.8 0 0 1 0 6.8M5.8 18.2a8.8 8.8 0 0 1 0-12.4M18.2 5.8a8.8 8.8 0 0 1 0 12.4"/>"##
        }
        "menu" => r##"<path d="M4.5 7.2h15M4.5 12h15M4.5 16.8h15"/>"##,
        "plus" => r##"<path d="M12 5v14M5 12h14"/>"##,
        "edit" => r##"<path d="M15.4 5.3l3.3 3.3-9.6 9.6H5.8v-3.3z"/><path d="M13.3 7.4l3.3 3.3"/>"##,
        "link" => {
            r##"<path d="M10 14l4-4"/><path d="M8.6 11.4L7.1 12.9a3.5 3.5 0 0 0 5 5l1.5-1.5M15.4 12.6l1.5-1.5a3.5 3.5 0 0 0-5-5l-1.5 1.5"/>"##
        }
        "calendar" => r##"<rect x="4.4" y="5.6" width="15.2" height="14" rx="2.6"/><path d="M4.4 10.2h15.2M8.4 3.6v4M15.6 3.6v4"/>"##,
        "trophy" => {
            r##"<path d="M8 4.6h8v5.2a4 4 0 0 1-8 0z"/><path d="M8 6.6H5.4v1.2a3 3 0 0 0 3 3M16 6.6h2.6v1.2a3 3 0 0 1-3 3M12 13.8v3.6M8.6 19.6h6.8"/>"##
        }
        "video_box" => r##"<rect x="3.6" y="5.6" width="16.8" height="12.8" rx="2.6"/><path fill="#fff" d="M10.4 9.6v4.8l3.9-2.4z"/>"##,
        // Two upright volumes and one leaning: the collection.
        "library" => r##"<path d="M5.4 4.8v14.4M9.8 4.8v14.4"/><path d="M13.8 5.6l4.6 13.4"/>"##,
        // Door frame with an arrow leaving it.
        "sign_out" => {
            r##"<path d="M10.2 4.8H6.8a1.8 1.8 0 0 0-1.8 1.8v10.8a1.8 1.8 0 0 0 1.8 1.8h3.4"/><path d="M14.6 8l4 4-4 4M18.4 12H9.8"/>"##
        }
        // Three bars rising, on a base line: statistics.
        "stats" => r##"<path d="M4.6 19.2h14.8M7.6 15.6v-3.2M12 15.6V8.4M16.4 15.6V5.4"/>"##,
        // Arrow rising out of a tray.
        "upload" => {
            r##"<path d="M12 15.2V4.8M7.8 9l4.2-4.2L16.2 9"/><path d="M5 14.6v3.2a1.6 1.6 0 0 0 1.6 1.6h10.8a1.6 1.6 0 0 0 1.6-1.6v-3.2"/>"##
        }
        "grid" => {
            r##"<rect x="4.6" y="4.6" width="6" height="6" rx="1.4"/><rect x="13.4" y="4.6" width="6" height="6" rx="1.4"/><rect x="4.6" y="13.4" width="6" height="6" rx="1.4"/><rect x="13.4" y="13.4" width="6" height="6" rx="1.4"/>"##
        }
        "list" => {
            r##"<path d="M9.4 6.8h9.6M9.4 12h9.6M9.4 17.2h9.6"/><circle fill="#fff" stroke="none" cx="5.4" cy="6.8" r="1.15"/><circle fill="#fff" stroke="none" cx="5.4" cy="12" r="1.15"/><circle fill="#fff" stroke="none" cx="5.4" cy="17.2" r="1.15"/>"##
        }
        "sync" => {
            r##"<path d="M5.11 10.78A7 7 0 0 1 18.06 8.5M15.06 7.41L18.06 8.5 18.62 5.35M18.89 13.22A7 7 0 0 1 5.94 15.5M8.94 16.59L5.94 15.5 5.38 18.65"/>"##
        }
        "win_min" => r##"<path stroke-width="1" stroke-linecap="butt" d="M6 12h12"/>"##,
        "win_max" => r##"<rect stroke-width="1" x="6.5" y="6.5" width="11" height="11" rx="1.2"/>"##,
        "win_restore" => {
            r##"<rect stroke-width="1" x="6.5" y="8.5" width="9" height="9" rx="1.2"/><path stroke-width="1" d="M8.5 6.5h7.8a1.2 1.2 0 0 1 1.2 1.2v7.8"/>"##
        }
        "fullscreen" => r##"<path d="M4 9V4h5m6 0h5v5M4 15v5h5m6 0h5v-5"/>"##,
        "exit_fullscreen" => r##"<path d="M4 9h5V4m6 0v5h5M4 15h5v5m6 0v-5h5"/>"##,
        "win_close" => r##"<path stroke-width="1" d="M7 7l10 10M17 7L7 17"/>"##,
        _ => return None,
    })
}

/// Angles the turning sync glyph is drawn at (7.5 degree steps).
const SYNC_STEPS: u32 = 48;

/// The sync glyph turned by `radians` (clockwise), at the nearest drawn step.
pub fn sync_turned(size: f32, color: Rgba, radians: f32) -> Svg {
    let k = ((radians / std::f32::consts::TAU).rem_euclid(1.0) * SYNC_STEPS as f32).round() as u32 % SYNC_STEPS;
    let px_size = (size * super::zoom()).round().max(1.0);
    svg().path(SharedString::from(format!("icons/sync@{k}.svg"))).size(px(px_size)).flex_none().text_color(color)
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, p: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        // The pointer of the album dropdown: a triangle with a softened apex.
        if p == "shapes/tip.svg" {
            return Ok(Some(Cow::Borrowed(
                br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 12"><path fill="#fff" d="M0 12L10.2 1.6Q12 0 13.8 1.6L24 12Z"/></svg>"##,
            )));
        }
        let Some(name) = p.strip_prefix("icons/").and_then(|n| n.strip_suffix(".svg")) else {
            return Ok(None);
        };
        // "sync@k": the sync glyph turned by k steps (see `sync_turned`), drawn
        // as vectors so every angle is as crisp as the still icon.
        if let Some(k) = name.strip_prefix("sync@").and_then(|k| k.parse::<u32>().ok()) {
            let deg = (k % SYNC_STEPS) as f32 * 360.0 / SYNC_STEPS as f32;
            let m = markup("sync").unwrap_or_default();
            return Ok(Some(Cow::Owned(
                format!(
                    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><g transform="rotate({deg} 12 12)">{m}</g></svg>"##
                )
                .into_bytes(),
            )));
        }
        // Stroke attributes are inherited by every shape; filled shapes set their own.
        Ok(markup(name).map(|m| {
            Cow::Owned(
                format!(
                    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">{m}</svg>"##
                )
                .into_bytes(),
            )
        }))
    }

    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
