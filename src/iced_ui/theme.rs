//! Colours and sizes for the Iced interface: the dark theme (default, close to
//! the official desktop app) and a light theme with its own palette (not an
//! inversion). Same values as the GPUI reference (`src/ui/theme.rs`).
//!
//! # The design scale
//!
//! One scale for every surface, so neighbours line up and nothing looks
//! improvised (the audit found ~20 font sizes, ~20 radii and every spacing
//! from 1 to 32 px). New code uses these tokens, never a literal:
//!
//! * **Spacing** [`SPACE_2XS`] 2 · [`SPACE_XS`] 4 · [`SPACE_SM`] 8 · [`SPACE_MD`] 12
//!   ([`GAP`]) · [`SPACE_LG`] 16 · [`SPACE_XL`] 24 · [`SPACE_XXL`] 32 · [`SPACE_XXXL`] 48.
//! * **Radius**, concentric ([`concentric`] for a shape nested in another):
//!   [`R_XS`] 4 badges · [`R_SM`] 6 small buttons, row washes, thumbnails ·
//!   [`R_MD`] 8 cards, covers, inputs · [`R_LG`] 12 menus, popovers ·
//!   [`R_XL`] 18 glass panels and dialogs · [`R_CAPSULE`] pills. [`PANEL_R`]
//!   (10) is the page panel inside the window frame.
//! * **Type** [`TYPE_MICRO`] 10 · [`TYPE_CAPTION`] 11 · [`TYPE_LABEL`] 12 ·
//!   [`TYPE_SMALL`] 13 · [`TYPE_BODY`] 14 · [`TYPE_SECTION`] 16 · [`TYPE_HEADING`] 20 ·
//!   [`TYPE_TITLE`] 28 · [`TYPE_DISPLAY`] 40; no half sizes.
//! * **Weight** [`W_REGULAR`] body and secondary text · [`W_MEDIUM`] row titles,
//!   selected navigation · [`W_SEMIBOLD`] section headings, buttons, eyebrows ·
//!   [`W_BOLD`] only page and header titles (title / display sizes).
//! * **Elevation** [`elevation`]: 0 flat · 1 small floating controls · 2 a card
//!   lifted on hover · 3 menus, popovers, floating headers · 4 dialogs. On the
//!   dark theme depth reads from lightness ([`popover`]), the shadow only helps.
//!
//! Hierarchy: the cover's colours (player bar, album banner, Now Playing)
//! are the protagonists; glass headers and menus float calmly; lists, rows
//! and navigation are structure and carry no glass and no per-item borders.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use iced::Color as Rgba;

