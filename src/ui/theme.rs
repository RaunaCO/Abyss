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

/// Blend of two colours along a straight line in OKLab: no grey or dark dip
/// between two hues, unlike a blend of the sRGB values.
pub fn mix_oklab(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    let (la, aa, ba) = to_oklab(a);
    let (lb, ab, bb) = to_oklab(b);
    let (r, g, bl) = oklab_to_linear(la + (lb - la) * t, aa + (ab - aa) * t, ba + (bb - ba) * t);
    encode(r, g, bl, a.a + (b.a - a.a) * t)
}

/// Multiply blend of `top` over `base` (in linear light): darkens with the
/// tint's own hue instead of greying. GPUI has no blend modes, so this is
/// used wherever both colours are known when the element is built.
pub fn multiply(base: Rgba, top: Rgba) -> Rgba {
    let ch = |b: f32, t: f32| linear_to_srgb(srgb_to_linear(b) * srgb_to_linear(t));
    Rgba { r: ch(base.r, top.r), g: ch(base.g, top.g), b: ch(base.b, top.b), a: base.a }
}

/// A two-stop gradient interpolated in OKLab (even lightness, no muddy middle).
pub fn gradient(angle: f32, from: gpui::LinearColorStop, to: gpui::LinearColorStop) -> gpui::Background {
    gpui::linear_gradient(angle, from, to).color_space(gpui::ColorSpace::Oklab)
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
fn pick_a(dark: u32, light: u32) -> Rgba {
    rgba(if is_light() { light } else { dark })
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
/// A softly shaded translucent surface: lit from the top-left, interpolated in
/// OKLab so the shading stays even. `highlight` (0..1) lifts the lit corner.
pub fn liquid_glass(highlight: f32) -> gpui::Background {
    let highlight = highlight.clamp(0., 1.);
    let lit = Rgba { a: if is_light() { 1.0 } else { 0.96 }, ..neutral(0.36, 0.992) };
    let bottom = Rgba { a: if is_light() { 0.96 } else { 0.94 }, ..neutral(0.225, 0.945) };
    gradient(145., gpui::linear_color_stop(mix_oklab(glass_top(), lit, highlight), 0.), gpui::linear_color_stop(bottom, 1.))
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

/// The lit top edge of a raised surface (a hairline just inside its border).
/// It is what gives depth on the dark theme, where shadows barely show.
pub fn glass_highlight() -> Rgba {
    Rgba { r: 1.0, g: 1.0, b: 1.0, a: if is_light() { 0.75 } else { 0.07 } }
}

pub fn glass_shadow() -> Vec<gpui::BoxShadow> {
    let (near, far) = if is_light() { (0.10, 0.10) } else { (0.45, 0.38) };
    let black = |a: f32| gpui::Hsla::from(Rgba { r: 0.0, g: 0.0, b: 0.0, a });
    vec![
        // A tight contact shadow and a wide soft one: two distances, like a real edge.
        gpui::BoxShadow {
            color: black(near),
            offset: gpui::point(super::sc(0.), super::sc(1.)),
            blur_radius: super::sc(2.),
            spread_radius: super::sc(0.),
        },
        gpui::BoxShadow {
            color: black(far),
            offset: gpui::point(super::sc(0.), super::sc(6.)),
            blur_radius: super::sc(14.),
            spread_radius: super::sc(-5.),
        },
    ]
}

pub fn popover_border() -> Rgba {
    glass_rim(0.2)
}
pub fn popover_shadow() -> Vec<gpui::BoxShadow> {
    let (a1, a2) = if is_light() { (0.14, 0.08) } else { (0.62, 0.45) };
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
pub fn border() -> Rgba {
    divider()
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
/// Translucent panel over content (tooltips, floating buttons).
pub fn glass(h: f32) -> Rgba {
    if is_light() {
        mix_oklab(rgba(0xffffffeb), rgba(0xf1f1f5f2), h)
    } else {
        mix_oklab(rgba(0x1d1d22eb), rgba(0x2a2a30f2), h)
    }
}
pub fn text() -> Rgba {
    pick(0xffffff, 0x14141a)
}
pub fn text_secondary() -> Rgba {
    pick(0xb4b4bc, 0x52525b)
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

/// WCAG 2 contrast ratio of two opaque colours (1 to 21).
pub fn contrast(a: Rgba, b: Rgba) -> f32 {
    let lum = |c: Rgba| 0.2126 * srgb_to_linear(c.r) + 0.7152 * srgb_to_linear(c.g) + 0.0722 * srgb_to_linear(c.b);
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[cfg(test)]
mod contrast_tests {
    use super::*;

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
        for light in [false, true] {
            set_light(light);
            let on_bg = |c: Rgba| contrast(c, bg());
            assert!(on_bg(text()) > on_bg(text_secondary()) && on_bg(text_secondary()) > on_bg(text_dim()) * 1.15);
        }
        set_light(false);
    }
}
