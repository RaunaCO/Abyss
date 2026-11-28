//! Liquid glass on the GPU.
//!
//! An Iced primitive cannot read what was drawn behind it, so the glass
//! refracts a known picture instead: the *scene*, an image that conceptually
//! lies behind the whole window (the page's cover, see `liquid_glass`). It is
//! blurred on a worker thread once per cover into a small mip chain
//! ([`SceneImage`]), uploaded once and cached here by key; nothing is uploaded
//! per frame but one uniform block per surface.
//!
//! Each surface is one full-viewport triangle (Iced sets the viewport to the
//! primitive's bounds). The fragment shader:
//!
//! * finds the pixel in a rounded rectangle (signed distance, antialiased at
//!   any scale factor) and the outward normal of its edge;
//! * displaces where the scene is read along that normal with a lens profile
//!   that is strong at the rim and flat in the middle (the background looks
//!   magnified and bent toward the edge), red more and blue less than green
//!   (chromatic dispersion);
//! * reads a blurred level of the scene (sharper toward the rim), saturates
//!   it and frosts it with the theme's glass tint so text stays readable;
//! * lights it: a thin specular rim, brightest on the edge facing the light
//!   (top left at rest, leaning toward the pointer on hover), a bevel glow
//!   inside it and a soft inner shadow on the far side, a sheen over the upper
//!   part, a soft light under the pointer and a glint that runs along the
//!   rim with it (the live reflection), and GPUI's 1 px rim stroke.
//!
//! The same pipeline also draws the page wash (`mode` 1): the scene, heavily
//! blurred, fading out below the top of the page, so what the glass refracts
//! is what the eye sees around it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use iced::widget::shader::{self, Viewport};
use iced::{Color, Rectangle};

/// Side of the sharpest level of a scene (px); each further level is half.
pub const SCENE_SIZE: u32 = 512;
/// Levels of a scene: 512, 256, 128 and 64 px, each blurrier than the last.
pub const SCENE_LEVELS: u32 = 4;
/// Scenes kept on the GPU (about 1.4 MB each).
const KEEP: usize = 3;
/// Bytes between two surfaces' uniform blocks (the dynamic offset alignment).
const STRIDE: u64 = 256;

/// The picture behind the window, blurred into a mip chain (RGBA8, sRGB values).
#[derive(Debug)]
pub struct SceneImage {
    pub size: u32,
    pub levels: Vec<Vec<u8>>,
}

impl SceneImage {
    /// Build the chain from any image: cropped to a square, scaled to
    /// [`SCENE_SIZE`], lightly saturated, then blurred more at each level.
    pub fn from_image(src: &image::DynamicImage) -> Self {
        use image::imageops::{self, FilterType};
        let side = src.width().min(src.height()).max(1);
        let square = src.crop_imm((src.width() - side) / 2, (src.height() - side) / 2, side, side);
        let mut base = square.resize_exact(SCENE_SIZE, SCENE_SIZE, FilterType::Triangle).to_rgba8();
        for p in base.pixels_mut() {
            let [r, g, b, _] = p.0.map(f32::from);
            let lum = 0.299 * r + 0.587 * g + 0.114 * b;
            let sat = |c: f32| (lum + (c - lum) * 1.12).clamp(0.0, 255.0) as u8;
            p.0 = [sat(r), sat(g), sat(b), 255];
        }
        // Blur (in px of each level): light at the top, very soft at the bottom.
        let sigmas = [1.4_f32, 2.4, 3.0, 3.0];
        let mut levels = Vec::with_capacity(SCENE_LEVELS as usize);
        let mut current = base;
        for (k, sigma) in sigmas.iter().enumerate() {
            if k > 0 {
                let s = SCENE_SIZE >> k;
                current = imageops::resize(&current, s, s, FilterType::Triangle);
            }
            levels.push(imageops::blur(&current, *sigma).into_raw());
        }
        Self { size: SCENE_SIZE, levels }
    }

    /// A soft field of the accent colour (no cover to show): two glows of the
    /// accent and a neighbouring hue over a dark base.
    pub fn accent(accent: Color) -> Self {
        let n = 64u32;
        let (r0, g0, b0) = (accent.r, accent.g, accent.b);
        // A neighbouring hue: rotate the channels a little.
        let other = [g0 * 0.6 + b0 * 0.4, b0 * 0.7 + r0 * 0.3, r0 * 0.6 + g0 * 0.4];
        let img = image::RgbaImage::from_fn(n, n, |x, y| {
            let (u, v) = (x as f32 / (n - 1) as f32, y as f32 / (n - 1) as f32);
            let glow = |cx: f32, cy: f32, rad: f32| {
                let d = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() / rad;
                (1.0 - d).clamp(0.0, 1.0).powi(2)
            };
            let a = glow(0.28, 0.22, 0.75);
            let b = glow(0.78, 0.7, 0.7);
            let base = 0.10;
            let ch = |c: f32, o: f32| ((base + c * a * 0.9 + o * b * 0.7).clamp(0.0, 1.0) * 255.0) as u8;
            image::Rgba([ch(r0, other[0]), ch(g0, other[1]), ch(b0, other[2]), 255])
        });
        Self::from_image(&image::DynamicImage::ImageRgba8(img))
    }

    /// A soft field made from the cover's three colours.
    pub fn palette([first, second, third]: [Color; 3]) -> Self {
        let colors = [first, second, third].map(|c| [c.r, c.g, c.b]);
        let img = image::RgbaImage::from_fn(64, 64, |x, y| {
            let (x, y) = (x as f32 / 63.0, y as f32 / 63.0);
            let weight = |cx: f32, cy: f32| (-5.5 * ((x - cx).powi(2) + (y - cy).powi(2))).exp();
            let weights = [weight(0.18, 0.22), weight(0.84, 0.34), weight(0.45, 0.87)];
            let total = weights.iter().sum::<f32>();
            let channel = |i: usize| {
                let mixed = colors.iter().zip(weights).map(|(c, w)| c[i] * w).sum::<f32>() / total;
                (mixed * 255.0) as u8
            };
            image::Rgba([channel(0), channel(1), channel(2), 255])
        });
        Self::from_image(&image::DynamicImage::ImageRgba8(img))
    }

    /// Bytes on the GPU.
    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.levels.iter().map(Vec::len).sum()
    }
}

/// A scene with the key it is cached under (the cover URL, or the accent).
#[derive(Debug, Clone)]
pub struct Scene {
    pub key: String,
    pub image: Arc<SceneImage>,
}

/// How a glass surface bends and lights the scene. Lengths are logical px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Optics {
    /// Largest displacement of the scene, at the rim.
    pub refraction: f32,
    /// Width of the curved edge (the lens); inside it the pane is flat.
    pub bevel: f32,
    /// Red is displaced `1 + dispersion` times as far as green, blue `1 - dispersion`.
    pub dispersion: f32,
    /// Blur: the scene level read in the flat middle (0 sharp .. 3 softest).
    pub frost: f32,
    /// Opacity of the theme's glass tint over the scene (dark theme; the light
    /// theme adds 0.14). Lowered by 60 % at the rim, where glass is clearest.
    pub tint: f32,
    /// Strength of the rim, bevel and sheen lights.
    pub specular: f32,
    /// Strength of the pointer's light and of the glint on the rim.
    pub sheen: f32,
    /// Whole-pane magnification (0.03: the scene appears 3 % larger).
    pub magnify: f32,
}

impl Optics {
    /// Large panels: headers, the featured card, the inline album, Replay.
    pub const PANEL: Optics =
        Optics { refraction: 60.0, bevel: 52.0, dispersion: 0.30, frost: 1.1, tint: 0.40, specular: 1.15, sheen: 1.1, magnify: 0.06 };
    /// A panel over the full-screen artwork (Now Playing's queue, similar and
    /// credits): the panel's lens with a little more frost, so the dimmest
    /// text (times, 11 px) keeps AA over a bright photograph.
    pub const OVER_ART: Optics =
        Optics { refraction: 60.0, bevel: 52.0, dispersion: 0.30, frost: 1.4, tint: 0.52, specular: 1.15, sheen: 1.1, magnify: 0.06 };
    /// The player bar (68 px tall: a narrower lens).
    pub const BAR: Optics =
        Optics { refraction: 40.0, bevel: 30.0, dispersion: 0.36, frost: 0.8, tint: 0.16, specular: 1.0, sheen: 1.0, magnify: 0.04 };
    /// Menus, popovers and tooltips: more frost and tint, they sit over text.
    pub const POPOVER: Optics =
        Optics { refraction: 30.0, bevel: 22.0, dispersion: 0.39, frost: 1.6, tint: 0.62, specular: 1.0, sheen: 0.9, magnify: 0.03 };
    /// Small controls (the top bar's capsules, the tabs): a lens as narrow as
    /// the control is short, frosted enough for 12 px text, a softer rim so a
    /// row of them does not sparkle.
    pub const CONTROL: Optics =
        Optics { refraction: 14.0, bevel: 12.0, dispersion: 0.40, frost: 1.5, tint: 0.58, specular: 0.85, sheen: 0.8, magnify: 0.02 };
    /// Prominent (tinted) glass, a page's primary action: the tint carries the
    /// colour, so it is nearly opaque (the label keeps AA contrast) and its
    /// sheen is low (it would wash the tint out on the light theme).
    pub const PROMINENT: Optics =
        Optics { refraction: 12.0, bevel: 12.0, dispersion: 0.36, frost: 1.5, tint: 0.84, specular: 0.7, sheen: 0.7, magnify: 0.0 };
    /// No bending at all (tests compare against it).
    pub const FLAT: Optics =
        Optics { refraction: 0.0, bevel: 34.0, dispersion: 0.0, frost: 1.0, tint: 0.46, specular: 1.0, sheen: 1.0, magnify: 0.0 };
}

