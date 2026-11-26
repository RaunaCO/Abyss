//! Interface scale: the user's zoom (Settings, Ctrl +/-) times an automatic
//! factor for large windows.

use std::sync::atomic::{AtomicU32, Ordering};

static ZOOM_BITS: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0
static AUTO_BITS: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0

/// Zoom levels, as in a browser.
pub const ZOOM_STEPS: [f32; 11] = [0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

/// The zoom the user chose: 1.0 = 100 %.
pub fn user_zoom() -> f32 {
    f32::from_bits(ZOOM_BITS.load(Ordering::Relaxed))
}

/// The extra scale the window's size asks for (see [`auto_zoom_for`]).
pub fn auto_zoom() -> f32 {
    f32::from_bits(AUTO_BITS.load(Ordering::Relaxed))
}

/// The scale every design pixel is drawn at.
pub fn zoom() -> f32 {
    user_zoom() * auto_zoom()
}

pub fn set_zoom(z: f32) {
    ZOOM_BITS.store(z.to_bits(), Ordering::Relaxed);
}

/// The window size (logical px) the interface is designed to fill at 100 %.
const AUTO_BASE: (f32, f32) = (2200.0, 1240.0);

/// Extra scale for a window `w` x `h` logical px. Above `AUTO_BASE` the
/// interface grows with the window, in steps of 5 % so a resize does not
/// re-measure every frame. It follows the geometric mean of the two sides, so
/// a tall, narrow window grows as much as a wide one of the same area. Never
/// below 1 and never above 2.
pub fn auto_zoom_for(w: f32, h: f32) -> f32 {
    let k = ((w / AUTO_BASE.0) * (h / AUTO_BASE.1)).sqrt();
    if k <= 1.0 {
        return 1.0;
    }
    ((k.min(2.0) * 20.0 + 0.001).floor() / 20.0).max(1.0)
}

/// Update the automatic scale for a window of `w` x `h` logical px; true when
/// it changed. `TIDE_AUTO_ZOOM=<factor>` forces it.
pub fn fit_viewport_size(w: f32, h: f32) -> bool {
    let forced = std::env::var("TIDE_AUTO_ZOOM").ok().and_then(|v| v.parse::<f32>().ok());
    let next = forced.unwrap_or_else(|| auto_zoom_for(w, h));
    let changed = (next - auto_zoom()).abs() > 0.001;
    if changed {
        AUTO_BITS.store(next.to_bits(), Ordering::Relaxed);
    }
    changed
}

/// The zoom step `dir` steps away from `cur` (snapping to the nearest step first).
#[cfg_attr(not(feature = "ui-iced"), allow(dead_code))]
pub fn step_from(cur: f32, dir: i32) -> f32 {
    let idx = ZOOM_STEPS.iter().position(|z| (z - cur).abs() < 0.01).unwrap_or_else(|| {
        ZOOM_STEPS.iter().enumerate().min_by(|a, b| (a.1 - cur).abs().total_cmp(&(b.1 - cur).abs())).map(|(i, _)| i).unwrap_or(5)
    });
    let next = (idx as i32 + dir).clamp(0, ZOOM_STEPS.len() as i32 - 1) as usize;
    ZOOM_STEPS[next]
}

#[cfg(test)]
mod tests {
    use super::auto_zoom_for;

    #[test]
    fn windows_up_to_1080p_and_small_ones_are_not_scaled() {
        assert_eq!(auto_zoom_for(1280.0, 720.0), 1.0);
        assert_eq!(auto_zoom_for(1920.0, 1080.0), 1.0);
        assert_eq!(auto_zoom_for(2200.0, 1240.0), 1.0);
    }

    #[test]
    fn a_1440p_and_a_4k_window_at_100_percent_grow_to_match() {
        assert_eq!(auto_zoom_for(2560.0, 1440.0), 1.15);
        assert_eq!(auto_zoom_for(3840.0, 2160.0), 1.7);
    }

    #[test]
    fn a_portrait_window_grows_like_a_landscape_one_of_the_same_area() {
        assert_eq!(auto_zoom_for(2160.0, 3840.0), auto_zoom_for(3840.0, 2160.0));
        assert_eq!(auto_zoom_for(1440.0, 2560.0), auto_zoom_for(2560.0, 1440.0));
        // Narrow and tall, but small: left alone.
        assert_eq!(auto_zoom_for(900.0, 3000.0), 1.0);
    }

    #[test]
    fn the_scale_is_capped_and_moves_in_steps_of_five_percent() {
        assert_eq!(auto_zoom_for(7680.0, 4320.0), 2.0);
        assert_eq!(auto_zoom_for(20000.0, 20000.0), 2.0);
        let z = auto_zoom_for(2900.0, 1700.0);
        assert!((z * 20.0 - (z * 20.0).round()).abs() < 1e-4, "{z}");
    }
}
