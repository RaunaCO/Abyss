//! Cover art loader for the Iced interface: disk cache + memory cache, fetched
//! and decoded on worker threads.
//!
//! Every image handed to Iced is already decoded and resampled (Lanczos) to
//! the device-pixel size it is drawn at: the GPU filters bilinearly without
//! mipmaps, so drawing a big image small would alias. Decoded images and
//! encoded originals are held in byte-limited caches; entries not asked for
//! recently are dropped when over budget (Iced releases a texture once its
//! handle is no longer drawn).

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

use iced::widget::image::Handle;
use iced::Color;
use parking_lot::Mutex;
use reqwest::blocking::Client;

/// Budget for decoded images (resampled covers) kept in RAM.
const DECODED_BUDGET: usize = 64 * 1024 * 1024;
/// Budget for encoded originals kept in memory (they are on disk anyway).
const ENCODED_BUDGET: usize = 24 * 1024 * 1024;
/// Disk budget for downloaded cover art.
const DISK_CACHE_MAX: u64 = 512 * 1024 * 1024;
/// Worker threads that download and decode.
const WORKERS: usize = 4;

enum Slot {
    Loading,
    Ready(Arc<Vec<u8>>, u64),
    Failed(Instant),
}

enum Sized {
    Pending,
    Ready(Handle, usize, u64),
    Failed(Instant),
}

struct Inner {
    encoded: HashMap<String, Slot>,
    encoded_bytes: usize,
    sized: HashMap<(String, u32, u32), Sized>,
    /// Last finished size of each URL: shown while another size is being made.
    latest: HashMap<String, (u32, u32)>,
    decoded_bytes: usize,
    colors: HashMap<String, Color>,
    palettes: HashMap<String, Vec<Color>>,
    backdrops: Vec<(String, Arc<image::RgbaImage>)>,
    /// Blurred mip chains behind the liquid glass (see `gpu::glass`), newest last.
    glass: Vec<(String, Arc<super::gpu::glass::SceneImage>)>,
    derive_pending: HashSet<String>,
    /// When each cover was first drawn (they fade in once).
    first_shown: HashMap<String, Instant>,
}

type Job = Box<dyn FnOnce() + Send>;

struct Images {
    inner: Mutex<Inner>,
    jobs: Mutex<mpsc::Sender<Job>>,
    http: Client,
    dir: PathBuf,
}

static IMAGES: OnceLock<Images> = OnceLock::new();
static USE: AtomicU64 = AtomicU64::new(1);
static SCALE_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);
/// Something finished in the background since the interface last looked.
static DIRTY: AtomicBool = AtomicBool::new(false);

/// Start the loader (once).
pub fn init(http: Client) {
    let dir = crate::config::cache_dir().join("images");
    let _ = std::fs::create_dir_all(&dir);
    let d = dir.clone();
    std::thread::Builder::new().name("image-cache-prune".into()).spawn(move || prune(&d, DISK_CACHE_MAX)).ok();
    let (tx, rx) = mpsc::channel::<Job>();
    let rx = Arc::new(Mutex::new(rx));
    for i in 0..WORKERS {
        let rx = rx.clone();
        std::thread::Builder::new()
            .name(format!("images-{i}"))
            .spawn(move || loop {
                let job = rx.lock().recv();
                match job {
                    Ok(job) => job(),
                    Err(_) => break,
                }
            })
            .ok();
    }
    let _ = IMAGES.set(Images {
        inner: Mutex::new(Inner {
            encoded: HashMap::new(),
            encoded_bytes: 0,
            sized: HashMap::new(),
            latest: HashMap::new(),
            decoded_bytes: 0,
            colors: HashMap::new(),
            palettes: HashMap::new(),
            backdrops: Vec::new(),
            glass: Vec::new(),
            derive_pending: HashSet::new(),
            first_shown: HashMap::new(),
        }),
        jobs: Mutex::new(tx),
        http,
        dir,
    });
}

fn images() -> Option<&'static Images> {
    IMAGES.get()
}

fn spawn(job: impl FnOnce() + Send + 'static) {
    if let Some(im) = images() {
        let _ = im.jobs.lock().send(Box::new(job));
    }
}

fn mark_dirty() {
    DIRTY.store(true, Ordering::Release);
    super::wake();
}

/// True (once) when images finished loading since the last call.
pub fn take_dirty() -> bool {
    DIRTY.swap(false, Ordering::AcqRel)
}

/// The window's device-pixel ratio times the interface zoom.
pub fn set_scale(s: f32) {
    SCALE_BITS.store(s.to_bits(), Ordering::Relaxed);
}