/// The glass's pointer light: position relative to the surface (-1..1 from
/// the centre), hover and press amounts (0..1).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Light {
    pub x: f32,
    pub y: f32,
    pub hover: f32,
    pub press: f32,
}

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    /// A glass surface whose rounded rectangle is the primitive's bounds less `inset`.
    /// `opacity` fades the whole pane (the entrance of a floating surface).
    Glass {
        radius: f32,
        inset: f32,
        optics: Optics,
        light: Light,
        tint: Color,
        rim: Color,
        opacity: f32,
        bar_light: Option<([Color; 3], f32)>,
        /// `bar_light` is an album's own colours in a large pane (cleaner,
        /// stronger mixing) rather than the player bar's.
        album_light: bool,
        /// The album banner behind the pane, refracted as it is drawn.
        page_light: Option<PageLight>,
    },
    /// The page wash: the scene fading out over `height` px below `top` (both
    /// relative to the primitive's bounds), at `alpha` at its top.
    /// `lights`: the cover's three colours and the seconds elapsed, drawn as
    /// slowly drifting glows instead of the scene (album pages).
    Wash { top: f32, height: f32, alpha: f32, lights: Option<([Color; 3], f32)> },
    /// The colour field behind the enlarged cover, fitted to the whole window:
    /// the cover itself, very soft, with its colours drifting over it as
    /// lights (`lights`: colours and the ambient clock), toned and vignetted.
    CoverBackdrop { lights: Option<([Color; 3], f32)> },
}

/// The album banner (the page wash's drifting glows), as panes over it see
/// it: where it is drawn (logical px), its colours, clock and opacity, and
/// the page colour under it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageLight {
    pub colors: [Color; 3],
    pub elapsed: f32,
    pub area: Rectangle,
    pub top: f32,
    pub height: f32,
    pub alpha: f32,
    pub bg: Color,
}

#[derive(Debug)]
pub struct GlassPrimitive {
    pub scene: Option<Scene>,
    pub kind: Kind,
    /// Palette of the surface (captured when the view was built: Now Playing
    /// is always dark).
    pub light_theme: bool,
    slot: AtomicU32,
}

