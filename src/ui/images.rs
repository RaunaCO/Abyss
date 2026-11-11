//! Cover art loader: memory cache + disk cache, fetched in the background.
//!
//! Memory: everything drawn is handed to GPUI as an already decoded
//! `RenderImage` (never an encoded `Image`), so GPUI's own asset cache, which
//! never evicts, is not used. Decoded images and the encoded originals are
//! held in byte-limited LRU caches; entries not drawn in the last frames are
//! dropped when over budget, and their GPU textures are released with
//! `drop_image` (see [`evict`]).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use gpui::{App, Global, Image, ImageFormat, RenderImage, Rgba, Window};
use reqwest::blocking::Client;

enum Slot {
    Loading,
    /// Encoded bytes, and the frame they were last asked for in.
    Ready(Arc<Image>, u64),
    Failed,
}

/// Budget for decoded images (resampled covers and blurs) held in RAM and as
/// GPU textures. A screen full of covers needs well under half of it.
const DECODED_BUDGET_MB: usize = 64;
/// Budget for encoded originals kept in memory (they are on disk anyway).
const ENCODED_BUDGET: usize = 24 * 1024 * 1024;
/// Entries drawn this recently are never evicted.
const KEEP_FRAMES: u64 = 2;

static FRAME: AtomicU64 = AtomicU64::new(1);