/// `logical` rounded so it covers a whole number of device pixels. A cover
/// drawn at that size maps its resampled texture one texel to one pixel; a
/// fractional size makes the GPU rescale it by a pixel and soften it.
pub fn snap(logical: f32) -> f32 {
    let s = f32::from_bits(SCALE_BITS.load(Ordering::Relaxed));
    if s <= 0.0 {
        return logical;
    }
    (logical * s).round().max(1.0) / s
}

fn device_px(logical: f32) -> u32 {
    let s = f32::from_bits(SCALE_BITS.load(Ordering::Relaxed));
    (logical * s).round().max(1.0) as u32
}

fn file_for(dir: &std::path::Path, url: &str) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    dir.join(format!("{:016x}.img", h.finish()))
}

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

fn fetch_bytes(http: &Client, dir: &std::path::Path, url: &str) -> Option<Vec<u8>> {
    // Artwork copied out of local files (never sent anywhere).
    if let Some(local) = url.strip_prefix("local:") {
        let b = std::fs::read(local).ok()?;
        return (b.len() <= 32 * 1024 * 1024).then_some(b);
    }
    let path = file_for(dir, url);
    if let Ok(b) = std::fs::read(&path) {
        return Some(b);
    }
    let resp = http.get(url).send().ok()?;
    if !resp.status().is_success() || resp.content_length().is_some_and(|n| n > 32 * 1024 * 1024) {
        return None;
    }
    let b = resp.bytes().ok()?.to_vec();
    let _ = crate::storage::atomic_write(&path, &b);
    Some(b)
}

/// The encoded bytes of `url` if loaded; starts loading them otherwise.
fn encoded(url: &str) -> Option<Arc<Vec<u8>>> {
    let im = images()?;
    let now = USE.load(Ordering::Relaxed);
    {
        let mut g = im.inner.lock();
        match g.encoded.get_mut(url) {
            Some(Slot::Ready(b, used)) => {
                *used = now;
                return Some(b.clone());
            }
            Some(Slot::Loading) => return None,
            // Damaged or missing artwork: retry now and then, not every frame.
            Some(Slot::Failed(at)) if at.elapsed() < Duration::from_secs(30) => return None,
            _ => {}
        }
        g.encoded.insert(url.to_string(), Slot::Loading);
    }
    let url = url.to_string();
    spawn(move || {
        let Some(im) = images() else { return };
        let bytes = fetch_bytes(&im.http, &im.dir, &url);
        let mut g = im.inner.lock();
        let slot = match bytes {
            Some(b) => {
                g.encoded_bytes += b.len();
                Slot::Ready(Arc::new(b), USE.load(Ordering::Relaxed))
            }
            None => Slot::Failed(Instant::now()),
        };
        g.encoded.insert(url, slot);
        drop(g);
        mark_dirty();
    });
    None
}

/// Decode for drawing at `w` x `h` device pixels: a Lanczos3 downscale
/// (cropped to fill) when the source is meaningfully larger, otherwise the
/// source as is (the GPU scales it up smoothly).
fn resample(bytes: &[u8], w: u32, h: u32, contain: bool) -> Option<image::RgbaImage> {
    let src = image::load_from_memory(bytes).ok()?;
    if (src.width() as f32) < w as f32 * 1.15 && (src.height() as f32) < h as f32 * 1.15 {
        return Some(src.to_rgba8());
    }
    let filter = image::imageops::FilterType::Lanczos3;
    let resized = if contain { src.resize(w, h, filter) } else { src.resize_to_fill(w, h, filter) };
    Some(resized.to_rgba8())
}

/// The image for `url` decoded to be drawn `w` x `h` logical px. While that
/// size is being made, the last size made for the URL is returned (a cover
/// never blinks on resize), or nothing on first load.
pub fn sized(url: &str, w: f32, h: f32) -> Option<Handle> {
    sized_image(url, w, h, false)
}

pub fn fitted_cover(url: &str, side: f32) -> Option<Handle> {
    sized_image(url, side, side, true)
}

