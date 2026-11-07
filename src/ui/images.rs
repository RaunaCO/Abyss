//! Cover art loader: memory cache + disk cache, fetched in the background.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, Global, Image, ImageFormat, Rgba};
use reqwest::blocking::Client;

enum Slot {
    Loading,
    Ready(Arc<Image>),
    Failed,
}

pub struct Images {
    map: HashMap<String, Slot>,
    /// Dominant (vivid average) colour of each loaded cover.
    colors: HashMap<String, Rgba>,
    /// Heavily blurred, saturated copy of each cover (Now Playing background).
    blurs: HashMap<String, Arc<Image>>,
    /// Up to three vivid, distinct colours of each cover.
    palettes: HashMap<String, Vec<Rgba>>,
    /// 180 pre-rotated (2 degrees apart) blurred copies of the playing cover, for the
    /// rotating Now Playing background. Only a few covers are kept.
    frames: Vec<(String, Arc<Vec<Arc<Image>>>)>,
    frames_pending: std::collections::HashSet<String>,
    /// Covers resampled (Lanczos) to the exact device-pixel size they are drawn
    /// at, keyed by (url, width, height). The GPU only filters bilinearly with
    /// no mipmaps, so drawing a big image small aliases; these stay crisp.
    sized: HashMap<(String, u32, u32), Sized>,
    http: Client,
    dir: PathBuf,
}

impl Global for Images {}

impl Images {
    pub fn init(http: Client, cx: &mut App) {
        let dir = crate::config::cache_dir().join("images");
        let _ = std::fs::create_dir_all(&dir);
        cx.set_global(Images { map: HashMap::new(), colors: HashMap::new(), blurs: HashMap::new(), palettes: HashMap::new(), frames: Vec::new(), frames_pending: Default::default(), sized: HashMap::new(), http, dir });
    }
}

fn file_for(dir: &std::path::Path, url: &str) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    dir.join(format!("{:016x}.img", h.finish()))
}

fn format_of(bytes: &[u8]) -> ImageFormat {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        ImageFormat::Png
    } else if bytes.starts_with(b"RIFF") {
        ImageFormat::Webp
    } else {
        ImageFormat::Jpeg
    }
}

/// The image for `url` if it is loaded; starts loading it otherwise.
pub fn get(url: &str, cx: &mut App) -> Option<Arc<Image>> {
    let images = cx.global::<Images>();
    match images.map.get(url) {
        Some(Slot::Ready(img)) => return Some(img.clone()),
        Some(_) => return None,
        None => {}
    }
    let http = images.http.clone();
    let path = file_for(&images.dir, url);
    cx.global_mut::<Images>().map.insert(url.to_string(), Slot::Loading);
    let url = url.to_string();
    cx.spawn(async move |cx| {
        let u = url.clone();
        let bytes = cx
            .background_executor()
            .spawn(async move {
                if let Ok(b) = std::fs::read(&path) {
                    return Some(b);
                }
                let resp = http.get(&u).send().ok()?;
                if !resp.status().is_success() {
                    return None;
                }
                let b = resp.bytes().ok()?.to_vec();
                let _ = std::fs::write(&path, &b);
                Some(b)
            })
            .await;
        // Colour extraction runs off the UI thread too.
        let color = match &bytes {
            Some(b) => {
                let b = b.clone();
                cx.background_executor().spawn(async move { dominant_color(&b) }).await
            }
            None => None,
        };
        let blur = match &bytes {
            Some(b) => {
                let b = b.clone();
                cx.background_executor().spawn(async move { blurred_png(&b) }).await
            }
            None => None,
        };
        let palette = match &bytes {
            Some(b) => {
                let b = b.clone();
                cx.background_executor().spawn(async move { palette_of(&b) }).await
            }
            None => None,
        };
        let _ = cx.update(|cx| {
            if let Some(pal) = palette {
                cx.global_mut::<Images>().palettes.insert(url.clone(), pal);
            }
            if let Some(png) = blur {
                cx.global_mut::<Images>().blurs.insert(url.clone(), Arc::new(Image::from_bytes(ImageFormat::Png, png)));
            }
            let slot = match bytes {
                Some(b) => Slot::Ready(Arc::new(Image::from_bytes(format_of(&b), b))),
                None => Slot::Failed,
            };
            if let Some(c) = color {
                cx.global_mut::<Images>().colors.insert(url.clone(), c);
            }
            cx.global_mut::<Images>().map.insert(url, slot);
            cx.refresh_windows();
        });
    })
    .detach();
    None
}