impl GlassPrimitive {
    pub fn new(scene: Option<Scene>, kind: Kind, light_theme: bool) -> Self {
        Self { scene, kind, light_theme, slot: AtomicU32::new(u32::MAX) }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    /// x, y, width, height of the glass (physical px in the frame).
    rect: [f32; 4],
    /// Window size (physical px), scale factor, mode (0 glass, 1 wash, 2 cover backdrop).
    screen: [f32; 4],
    /// Corner radius, bevel, refraction (physical px), dispersion.
    shape: [f32; 4],
    /// Frost (level), tint opacity, specular, sheen.
    look: [f32; 4],
    /// Tint colour (sRGB) and the light theme flag.
    tint: [f32; 4],
    /// Pointer (physical px in the frame), hover, press.
    light: [f32; 4],
    /// Rim stroke colour (sRGB) and opacity.
    rim: [f32; 4],
    /// sRGB target, wash top, wash height (physical px), wash opacity.
    extra: [f32; 4],
    /// Scene gain (dark theme), saturation, magnification, scene present.
    scene: [f32; 4],
    /// Bar palette in sRGB, each packed with the light's horizontal position.
    bar_colors: [[f32; 4]; 3],
    /// Vertical light positions, and which lights are active (1 bar, 2 the album banner).
    bar_y: [f32; 4],
    /// The album banner's left edge and width (physical px), opacity, and the
    /// seconds on the lights' clock (the liquid flow of banner, bar and panes).
    page: [f32; 4],
    /// The page colour under the banner (sRGB).
    page_bg: [f32; 4],
}

const WGSL: &str = r#"
struct U {
    rect: vec4<f32>, screen: vec4<f32>, shape: vec4<f32>, look: vec4<f32>,
    tint: vec4<f32>, light: vec4<f32>, rim: vec4<f32>, extra: vec4<f32>, scene: vec4<f32>,
    bar_colors: array<vec4<f32>, 3>, bar_y: vec4<f32>, page: vec4<f32>, page_bg: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32((vi << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vi & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// Signed distance to a rounded rectangle centred at the origin (negative inside).
fn sd_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Outward normal of the nearest edge (the SDF's gradient).
fn box_normal(p: vec2<f32>, half: vec2<f32>, r: f32) -> vec2<f32> {
    let s = select(vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), p >= vec2<f32>(0.0, 0.0));
    let q = abs(p) - half + vec2<f32>(r, r);
    var g = vec2<f32>(0.0, 1.0);
    if (q.x > 0.0 && q.y > 0.0) {
        g = normalize(q);
    } else if (q.x > q.y) {
        g = vec2<f32>(1.0, 0.0);
    }
    return g * s;
}

// Lens profile: 1 at the rim (s = 0), falling to 0 at the inner edge of the bevel (s = 1).
fn lens(s: f32) -> f32 {
    let k = clamp(1.0 - s, 0.0, 1.0);
    return k * sqrt(k);
}

fn vivid(c: vec3<f32>, amount: f32) -> vec3<f32> {
    let l = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    return clamp(mix(vec3<f32>(l), c, amount), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let k = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(k, vec3<f32>(1.0 / 2.4)) - 0.055, k * 12.92, k <= vec3<f32>(0.0031308));
}

// OKLab (from linear sRGB) and back: mixing there keeps hues clean between
// two colours instead of passing through grey or brown.
fn oklab(c: vec3<f32>) -> vec3<f32> {
    let l = pow(max(0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b, 0.0), 1.0 / 3.0);
    let m = pow(max(0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b, 0.0), 1.0 / 3.0);
    let s = pow(max(0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b, 0.0), 1.0 / 3.0);
    return vec3<f32>(
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    );
}

fn from_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = pow(c.x + 0.3963377774 * c.y + 0.2158037573 * c.z, 3.0);
    let m = pow(c.x - 0.1055613458 * c.y - 0.0638541728 * c.z, 3.0);
    let s = pow(c.x - 0.0894841775 * c.y - 1.2914855480 * c.z, 3.0);
    return vec3<f32>(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    );
}

// Liquid flow: a slow, smooth warp of a colour field (`uv`, 0..1 over the
// surface) at `t` seconds, as if the colour were carried by a gentle current.
// Two crossed waves, each bent by the other, so it never reads as a sine.
// `amount` is the largest shift, in surface fractions: a few dozen px at most.
fn flow(uv: vec2<f32>, t: f32, amount: vec2<f32>) -> vec2<f32> {
    // Not at an even pace: the current swells and eases (three slow,
    // unrelated periods, so it never repeats a rhythm).
    let s = 1.45 * t + 2.8 * sin(t * 0.137) + 1.9 * sin(t * 0.089 + 1.7) + 1.1 * sin(t * 0.229 + 4.1);
    // A first warp, then a second one read through it: the field folds into
    // curls, as ink does in water, instead of travelling as waves.
    let q = vec2<f32>(
        sin(uv.y * 6.0 + s * 0.50 + 1.3 * sin(uv.x * 3.1 - s * 0.37)),
        sin(uv.x * 5.0 - s * 0.43 + 1.2 * sin(uv.y * 2.6 + s * 0.31)),
    );
    let w = uv + 0.06 * q;
    let r = vec2<f32>(sin(w.y * 4.3 - s * 0.29 + 1.4 * q.x), sin(w.x * 3.7 + s * 0.33 + 1.4 * q.y));
    return uv + amount * (0.6 * q + 0.6 * r);
}

// The album banner at a frame position, for a band `w` wide from `x0`,
// fading over `h` below `top`: the cover's colours as three soft lights
// (mixed in OKLab; squared weights keep each in its own region), brighter at
// their hearts. Alpha holds near the top and falls off like light, not linearly.
fn banner(p: vec2<f32>, x0: f32, w: f32, top: f32, h: f32) -> vec4<f32> {
    let at = vec2<f32>((p.x - x0) / max(w, 1.0), (p.y - top) / max(h, 1.0));
    let uv = flow(at, u.page.w, vec2<f32>(0.05, 0.12));
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    var reach = 0.0;
    for (var i = 0u; i < 3u; i++) {
        let light = u.bar_colors[i];
        let d = (uv - vec2<f32>(light.w, u.bar_y[i])) * vec2<f32>(2.2, 1.4);
        let k = exp(-dot(d, d));
        sum += k * k * oklab(to_linear(light.rgb));
        weight += k * k;
        reach += k;
    }
    var lab = sum / max(weight, 0.0001);
    lab.x = lab.x * clamp(0.80 + 0.28 * reach, 0.80, 1.06);
    let t = clamp(at.y, 0.0, 1.0);
    let hold = (exp(-3.2 * t * t) - exp(-3.2)) / (1.0 - exp(-3.2));
    return vec4<f32>(clamp(to_srgb(from_oklab(lab)), vec3<f32>(0.0), vec3<f32>(1.0)), hold);
}

// The scene at a frame position: the picture covers the window (cropped to
// its aspect), so every surface reads the same picture at its own place.
fn scene_at(sp: vec2<f32>, lod: f32) -> vec3<f32> {
    if (u.bar_y.w > 2.5) {
        // An album's own pane: its cover's colours as soft lights drifting
        // inside the glass (mixed in OKLab, each keeping its own region).
        let uv = flow((sp - u.rect.xy) / max(u.rect.zw, vec2<f32>(1.0)), u.page.w, vec2<f32>(0.05, 0.10));
        var sum = vec3<f32>(0.0);
        var weight = 0.0;
        var reach = 0.0;
        for (var i = 0u; i < 3u; i++) {
            let light = u.bar_colors[i];
            let d = (uv - vec2<f32>(light.w, u.bar_y[i])) * vec2<f32>(2.4, 1.3);
            let k = exp(-dot(d, d));
            sum += k * k * oklab(to_linear(light.rgb));
            weight += k * k;
            reach += k;
        }
        let lab = sum / max(weight, 0.0001);
        let lights = clamp(to_srgb(from_oklab(lab)), vec3<f32>(0.0), vec3<f32>(1.0));
        // A glow behind the text, not paint over it: secondary text stays readable.
        let strength = select(0.58, 0.45, u.tint.w > 0.5);
        return mix(u.tint.rgb, lights, strength * (1.0 - exp(-2.2 * reach)));
    }
    if (u.bar_y.w > 1.5) {
        // Over the album banner: exactly what is drawn behind the pane.
        let b = banner(sp, u.page.x, u.page.y, u.extra.y, u.extra.z);
        return mix(u.page_bg.rgb, b.rgb, b.a * u.page.z);
    }
    if (u.bar_y.w > 0.5) {
        // The player bar: the cover's colours (see `bar_palette`) as three
        // lights carried by the current, mixed in OKLab (two opposite hues
        // meet through colour, not grey) with squared weights so each keeps
        // a region of its own.
        let uv = flow((sp - u.rect.xy) / max(u.rect.zw, vec2<f32>(1.0)), u.page.w, vec2<f32>(0.05, 0.24));
        var sum = vec3<f32>(0.0);
        var weight = 0.0;
        var reach = 0.0;
        for (var i = 0u; i < 3u; i++) {
            let light = u.bar_colors[i];
            let d = (uv - vec2<f32>(light.w, u.bar_y[i])) * vec2<f32>(3.2 / u.page_bg[i], 0.85);
            let w = exp(-dot(d, d));
            sum += w * w * oklab(to_linear(light.rgb));
            weight += w * w;
            reach += w;
        }
        let palette = clamp(to_srgb(from_oklab(sum / max(weight, 0.0001))), vec3<f32>(0.0), vec3<f32>(1.0));
        let strength = select(0.86, 0.40, u.tint.w > 0.5);
        return mix(u.tint.rgb, palette, strength * (1.0 - exp(-2.2 * reach)));
    }
    let m = max(u.screen.x, u.screen.y);
    let uv = (sp - u.screen.xy * 0.5) / m + vec2<f32>(0.5, 0.5);
    return textureSampleLevel(tex, samp, uv, lod).rgb;
}

fn finish(col: vec3<f32>, a: f32, pos: vec2<f32>) -> vec4<f32> {
    let n = fract(sin(dot(floor(pos), vec2<f32>(12.9898, 78.233))) * 43758.5453) - 0.5;
    var c = clamp(col + vec3<f32>(n / 255.0), vec3<f32>(0.0), vec3<f32>(1.0));
    if (u.extra.x > 0.5) {
        c = to_linear(c);
    }
    return vec4<f32>(c * a, a);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = pos.xy;
    let scale = u.screen.z;
    let light_theme = u.tint.w > 0.5;

    if (u.screen.w > 1.5) {
        // Behind the enlarged cover: the cover itself, as soft as it gets and
        // covering the window, carried by the current; its colours drifting
        // over it; then toned down in OKLab lightness (chroma mostly kept,
        // so a colour stays a colour and a black-and-white cover stays a
        // composition of its own greys) with a vignette that gives it depth.
        let at = (p - u.rect.xy) / max(u.rect.zw, vec2<f32>(1.0));
        let aspect = u.rect.zw / max(u.rect.z, u.rect.w);
        let fp = flow(at, u.page.w * 0.6, vec2<f32>(0.035, 0.035));
        let cuv = (fp - vec2<f32>(0.5)) * aspect / 1.12 + vec2<f32>(0.5);
        var col = textureSampleLevel(tex, samp, cuv, 3.0).rgb;
        if (u.bar_y.w > 0.5) {
            var sum = vec3<f32>(0.0);
            var weight = 0.0;
            var reach = 0.0;
            for (var i = 0u; i < 3u; i++) {
                let light = u.bar_colors[i];
                let d = (fp - vec2<f32>(light.w, u.bar_y[i])) * aspect / u.page_bg[i];
                let w = exp(-dot(d, d));
                sum += w * w * oklab(to_linear(light.rgb));
                weight += w * w;
                reach += w;
            }
            let lights = clamp(to_srgb(from_oklab(sum / max(weight, 0.0001))), vec3<f32>(0.0), vec3<f32>(1.0));
            col = mix(col, lights, 0.30 * (1.0 - exp(-1.6 * reach)));
        }
        var lab = oklab(to_linear(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0))));
        let corner = smoothstep(0.25, 0.9, length((at - vec2<f32>(0.5)) * vec2<f32>(1.0, aspect.y / aspect.x * 1.4)));
        let k = 0.60 * (1.0 - 0.55 * corner);
        lab = vec3<f32>(lab.x * k, lab.yz * mix(1.0, k, 0.4));
        return finish(clamp(to_srgb(from_oklab(lab)), vec3<f32>(0.0), vec3<f32>(1.0)), 1.0, p);
    }
    if (u.screen.w > 0.5) {
        // The page wash.
        let t = clamp((p.y - u.extra.y) / max(u.extra.z, 1.0), 0.0, 1.0);
        let a = u.extra.w * pow(1.0 - t, 1.6) * u.scene.w;
        if (a <= 0.0005) {
            discard;
        }
        if (u.bar_y.w > 0.5) {
            // The album banner: the cover's colours as drifting lights.
            let b = banner(p, u.rect.x, u.rect.z, u.extra.y, u.extra.z);
            return finish(b.rgb, u.extra.w * b.a, p);
        }
        var c = vivid(scene_at(p, 3.0), u.scene.y);
        if (light_theme) {
            c = mix(c, vec3<f32>(1.0), 0.3);
        } else {
            c = c * u.scene.x;
        }
        return finish(c, a, p);
    }

    let half = u.rect.zw * 0.5;
    let center = u.rect.xy + half;
    let q = p - center;
    let r = min(u.shape.x, min(half.x, half.y));
    let d = sd_box(q, half, r);
    let cov = clamp(0.5 - d, 0.0, 1.0);
    if (cov <= 0.0) {
        discard;
    }
    let bevel = max(min(u.shape.y, min(half.x, half.y) * 0.9), 1.0);
    // The normal of a rounder box: no seam along the diagonals inside small corners.
    let n = box_normal(q, half, max(r, bevel));
    let k = lens(clamp(-d / bevel, 0.0, 1.0));

