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
    http: Client,
    dir: PathBuf,
}

impl Global for Images {}

impl Images {
    pub fn init(http: Client, cx: &mut App) {
        let dir = crate::config::cache_dir().join("images");
        let _ = std::fs::create_dir_all(&dir);
        cx.set_global(Images { map: HashMap::new(), colors: HashMap::new(), http, dir });
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
        let _ = cx.update(|cx| {
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

/// Dominant colour of a loaded cover (None until the image has loaded).
pub fn color(url: &str, cx: &mut App) -> Option<Rgba> {
    if let Some(c) = cx.global::<Images>().colors.get(url) {
        return Some(*c);
    }
    let _ = get(url, cx);
    None
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