enum Sized {
    Pending,
    /// The original is already about the right size: draw it as is.
    Original,
    Ready(Arc<gpui::RenderImage>),
}

static SCALE_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);

/// The window's device-pixel ratio (set every frame by the root view).
pub fn set_scale(s: f32) {
    SCALE_BITS.store(s.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

fn device_px(logical: f32) -> u32 {
    let s = f32::from_bits(SCALE_BITS.load(std::sync::atomic::Ordering::Relaxed));
    (logical * super::zoom() * s).round().max(1.0) as u32
}

/// The image for `url` resampled to be drawn `w` x `h` logical px (before the
/// UI zoom). Falls back to the original while the resampled copy is made.
pub fn sized(url: &str, w: f32, h: f32, cx: &mut App) -> Option<gpui::ImageSource> {
    let original = get(url, cx)?;
    let (mut dw, mut dh) = (device_px(w), device_px(h));
    // Big images (Now Playing) change size with the window: step them in 32 px
    // so a resize reuses a handful of copies instead of making one per pixel.
    if dw > 256 && dh > 256 {
        dw = dw.div_ceil(32) * 32;
        dh = dh.div_ceil(32) * 32;
    }
    let key = (url.to_string(), dw, dh);
    match cx.global::<Images>().sized.get(&key) {
        Some(Sized::Ready(i)) => return Some(gpui::ImageSource::Render(i.clone())),
        Some(_) => return Some(original.into()),
        None => {}
    }
    let images = cx.global_mut::<Images>();
    // Keep memory bounded: drop old resamples when the cache grows large.
    if images.sized.len() > 4000 {
        images.sized.retain(|_, v| matches!(v, Sized::Pending));
    }
    images.sized.insert(key.clone(), Sized::Pending);
    let src = original.clone();
    cx.spawn(async move |cx| {
        let out = cx.background_executor().spawn(async move { resample(&src.bytes, dw, dh) }).await;
        let _ = cx.update(|cx| {
            let slot = match out {
                Some(r) => Sized::Ready(Arc::new(r)),
                None => Sized::Original,
            };
            cx.global_mut::<Images>().sized.insert(key, slot);
            cx.refresh_windows();
        });
    })
    .detach();
    Some(original.into())
}

/// High-quality downscale (Lanczos3, cropped to fill) to exactly `w` x `h`.
/// None when the source is not meaningfully larger (the GPU draws it fine).
/// Delivered already decoded (BGRA), so swapping it in never blanks a frame.
fn resample(bytes: &[u8], w: u32, h: u32) -> Option<gpui::RenderImage> {
    let src = image::load_from_memory(bytes).ok()?;
    if (src.width() as f32) < w as f32 * 1.15 && (src.height() as f32) < h as f32 * 1.15 {
        return None;
    }
    let mut small = src.resize_to_fill(w, h, image::imageops::FilterType::Lanczos3).to_rgba8();
    for p in small.pixels_mut() {
        p.0.swap(0, 2);
    }
    Some(gpui::RenderImage::new(vec![image::Frame::new(small)]))
}

/// Dominant colour of a loaded cover (None until the image has loaded).
pub fn color(url: &str, cx: &mut App) -> Option<Rgba> {
    if let Some(c) = cx.global::<Images>().colors.get(url) {
        return Some(*c);
    }
    let _ = get(url, cx);
    None
}

/// Distinct vivid colours of a loaded cover (empty until it has loaded).
pub fn palette(url: &str, cx: &mut App) -> Vec<Rgba> {
    if let Some(p) = cx.global::<Images>().palettes.get(url) {
        return p.clone();
    }
    let _ = get(url, cx);
    Vec::new()
}

fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } * 60.0;
    (h, if max > 0.0 { d / max } else { 0.0 }, max)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Rgba {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Rgba { r: r + m, g: g + m, b: b + m, a: 1.0 }
}

/// Three vivid, distinct colours of a cover. Pixels are clustered with a few
/// rounds of k-means; clusters are ranked by how much of the picture they cover
/// and how colourful they are, and a colour too close in hue to a better one
/// is skipped, so a cover with one big orange area and a small green logo
/// yields both.
fn palette_of(bytes: &[u8]) -> Option<Vec<Rgba>> {
    let img = image::load_from_memory(bytes).ok()?.thumbnail(40, 40).to_rgb8();
    let px: Vec<[f32; 3]> = img
        .pixels()
        .map(|p| p.0.map(|c| c as f32 / 255.0))
        .filter(|c| c[0].max(c[1]).max(c[2]) > 0.12)
        .collect();
    if px.is_empty() {
        return Some(vec![Rgba { r: 0.5, g: 0.5, b: 0.5, a: 1.0 }; 3]);
    }
    let dist = |a: &[f32; 3], b: &[f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
    let sat = |c: &[f32; 3]| {
        let (_, s, _) = rgb_to_hsv(c[0], c[1], c[2]);
        s
    };
    const K: usize = 6;
    // Farthest-point initialisation, starting from the most colourful pixel.
    let mut centers: Vec<[f32; 3]> = vec![*px.iter().max_by(|a, b| sat(a).total_cmp(&sat(b)))?];
    while centers.len() < K.min(px.len()) {
        let next = px.iter().max_by(|a, b| {
            let da = centers.iter().map(|c| dist(a, c)).fold(f32::MAX, f32::min);
            let db = centers.iter().map(|c| dist(b, c)).fold(f32::MAX, f32::min);
            da.total_cmp(&db)
        })?;
        centers.push(*next);
    }
    let mut counts = vec![0usize; centers.len()];
    for _ in 0..8 {
        let mut sums = vec![[0f32; 3]; centers.len()];
        counts.iter_mut().for_each(|c| *c = 0);
        for p in &px {
            let i = (0..centers.len()).min_by(|&x, &y| dist(p, &centers[x]).total_cmp(&dist(p, &centers[y])))?;
            sums[i][0] += p[0];
            sums[i][1] += p[1];
            sums[i][2] += p[2];
            counts[i] += 1;
        }
        for i in 0..centers.len() {
            if counts[i] > 0 {
                let n = counts[i] as f32;
                centers[i] = [sums[i][0] / n, sums[i][1] / n, sums[i][2] / n];
            }
        }
    }
    // Rank: coverage x colourfulness (greys count, but little).
    let total = px.len() as f32;
    let mut ranked: Vec<(f32, [f32; 3])> = (0..centers.len())
        .filter(|&i| counts[i] > 0)
        .map(|i| {
            let s = sat(&centers[i]);
            ((counts[i] as f32 / total).sqrt() * (0.25 + s), centers[i])
        })
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    let hue_of = |c: &[f32; 3]| rgb_to_hsv(c[0], c[1], c[2]).0;
    let mut chosen: Vec<[f32; 3]> = Vec::new();
    for (_, c) in &ranked {
        let h = hue_of(c);
        let far = chosen.iter().all(|o| {
            let d = (hue_of(o) - h).abs();
            d.min(360.0 - d) > 28.0 || sat(o) < 0.12 || sat(c) < 0.12
        });
        if far {
            chosen.push(*c);
        }
        if chosen.len() == 3 {
            break;
        }
    }
    // Make them glow on a dark UI: keep the hue, raise saturation and brightness.
    let mut out: Vec<Rgba> = chosen
        .iter()
        .map(|c| {
            let (h, s, v) = rgb_to_hsv(c[0], c[1], c[2]);
            let s = if s < 0.12 { s } else { (s * 1.2 + 0.1).clamp(0.45, 1.0) };
            hsv_to_rgb(h, s, (v * 1.2).clamp(0.62, 0.96))
        })
        .collect();
    // Fewer hues than slots: neighbours of the first, so the motion still reads.
    while out.len() < 3 {
        let c = out[out.len() - 1];
        let (h, s, v) = rgb_to_hsv(c.r, c.g, c.b);
        out.push(hsv_to_rgb(h + 32.0, s, (v * 0.88).max(0.5)));
    }
    Some(out)
}

/// The rotating blurred background of the official app, as frames: the cover blurred
/// (28 px at its size) and saturated 1.3, blown up to twice the screen's larger side
/// and turned about its centre. Returns the frames once they exist; starts making
/// them otherwise. The caller picks the frame for the current angle.
pub fn frames(url: &str, cx: &mut App) -> Option<Arc<Vec<Arc<Image>>>> {
    let images = cx.global::<Images>();
    if let Some((_, f)) = images.frames.iter().find(|(u, _)| u == url) {
        return Some(f.clone());
    }
    if images.frames_pending.contains(url) {
        return None;
    }
    // The cover itself has to be on disk first.
    let path = file_for(&images.dir, url);
    if !path.exists() {
        let _ = get(url, cx);
        return None;
    }
    cx.global_mut::<Images>().frames_pending.insert(url.to_string());
    let url = url.to_string();
    cx.spawn(async move |cx| {
        let p = path.clone();
        let pngs = cx
            .background_executor()
            .spawn(async move { std::fs::read(&p).ok().and_then(|b| rotated_pngs(&b)) })
            .await;
        let _ = cx.update(|cx| {
            let g = cx.global_mut::<Images>();
            g.frames_pending.remove(&url);
            if let Some(list) = pngs {
                let imgs: Vec<Arc<Image>> = list.into_iter().map(|b| Arc::new(Image::from_bytes(ImageFormat::Png, b))).collect();
                g.frames.push((url, Arc::new(imgs)));
                while g.frames.len() > 2 {
                    g.frames.remove(0);
                }
            }
            cx.refresh_windows();
        });
    })
    .detach();
    None
}

pub const FRAME_STEP_DEG: f32 = 2.0;
const FRAME_COUNT: usize = 180;

fn rotated_pngs(bytes: &[u8]) -> Option<Vec<Vec<u8>>> {
    const SRC: u32 = 128;
    const OUT: u32 = 192;
    // The official image is 40vmax x 4 x 1.26 = about twice the larger side of the window.
    const SIDE: f32 = 2.4;
    let small = image::load_from_memory(bytes).ok()?.thumbnail(SRC, SRC).to_rgba8();
    let mut src = image::imageops::blur(&small, 7.5);
    for p in src.pixels_mut() {
        let [r, g, b, a] = p.0;
        let (rf, gf, bf) = (r as f32, g as f32, b as f32);
        let lum = 0.299 * rf + 0.587 * gf + 0.114 * bf;
        let sat = |c: f32| (lum + (c - lum) * 1.3).clamp(0.0, 255.0) as u8;
        p.0 = [sat(rf), sat(gf), sat(bf), a];
    }
    let (w, h) = (src.width() as f32, src.height() as f32);
    let sample = |x: f32, y: f32| -> [f32; 3] {
        let x = (x * w - 0.5).clamp(0.0, w - 1.001);
        let y = (y * h - 0.5).clamp(0.0, h - 1.001);
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let px = |xx: u32, yy: u32| src.get_pixel(xx.min(src.width() - 1), yy.min(src.height() - 1)).0;
        let (a, b, c, d) = (px(x0, y0), px(x0 + 1, y0), px(x0, y0 + 1), px(x0 + 1, y0 + 1));
        let mut out = [0f32; 3];
        for i in 0..3 {
            let top = a[i] as f32 * (1.0 - fx) + b[i] as f32 * fx;
            let bot = c[i] as f32 * (1.0 - fx) + d[i] as f32 * fx;
            out[i] = top * (1.0 - fy) + bot * fy;
        }
        out
    };
    let mut frames = Vec::with_capacity(FRAME_COUNT);
    for k in 0..FRAME_COUNT {
        let theta = (k as f32 * FRAME_STEP_DEG).to_radians();
        let (sin, cos) = theta.sin_cos();
        let mut buf = image::RgbaImage::new(OUT, OUT);
        for iy in 0..OUT {
            for ix in 0..OUT {
                // Position in units of the window's larger side, centred.
                let (x, y) = ((ix as f32 + 0.5) / OUT as f32 - 0.5, (iy as f32 + 0.5) / OUT as f32 - 0.5);
                let (rx, ry) = (x * cos + y * sin, -x * sin + y * cos);
                let c = sample(rx / SIDE + 0.5, ry / SIDE + 0.5);
                // Tiny noise (dithering) so the slow gradients show no banding or blocks.
                let n = ((ix.wrapping_mul(73856093) ^ iy.wrapping_mul(19349663) ^ (k as u32).wrapping_mul(83492791)) % 1000) as f32 / 1000.0 - 0.5;
                let q = |v: f32| (v + n * 2.2).clamp(0.0, 255.0) as u8;
                buf.put_pixel(ix, iy, image::Rgba([q(c[0]), q(c[1]), q(c[2]), 255]));
            }
        }
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(buf).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
        frames.push(out);
    }
    Some(frames)
}

/// The cover as the official app paints Now Playing: blur(28px) saturate(1.3).
/// Computed on a tiny thumbnail; the GPU upscales it smoothly.
pub fn blurred(url: &str, cx: &mut App) -> Option<Arc<Image>> {
    if let Some(i) = cx.global::<Images>().blurs.get(url) {
        return Some(i.clone());
    }
    let _ = get(url, cx);
    None
}

fn blurred_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let small = image::load_from_memory(bytes).ok()?.thumbnail(56, 56).to_rgba8();
    let mut blurred = image::imageops::blur(&small, 3.5);
    for p in blurred.pixels_mut() {
        let [r, g, b, a] = p.0;
        let (rf, gf, bf) = (r as f32, g as f32, b as f32);
        let lum = 0.299 * rf + 0.587 * gf + 0.114 * bf;
        let sat = |c: f32| (lum + (c - lum) * 1.3).clamp(0.0, 255.0) as u8;
        p.0 = [sat(rf), sat(gf), sat(bf), a];
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(blurred)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}

/// A vivid "average" colour: saturated pixels weigh more than greys, so a
/// mostly black cover with a red logo comes out red, not grey.
fn dominant_color(bytes: &[u8]) -> Option<Rgba> {
    let img = image::load_from_memory(bytes).ok()?.thumbnail(24, 24).to_rgb8();
    let (mut r, mut g, mut b, mut w) = (0f32, 0f32, 0f32, 0f32);
    for p in img.pixels() {
        let [pr, pg, pb] = p.0.map(|c| c as f32 / 255.0);
        let max = pr.max(pg).max(pb);
        let min = pr.min(pg).min(pb);
        let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
        // Ignore near-black and near-white, favour colourful pixels.
        let weight = (0.08 + sat * sat * 3.0) * if max < 0.12 || min > 0.92 { 0.1 } else { 1.0 };
        r += pr * weight;
        g += pg * weight;
        b += pb * weight;
        w += weight;
    }
    if w <= 0.0 {
        return None;
    }
    let (mut r, mut g, mut b) = (r / w, g / w, b / w);
    // Lift dark results so the glow stays visible on a black UI.
    let max = r.max(g).max(b).max(0.001);
    if max < 0.55 {
        let k = 0.55 / max;
        r *= k;
        g *= k;
        b *= k;
    }
    Some(Rgba { r, g, b, a: 1.0 })
}

/// The app logo (assets/logo.png), decoded once.
pub fn logo() -> Arc<Image> {
    static LOGO: std::sync::OnceLock<Arc<Image>> = std::sync::OnceLock::new();
    LOGO.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, include_bytes!("../../assets/logo.png").to_vec())))
        .clone()
}