    // Refraction: the whole pane magnifies a little; the rim bends strongly inward.
    let base = center + q * (1.0 - u.scene.z);
    let off = -n * u.shape.z * k;
    let disp = u.shape.w;
    // Sharper toward the rim, but never the sharpest level: a photograph's
    // fine detail, bent and split into red and blue there, read as noise.
    let lod = max(u.look.x - 0.9 * k, min(u.look.x, 0.8));
    let cr = scene_at(base + off * (1.0 + disp), lod).r;
    let cg = scene_at(base + off, lod).g;
    let cb = scene_at(base + off * (1.0 - disp), lod).b;
    var col = vivid(vec3<f32>(cr, cg, cb), u.scene.y);
    if (u.bar_y.w < 0.5) {
        if (light_theme) {
            col = mix(col, vec3<f32>(1.0), 0.22);
        } else {
            col = col * u.scene.x;
        }
    }
    if (u.scene.w < 0.5) {
        // Nothing behind (the chrome): the pane's own body, a touch lighter
        // at its top than at its foot, so it reads as a lens, not a grey slab.
        let yy0 = clamp((p.y - u.rect.y) / max(u.rect.w, 1.0), 0.0, 1.0);
        col = u.tint.rgb * select(1.10 - 0.22 * yy0, 1.0 - 0.04 * yy0, light_theme);
    }
    // Over the album banner the pane is lit by it: thinner frost so its
    // colour comes through, and the light caught in the edges takes the
    // banner's colour (read at the pane's top, where the light comes from).
    let banner_lit = u.bar_y.w > 1.5;
    var ambient = vec3<f32>(0.0);
    var ambient_a = 0.0;
    if (u.bar_y.w > 2.5) {
        // An album's pane is lit from inside: its edges catch its own colours.
        ambient = scene_at(vec2<f32>(p.x, u.rect.y + 0.2 * u.rect.w), 0.0);
        ambient_a = 0.7;
    } else if (banner_lit) {
        let b = banner(vec2<f32>(p.x, u.rect.y), u.page.x, u.page.y, u.extra.y, u.extra.z);
        ambient = b.rgb;
        // The light falls from above: a pane under the banner's fading tail still catches some.
        ambient_a = sqrt(b.a) * u.page.z;
    }
    // Frosting: the theme's glass colour, a little thinner at the clear rim.
    let frost = u.look.y * select(1.0, 1.0 - 0.55 * ambient_a, banner_lit);
    col = mix(col, u.tint.rgb, clamp(frost * (1.0 - 0.6 * k), 0.0, 1.0));
    // Light scattered inside the frosted pane, strongest toward its top.
    let from_top = clamp((p.y - u.rect.y) / max(u.rect.w, 1.0), 0.0, 1.0);
    col = mix(col, ambient, 0.26 * ambient_a * (1.0 - 0.55 * from_top));

    let spec_c = mix(
        select(vec3<f32>(0.788, 0.804, 0.851), vec3<f32>(1.0, 1.0, 1.0), light_theme),
        mix(ambient, vec3<f32>(1.0), 0.35),
        0.55 * ambient_a,
    );
    let shade_c = select(vec3<f32>(0.788, 0.804, 0.851), vec3<f32>(0.125, 0.133, 0.173), light_theme);
    let hover = u.light.z;
    let press = u.light.w;
    let spec = u.look.z;

    // Where the light comes from: the top left, leaning toward the pointer.
    var ldir = vec2<f32>(-0.6, -0.8);
    let to_ptr = u.light.xy - center;
    if (hover > 0.001 && length(to_ptr) > 1.0) {
        ldir = normalize(mix(ldir, normalize(to_ptr), 0.30 * hover));
    }
    let facing = dot(n, ldir);
    let lit = max(facing, 0.0);
    let far = max(-facing, 0.0);

    // The pane's thickness: light caught inside the curved edge, shadow opposite.
    let glow = k * k * k * (0.10 + 0.32 * lit * lit) * spec * select(1.0, 0.6, light_theme);
    col = mix(col, spec_c, clamp(glow, 0.0, 1.0));
    col = col * (1.0 - k * k * far * select(0.30, 0.10, light_theme));

    // Sheen over the upper part.
    let yy = clamp((p.y - u.rect.y) / max(u.rect.w, 1.0), 0.0, 1.0);
    let sheen = select(0.05 + 0.015 * hover, 0.30 + 0.03 * hover, light_theme) * spec * (1.0 - smoothstep(0.0, 0.62, yy));
    col = mix(col, spec_c, sheen);

    // The pointer's light, and its reflection running along the bevel.
    let dist = length(p - u.light.xy);
    let sigma = clamp(min(u.rect.z, u.rect.w) * 0.45, 26.0 * scale, 170.0 * scale);
    let spot = exp(-dist * dist / (2.0 * sigma * sigma)) * hover * u.look.w;
    col = mix(col, spec_c, clamp(spot * (select(0.10, 0.05, light_theme) + 0.03 * press) + spot * k * k * 0.16, 0.0, 1.0));

    // Light split by the curved edge: a faint rainbow running along the bevel,
    // its hue turning with the edge's direction (and drifting a little toward
    // the pointer). Strongest on the lit side, gone on flat glass (`disp` 0).
    let along = (q.x / max(half.x, 1.0) + q.y / max(half.y, 1.0)) * 0.22;
    let fringe_t = atan2(n.y, n.x) / 6.2831853 + 0.5 + along + 0.08 * dot(n, ldir) + 0.05 * hover;
    let rainbow = clamp(abs(fract(fringe_t + vec3<f32>(0.0, 0.6667, 0.3333)) * 6.0 - 3.0) - 1.0, vec3<f32>(0.0), vec3<f32>(1.0));
    let ring = k * k * (1.0 - k) * 4.0;
    let fringe = ring * (0.35 + 0.65 * lit) * clamp(disp * 2.0, 0.0, 1.0) * select(0.10, 0.07, light_theme);
    col = mix(col, mix(col, rainbow, 0.85) * select(1.15, 1.0, light_theme), fringe);

    // GPUI's 1 px rim (glass_rim), then the specular edge over it.
    let stroke = 1.0 - smoothstep(0.5 * scale, 1.25 * scale, -d);
    col = mix(col, u.rim.rgb, u.rim.a * stroke);
    let band = 1.0 - smoothstep(0.6 * scale, 2.2 * scale, -d);
    let rim_lit = (select(0.30, 0.8, light_theme) + 0.04 * hover + 0.03 * press) * spec;
    // Bright on the edge facing the light, nearly gone on the far side: a
    // pane catches light, it is not framed in white.
    let edge = band * rim_lit * (0.12 + 0.88 * pow(lit, 1.5));
    let gs = sigma * 0.6;
    let glint = band * exp(-dist * dist / (2.0 * gs * gs)) * hover * 0.28 * u.look.w;
    col = mix(col, spec_c, clamp(edge + glint, 0.0, 1.0));
    col = mix(col, shade_c, band * far * select(0.10 + 0.05 * hover, 0.07, light_theme));

    // The pane's opacity (`extra.w`): 1 at rest, lower while a menu fades in.
    return finish(col, cov * u.extra.w, p);
}
"#;

struct SceneGpu {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    group: wgpu::BindGroup,
    used: u64,
}

pub struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    capacity: u64,
    frame_data: Vec<Uniforms>,
    scenes: HashMap<String, SceneGpu>,
    srgb: bool,
    frame: u64,
}

const BLANK: &str = "\u{0}blank";

fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glass"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: uniforms,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                }),
            },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