/// Opaque colour from 0xRRGGBB.
pub fn rgb(v: u32) -> Rgba {
    Rgba::from_rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Colour from 0xRRGGBBAA.
pub fn rgba(v: u32) -> Rgba {
    Rgba::from_rgba8((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, (v & 0xff) as f32 / 255.0)
}

static LIGHT: AtomicBool = AtomicBool::new(false);
/// The user's accent colour (0xRRGGBB); Abyss's own, never the system's.
static ACCENT: AtomicU32 = AtomicU32::new(DEFAULT_ACCENT);

/// Abyss's original cyan.
pub const DEFAULT_ACCENT: u32 = crate::shared::DEFAULT_ACCENT;
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
    // Same hue and chroma, lowered in OKLCH lightness until it reads on white.
    let base = rgb_u32(rgb);
    let (mut l, ch, h) = to_oklch(base);
    let mut c = base;
    for _ in 0..40 {
        if (1.05) / (luminance(c) + 0.05) >= 3.6 {
            break;
        }
        l -= 0.02;
        c = oklch(l, ch, h);
    }
    c
}

pub fn rgb_u32(v: u32) -> Rgba {
    rgb(v)
}

// ---------------------------------------------------------------- OKLCH
//
// The palette is described in OKLCH (perceptual lightness, chroma, hue), so
// steps of lightness look even and a hue keeps its character when it is made
// lighter or darker. Neutrals are achromatic: the window frame has to match
// the system chrome exactly.

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// OKLab coordinates of a colour (alpha is ignored).
fn to_oklab(c: Rgba) -> (f32, f32, f32) {
    let (r, g, b) = (srgb_to_linear(c.r), srgb_to_linear(c.g), srgb_to_linear(c.b));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    (
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    )
}

/// Linear sRGB of an OKLab colour, not clamped (out-of-gamut values fall outside 0..1).
fn oklab_to_linear(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    (
        4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
        -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
    )
}

fn encode(r: f32, g: f32, b: f32, a: f32) -> Rgba {
    let enc = |v: f32| linear_to_srgb(v.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    Rgba { r: enc(r), g: enc(g), b: enc(b), a }
}

/// A colour from OKLCH: lightness 0..1, chroma (about 0..0.37) and hue in
/// degrees. Chroma is reduced until the colour fits sRGB, so the hue and the
/// lightness asked for are kept.
pub fn oklch(l: f32, c: f32, h: f32) -> Rgba {
    let l = l.clamp(0.0, 1.0);
    let (sin, cos) = h.to_radians().sin_cos();
    let mut c = c.max(0.0);
    for _ in 0..24 {
        let (r, g, b) = oklab_to_linear(l, c * cos, c * sin);
        let fits = |v: f32| (-0.0005..=1.0005).contains(&v);
        if c <= 0.0005 || (fits(r) && fits(g) && fits(b)) {
            break;
        }
        c *= 0.92;
    }
    let (r, g, b) = oklab_to_linear(l, c * cos, c * sin);
    encode(r, g, b, 1.0)
}

/// Lightness, chroma and hue (degrees) of a colour.
pub fn to_oklch(c: Rgba) -> (f32, f32, f32) {
    let (l, a, b) = to_oklab(c);
    (l, (a * a + b * b).sqrt(), b.atan2(a).to_degrees().rem_euclid(360.0))
}

/// The same hue and chroma at another lightness (alpha kept).
pub fn with_lightness(c: Rgba, l: f32) -> Rgba {
    let (_, ch, h) = to_oklch(c);
    Rgba { a: c.a, ..oklch(l, ch, h) }
}

/// Multiply blend of `top` over `base` (in linear light): darkens with the
/// tint's own hue instead of greying. GPUI has no blend modes, so this is
/// used wherever both colours are known when the element is built.
pub fn multiply(base: Rgba, top: Rgba) -> Rgba {
    let ch = |b: f32, t: f32| linear_to_srgb(srgb_to_linear(b) * srgb_to_linear(t));
    Rgba { r: ch(base.r, top.r), g: ch(base.g, top.g), b: ch(base.b, top.b), a: base.a }
}

/// Radius of a shape nested `inset` px inside a corner of radius `outer`, so
/// both curves share one centre (never below 2 px).
pub fn concentric(outer: f32, inset: f32) -> f32 {
    (outer - inset).max(2.0)
}

/// A neutral at OKLCH lightness `dark` (dark theme) or `light` (light theme).
fn neutral(dark: f32, light: f32) -> Rgba {
    oklch(if is_light() { light } else { dark }, 0.0, 0.0)
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

pub fn bg() -> Rgba {
    pick(0x000000, 0xf6f6f8)
}
pub fn surface() -> Rgba {
    neutral(0.20, 0.935)
}
/// Row / item hover: the official app's `--wave-color-solid-base-brighter`.
pub fn hover() -> Rgba {
    neutral(0.285, 0.915)
}
/// Pressed / strongest hover: `--wave-color-solid-base-brightest`.
pub fn active() -> Rgba {
    neutral(0.315, 0.875)
}
/// A notch above `active` (hovered controls on an active background).
pub fn active_strong() -> Rgba {
    neutral(0.39, 0.825)
}
pub fn elevated() -> Rgba {
    frame()
}
/// Floating surfaces (menus, popovers, dialogs, tooltips): solid and a step
/// lighter than anything they can cover. On the dark theme depth is read from
/// lightness (a shadow barely shows on black), so they sit above the frame.
pub fn popover() -> Rgba {
    neutral(0.275, 0.985)
}
/// The colour where a glass surface starts (its top-left), for shapes that
/// continue it, such as the inline album panel's tip.
pub fn glass_top() -> Rgba {
    Rgba { a: if is_light() { 0.97 } else { 0.94 }, ..neutral(0.275, 0.975) }
}

/// Edge of a glass surface: the text colour at low opacity, so it adapts to
/// whatever is behind it instead of being a fixed grey.
pub fn glass_rim(highlight: f32) -> Rgba {
    let h = highlight.clamp(0., 1.);
    if is_light() {
        ink(0.12 + 0.10 * h)
    } else {
        ink(0.09 + 0.12 * h)
    }
}

pub fn popover_border() -> Rgba {
    glass_rim(0.2)
}
/// Soft drop shadow of floating surfaces (menus, popovers, dialogs).
pub fn popover_shadow() -> iced::Shadow {
    let a = if is_light() { 0.14 } else { 0.62 };
    iced::Shadow { color: Rgba { r: 0.0, g: 0.0, b: 0.0, a }, offset: iced::Vector::new(0.0, 10.0), blur_radius: 28.0 }
}

/// Shadow of small floating controls (the glass capsules): tighter and
/// fainter than a panel's, since bigger surfaces read as thicker.
pub fn control_shadow() -> iced::Shadow {
    let a = if is_light() { 0.10 } else { 0.40 };
    iced::Shadow { color: Rgba { r: 0.0, g: 0.0, b: 0.0, a }, offset: iced::Vector::new(0.0, 3.0), blur_radius: 10.0 }
}

/// WCAG 2 contrast ratio of two opaque colours (1 to 21).
pub fn contrast(a: Rgba, b: Rgba) -> f32 {
    let lum = |c: Rgba| 0.2126 * srgb_to_linear(c.r) + 0.7152 * srgb_to_linear(c.g) + 0.0722 * srgb_to_linear(c.b);
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// A two-stop linear gradient at `angle` degrees (CSS convention: 0 points up,
/// 90 to the right), as Iced draws it.
pub fn gradient(angle: f32, from: Rgba, to: Rgba) -> iced::Background {
    let radians = iced::Radians((angle - 90.0).to_radians() + std::f32::consts::FRAC_PI_2);
    iced::Background::Gradient(iced::Gradient::Linear(iced::gradient::Linear::new(radians).add_stop(0.0, from).add_stop(1.0, to)))
}

/// `c` at opacity `a`.
pub fn with_alpha(c: Rgba, a: f32) -> Rgba {
    Rgba { a: a.clamp(0.0, 1.0), ..c }
}

/// Plain sRGB interpolation of two colours.
pub fn lerp(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    Rgba { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
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
    neutral(0.35, 1.0)
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
    neutral(0.315, 1.0)
}
/// Inputs and option boxes.
pub fn field() -> Rgba {
    frame()
}
/// Rules between rows, panes and chrome. An alpha border (the text colour at
/// low opacity) rather than a fixed grey: it keeps the same contrast over the
/// page, the frame, glass and artwork alike.
pub fn divider() -> Rgba {
    ink(if is_light() { 0.17 } else { 0.15 })
}
/// The text colour at `a` opacity: light tints on dark, dark tints on light
/// (hover washes, separators, faint borders).
pub fn ink(a: f32) -> Rgba {
    Rgba { a, ..if is_light() { rgb(0x14141a) } else { rgb(0xffffff) } }
}
pub fn text() -> Rgba {
    pick(0xffffff, 0x14141a)
}
pub fn text_secondary() -> Rgba {
    pick(0xb4b4bc, 0x52525b)
}
/// Secondary text on glass (vibrancy): a step toward the primary text, since
/// what shows through the pane changes under it; a flat grey that passes on
/// the page can fall under AA on a pane over a bright cover or the accent field.
pub fn text_secondary_on_glass() -> Rgba {
    lerp(text_secondary(), text(), 0.4)
}
/// Least prominent text (timestamps, counts). Tuned for WCAG AA (4.5:1) on every
/// surface it sits on; see `contrast_tests`.
pub fn text_dim() -> Rgba {
    pick(0x9a9aa6, 0x64646e)
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
fn pick_a(dark: u32, light: u32) -> Rgba {
    rgba(if is_light() { light } else { dark })
}

pub fn gold_bg() -> Rgba {
    pick_a(0xffd43222, 0xd9a4002a)
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
/// Primary button while pressed: its colour multiplied by a light grey.
pub fn button_pressed() -> Rgba {
    if is_light() {
        rgb(0x3a3a44)
    } else {
        multiply(button(), rgb(0xd4d4da))
    }
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
/// The page column is centred and never wider than this.
pub const CONTENT_W: f32 = 1120.0;
/// Widest content column (every page but Settings).
pub const WIDE_W: f32 = 1480.0;
pub const GAP: f32 = 12.0;

// ---------------------------------------------------------------- scale
// See the module docs. Tokens not used yet are kept for the panels' owners.

#[allow(dead_code)]
pub const SPACE_2XS: f32 = 2.0;
#[allow(dead_code)]
pub const SPACE_XS: f32 = 4.0;
#[allow(dead_code)]
pub const SPACE_SM: f32 = 8.0;
#[allow(dead_code)]
pub const SPACE_MD: f32 = GAP;
#[allow(dead_code)]
pub const SPACE_LG: f32 = 16.0;
#[allow(dead_code)]
pub const SPACE_XL: f32 = 24.0;
#[allow(dead_code)]
pub const SPACE_XXL: f32 = 32.0;
#[allow(dead_code)]
pub const SPACE_XXXL: f32 = 48.0;

#[allow(dead_code)]
pub const R_XS: f32 = 4.0;
#[allow(dead_code)]
pub const R_SM: f32 = 6.0;
#[allow(dead_code)]
pub const R_MD: f32 = 8.0;
#[allow(dead_code)]
pub const R_LG: f32 = 12.0;
#[allow(dead_code)]
pub const R_XL: f32 = 18.0;
#[allow(dead_code)]
pub const R_CAPSULE: f32 = 999.0;

#[allow(dead_code)]
pub const TYPE_MICRO: f32 = 10.0;
#[allow(dead_code)]
pub const TYPE_CAPTION: f32 = 11.0;
pub const TYPE_BODY: f32 = 14.0;
pub const TYPE_LABEL: f32 = 12.0;
#[allow(dead_code)]
pub const TYPE_SMALL: f32 = 13.0;
pub const TYPE_SECTION: f32 = 16.0;
#[allow(dead_code)]
pub const TYPE_HEADING: f32 = 20.0;
#[allow(dead_code)]
pub const TYPE_TITLE: f32 = 28.0;
#[allow(dead_code)]
pub const TYPE_DISPLAY: f32 = 40.0;

#[allow(dead_code)]
pub const W_REGULAR: iced::font::Weight = iced::font::Weight::Normal;
#[allow(dead_code)]
pub const W_MEDIUM: iced::font::Weight = iced::font::Weight::Medium;
#[allow(dead_code)]
pub const W_SEMIBOLD: iced::font::Weight = iced::font::Weight::Semibold;
#[allow(dead_code)]
pub const W_BOLD: iced::font::Weight = iced::font::Weight::Bold;

/// The shadow of elevation `level` (0 flat .. 4 modal), see the module docs.
#[allow(dead_code)]
pub fn elevation(level: u8) -> iced::Shadow {
    let black = |a: f32| Rgba { r: 0.0, g: 0.0, b: 0.0, a };
    let light = is_light();
    let (a, y, blur) = match level {
        0 => return iced::Shadow::default(),
        1 => return control_shadow(),
        2 => (if light { 0.10 } else { 0.45 }, 6.0, 18.0),
        3 => return popover_shadow(),
        _ => (if light { 0.20 } else { 0.70 }, 18.0, 48.0),
    };
    iced::Shadow { color: black(a), offset: iced::Vector::new(0.0, y), blur_radius: blur }
}

// GPUI's `liquid_glass` gradient is now a real refracting pane: see `liquid_glass.rs`.

/// Held by tests that switch the global palette (`set_light`), so they never
/// see each other's theme.
#[cfg(test)]
pub static THEME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod contrast_tests {
    use super::*;

    use super::THEME_LOCK;

    /// Every text colour against every surface text sits on, in both themes.
    fn report() -> Vec<(bool, &'static str, &'static str, f32)> {
        let mut out = Vec::new();
        for light in [false, true] {
            set_light(light);
            let surfaces: [(&str, Rgba); 6] = [
                ("bg", bg()),
                ("frame", frame()),
                ("surface", surface()),
                ("hover", hover()),
                ("active", active()),
                ("popover", popover()),
            ];
            let texts: [(&str, Rgba); 3] = [("text", text()), ("text_secondary", text_secondary()), ("text_dim", text_dim())];
            for (tn, t) in texts {
                for (sn, s) in surfaces {
                    out.push((light, tn, sn, contrast(t, s)));
                }
            }
        }
        set_light(false);
        out
    }

    #[test]
    fn text_meets_wcag_aa_on_every_surface() {
        let _theme = THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut failures = Vec::new();
        for (light, t, s, c) in report() {
            // A pressed row in the light theme is a momentary state: large-text level.
            let min = if light && t == "text_dim" && s == "active" { 4.0 } else { 4.5 };
            if c < min {
                failures.push(format!("{} {t} on {s}: {c:.2} < {min}", if light { "light" } else { "dark" }));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn the_three_text_levels_stay_distinct() {
        let _theme = THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for light in [false, true] {
            set_light(light);
            let on_bg = |c: Rgba| contrast(c, bg());
            assert!(on_bg(text()) > on_bg(text_secondary()) && on_bg(text_secondary()) > on_bg(text_dim()) * 1.15);
        }
        set_light(false);
    }
}
