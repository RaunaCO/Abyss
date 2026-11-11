//! Colours and sizes: the dark theme (default, close to the official desktop
//! app) and a light theme with its own palette (not an inversion).

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use gpui::{rgb, rgba, Rgba};

static LIGHT: AtomicBool = AtomicBool::new(false);
/// The user's accent colour (0xRRGGBB); Abyss's own, never the system's.
static ACCENT: AtomicU32 = AtomicU32::new(DEFAULT_ACCENT);

/// Abyss's original cyan.
pub const DEFAULT_ACCENT: u32 = 0x33ffee;
/// Ready-made accents offered in Ajustes (value, name).
pub fn accent_presets() -> [(u32, &'static str); 8] {
    [
        (DEFAULT_ACCENT, crate::i18n::t("cyan")),
        (0x4d9dff, crate::i18n::t("blue")),
        (0x9d7bff, crate::i18n::t("violet")),
        (0xff6fb0, crate::i18n::t("pink")),
        (0xff6b57, crate::i18n::t("coral")),
        (0xffb43a, crate::i18n::t("amber")),
        (0x5fd98a, crate::i18n::t("green")),
        (0xe8e8ee, crate::i18n::t("white")),
    ]
}

/// Set the accent colour (0xRRGGBB).
pub fn set_accent(rgb: u32) {
    ACCENT.store(rgb & 0xffffff, Ordering::Relaxed);
}

pub fn accent_rgb() -> u32 {
    ACCENT.load(Ordering::Relaxed)
}

/// Relative luminance (WCAG) of a colour.
fn luminance(c: Rgba) -> f32 {
    let f = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
}

/// The accent as drawn: the chosen colour on the dark theme; on the light
/// theme the same hue darkened just enough to read on white (contrast >= 3.6).
fn accent_color() -> Rgba {
    let rgb = accent_rgb();
    if !is_light() {
        return rgb_u32(rgb);
    }
    if rgb == DEFAULT_ACCENT {
        // The tuned teal of the original light palette.
        return rgb_u32(0x00877d);
    }
    let base = rgb_u32(rgb);
    let mut c = base;
    for _ in 0..40 {
        if (1.05) / (luminance(c) + 0.05) >= 3.6 {
            break;
        }
        c = Rgba { r: c.r * 0.93, g: c.g * 0.93, b: c.b * 0.93, a: 1.0 };
    }
    c
}

pub fn rgb_u32(v: u32) -> Rgba {
    rgb(v)
}

thread_local! {
    /// Depth of `dark_scope` calls: the immersive Now Playing view (and the
    /// bar over it) keep the dark palette in both themes, since they sit on
    /// the darkened artwork.
    static FORCE_DARK: Cell<u32> = const { Cell::new(0) };
}

/// Switch between the dark (default) and light palettes.
pub fn set_light(on: bool) {
    LIGHT.store(on, Ordering::Relaxed);
}

/// The light palette is in use (outside a `dark_scope`).
pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed) && FORCE_DARK.with(|f| f.get() == 0)
}

/// Build `f` with the dark palette whatever the theme.
pub fn dark_scope<T>(f: impl FnOnce() -> T) -> T {
    FORCE_DARK.with(|d| d.set(d.get() + 1));
    let out = f();
    FORCE_DARK.with(|d| d.set(d.get() - 1));
    out
}

/// Dark value or light value, depending on the palette in use.
fn pick(dark: u32, light: u32) -> Rgba {
    rgb(if is_light() { light } else { dark })
}
fn pick_a(dark: u32, light: u32) -> Rgba {
    rgba(if is_light() { light } else { dark })
}