fn uniform_buffer(device: &wgpu::Device, capacity: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("glass uniforms"),
        size: capacity * STRIDE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl Pipeline {
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, key: &str, image: &SceneImage) {
        let levels = image.levels.len().max(1) as u32;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glass scene"),
            size: wgpu::Extent3d { width: image.size, height: image.size, depth_or_array_layers: 1 },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (k, data) in image.levels.iter().enumerate() {
            let s = (image.size >> k).max(1);
            if data.len() < (s * s * 4) as usize {
                continue;
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: k as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * s), rows_per_image: Some(s) },
                wgpu::Extent3d { width: s, height: s, depth_or_array_layers: 1 },
            );
        }
        let view = texture.create_view(&Default::default());
        let group = bind_group(device, &self.layout, &self.uniforms, &view, &self.sampler);
        self.scenes.insert(key.to_string(), SceneGpu { _texture: texture, view, group, used: self.frame });
    }
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let module = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("glass"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glass"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glass"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glass"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::MirrorRepeat,
            address_mode_v: wgpu::AddressMode::MirrorRepeat,
            ..Default::default()
        });
        let capacity = 16;
        let uniforms = uniform_buffer(device, capacity);
        let mut pipeline = Pipeline {
            pipeline,
            layout,
            sampler,
            uniforms,
            capacity,
            frame_data: Vec::new(),
            scenes: HashMap::new(),
            srgb: format.is_srgb(),
            frame: 0,
        };
        // A 1 px grey scene for surfaces drawn before any picture exists.
        let blank = SceneImage { size: 1, levels: vec![vec![40, 40, 46, 255]] };
        pipeline.upload(device, queue, BLANK, &blank);
        pipeline
    }

    fn trim(&mut self) {
        self.frame += 1;
        self.frame_data.clear();
        if self.scenes.len() > KEEP + 1 {
            let mut old: Vec<(String, u64)> =
                self.scenes.iter().filter(|(k, _)| k.as_str() != BLANK).map(|(k, s)| (k.clone(), s.used)).collect();
            old.sort_by_key(|(_, used)| *used);
            let excess = self.scenes.len() - (KEEP + 1);
            for (k, used) in old.into_iter().take(excess) {
                // Never drop what the frame just drawn used.
                if used + 1 < self.frame {
                    self.scenes.remove(&k);
                }
            }
        }
    }
}

