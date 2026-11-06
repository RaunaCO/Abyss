//! Colours and sizes of the dark theme (close to the official desktop app).

use gpui::{rgb, rgba, Rgba};

pub fn bg() -> Rgba {
    rgb(0x000000)
}
pub fn sidebar() -> Rgba {
    rgb(0x000000)
}
pub fn player_bar() -> Rgba {
    rgb(0x0b0b0b)
}
pub fn surface() -> Rgba {
    rgb(0x161616)
}
pub fn hover() -> Rgba {
    rgba(0xffffff14)
}
pub fn active() -> Rgba {
    rgba(0xffffff22)
}
pub fn elevated() -> Rgba {
    rgb(0x232327)
}
pub fn border() -> Rgba {
    rgba(0xffffff1a)
}
pub fn text() -> Rgba {
    rgb(0xffffff)
}
pub fn text_secondary() -> Rgba {
    rgb(0xa9a9ae)
}
pub fn text_dim() -> Rgba {
    rgb(0x6e6e74)
}
/// The cyan accent used for the playing track, progress and active items.
pub fn accent() -> Rgba {
    rgb(0x33ffee)
}
pub fn accent_dim() -> Rgba {
    rgba(0x33ffee33)
}
/// "MAX" quality badge.
pub fn gold() -> Rgba {
    rgb(0xffd432)
}
pub fn gold_bg() -> Rgba {
    rgba(0xffd43222)
}
pub fn high() -> Rgba {
    rgb(0x33ffee)
}
pub fn high_bg() -> Rgba {
    rgba(0x33ffee1f)
}
pub fn selection() -> Rgba {
    rgba(0x33ffee40)
}
pub fn danger() -> Rgba {
    rgb(0xff5a5a)
}
pub fn button() -> Rgba {
    rgb(0xffffff)
}
pub fn button_text() -> Rgba {
    rgb(0x000000)
}

pub const SIDEBAR_W: f32 = 220.0;
pub const PLAYER_H: f32 = 68.0;
pub const QUEUE_W: f32 = 360.0;
/// Target cover width in grids (albums, artists, playlists...).
pub const CARD_W: f32 = 176.0;
/// Cover width of the horizontal rows (home, artist page).
pub const ROW_CARD_W: f32 = 130.0;
pub const ROW_H: f32 = 36.0;
/// Height of the caption row (menu, name, window buttons) and of the header row below it.
pub const CAPTION_H: f32 = 30.0;
pub const HEADER_H: f32 = 46.0;
/// The page column is centred and never wider than this.
pub const CONTENT_W: f32 = 1120.0;
pub const GAP: f32 = 12.0;

pub const FONT: &str = "Inter";