/// Decoded-image budget in bytes (`TIDE_IMAGE_BUDGET_MB` overrides it, for
/// testing eviction with a small cache).
fn decoded_budget() -> usize {
    static B: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *B.get_or_init(|| std::env::var("TIDE_IMAGE_BUDGET_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(DECODED_BUDGET_MB) * 1024 * 1024)
}

fn frame() -> u64 {
    FRAME.load(AtomicOrdering::Relaxed)
}

fn render_bytes(r: &RenderImage) -> usize {
    (0..r.frame_count()).map(|i| r.as_bytes(i).map_or(0, |b| b.len())).sum()
}

/// BGRA render image from an RGBA buffer.
fn to_render(mut img: image::RgbaImage) -> RenderImage {
    for p in img.pixels_mut() {
        p.0.swap(0, 2);
    }
    RenderImage::new(vec![image::Frame::new(img)])
}

pub struct Images {
    map: HashMap<String, Slot>,
    /// Dominant (vivid average) colour of each loaded cover.
    colors: HashMap<String, Rgba>,
    /// Heavily blurred, saturated copy of each cover (settings preview).
    blurs: HashMap<String, (Arc<RenderImage>, u64)>,
    /// Up to three vivid, distinct colours of each cover.
    palettes: HashMap<String, Vec<Rgba>>,
    /// Low-resolution samples for the rotating Now Playing background.
    frames: Vec<(String, Arc<Vec<Arc<RenderImage>>>)>,
    /// Prevent duplicate jobs and repeated attempts to decode invalid artwork.
    frames_requested: std::collections::HashSet<String>,
    /// Covers resampled (Lanczos) to the exact device-pixel size they are drawn
    /// at, keyed by (url, width, height). The GPU only filters bilinearly with
    /// no mipmaps, so drawing a big image small aliases; these stay crisp.
    sized: HashMap<(String, u32, u32), Sized>,
    /// Last size made for each URL: shown while another size is being made
    /// (window resize), so a cover never blinks.
    latest: HashMap<String, (u32, u32)>,
    /// Bytes of decoded images held (sized + blurs), and of encoded originals.
    decoded_bytes: usize,
    encoded_bytes: usize,
    /// Textures to release on the next frame (frames of covers no longer shown).
    to_drop: Vec<Arc<RenderImage>>,
    /// Derivations (colour, palette, blur) already started, so each runs once.
    derive_pending: std::collections::HashSet<String>,
    http: Client,
    dir: PathBuf,
}

impl Global for Images {}

impl Images {
    pub fn init(http: Client, cx: &mut App) {
        let dir = crate::config::cache_dir().join("images");
        let _ = std::fs::create_dir_all(&dir);
        // Covers accumulate forever otherwise; trim the oldest in the background.
        let d = dir.clone();
        std::thread::Builder::new().name("image-cache-prune".into()).spawn(move || prune(&d, DISK_CACHE_MAX)).ok();
        cx.set_global(Images {
            map: HashMap::new(),
            colors: HashMap::new(),
            blurs: HashMap::new(),
            palettes: HashMap::new(),
            frames: Vec::new(),
            frames_requested: Default::default(),
            sized: HashMap::new(),
            latest: HashMap::new(),
            decoded_bytes: 0,
            encoded_bytes: 0,
            to_drop: Vec::new(),
            derive_pending: Default::default(),
            http,
            dir,
        });
    }
}

/// Disk budget for downloaded cover art.
const DISK_CACHE_MAX: u64 = 512 * 1024 * 1024;

/// Delete the least recently written files in `dir` until it fits in `max`
/// bytes (down to 80 % of it, so this does not run on every start).
fn prune(dir: &std::path::Path, max: u64) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .filter_map(|e| {
            let e = e.ok()?;
            let m = e.metadata().ok()?;
            m.is_file().then(|| (m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= max {
        return;
    }
    files.sort_by_key(|f| f.0);
    let target = max / 10 * 8;
    for (_, len, path) in files {
        if total <= target {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
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
    let now = frame();
    match cx.global_mut::<Images>().map.get_mut(url) {
        Some(Slot::Ready(img, used)) => {
            *used = now;
            return Some(img.clone());
        }
        Some(_) => return None,
        None => {}
    }
    let images = cx.global::<Images>();
    let http = images.http.clone();
    let path = file_for(&images.dir, url);
    cx.global_mut::<Images>().map.insert(url.to_string(), Slot::Loading);
    let url = url.to_string();
    cx.spawn(async move |cx| {
        let u = url.clone();
        let bytes = cx
            .background_executor()
            .spawn(async move {
                // Artwork copied out of local files (never sent anywhere).
                if let Some(local) = u.strip_prefix("local:") {
                    let b = std::fs::read(local).ok()?;
                    return (b.len() <= 32 * 1024 * 1024).then_some(b);
                }
                if let Ok(b) = std::fs::read(&path) {
                    return Some(b);
                }
                let resp = http.get(&u).send().ok()?;
                if !resp.status().is_success() {
                    return None;
                }
                // Cover art is a few hundred KB; refuse anything absurd.
                if resp.content_length().is_some_and(|n| n > 32 * 1024 * 1024) {
                    return None;
                }
                let b = resp.bytes().ok()?.to_vec();
                let _ = std::fs::write(&path, &b);
                Some(b)
            })
            .await;
        // Colour, palette and blur are derived on demand (see `derive`): only
        // a handful of images ever need them, so loading a page of covers does
        // not decode each one four times.
        let _ = cx.update(|cx| {
            let g = cx.global_mut::<Images>();
            let slot = match bytes {
                Some(b) => {
                    g.encoded_bytes += b.len();
                    Slot::Ready(Arc::new(Image::from_bytes(format_of(&b), b)), frame())
                }
                None => Slot::Failed,
            };
            g.map.insert(url, slot);
            cx.refresh_windows();
        });
    })
    .detach();
    None
}

enum Sized {
    Pending,
    /// Decoded at the drawn size, and the frame it was last drawn in.
    Ready(Arc<RenderImage>, u64),
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

/// The image for `url` decoded (and resampled when larger) to be drawn
/// `w` x `h` logical px (before the UI zoom). While that size is being made,
/// the last size made for the URL is shown, or nothing on first load.
pub fn sized(url: &str, w: f32, h: f32, cx: &mut App) -> Option<gpui::ImageSource> {
    let (mut dw, mut dh) = (device_px(w), device_px(h));
    // Big images (Now Playing) change size with the window: step them in 32 px
    // so a resize reuses a handful of copies instead of making one per pixel.
    if dw > 256 && dh > 256 {
        dw = dw.div_ceil(32) * 32;
        dh = dh.div_ceil(32) * 32;
    }
    let key = (url.to_string(), dw, dh);
    let now = frame();
    let images = cx.global_mut::<Images>();
    match images.sized.get_mut(&key) {
        Some(Sized::Ready(i, used)) => {
            *used = now;
            return Some(gpui::ImageSource::Render(i.clone()));
        }
        Some(Sized::Pending) => return fallback(images, url, now),
        None => {}
    }
    // An already decoded cover needs no access to its encoded original.
    let original = get(url, cx)?;
    let images = cx.global_mut::<Images>();
    images.sized.insert(key.clone(), Sized::Pending);
    let shown = fallback(images, url, now);
    let src = original.clone();
    cx.spawn(async move |cx| {
        let out = cx.background_executor().spawn(async move { resample(&src.bytes, dw, dh) }).await;
        let _ = cx.update(|cx| {
            let g = cx.global_mut::<Images>();
            match out {
                Some(r) => {
                    g.decoded_bytes += render_bytes(&r);
                    g.latest.insert(key.0.clone(), (key.1, key.2));
                    g.sized.insert(key, Sized::Ready(Arc::new(r), frame()));
                }
                None => {
                    g.sized.remove(&key);
                }
            }
            cx.refresh_windows();
        });
    })
    .detach();
    shown
}

/// How long a cover takes to fade in the first time it appears.
const FADE_IN: f32 = super::motion::CONTROL.as_secs_f32();

thread_local! {
    /// When each cover was first drawn this session (later draws do not fade).
    static FIRST_SHOWN: std::cell::RefCell<HashMap<String, std::time::Instant>> = std::cell::RefCell::new(HashMap::new());
    static LAST_FADE: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

/// Opacity of a cover that is being drawn: it eases in (0 to 1) the first time
/// it appears, then stays at 1 (scrolling back to it does not fade it again).
pub fn fade_in(url: &str) -> f32 {
    if super::motion::reduced_motion() {
        return 1.0;
    }
    let at = FIRST_SHOWN.with(|m| {
        *m.borrow_mut().entry(url.to_string()).or_insert_with(|| {
            LAST_FADE.with(|l| l.set(Some(std::time::Instant::now())));
            std::time::Instant::now()
        })
    });
    let t = (at.elapsed().as_secs_f32() / FADE_IN).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// True while a cover is still fading in (the root view keeps drawing frames).
pub fn fading() -> bool {
    !super::motion::reduced_motion() && LAST_FADE.with(|l| l.get().is_some_and(|t| t.elapsed().as_secs_f32() < FADE_IN + 0.05))
}

/// The last finished size of `url`, if any (shown while a new size is made).
fn fallback(images: &mut Images, url: &str, now: u64) -> Option<gpui::ImageSource> {
    let (w, h) = *images.latest.get(url)?;
    match images.sized.get_mut(&(url.to_string(), w, h)) {
        Some(Sized::Ready(i, used)) => {
            *used = now;
            Some(gpui::ImageSource::Render(i.clone()))
        }
        _ => None,
    }
}

/// Advance the frame counter and, when over budget, drop the least recently
/// drawn images and release their GPU textures. Called once per frame by the
/// root view, before anything is drawn.
pub fn evict(window: &mut Window, cx: &mut App) {
    let now = FRAME.fetch_add(1, AtomicOrdering::Relaxed) + 1;
    let keep_after = now.saturating_sub(KEEP_FRAMES + 1);
    let g = cx.global_mut::<Images>();
    let mut dropped: Vec<Arc<RenderImage>> = std::mem::take(&mut g.to_drop);
    if g.decoded_bytes > decoded_budget() {
        let mut old: Vec<((String, u32, u32), u64)> = g
            .sized
            .iter()
            .filter_map(|(k, v)| match v {
                Sized::Ready(_, used) if *used < keep_after => Some((k.clone(), *used)),
                _ => None,
            })
            .collect();
        old.sort_by_key(|(_, used)| *used);
        for (k, _) in old {
            if g.decoded_bytes <= decoded_budget() * 9 / 10 {
                break;
            }
            if let Some(Sized::Ready(img, _)) = g.sized.remove(&k) {
                g.decoded_bytes -= render_bytes(&img);
                if g.latest.get(&k.0) == Some(&(k.1, k.2)) {
                    g.latest.remove(&k.0);
                }
                dropped.push(img);
            }
        }
        let stale: Vec<String> = g.blurs.iter().filter(|(_, (_, used))| *used < keep_after).map(|(k, _)| k.clone()).collect();
        for k in stale {
            if g.decoded_bytes <= decoded_budget() * 9 / 10 {
                break;
            }
            if let Some((img, _)) = g.blurs.remove(&k) {
                g.decoded_bytes -= render_bytes(&img);
                g.derive_pending.remove(&format!("blur|{k}"));
                dropped.push(img);
            }
        }
    }
    if g.encoded_bytes > ENCODED_BUDGET {
        let mut old: Vec<(String, u64)> = g
            .map
            .iter()
            .filter_map(|(k, v)| match v {
                Slot::Ready(_, used) if *used < keep_after => Some((k.clone(), *used)),
                _ => None,
            })
            .collect();
        old.sort_by_key(|(_, used)| *used);
        for (k, _) in old {
            if g.encoded_bytes <= ENCODED_BUDGET * 9 / 10 {
                break;
            }
            if let Some(Slot::Ready(img, _)) = g.map.remove(&k) {
                g.encoded_bytes -= img.bytes.len();
            }
        }
    }
    let released = dropped.len();
    for img in dropped {
        cx.drop_image(img, Some(window));
    }
    log_usage(cx, released);
}

/// `TIDE_PERF=1`: every 5 s, log what the image caches hold.
fn log_usage(cx: &App, released: usize) {
    use std::cell::Cell;
    thread_local!(static LAST: Cell<Option<std::time::Instant>> = const { Cell::new(None) });
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("TIDE_PERF").is_some()) {
        return;
    }
    if LAST.with(|l| l.get().is_some_and(|t| t.elapsed().as_secs() < 5)) && released == 0 {
        return;
    }
    LAST.with(|l| l.set(Some(std::time::Instant::now())));
    let g = cx.global::<Images>();
    let frames: usize = g.frames.iter().map(|(_, f)| f.iter().map(|r| render_bytes(r)).sum::<usize>()).sum();
    let mb = |b: usize| b as f64 / 1048576.0;
    log::info!(
        "images: decoded {:.1} MB ({} sizes, {} blurs), encoded {:.1} MB ({} files), frames {:.1} MB ({} covers), released {released}",
        mb(g.decoded_bytes),
        g.sized.len(),
        g.blurs.len(),
        mb(g.encoded_bytes),
        g.map.len(),
        mb(frames),
        g.frames.len()
    );
}

/// Decode for drawing at `w` x `h` device pixels: a high-quality downscale
/// (Lanczos3, cropped to fill) when the source is meaningfully larger,
/// otherwise the source as is (the GPU scales it up smoothly). Delivered
/// already decoded (BGRA), so swapping it in never blanks a frame.
fn resample(bytes: &[u8], w: u32, h: u32) -> Option<RenderImage> {
    let src = image::load_from_memory(bytes).ok()?;
    if (src.width() as f32) < w as f32 * 1.15 && (src.height() as f32) < h as f32 * 1.15 {
        return Some(to_render(src.to_rgba8()));
    }
    Some(to_render(src.resize_to_fill(w, h, image::imageops::FilterType::Lanczos3).to_rgba8()))
}

/// Dominant colour of a loaded cover (None until the image has loaded).
pub fn color(url: &str, cx: &mut App) -> Option<Rgba> {
    if let Some(c) = cx.global::<Images>().colors.get(url) {
        return Some(*c);
    }
    derive(url, "color", cx, dominant_color, |im, url, c| {
        im.colors.insert(url, c);
    });
    None
}

/// Distinct vivid colours of a loaded cover (empty until it has loaded).
pub fn palette(url: &str, cx: &mut App) -> Vec<Rgba> {
    if let Some(p) = cx.global::<Images>().palettes.get(url) {
        return p.clone();
    }
    derive(url, "palette", cx, palette_of, |im, url, p| {
        im.palettes.insert(url, p);
    });
    Vec::new()
}

/// Compute something from the image bytes of `url` once, in the background,
/// the first time it is asked for. `kind` tells derivations of one URL apart.
fn derive<T: Send + 'static>(
    url: &str,
    kind: &'static str,
    cx: &mut App,
    compute: fn(&[u8]) -> Option<T>,
    store: fn(&mut Images, String, T),
) {
    let Some(original) = get(url, cx) else { return };
    if !cx.global_mut::<Images>().derive_pending.insert(format!("{kind}|{url}")) {
        return;
    }
    let url = url.to_string();
    cx.spawn(async move |cx| {
        let out = cx.background_executor().spawn(async move { compute(&original.bytes) }).await;
        let _ = cx.update(|cx| {
            if let Some(v) = out {
                store(cx.global_mut::<Images>(), url, v);
                cx.refresh_windows();
            }
        });
    })
    .detach();
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
    let px: Vec<[f32; 3]> = img.pixels().map(|p| p.0.map(|c| c as f32 / 255.0)).filter(|c| c[0].max(c[1]).max(c[2]) > 0.12).collect();
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
pub fn frames(url: &str, cx: &mut App) -> Option<Arc<Vec<Arc<RenderImage>>>> {
    let images = cx.global::<Images>();
    if let Some((_, f)) = images.frames.iter().find(|(u, _)| u == url) {
        return Some(f.clone());
    }
    if images.frames_requested.contains(url) {
        return None;
    }
    let original = get(url, cx)?;
    cx.global_mut::<Images>().frames_requested.insert(url.to_string());
    let url = url.to_string();
    cx.spawn(async move |cx| {
        let made = cx.background_executor().spawn(async move { rotated_frames(&original.bytes) }).await;
        let _ = cx.update(|cx| {
            let g = cx.global_mut::<Images>();
            if let Some(list) = made {
                g.frames_requested.remove(&url);
                let imgs: Vec<Arc<RenderImage>> = list.into_iter().map(Arc::new).collect();
                g.frames.push((url, Arc::new(imgs)));
                // Keep the playing cover and the previous one; release the rest.
                while g.frames.len() > 2 {
                    let (_, old) = g.frames.remove(0);
                    g.to_drop.extend(old.iter().cloned());
                }
            }
            cx.refresh_windows();
        });
    })
    .detach();
    None
}

// The backdrop interpolates between samples, so a larger step still moves continuously.
const FRAME_COUNT: usize = 48;
pub const FRAME_STEP_DEG: f32 = 360. / FRAME_COUNT as f32;

fn rotated_frames(bytes: &[u8]) -> Option<Vec<RenderImage>> {
    const SRC: u32 = 128;
    const OUT: u32 = 128;
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
        let x = (x * w - 0.5).clamp(0.0, w - 1.0);
        let y = (y * h - 0.5).clamp(0.0, h - 1.0);
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
                let n = ((ix.wrapping_mul(73856093) ^ iy.wrapping_mul(19349663)) % 1000) as f32 / 1000.0 - 0.5;
                let q = |v: f32| (v + n * 2.2).clamp(0.0, 255.0) as u8;
                // Written straight in GPUI's BGRA order: no PNG round trip.
                buf.put_pixel(ix, iy, image::Rgba([q(c[2]), q(c[1]), q(c[0]), 255]));
            }
        }
        frames.push(RenderImage::new(vec![image::Frame::new(buf)]));
    }
    Some(frames)
}

/// The cover as the official app paints Now Playing: blur(28px) saturate(1.3).
/// Computed on a tiny thumbnail; the GPU upscales it smoothly.
pub fn blurred(url: &str, cx: &mut App) -> Option<Arc<RenderImage>> {
    let now = frame();
    if let Some((i, used)) = cx.global_mut::<Images>().blurs.get_mut(url) {
        *used = now;
        return Some(i.clone());
    }
    derive(url, "blur", cx, blurred_image, |im, url, img| {
        im.decoded_bytes += render_bytes(&img);
        im.blurs.insert(url, (Arc::new(img), frame()));
    });
    None
}

fn blurred_image(bytes: &[u8]) -> Option<RenderImage> {
    let small = image::load_from_memory(bytes).ok()?.thumbnail(56, 56).to_rgba8();
    let mut blurred = image::imageops::blur(&small, 3.5);
    for p in blurred.pixels_mut() {
        let [r, g, b, a] = p.0;
        let (rf, gf, bf) = (r as f32, g as f32, b as f32);
        let lum = 0.299 * rf + 0.587 * gf + 0.114 * bf;
        let sat = |c: f32| (lum + (c - lum) * 1.3).clamp(0.0, 255.0) as u8;
        p.0 = [sat(rf), sat(gf), sat(bf), a];
    }
    Some(to_render(blurred))
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
    LOGO.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, include_bytes!("../../assets/logo.png").to_vec()))).clone()
}

#[cfg(test)]
mod tests {
    #[test]
    fn backdrop_handles_tiny_artwork_within_its_budget() {
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(1, 64).write_to(&mut encoded, image::ImageFormat::Png).unwrap();
        let frames = super::rotated_frames(encoded.get_ref()).unwrap();
        assert_eq!(frames.len(), 48);
        assert_eq!(frames.iter().map(super::render_bytes).sum::<usize>(), 3 * 1024 * 1024);
    }

    #[test]
    fn prune_removes_oldest_first() {
        let dir = std::env::temp_dir().join(format!("tide-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..5 {
            std::fs::write(dir.join(format!("{i}.img")), vec![0u8; 1000]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        super::prune(&dir, 3000);
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert!(left.len() <= 2, "{left:?}");
        assert!(left.contains(&"4.img".to_string()), "newest kept: {left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