fn premul_free(c: Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

/// The uniforms of a primitive at `bounds` (logical px), in a frame of
/// `physical` px at `scale`.
pub fn uniforms(primitive: &GlassPrimitive, bounds: &Rectangle, physical: (f32, f32), scale: f32, srgb: bool) -> Uniforms {
    let light_theme = primitive.light_theme;
    let has_scene = primitive.scene.is_some();
    let px = |v: f32| v * scale;
    let mut u = Uniforms {
        screen: [physical.0.max(1.0), physical.1.max(1.0), scale, 0.0],
        extra: [if srgb { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        scene: [0.62, 1.15, 0.0, if has_scene { 1.0 } else { 0.0 }],
        tint: [0.0, 0.0, 0.0, if light_theme { 1.0 } else { 0.0 }],
        ..Default::default()
    };
    match primitive.kind {
        Kind::Glass { radius, inset, optics, light, tint, rim, opacity, bar_light, album_light, page_light } => {
            let rect = Rectangle {
                x: bounds.x + inset,
                y: bounds.y + inset,
                width: (bounds.width - 2.0 * inset).max(0.0),
                height: (bounds.height - 2.0 * inset).max(0.0),
            };
            u.rect = [px(rect.x), px(rect.y), px(rect.width), px(rect.height)];
            u.shape = [px(radius), px(optics.bevel), px(optics.refraction), optics.dispersion];
            // The accent field is an invented light (no cover to show), not
            // content: glass over it is frostier, so the dim text on it keeps
            // AA (a settings or library pane read 2-4:1 over its cyan).
            let invented = primitive.scene.as_ref().is_some_and(|s| s.key.starts_with("accent:"));
            let tint_a = optics.tint + if light_theme { 0.14 } else { 0.0 } + if invented { 0.18 } else { 0.0 };
            let tint_a = tint_a.min(0.95);
            u.look = [optics.frost, tint_a, optics.specular, optics.sheen];
            u.tint = [tint.r, tint.g, tint.b, u.tint[3]];
            let center = (rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
            u.light = [
                px(center.0 + light.x.clamp(-1.0, 1.0) * rect.width * 0.5),
                px(center.1 + light.y.clamp(-1.0, 1.0) * rect.height * 0.5),
                light.hover.clamp(0.0, 1.0),
                light.press.clamp(0.0, 1.0),
            ];
            u.rim = premul_free(rim);
            u.scene[2] = optics.magnify;
            u.extra[3] = opacity.clamp(0.0, 1.0);
            if let Some((colors, elapsed)) = bar_light {
                let centers = if album_light { wash_centers(elapsed) } else { bar_centers(elapsed) };
                if !album_light {
                    let k = bar_sizes(elapsed);
                    u.page_bg = [k[0], k[1], k[2], 1.0];
                }
                u.bar_colors = std::array::from_fn(|i| [colors[i].r, colors[i].g, colors[i].b, centers[i][0]]);
                let mode = if album_light { 3.0 } else { 1.0 };
                // The band's lights sit near its top; in a pane they spread over its height.
                let y = |v: f32| if album_light { 0.15 + 0.7 * ((v + 0.2) / 0.6) } else { v };
                u.bar_y = [y(centers[0][1]), y(centers[1][1]), y(centers[2][1]), mode];
                u.page[3] = elapsed;
                u.scene[3] = 1.0;
            } else if let Some(page) = page_light {
                let centers = wash_centers(page.elapsed);
                let colors = page.colors;
                u.bar_colors = std::array::from_fn(|i| [colors[i].r, colors[i].g, colors[i].b, centers[i][0]]);
                u.bar_y = [centers[0][1], centers[1][1], centers[2][1], 2.0];
                u.extra[1] = px(page.area.y + page.top);
                u.extra[2] = px(page.height);
                u.page = [px(page.area.x), px(page.area.width), page.alpha, page.elapsed];
                u.page_bg = [page.bg.r, page.bg.g, page.bg.b, 1.0];
                u.scene[3] = 1.0;
            }
        }
        Kind::Wash { top, height, alpha, lights } => {
            u.rect = [px(bounds.x), px(bounds.y), px(bounds.width), px(bounds.height)];
            u.screen[3] = 1.0;
            u.extra = [u.extra[0], px(bounds.y + top), px(height), alpha];
            u.scene[0] = 0.8;
            if let Some((colors, elapsed)) = lights {
                let centers = wash_centers(elapsed);
                u.bar_colors = std::array::from_fn(|i| [colors[i].r, colors[i].g, colors[i].b, centers[i][0]]);
                u.bar_y = [centers[0][1], centers[1][1], centers[2][1], 1.0];
                u.page[3] = elapsed;
                u.scene[3] = 1.0;
            }
        }
        Kind::CoverBackdrop { lights } => {
            u.rect = [px(bounds.x), px(bounds.y), px(bounds.width), px(bounds.height)];
            u.screen[3] = 2.0;
            if let Some((colors, elapsed)) = lights {
                let spots = super::backdrop::light_spots(elapsed);
                u.bar_colors = std::array::from_fn(|i| [colors[i].r, colors[i].g, colors[i].b, spots[i][0]]);
                u.bar_y = [spots[0][1], spots[1][1], spots[2][1], 1.0];
                u.page_bg = [spots[0][2], spots[1][2], spots[2][2], 1.0];
                u.page[3] = elapsed;
            }
        }
    }
    u
}

/// OKLab of an sRGB colour: lightness, then the a / b axes.
pub fn to_oklab(c: Color) -> [f32; 3] {
    let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let (r, g, b) = (lin(c.r), lin(c.g), lin(c.b));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).max(0.0).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).max(0.0).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).max(0.0).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// The sRGB colour of OKLab coordinates (clipped into the gamut).
pub fn from_oklab([l, a, b]: [f32; 3]) -> Color {
    from_oklch(l, a.hypot(b), b.atan2(a))
}

/// The sRGB colour of OKLCh lightness `l`, chroma `c` and hue `h` (radians),
/// clipped into the gamut.
fn from_oklch(l: f32, c: f32, h: f32) -> Color {
    let (a, b) = (c * h.cos(), c * h.sin());
    let lm = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let mm = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let sm = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    let enc = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    };
    Color::from_rgb(
        enc(4.076_741_7 * lm - 3.307_711_6 * mm + 0.230_969_94 * sm),
        enc(-1.268_438 * lm + 2.609_757_4 * mm - 0.341_319_38 * sm),
        enc(-0.004_196_086_3 * lm - 0.703_418_6 * mm + 1.707_614_7 * sm),
    )
}

/// The album banner's three lights from a cover's palette, each with a role.
/// The most colourful colour leads (first: the light beside the cover); a
/// grey next to it becomes a deep shade of the same hue instead of diluting
/// it to mud; a black-and-white cover gets a dim graphite glow, not a grey
/// slab. Lightness and chroma stay in a band that glows behind white text
/// without turning neon (lighter and softer on the light theme).
pub fn banner_palette(palette: [Color; 3], light_theme: bool) -> [Color; 3] {
    let lab = palette.map(to_oklab);
    let chroma = |i: usize| lab[i][1].hypot(lab[i][2]);
    let hue = |i: usize| lab[i][2].atan2(lab[i][1]);
    let lead = (0..3).max_by(|&a, &b| chroma(a).total_cmp(&chroma(b))).unwrap_or(0);
    let order: Vec<usize> = std::iter::once(lead).chain((0..3).filter(|&i| i != lead)).collect();
    let (low, high, shade, max_c): (f32, f32, f32, f32) = if light_theme { (0.74, 0.88, 0.80, 0.10) } else { (0.48, 0.64, 0.38, 0.15) };
    let lead_c = chroma(lead);
    let mono = lead_c < 0.035;
    // One hue in several tones (a sepia photo): the tones keep their
    // difference in light, or the three lights would be one flat colour.
    let one_hue = !mono
        && (0..3).all(|i| {
            let d = (hue(i) - hue(lead)).to_degrees().rem_euclid(360.0);
            chroma(i) >= 0.035 && d.min(360.0 - d) < 20.0
        });
    std::array::from_fn(|slot| {
        let i = order[slot];
        if mono {
            // Graphite: the lead a little brighter than the two beside it.
            let l = if light_theme { [0.86, 0.80, 0.83][slot] } else { [0.46, 0.36, 0.40][slot] };
            from_oklch(l, chroma(i).min(0.012), hue(i))
        } else if slot > 0 && chroma(i) < 0.045 {
            // A grey beside a colour takes the colour's hue: a white or cream
            // of the cover is light (a soft highlight), a dark grey is shade.
            if lab[i][0] > 0.78 {
                from_oklch(if light_theme { 0.92 } else { 0.74 }, lead_c.min(max_c) * 0.3, hue(lead))
            } else {
                // A dark orange or yellow is brown or olive: beside a warm
                // colour the shade is a warm graphite instead.
                let lead_deg = hue(lead).to_degrees().rem_euclid(360.0);
                let warm_lead = (35.0..125.0).contains(&lead_deg);
                let c = lead_c.min(max_c) * if warm_lead { 0.18 } else { 0.6 };
                from_oklch(if warm_lead { shade + 0.04 } else { shade }, c, hue(lead))
            }
        } else {
            // Orange, yellow and olive darkened turn brown or mud: they keep more light.
            let deg = hue(i).to_degrees().rem_euclid(360.0);
            let warm = (35.0..125.0).contains(&deg);
            let low = if warm && !light_theme { 0.64 } else { low };
            let l = if one_hue {
                // The tones spread over a luminous band, darkest at its foot
                // (warm hues stay above brown), in their order on the cover.
                let (foot, top) = match (light_theme, warm) {
                    (true, _) => (0.72, 0.90),
                    (false, true) => (0.60, 0.78),
                    (false, false) => (0.44, 0.70),
                };
                let ls = [lab[0][0], lab[1][0], lab[2][0]];
                let (lo, hi) = (ls.iter().cloned().fold(1.0, f32::min), ls.iter().cloned().fold(0.0, f32::max));
                let spread = (hi - lo).max(0.001);
                let span = (spread * 0.9).min(top - foot);
                (foot + top) / 2.0 + (lab[i][0] - (lo + hi) / 2.0) / spread * span
            } else {
                lab[i][0].clamp(low, high.max(low))
            };
            from_oklch(l, chroma(i).min(max_c), hue(i))
        }
    })
}

/// The player bar's three lights from a cover's palette. The bar sits under
/// white text, so each colour is brought into a band of lightness where it
/// glows and the text still reads, by moving its OKLab lightness only: hue
/// and chroma stay the cover's (chroma capped, never raised). Darkening an
/// sRGB colour toward black instead (as before) took the chroma with it and
/// turned a vivid red or orange cover into a dim brown. Warm hues keep more
/// light (darkened they are brown). Colours of one hue spread over the band
/// by their own lightness, and the greys of a black-and-white cover keep
/// their tones (graphite to smoke), so a single-hue or monochrome cover still
/// shows light moving across the bar. Never a hue the cover lacks.
pub fn bar_palette(palette: [Color; 3], light_theme: bool) -> [Color; 3] {
    let lab = palette.map(to_oklab);
    let chroma = |i: usize| lab[i][1].hypot(lab[i][2]);
    let hue = |i: usize| lab[i][2].atan2(lab[i][1]);
    let deg = |i: usize| hue(i).to_degrees().rem_euclid(360.0);
    let warm = |i: usize| (35.0..125.0).contains(&deg(i));
    let (lo, hi) = (lab.iter().map(|c| c[0]).fold(1.0, f32::min), lab.iter().map(|c| c[0]).fold(0.0, f32::max));
    let spread = (hi - lo).max(0.0001);
    // Where a colour sits among the cover's three, by lightness (0 darkest).
    let rank = |i: usize| if hi - lo < 0.03 { [0.7, 0.3, 0.5][i] } else { (lab[i][0] - lo) / spread };
    let lead = (0..3).max_by(|&a, &b| chroma(a).total_cmp(&chroma(b))).unwrap_or(0);
    let one_hue = (0..3).all(|i| {
        let d = (hue(i) - hue(lead)).to_degrees().rem_euclid(360.0);
        chroma(i) < 0.035 || d.min(360.0 - d) < 25.0
    });
    std::array::from_fn(|i| {
        let c = chroma(i);
        if light_theme {
            let l = 0.80 + 0.10 * rank(i);
            return from_oklch(l, c.min(0.12), hue(i));
        }
        if c < 0.035 {
            // A grey: its own tone, from graphite to smoke.
            return from_oklch(0.27 + 0.28 * rank(i), c, hue(i));
        }
        let (floor, top) = if warm(i) { (0.50, 0.64) } else { (0.38, 0.58) };
        let l = if one_hue { floor + (top - floor) * rank(i) } else { lab[i][0].clamp(floor, top) };
        from_oklch(l, c.min(0.19), hue(i))
    })
}

/// A smooth path through pseudorandom points in 0..1 (`start` at time 0).
fn drift(time: f32, seed: u32, start: f32) -> f32 {
    let point = |i: u32| {
        if i == 0 {
            return start;
        }
        let mut value = i.wrapping_mul(0x9e37_79b9).wrapping_add(seed);
        value = (value ^ (value >> 16)).wrapping_mul(0x85eb_ca6b);
        value = (value ^ (value >> 13)).wrapping_mul(0xc2b2_ae35);
        ((value ^ (value >> 16)) >> 8) as f32 / 16_777_215.0
    };
    let step = time as u32;
    let fraction = time.fract();
    let blend = fraction * fraction * (3.0 - 2.0 * fraction);
    point(step) + (point(step + 1) - point(step)) * blend
}

/// The album band's glows: slower and wider than the bar's, each over its own
/// third of the width so the three colours always all show.
pub(super) fn wash_centers(elapsed: f32) -> [[f32; 2]; 3] {
    std::array::from_fn(|i| {
        let seed = 709 * i as u32;
        let x = drift(elapsed / [6.3, 7.9, 7.1][i], 211 + seed, 0.5);
        let y = drift(elapsed / [5.2, 6.2, 4.7][i], 457 + seed, [0.40, 0.55, 0.30][i]);
        [[0.04, 0.36, 0.68][i] + 0.30 * x, -0.20 + 0.60 * y]
    })
}

/// A pseudorandom value in 0..1 for step `i` of a path.
fn hash01(i: i64, seed: u32) -> f32 {
    let mut value = (i as u32).wrapping_mul(0x9e37_79b9).wrapping_add(seed);
    value = (value ^ (value >> 16)).wrapping_mul(0x85eb_ca6b);
    value = (value ^ (value >> 13)).wrapping_mul(0xc2b2_ae35);
    ((value ^ (value >> 16)) >> 8) as f32 / 16_777_215.0
}

/// A path through pseudorandom points in 0..1 with continuous speed
/// (Catmull-Rom): it never halts at the points, as a smoothstep walk did
/// (every light stopped dead every few seconds, a mechanical pulse).
pub(super) fn wander(time: f32, seed: u32) -> f32 {
    let i = time.floor() as i64;
    let f = time - time.floor();
    let [p0, p1, p2, p3] = [-1, 0, 1, 2].map(|k| hash01(i + k, seed));
    let (f2, f3) = (f * f, f * f * f);
    let v = 0.5 * (2.0 * p1 + (p2 - p0) * f + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f2 + (3.0 * p1 - p0 - 3.0 * p2 + p3) * f3);
    v.clamp(0.0, 1.0)
}

/// The current's uneven pace: time that swells and eases (three slow,
/// unrelated periods; always forward, between 0.2 and 1.8 times real time).
pub fn pace(t: f32) -> f32 {
    t + 2.8 * (t * 0.137).sin() + 1.9 * (t * 0.089 + 1.7).sin() + 1.1 * (t * 0.229 + 4.1).sin()
}

/// The player bar's three lights: each wanders around its own part of the
/// bar (left, middle, right, overlapping so they can pass each other) on a
/// continuous path at an uneven pace, so all three colours always show and
/// the bar never settles into a rhythm. About 40 px/s typical at 1440 px,
/// rarely over 200.
pub(crate) fn bar_centers(elapsed: f32) -> [[f32; 2]; 3] {
    let s = pace(elapsed);
    std::array::from_fn(|i| {
        let seed = 317 * i as u32;
        let x = wander(s / [4.8, 5.8, 5.3][i], 113 + seed);
        let y = wander(s / [3.9, 4.7, 4.3][i], 593 + seed);
        [[0.18, 0.50, 0.82][i] + 0.60 * (x - 0.5), 0.1 + 0.8 * y]
    })
}

/// How wide each bar light is (1 = its usual size): they swell and shrink
/// slowly, so which colour leads keeps changing over a song.
pub(super) fn bar_sizes(elapsed: f32) -> [f32; 3] {
    let s = pace(elapsed * 0.8);
    std::array::from_fn(|i| 0.75 + 0.55 * wander(s / [7.7, 9.1, 8.3][i], 941 + 131 * i as u32))
}

impl shader::Primitive for GlassPrimitive {
    type Pipeline = Pipeline;

    fn prepare(&self, pipeline: &mut Pipeline, device: &wgpu::Device, queue: &wgpu::Queue, bounds: &Rectangle, viewport: &Viewport) {
        if let Some(scene) = &self.scene {
            if let Some(s) = pipeline.scenes.get_mut(&scene.key) {
                s.used = pipeline.frame;
            } else {
                pipeline.upload(device, queue, &scene.key, &scene.image);
            }
        }
        let size = viewport.physical_size();
        let u = uniforms(self, bounds, (size.width as f32, size.height as f32), viewport.scale_factor(), pipeline.srgb);
        let slot = pipeline.frame_data.len() as u64;
        pipeline.frame_data.push(u);
        if slot >= pipeline.capacity {
            // Grow: a new buffer, every scene's bind group again, and this frame's blocks rewritten.
            pipeline.capacity = (slot + 1).next_power_of_two();
            pipeline.uniforms = uniform_buffer(device, pipeline.capacity);
            let (layout, uniforms, sampler) = (&pipeline.layout, &pipeline.uniforms, &pipeline.sampler);
            for scene in pipeline.scenes.values_mut() {
                scene.group = bind_group(device, layout, uniforms, &scene.view, sampler);
            }
            for (i, block) in pipeline.frame_data.iter().enumerate() {
                queue.write_buffer(&pipeline.uniforms, i as u64 * STRIDE, bytemuck::bytes_of(block));
            }
        } else {
            queue.write_buffer(&pipeline.uniforms, slot * STRIDE, bytemuck::bytes_of(&u));
        }
        self.slot.store(slot as u32, Ordering::Relaxed);
    }

    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        let slot = self.slot.load(Ordering::Relaxed);
        if slot == u32::MAX {
            return true;
        }
        let key = self.scene.as_ref().map_or(BLANK, |s| s.key.as_str());
        let Some(scene) = pipeline.scenes.get(key).or_else(|| pipeline.scenes.get(BLANK)) else { return true };
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &scene.group, &[(slot as u64 * STRIDE) as u32]);
        pass.draw(0..3, 0..1);
        true
    }
}

