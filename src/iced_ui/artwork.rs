//! Colours read from cover art (dominant colour, palette) and the blurred,
//! saturated thumbnail behind Now Playing. Same algorithms as the GPUI
//! reference (`src/ui/images.rs`).

use iced::Color as Rgba;

pub(super) fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
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
pub fn palette_of(bytes: &[u8]) -> Option<Vec<Rgba>> {
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
    const K: usize = 8;
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
    let mut ranked: Vec<(f32, [f32; 3], f32)> = (0..centers.len())
        .filter(|&i| counts[i] > 0)
        .map(|i| {
            let s = sat(&centers[i]);
            let share = counts[i] as f32 / total;
            (share.sqrt() * (0.25 + s), centers[i], share)
        })
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    #[cfg(test)]
    if std::env::var("ABYSS_PALETTE_DEBUG").is_ok() {
        for r in &ranked {
            let (h, sv, v) = rgb_to_hsv(r.1[0], r.1[1], r.1[2]);
            eprintln!("cluster share {:.3} hsv ({h:.0}, {sv:.2}, {v:.2}) rgb {:?}", r.2, r.1.map(|x| (x * 255.0) as u8));
        }
    }
    // The cover's colours come first: a colour on a fair part of the cover
    // (3 %) beats a larger grey, so a sky blue is not lost to a grey wall.
    // Greys only fill the slots colours leave (all of them on a black-and-white cover).
    let order: Vec<[f32; 3]> = {
        let colourful = |c: &[f32; 3], share: f32| sat(c) >= 0.15 && share >= 0.03;
        let first = ranked.iter().filter(|r| colourful(&r.1, r.2));
        let rest = ranked.iter().filter(|r| !colourful(&r.1, r.2));
        first.chain(rest).map(|r| r.1).collect()
    };
    let hue_of = |c: &[f32; 3]| rgb_to_hsv(c[0], c[1], c[2]).0;
    let mut chosen: Vec<[f32; 3]> = Vec::new();
    for c in &order {
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
    let glow = |c: &[f32; 3]| {
        let (h, s, v) = rgb_to_hsv(c[0], c[1], c[2]);
        // In proportion to the cover's own: a muted slate stays a slate (a
        // floor of 45 % turned faint tints into vivid periwinkle or mint).
        let s = if s < 0.12 { s } else { (s * 1.3 + 0.06).min(0.95) };
        (h, s, (v * 1.2).clamp(0.62, 0.96))
    };
    let mut out: Vec<Rgba> = chosen.iter().map(&glow).map(|(h, s, v)| hsv_to_rgb(h, s, v)).collect();
    // A cover of one hue (a sepia photo, a peach drawing) gives fewer hues
    // than slots: fill them with its own other tones, lighter and darker, as
    // they appear on it. Never a rotated hue: that invents a green the cover
    // does not have.
    let value = |c: &[f32; 3]| rgb_to_hsv(c[0], c[1], c[2]).2;
    for c in &order {
        if out.len() == 3 {
            break;
        }
        if chosen.iter().all(|o| (value(o) - value(c)).abs() >= 0.18) {
            chosen.push(*c);
            let (h, s, _) = glow(&chosen[0]);
            let (_, own_s, _) = rgb_to_hsv(c[0], c[1], c[2]);
            // The tone keeps its distance in light from the lead (a little
            // compressed), on the lead's hue.
            let (_, _, lead_v) = glow(&chosen[0]);
            let v = (lead_v + (value(c) - value(&chosen[0])) * 0.6).clamp(0.42, 0.96);
            out.push(hsv_to_rgb(h, if own_s < 0.12 { own_s } else { s }, v));
        }
    }
    // Still short (a flat cover): a deeper and a paler tone of the lead.
    while out.len() < 3 {
        let (h, s, v) = rgb_to_hsv(out[0].r, out[0].g, out[0].b);
        let paler = out.len() == 2;
        out.push(if paler { hsv_to_rgb(h, s * 0.7, (v * 1.15).min(0.96)) } else { hsv_to_rgb(h, s, (v * 0.62).max(0.4)) });
    }
    Some(out)
}

/// A vivid "average" colour: saturated pixels weigh more than greys, so a
/// mostly black cover with a red logo comes out red, not grey.
pub fn dominant_color(bytes: &[u8]) -> Option<Rgba> {
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

/// The cover as the official app paints Now Playing: blurred (28 px at its
/// size) and saturated 1.3, as a small RGBA image. The GPU stretches, turns
/// and dithers it (see `gpu::backdrop`).
pub fn backdrop_image(bytes: &[u8]) -> Option<image::RgbaImage> {
    const SRC: u32 = 128;
    let small = image::load_from_memory(bytes).ok()?.thumbnail(SRC, SRC).to_rgba8();
    let mut src = image::imageops::blur(&small, 7.5);
    for p in src.pixels_mut() {
        let [r, g, b, _] = p.0;
        let (rf, gf, bf) = (r as f32, g as f32, b as f32);
        let lum = 0.299 * rf + 0.587 * gf + 0.114 * bf;
        let sat = |c: f32| (lum + (c - lum) * 1.3).clamp(0.0, 255.0) as u8;
        p.0 = [sat(rf), sat(gf), sat(bf), 255];
    }
    Some(src)
}

#[cfg(test)]
mod tests {
    fn png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(w, h, image::Rgb(rgb)).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_red_cover_gives_a_red_ambient_and_three_palette_entries() {
        let bytes = png(32, 32, [200, 20, 20]);
        let c = super::dominant_color(&bytes).unwrap();
        assert!(c.r > c.g * 3.0 && c.r > c.b * 3.0, "{c:?}");
        assert_eq!(super::palette_of(&bytes).unwrap().len(), 3);
    }

    #[test]
    fn the_backdrop_is_small_and_opaque_even_for_tiny_artwork() {
        let img = super::backdrop_image(&png(1, 64, [10, 200, 30])).unwrap();
        assert!(img.width() <= 128 && img.height() <= 128);
        assert!(img.pixels().all(|p| p.0[3] == 255));
    }
}
