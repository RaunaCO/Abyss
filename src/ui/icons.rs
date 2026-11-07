//! Embedded SVG icons (Material Design icon paths, Apache-2.0).

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
}

impl Icon {
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
        }
    }

    pub fn el(self, size: f32, color: Rgba) -> Svg {
        // Whole device pixels: crisp edges, and the rasterised glyph is reused.
        let px_size = (size * super::zoom()).round().max(1.0);
        svg().path(SharedString::from(format!("icons/{}.svg", self.name()))).size(px(px_size)).flex_none().text_color(color)
    }
}

fn path(name: &str) -> Option<&'static str> {
    Some(match name {
        "play" => "M8 5v14l11-7z",
        "pause" => "M6 5h4v14H6zM14 5h4v14h-4z",
        "chev_up" => "M7.41 15.41L12 10.83l4.59 4.58L18 14l-6-6-6 6z",
        "play_round" => "M8 6.82v10.36c0 .79.87 1.27 1.54.84l8.14-5.18c.62-.39.62-1.29 0-1.69L9.54 5.98C8.87 5.55 8 6.03 8 6.82z",
        "pause_round" => "M8 19c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2s-2 .9-2 2v10c0 1.1.9 2 2 2zm6-12v10c0 1.1.9 2 2 2s2-.9 2-2V7c0-1.1-.9-2-2-2s-2 .9-2 2z",
        "next" => "M6 6l8.5 6L6 18zM16 6h2v12h-2z",
        "prev" => "M18 6l-8.5 6L18 18zM6 6h2v12H6z",
        "shuffle" => "M10.59 9.17L5.41 4 4 5.41l5.17 5.17 1.42-1.41zM14.5 4l2.04 2.04L4 18.59 5.41 20 17.96 7.46 20 9.5V4h-5.5zm.33 9.41l-1.41 1.41 3.13 3.13L14.5 20H20v-5.5l-2.04 2.04-3.13-3.13z",
        "repeat" => "M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4z",
        "repeat_one" => "M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4zm-4-2V9h-1l-2 1v1h1.5v4H13z",
        "heart" => "M16.5 3c-1.74 0-3.41.81-4.5 2.09C10.91 3.81 9.24 3 7.5 3 4.42 3 2 5.42 2 8.5c0 3.78 3.4 6.86 8.55 11.54L12 21.35l1.45-1.32C18.6 15.36 22 12.28 22 8.5 22 5.42 19.58 3 16.5 3zm-4.4 15.55l-.1.1-.1-.1C7.14 14.24 4 11.39 4 8.5 4 6.5 5.5 5 7.5 5c1.54 0 3.04.99 3.57 2.36h1.87C13.46 5.99 14.96 5 16.5 5c2 0 3.5 1.5 3.5 3.5 0 2.89-3.14 5.74-7.9 10.05z",
        "heart_filled" => "M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z",
        "collapse" => "M18.41 16.59L13.82 12l4.59-4.59L17 6l-6 6 6 6zM6 6h2v12H6z",
        "star" => "M12 17.27L18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z",
        "star_half" => "M22 9.24l-7.19-.62L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21 12 17.27 18.18 21l-1.63-7.03L22 9.24zM12 15.4V6.1l1.71 4.04 4.38.38-3.32 2.88 1 4.28L12 15.4z",
        "star_outline" => "M22 9.24l-7.19-.62L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21 12 17.27 18.18 21l-1.63-7.03L22 9.24zM12 15.4l-3.76 2.27 1-4.28-3.32-2.88 4.38-.38L12 6.1l1.71 4.04 4.38.38-3.32 2.88 1 4.28L12 15.4z",
        "search" => "M15.5 14h-.79l-.28-.27C15.41 12.59 16 11.11 16 9.5 16 5.91 13.09 3 9.5 3S3 5.91 3 9.5 5.91 16 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z",
        "home" => "M10 20v-6h4v6h5v-8h3L12 3 2 12h3v8z",
        "explore" => "M12 10.9c-.61 0-1.1.49-1.1 1.1s.49 1.1 1.1 1.1c.61 0 1.1-.49 1.1-1.1s-.49-1.1-1.1-1.1zM12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm2.19 12.19L6 18l3.81-8.19L18 6l-3.81 8.19z",
        "volume_up" => "M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z",
        "volume_off" => "M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51C20.63 14.91 21 13.5 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3L3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06c1.38-.31 2.63-.95 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4L9.91 6.09 12 8.18V4z",
        "queue" => "M15 6H3v2h12V6zm0 4H3v2h12v-2zM3 16h8v-2H3v2zM17 6v8.18c-.31-.11-.65-.18-1-.18-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3V8h3V6h-5z",
        "back" => "M15.41 7.41L14 6l-6 6 6 6 1.41-1.41L10.83 12z",
        "forward" => "M10 6L8.59 7.41 13.17 12l-4.58 4.59L10 18l6-6z",
        "settings" => "M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58c.18-.14.23-.41.12-.61l-1.92-3.32c-.12-.22-.37-.29-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54c-.04-.24-.24-.41-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.09.63-.09.94s.02.64.07.94l-2.03 1.58c-.18.14-.23.41-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z",
        "playlist" => "M19 9H2v2h17V9zm0-4H2v2h17V5zM2 15h13v-2H2v2zm15-2v6l5-3-5-3z",
        "album" => "M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 14.5c-2.49 0-4.5-2.01-4.5-4.5S9.51 7.5 12 7.5s4.5 2.01 4.5 4.5-2.01 4.5-4.5 4.5zm0-5.5c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1z",
        "track" => "M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z",
        "artist" => "M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z",
        "mix" => "M7 18h2V6H7v12zm4 4h2V2h-2v20zm-8-8h2v-4H3v4zm12 4h2V6h-2v12zm4-8v4h2v-4h-2z",
        "lyrics" => "M20 2H4c-1.1 0-1.99.9-1.99 2L2 22l4-4h14c1.1 0 2-.9 2-2V4c0-1.1-.9-2-2-2zM6 9h12v2H6V9zm8 5H6v-2h8v2zm4-6H6V6h12v2z",
        "more" => "M6 10c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm12 0c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm-6 0c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z",
        "close" => "M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z",
        "explicit" => "M19 3H5c-1.11 0-2 .9-2 2v14c0 1.1.89 2 2 2h14c1.11 0 2-.9 2-2V5c0-1.1-.89-2-2-2zm-4 6h-4v2h4v2h-4v2h4v2H9V7h6v2z",
        "clock" => "M11.99 2C6.47 2 2 6.48 2 12s4.47 10 9.99 10C17.52 22 22 17.52 22 12S17.52 2 11.99 2zM12 20c-4.42 0-8-3.58-8-8s3.58-8 8-8 8 3.58 8 8-3.58 8-8 8zm.5-13H11v6l5.25 3.15.75-1.23-4.5-2.67z",
        "radio" => "M3.24 6.15C2.51 6.43 2 7.17 2 8v12c0 1.1.89 2 2 2h16c1.11 0 2-.9 2-2V8c0-1.11-.89-2-2-2H8.3l8.26-3.34L15.88 1 3.24 6.15zM7 20c-1.66 0-3-1.34-3-3s1.34-3 3-3 3 1.34 3 3-1.34 3-3 3zm13-8h-2v-2h-2v2H4V8h16v4z",
        "calendar" => "M19 4h-1V2h-2v2H8V2H6v2H5c-1.11 0-1.99.9-1.99 2L3 20c0 1.1.89 2 2 2h14c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm0 16H5V10h14v10zM9 14H7v-2h2v2zm4 0h-2v-2h2v2zm4 0h-2v-2h2v2zm-8 4H7v-2h2v2zm4 0h-2v-2h2v2zm4 0h-2v-2h2v2z",
        "trophy" => "M19 5h-2V3H7v2H5c-1.1 0-2 .9-2 2v1c0 2.55 1.92 4.63 4.39 4.94A5.01 5.01 0 0011 15.9V19H7v2h10v-2h-4v-3.1a5.01 5.01 0 003.61-2.96C19.08 12.63 21 10.55 21 8V7c0-1.1-.9-2-2-2zM5 8V7h2v3.82C5.84 10.4 5 9.3 5 8zm14 0c0 1.3-.84 2.4-2 2.82V7h2v1z",
        "video_box" => "M21 3H3c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h18c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm0 16H3V5h18v14zM10 8v8l6-4z",
        "edit" => "M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25zM20.71 7.04a1 1 0 000-1.41l-2.34-2.34a1 1 0 00-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z",
        "link" => "M3.9 12c0-1.71 1.39-3.1 3.1-3.1h4V7H7c-2.76 0-5 2.24-5 5s2.24 5 5 5h4v-1.9H7c-1.71 0-3.1-1.39-3.1-3.1zM8 13h8v-2H8v2zm9-6h-4v1.9h4c1.71 0 3.1 1.39 3.1 3.1s-1.39 3.1-3.1 3.1h-4V17h4c2.76 0 5-2.24 5-5s-2.24-5-5-5z",
        "chev_down" => "M7.4 9.6L12 14.2l4.6-4.6 1.4 1.4-6 6-6-6z",
        "menu" => "M3 6h18v1.6H3zM3 11.2h18v1.6H3zM3 16.4h18V18H3z",
        "plus" => "M11.2 4h1.6v7.2H20v1.6h-7.2V20h-1.6v-7.2H4v-1.6h7.2z",
        // Thin window-control glyphs.
        "win_min" => "M5 11.5h14v1H5z",
        "win_max" => "M5 5h14v1H5zM5 18h14v1H5zM5 5h1v14H5zM18 5h1v14h-1z",
        "win_restore" => "M5 8h11v1H5zM5 18h11v1H5zM5 8h1v11H5zM15 8h1v11h-1zM8 5h11v1H8zM18 5h1v11h-1zM8 5h1v3H8zM16 15h3v1h-3z",
        "win_close" => "M6.7 6l5.3 5.3L17.3 6l.7.7-5.3 5.3 5.3 5.3-.7.7-5.3-5.3L6.7 18l-.7-.7 5.3-5.3L6 6.7z",
        _ => return None,
    })
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, p: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let Some(name) = p.strip_prefix("icons/").and_then(|n| n.strip_suffix(".svg")) else {
            return Ok(None);
        };
        Ok(path(name).map(|d| {
            Cow::Owned(
                format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#fff" d="{d}"/></svg>"##).into_bytes(),
            )
        }))
    }

    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