// ------------------------------------------------------------------ CPU mirrors
//
// The shader's geometry, in Rust, for the unit tests and for anyone tuning it.

/// Signed distance from `p` (relative to the centre) to a rounded rectangle
/// of half size `half` and corner radius `r`: negative inside.
#[cfg(test)]
pub fn sd_box(p: (f32, f32), half: (f32, f32), r: f32) -> f32 {
    let q = (p.0.abs() - half.0 + r, p.1.abs() - half.1 + r);
    let outside = (q.0.max(0.0).powi(2) + q.1.max(0.0).powi(2)).sqrt();
    outside + q.0.max(q.1).min(0.0) - r
}

/// Outward normal of the rounded rectangle's nearest edge at `p`.
#[cfg(test)]
pub fn box_normal(p: (f32, f32), half: (f32, f32), r: f32) -> (f32, f32) {
    let s = (if p.0 >= 0.0 { 1.0 } else { -1.0 }, if p.1 >= 0.0 { 1.0 } else { -1.0 });
    let q = (p.0.abs() - half.0 + r, p.1.abs() - half.1 + r);
    let g = if q.0 > 0.0 && q.1 > 0.0 {
        let l = (q.0 * q.0 + q.1 * q.1).sqrt();
        (q.0 / l, q.1 / l)
    } else if q.0 > q.1 {
        (1.0, 0.0)
    } else {
        (0.0, 1.0)
    };
    (g.0 * s.0, g.1 * s.1)
}

/// The lens profile: 1 at the rim (`s` = 0), 0 from the inner edge of the bevel (`s` = 1) inward.
#[cfg(test)]
pub fn lens(s: f32) -> f32 {
    let k = (1.0 - s).clamp(0.0, 1.0);
    k * k.sqrt()
}