fn sized_image(url: &str, w: f32, h: f32, contain: bool) -> Option<Handle> {
    let im = images()?;
    let (mut dw, mut dh) = (device_px(w), device_px(h));
    // Big images change size with the window: step them in 32 px.
    if dw > 256 && dh > 256 {
        dw = dw.div_ceil(32) * 32;
        dh = dh.div_ceil(32) * 32;
    }
    let key = (if contain { format!("fit|{url}") } else { url.to_string() }, dw, dh);
    let now = USE.load(Ordering::Relaxed);
    {
        let mut g = im.inner.lock();
        match g.sized.get_mut(&key) {
            Some(Sized::Ready(h, _, used)) => {
                *used = now;
                return Some(h.clone());
            }
            Some(Sized::Pending) => return fallback(&mut g, &key.0, now),
            Some(Sized::Failed(at)) if at.elapsed() < Duration::from_secs(30) => return fallback(&mut g, &key.0, now),
            _ => {}
        }
    }
    let original = encoded(url)?;
    let mut g = im.inner.lock();
    g.sized.insert(key.clone(), Sized::Pending);
    let shown = fallback(&mut g, &key.0, now);
    drop(g);
    spawn(move || {
        let out = resample(&original, dw, dh, contain);
        let Some(im) = images() else { return };
        let mut g = im.inner.lock();
        match out {
            Some(img) => {
                let bytes = img.as_raw().len();
                let (w, h) = img.dimensions();
                g.decoded_bytes += bytes;
                g.latest.insert(key.0.clone(), (key.1, key.2));
                g.sized.insert(key, Sized::Ready(Handle::from_rgba(w, h, img.into_raw()), bytes, USE.load(Ordering::Relaxed)));
            }
            None => {
                g.sized.insert(key, Sized::Failed(Instant::now()));
            }
        }
        drop(g);
        mark_dirty();
    });
    shown
}

fn fallback(g: &mut Inner, url: &str, now: u64) -> Option<Handle> {
    let (w, h) = *g.latest.get(url)?;
    match g.sized.get_mut(&(url.to_string(), w, h)) {
        Some(Sized::Ready(handle, _, used)) => {
            *used = now;
            Some(handle.clone())
        }
        _ => None,
    }
}

/// How long a cover takes to fade in the first time it appears.
const FADE_IN: f32 = 0.143;

/// Opacity of a cover being drawn: it eases in the first time it appears,
/// then stays at 1.
pub fn fade_in(url: &str) -> f32 {
    if crate::shared::lyrics::reduced_motion() {
        return 1.0;
    }
    let Some(im) = images() else { return 1.0 };
    let at = *im.inner.lock().first_shown.entry(url.to_string()).or_insert_with(Instant::now);
    let t = (at.elapsed().as_secs_f32() / FADE_IN).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// True while a cover is still fading in.
pub fn fading() -> bool {
    if crate::shared::lyrics::reduced_motion() {
        return false;
    }
    let Some(im) = images() else { return false };
    let g = im.inner.lock();
    g.first_shown.values().any(|at| at.elapsed().as_secs_f32() < FADE_IN + 0.05)
}

/// Advance the use counter and drop what is over budget (least recently
/// asked for first). Called once per view.
pub fn evict() {
    let Some(im) = images() else { return };
    let now = USE.fetch_add(1, Ordering::Relaxed) + 1;
    let keep_after = now.saturating_sub(3);
    let mut g = im.inner.lock();
    if g.decoded_bytes > DECODED_BUDGET {
        let mut old: Vec<((String, u32, u32), u64)> = g
            .sized
            .iter()
            .filter_map(|(k, v)| match v {
                Sized::Ready(_, _, used) if *used < keep_after => Some((k.clone(), *used)),
                _ => None,
            })
            .collect();
        old.sort_by_key(|(_, used)| *used);
        for (k, _) in old {
            if g.decoded_bytes <= DECODED_BUDGET * 9 / 10 {
                break;
            }
            if let Some(Sized::Ready(_, bytes, _)) = g.sized.remove(&k) {
                g.decoded_bytes -= bytes;
                if g.latest.get(&k.0) == Some(&(k.1, k.2)) {
                    g.latest.remove(&k.0);
                }
            }
        }
    }
    if g.encoded_bytes > ENCODED_BUDGET {
        let mut old: Vec<(String, u64)> = g
            .encoded
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
            if let Some(Slot::Ready(b, _)) = g.encoded.remove(&k) {
                g.encoded_bytes -= b.len();
            }
        }
    }
    if g.first_shown.len() > 4096 {
        g.first_shown.retain(|_, at| at.elapsed() < Duration::from_secs(1));
    }
}

/// Bytes held: (decoded, encoded) — for the performance log.
pub fn usage() -> (usize, usize) {
    images()
        .map(|im| {
            let g = im.inner.lock();
            (g.decoded_bytes, g.encoded_bytes)
        })
        .unwrap_or_default()
}

/// Entries held, for the performance log: "sized/encoded/derived".
pub fn entries() -> String {
    images()
        .map(|im| {
            let g = im.inner.lock();
            format!(
                "{} sized, {} encoded, {} colours, {} palettes, {} pending, {} shown",
                g.sized.len(),
                g.encoded.len(),
                g.colors.len(),
                g.palettes.len(),
                g.derive_pending.len(),
                g.first_shown.len()
            )
        })
        .unwrap_or_default()
}