pub fn bg() -> Rgba {
    pick(0x000000, 0xf6f6f8)
}
pub fn surface() -> Rgba {
    pick(0x161616, 0xe9e9e9)
}
/// Row / item hover: the official app's `--wave-color-solid-base-brighter`.
pub fn hover() -> Rgba {
    pick(0x292929, 0xe2e2e2)
}
/// Pressed / strongest hover: `--wave-color-solid-base-brightest`.
pub fn active() -> Rgba {
    pick(0x303030, 0xd6d6d6)
}
/// A notch above `active` (hovered controls on an active background).
pub fn active_strong() -> Rgba {
    pick(0x454545, 0xc6c6c6)
}
pub fn elevated() -> Rgba {
    frame()
}
/// Floating surfaces (menus, popovers, dialogs, tooltips): solid, a step
/// above anything they can cover, with a visible edge and a moderate shadow.
pub fn popover() -> Rgba {
    frame()
}
/// A softly shaded translucent surface.
pub fn liquid_glass(highlight: f32) -> gpui::Background {
    let highlight = highlight.clamp(0., 1.);
    let top = glass_top();
    let lit = pick_a(0x303030f5, 0xfcfcfcff);
    let bottom = pick_a(0x1d1d1dec, 0xefefeff5);
    gpui::linear_gradient(145., gpui::linear_color_stop(super::interact::mix(top, lit, highlight), 0.), gpui::linear_color_stop(bottom, 1.))
}

/// The colour where a glass surface starts (its top-left), for shapes that
/// continue it, such as the inline album panel's tip.
pub fn glass_top() -> Rgba {
    pick_a(0x252525ee, 0xf7f7f7f7)
}

pub fn glass_rim(highlight: f32) -> Rgba {
    super::interact::mix(pick_a(0xffffff38, 0x73737348), pick_a(0xffffff5b, 0x73737368), highlight.clamp(0., 1.))
}

pub fn glass_shadow() -> Vec<gpui::BoxShadow> {
    vec![gpui::BoxShadow {
        color: gpui::Hsla::from(pick_a(0x00000058, 0x00000020)),
        offset: gpui::point(super::sc(0.), super::sc(5.)),
        blur_radius: super::sc(16.),
        spread_radius: super::sc(-4.),
    }]
}

pub fn popover_border() -> Rgba {
    glass_rim(0.2)
}
pub fn popover_shadow() -> Vec<gpui::BoxShadow> {
    let (a1, a2) = if is_light() { (0.14, 0.08) } else { (0.55, 0.35) };
    vec![
        gpui::BoxShadow {
            color: gpui::Hsla::from(Rgba { r: 0.0, g: 0.0, b: 0.0, a: a1 }),
            offset: gpui::point(gpui::px(0.), gpui::px(10.)),
            blur_radius: gpui::px(28.),
            spread_radius: gpui::px(-2.),
        },
        gpui::BoxShadow {
            color: gpui::Hsla::from(Rgba { r: 0.0, g: 0.0, b: 0.0, a: a2 }),
            offset: gpui::point(gpui::px(0.), gpui::px(2.)),
            blur_radius: gpui::px(6.),
            spread_radius: gpui::px(0.),
        },
    ]
}
pub fn menu_hover() -> Rgba {
    hover()
}
/// Surface of compact controls (segmented switches, selectors, search):
/// a dark grey just above the page background.
pub fn control() -> Rgba {
    frame()
}
/// Edge of compact controls (so they read against the page in both themes).
pub fn control_border() -> Rgba {
    divider()
}
/// The lifted option inside a segmented control.
pub fn control_active() -> Rgba {
    pick(0x3a3a3a, 0xffffff)
}
/// The window frame: the sidebar rail and the thin margin around the page
/// panel, like a browser's chrome around its tab.
pub const DARK_FRAME: u32 = 0x202020;
pub const LIGHT_FRAME: u32 = 0xebebeb;

pub fn frame_rgb() -> u32 {
    if is_light() {
        LIGHT_FRAME
    } else {
        DARK_FRAME
    }
}