/// Where the scene is read for the green channel at `p` (relative to the
/// centre, physical px), as an offset from `p` (magnification left out).
#[cfg(test)]
pub fn displacement(p: (f32, f32), half: (f32, f32), radius: f32, bevel: f32, refraction: f32) -> (f32, f32) {
    let r = radius.min(half.0.min(half.1));
    let d = sd_box(p, half, r);
    let bevel = bevel.min(half.0.min(half.1) * 0.9).max(1.0);
    let n = box_normal(p, half, r.max(bevel));
    let k = lens((-d / bevel).clamp(0.0, 1.0));
    (-n.0 * refraction * k, -n.1 * refraction * k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdf_is_zero_on_the_edge_negative_inside_and_rounded_at_corners() {
        let half = (100.0, 40.0);
        assert!(sd_box((100.0, 0.0), half, 12.0).abs() < 1e-4);
        assert!(sd_box((0.0, -40.0), half, 12.0).abs() < 1e-4);
        assert!((sd_box((0.0, 0.0), half, 12.0) + 40.0).abs() < 1e-4);
        assert!((sd_box((110.0, 0.0), half, 12.0) - 10.0).abs() < 1e-4);
        // The corner of the bounding box is outside a rounded rectangle.
        let corner = sd_box((100.0, 40.0), half, 12.0);
        assert!((corner - (12.0 * std::f32::consts::SQRT_2 - 12.0)).abs() < 1e-3, "{corner}");
        // On the arc itself the distance is zero.
        let c = (88.0 + 12.0 * std::f32::consts::FRAC_1_SQRT_2, 28.0 + 12.0 * std::f32::consts::FRAC_1_SQRT_2);
        assert!(sd_box(c, half, 12.0).abs() < 1e-3);
    }

    #[test]
    fn normals_point_out_of_the_nearest_edge() {
        let half = (100.0, 40.0);
        assert_eq!(box_normal((95.0, 0.0), half, 12.0), (1.0, 0.0));
        assert_eq!(box_normal((-95.0, 3.0), half, 12.0), (-1.0, 0.0));
        assert_eq!(box_normal((10.0, -38.0), half, 12.0), (0.0, -1.0));
        let (x, y) = box_normal((98.0, 38.0), half, 12.0);
        assert!(x > 0.6 && y > 0.6 && ((x * x + y * y) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn the_lens_is_strong_at_the_rim_and_flat_inside() {
        assert_eq!(lens(0.0), 1.0);
        assert_eq!(lens(1.0), 0.0);
        assert_eq!(lens(3.0), 0.0);
        assert!(lens(0.5) < 0.4);
        let mut last = f32::MAX;
        for i in 0..=20 {
            let v = lens(i as f32 / 20.0);
            assert!(v <= last);
            last = v;
        }
        let half = (200.0, 80.0);
        // Near the rim the scene is read from further inside (inward along the normal).
        let (dx, dy) = displacement((198.0, 0.0), half, 18.0, 22.0, 26.0);
        assert!(dx < -20.0 && dy.abs() < 1e-4, "{dx} {dy}");
        let (_, dy) = displacement((0.0, -79.0), half, 18.0, 22.0, 26.0);
        assert!(dy > 20.0);
        // The middle is flat.
        assert_eq!(displacement((0.0, 0.0), half, 18.0, 22.0, 26.0), (0.0, 0.0));
        assert_eq!(displacement((150.0, 30.0), half, 18.0, 22.0, 26.0), (0.0, 0.0));
    }

    #[test]
    fn uniform_blocks_fit_the_dynamic_offset_stride() {
        assert!(std::mem::size_of::<Uniforms>() as u64 <= STRIDE);
        assert_eq!(std::mem::size_of::<Uniforms>() % 16, 0);
    }

    #[test]
    fn bar_lights_follow_independent_smooth_paths() {
        let start = bar_centers(0.0);
        let later = bar_centers(8.0);
        assert!(start.iter().zip(later).any(|(a, b)| (a[0] - b[0]).abs() > 0.25));
        assert!(start.iter().zip(later).any(|(a, b)| (a[1] - b[1]).abs() > 0.25));
        let before = bar_centers(5.299);
        let after = bar_centers(5.301);
        assert!((before[0][0] - after[0][0]).abs() < 0.001);
    }

    #[test]
    fn album_band_glows_drift_slowly_and_keep_their_thirds() {
        for t in (0..600).map(|i| i as f32 * 0.5) {
            for (i, [x, y]) in wash_centers(t).into_iter().enumerate() {
                let left = [0.04, 0.36, 0.68][i];
                assert!((left..=left + 0.30).contains(&x), "glow {i} at x {x} (t {t})");
                assert!((-0.20..=0.40).contains(&y), "glow {i} at y {y} (t {t})");
            }
        }
        // It moves, but gently: under a tenth of the width in a second.
        let (a, b) = (wash_centers(12.0), wash_centers(13.0));
        assert!(a.iter().zip(b).all(|(p, q)| (p[0] - q[0]).abs() < 0.1));
        assert!(wash_centers(0.0).iter().zip(wash_centers(30.0)).any(|(p, q)| (p[0] - q[0]).abs() > 0.05));
    }

    #[test]
    fn scene_levels_halve_and_stay_opaque() {
        let img = image::RgbaImage::from_fn(300, 200, |x, y| image::Rgba([(x % 255) as u8, (y % 255) as u8, 90, 255]));
        let scene = SceneImage::from_image(&image::DynamicImage::ImageRgba8(img));
        assert_eq!(scene.levels.len(), SCENE_LEVELS as usize);
        for (k, level) in scene.levels.iter().enumerate() {
            let s = (SCENE_SIZE >> k) as usize;
            assert_eq!(level.len(), s * s * 4);
            assert!(level.chunks(4).all(|p| p[3] == 255));
        }
        assert!(scene.bytes() < 1_500_000);
        let accent = SceneImage::accent(Color::from_rgb(0.1, 0.8, 0.9));
        assert_eq!(accent.levels.len(), SCENE_LEVELS as usize);
    }
}

#[cfg(test)]
mod palette_probe {
    /// Prints a cover's palette and its banner roles (`ABYSS_PALETTE_OF` = an image file).
    #[test]
    #[ignore = "diagnostic: needs an image file"]
    fn print_banner_palette() {
        let path = match (std::env::var("ABYSS_PALETTE_OF"), std::env::var("ABYSS_COVER_ID")) {
            (Ok(path), _) => path,
            // A TIDAL cover id: its 320 px copy in the cache folder `ABYSS_COVERS`.
            (_, Ok(id)) => {
                use std::hash::{Hash, Hasher};
                let url = format!("https://resources.tidal.com/images/{}/320x320.jpg", id.replace('-', "/"));
                let mut h = std::collections::hash_map::DefaultHasher::new();
                url.hash(&mut h);
                format!("{}/{:016x}.img", std::env::var("ABYSS_COVERS").unwrap(), h.finish())
            }
            _ => return,
        };
        let mut bytes = std::fs::read(&path).unwrap();
        eprintln!("cover file {path}");
        // `ABYSS_CROP` = "x,y,w,h": a region of the image (a cover on a screenshot).
        if let Ok(crop) = std::env::var("ABYSS_CROP") {
            let v: Vec<u32> = crop.split(',').map(|n| n.trim().parse().unwrap()).collect();
            let img = image::load_from_memory(&bytes).unwrap().crop_imm(v[0], v[1], v[2], v[3]);
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Png).unwrap();
            bytes = out.into_inner();
        }
        // With `ABYSS_COVERS` set, `path` is a screen capture: use the cached
        // cover that looks most like it (grey 8 x 8 thumbnails).
        if let (Ok(dir), Err(_)) = (std::env::var("ABYSS_COVERS"), std::env::var("ABYSS_COVER_ID")) {
            let small = |b: &[u8]| {
                image::load_from_memory(b).ok().map(|i| i.resize_exact(8, 8, image::imageops::FilterType::Triangle).to_luma8().into_raw())
            };
            let probe = small(&bytes).unwrap();
            let best = std::fs::read_dir(dir)
                .unwrap()
                .filter_map(|e| {
                    let p = e.ok()?.path();
                    let b = std::fs::read(&p).ok()?;
                    let t = small(&b)?;
                    let d: i64 = t.iter().zip(&probe).map(|(a, b)| (i64::from(*a) - i64::from(*b)).abs()).sum();
                    Some((d, p, b))
                })
                .min_by_key(|x| x.0)
                .unwrap();
            eprintln!("closest cover {} (distance {})", best.1.display(), best.0);
            bytes = best.2;
        }
        let raw: [iced::Color; 3] = crate::iced_ui::artwork::palette_of(&bytes).unwrap().try_into().unwrap();
        let hex = |c: iced::Color| format!("#{:02x}{:02x}{:02x}", (c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8);
        for c in raw {
            let (h, sv, v) = crate::iced_ui::artwork::rgb_to_hsv(c.r, c.g, c.b);
            eprintln!("raw hsv ({h:.0}, {sv:.2}, {v:.2})");
            let l = super::to_oklab(c);
            eprintln!("raw {} L {:.2} C {:.3} h {:.0}", hex(c), l[0], l[1].hypot(l[2]), l[2].atan2(l[1]).to_degrees());
        }
        for c in super::banner_palette(raw, false) {
            eprintln!("banner {}", hex(c));
        }
    }
}

#[cfg(test)]
mod palette_audit {
    /// A sheet of real covers (`ABYSS_COVERS`) with, beside each, the raw
    /// palette (top row) and the banner's three lights (bottom row), spread
    /// over the range of saturation: `%TEMP%\abyss-palette-audit.png`.
    #[test]
    #[ignore = "diagnostic: needs a folder of covers"]
    fn palette_audit_sheet() {
        let Ok(dir) = std::env::var("ABYSS_COVERS") else { return };
        let mut covers: Vec<(f32, Vec<u8>)> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| std::fs::read(e.ok()?.path()).ok())
            .filter(|b| (8_000..400_000).contains(&b.len()))
            .filter_map(|b| {
                let img = image::load_from_memory(&b).ok()?;
                (img.width() >= 300 && img.width() == img.height()).then_some(())?;
                let small = img.thumbnail(16, 16).to_rgb8();
                let s: f32 = small
                    .pixels()
                    .map(|p| {
                        let [r, g, b] = p.0.map(|c| c as f32 / 255.0);
                        let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
                        if mx > 0.0 {
                            (mx - mn) / mx
                        } else {
                            0.0
                        }
                    })
                    .sum::<f32>()
                    / small.pixels().len() as f32;
                Some((s, b))
            })
            .collect();
        covers.sort_by(|a, b| a.0.total_cmp(&b.0));
        let n = 48.min(covers.len());
        let (cell, cols) = (96u32, 6u32);
        let tile_w = cell + 3 * 30 + 12;
        let mut sheet =
            image::RgbaImage::from_pixel(cols * tile_w, n.div_ceil(cols as usize) as u32 * (cell + 8), image::Rgba([18, 18, 20, 255]));
        for k in 0..n {
            let (_, bytes) = &covers[k * (covers.len() - 1) / (n - 1).max(1)];
            let (x0, y0) = ((k as u32 % cols) * tile_w, (k as u32 / cols) * (cell + 8));
            let img = image::load_from_memory(bytes).unwrap().resize_exact(cell, cell, image::imageops::FilterType::Triangle).to_rgba8();
            image::imageops::overlay(&mut sheet, &img, i64::from(x0), i64::from(y0));
            let raw: [iced::Color; 3] = crate::iced_ui::artwork::palette_of(bytes).unwrap().try_into().unwrap();
            let banner = super::banner_palette(raw, false);
            for (row, colors) in [raw, banner].iter().enumerate() {
                for (j, c) in colors.iter().enumerate() {
                    let px = image::Rgba([(c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, 255]);
                    for dy in 0..44 {
                        for dx in 0..28 {
                            sheet.put_pixel(x0 + cell + 6 + j as u32 * 30 + dx, y0 + 2 + row as u32 * 48 + dy, px);
                        }
                    }
                }
            }
        }
        let path = std::env::temp_dir().join("abyss-palette-audit.png");
        sheet.save(&path).unwrap();
        eprintln!("palette audit: {} ({n} covers)", path.display());
    }
}