/// Compute something from the bytes of `url` once, in the background.
fn derive<T: Send + 'static>(
    url: &str,
    kind: &'static str,
    compute: impl FnOnce(&[u8]) -> Option<T> + Send + 'static,
    store: impl FnOnce(&mut Inner, String, T) + Send + 'static,
) {
    let Some(im) = images() else { return };
    let Some(original) = encoded(url) else { return };
    if !im.inner.lock().derive_pending.insert(format!("{kind}|{url}")) {
        return;
    }
    let url = url.to_string();
    spawn(move || {
        if let Some(v) = compute(&original) {
            if let Some(im) = images() {
                store(&mut im.inner.lock(), url, v);
                mark_dirty();
            }
        }
    });
}

/// Dominant colour of a loaded cover (None until it has loaded).
pub fn color(url: &str) -> Option<Color> {
    if let Some(c) = images()?.inner.lock().colors.get(url) {
        return Some(*c);
    }
    derive(url, "color", super::artwork::dominant_color, |g, url, c| {
        g.colors.insert(url, c);
    });
    None
}

/// Three vivid, distinct colours of a loaded cover (empty until loaded).
pub fn palette(url: &str) -> Vec<Color> {
    if let Some(p) = images().and_then(|im| im.inner.lock().palettes.get(url).cloned()) {
        return p;
    }
    derive(url, "palette", super::artwork::palette_of, |g, url, p| {
        g.palettes.insert(url, p);
    });
    Vec::new()
}

/// The blurred, saturated thumbnail behind Now Playing (None until made).
/// Only the playing cover and the previous one are kept.
pub fn backdrop(url: &str) -> Option<Arc<image::RgbaImage>> {
    if let Some((_, img)) = images()?.inner.lock().backdrops.iter().find(|(u, _)| u == url) {
        return Some(img.clone());
    }
    derive(url, "backdrop", super::artwork::backdrop_image, |g, url, img| {
        g.backdrops.push((url, Arc::new(img)));
        while g.backdrops.len() > 2 {
            g.backdrops.remove(0);
        }
    });
    None
}

/// The picture behind the liquid glass for `url`: blurred into a mip chain
/// on a worker thread (None until made). The last three are kept.
pub fn glass_scene(url: &str, palette_only: bool) -> Option<super::gpu::glass::Scene> {
    use super::gpu::glass::{Scene, SceneImage};
    let kind = if palette_only { "glass-palette" } else { "glass" };
    let key = format!("{kind}|{url}");
    if let Some((_, img)) = images()?.inner.lock().glass.iter().find(|(u, _)| *u == key) {
        return Some(Scene { key, image: img.clone() });
    }
    derive(
        url,
        kind,
        move |bytes| {
            if palette_only {
                let colors: [Color; 3] = super::artwork::palette_of(bytes)?.try_into().ok()?;
                Some(SceneImage::palette(colors))
            } else {
                image::load_from_memory(bytes).ok().map(|img| SceneImage::from_image(&img))
            }
        },
        move |g, _, img| {
            g.glass.retain(|(u, _)| *u != key);
            g.glass.push((key, Arc::new(img)));
            while g.glass.len() > 3 {
                let (old, _) = g.glass.remove(0);
                // Evicted: it may be made again when the page comes back.
                g.derive_pending.remove(&old);
            }
        },
    );
    None
}

/// The app logo (assets/logo.png).
pub fn logo() -> Handle {
    static LOGO: OnceLock<Handle> = OnceLock::new();
    LOGO.get_or_init(|| Handle::from_bytes(include_bytes!("../../assets/logo.png").as_slice())).clone()
}

#[cfg(test)]
mod tests {
    #[test]
    fn prune_removes_oldest_first() {
        let dir = std::env::temp_dir().join(format!("abyss-iced-prune-{}", std::process::id()));
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

    #[test]
    fn resampling_fills_the_box_and_keeps_small_sources() {
        let mut big = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(400, 200).write_to(&mut big, image::ImageFormat::Png).unwrap();
        let out = super::resample(big.get_ref(), 100, 100, false).unwrap();
        assert_eq!(out.dimensions(), (100, 100));
        let kept = super::resample(big.get_ref(), 390, 190, false).unwrap();
        assert_eq!(kept.dimensions(), (400, 200));
        let fitted = super::resample(big.get_ref(), 100, 100, true).unwrap();
        assert_eq!(fitted.dimensions(), (100, 50));
    }
}