pub fn frame() -> Rgba {
    rgb(frame_rgb())
}
/// Edge of the page panel inside the frame.
pub fn panel_border() -> Rgba {
    divider()
}
/// Gap between the page panel and the window edge / player bar (logical px).
pub const FRAME_GAP: f32 = 6.0;
/// Corner radius of the page panel.
pub const PANEL_R: f32 = 10.0;
/// Selected navigation row in the sidebar.
pub fn nav_active() -> Rgba {
    pick(0x303030, 0xffffff)
}
/// Inputs and option boxes.
pub fn field() -> Rgba {
    frame()
}
pub fn border() -> Rgba {
    divider()
}
/// Rules between rows, panes and chrome, visible over both the page and frame.
pub fn divider() -> Rgba {
    pick(0x3d3d3d, 0xc2c2c2)
}
/// The text colour at `a` opacity: light tints on dark, dark tints on light
/// (hover washes, separators, faint borders).
pub fn ink(a: f32) -> Rgba {
    Rgba { a, ..if is_light() { rgb(0x14141a) } else { rgb(0xffffff) } }
}
/// Translucent panel over content (tooltips, floating buttons).
pub fn glass(h: f32) -> Rgba {
    if is_light() {
        super::interact::mix(rgba(0xffffffeb), rgba(0xf1f1f5f2), h)
    } else {
        super::interact::mix(rgba(0x1d1d22eb), rgba(0x2a2a30f2), h)
    }
}
pub fn text() -> Rgba {
    pick(0xffffff, 0x14141a)
}
pub fn text_secondary() -> Rgba {
    pick(0xb4b4bc, 0x52525b)
}
pub fn text_dim() -> Rgba {
    pick(0x8a8a97, 0x6b6b77)
}
/// The accent used for the playing track, progress and active items: cyan on
/// dark, a deeper teal on light so it stays readable on white.
pub fn accent() -> Rgba {
    accent_color()
}
pub fn accent_dim() -> Rgba {
    Rgba { a: if is_light() { 0.18 } else { 0.20 }, ..accent_color() }
}
/// "MAX" quality badge.
pub fn gold() -> Rgba {
    pick(0xffd432, 0x9a6f00)
}
pub fn gold_bg() -> Rgba {
    pick_a(0xffd43222, 0xd9a4002a)
}
/// Solid gold-tinted background of a selected quality option.
pub fn gold_solid() -> Rgba {
    pick(0x2a2a20, 0xf5edd2)
}
pub fn high() -> Rgba {
    accent()
}
pub fn high_bg() -> Rgba {
    Rgba { a: 0.12, ..accent_color() }
}
pub fn selection() -> Rgba {
    Rgba { a: if is_light() { 0.22 } else { 0.25 }, ..accent_color() }
}
pub fn danger() -> Rgba {
    pick(0xff5a5a, 0xd12f2f)
}
/// Primary button: white on dark, near-black on light.
pub fn button() -> Rgba {
    pick(0xffffff, 0x14141a)
}
pub fn button_text() -> Rgba {
    pick(0x000000, 0xffffff)
}
pub fn button_hover() -> Rgba {
    pick(0xececf0, 0x2c2c34)
}
/// Switch track off / on and its knob off / on.
pub fn switch_colors() -> (Rgba, Rgba, Rgba, Rgba) {
    if is_light() {
        (rgb(0xcfcfd8), rgb(0x14141a), rgb(0xffffff), rgb(0xffffff))
    } else {
        (rgb(0x303036), rgb(0xe4e4e7), rgb(0x8a8a93), rgb(0x111111))
    }
}

pub const SIDEBAR_W: f32 = 280.0;
pub const PLAYER_H: f32 = 68.0;
pub const QUEUE_W: f32 = 360.0;
/// Target cover width in grids (albums, artists, playlists...).
pub const CARD_W: f32 = 176.0;
/// Cover width of the horizontal rows (home, artist page).
pub const ROW_CARD_W: f32 = 130.0;
pub const ROW_H: f32 = 46.0;
/// Height of the top bar (breadcrumb, window controls, search). Now Playing
/// and overlays start below it; the sidebar's identity row has the same height.
pub const TOP_H: f32 = 52.0;
/// Where Now Playing and menus start (below the top bar).
pub const CAPTION_H: f32 = TOP_H;
/// The page column is centred and never wider than this.
pub const CONTENT_W: f32 = 1120.0;
/// Widest content column (every page but Settings).
pub const WIDE_W: f32 = 1480.0;
pub const GAP: f32 = 12.0;

pub const FONT: &str = "Inter";

/// Shared type hierarchy, using the bundled OFL Inter weights and system CJK fallback.
pub const TYPE_BODY: f32 = 14.0;
pub const TYPE_LABEL: f32 = 12.0;
pub const TYPE_SECTION: f32 = 16.0;
pub const TYPE_TITLE: f32 = 28.0;
pub const TYPE_BRAND: f32 = 20.0;
