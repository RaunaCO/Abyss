//! Visual effects: ambient colour from the playing cover, staggered entrances,
//! the "pop" of a liked heart, a pulse around the play button.

use super::sc;
use std::time::{Duration, Instant};

use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, Animation, AnimationExt, AnyElement, BoxShadow, Context, ElementId, Hsla, Rgba,
};

use super::{images, motion, theme, AppView};

pub const AMBIENT_FADE: Duration = motion::AMBIENT_FADE;
pub const HEART_POP: Duration = motion::PANEL;

pub fn with_alpha(c: Rgba, a: f32) -> Rgba {
    Rgba { a, ..c }
}

fn lerp(a: Rgba, b: Rgba, t: f32) -> Rgba {
    Rgba { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
}

/// Vertical gradient from a colour (top) into the page background.
pub fn tint(color: Rgba, strength: f32) -> gpui::Background {
    linear_gradient(180., linear_color_stop(with_alpha(color, strength), 0.), linear_color_stop(with_alpha(theme::bg(), 0.0), 1.))
}

/// Delay an animation's start: the first `delay` fraction stays at 0.
pub fn delayed(delay: f32) -> impl Fn(f32) -> f32 {
    move |t| motion::ease_out(((t - delay) / (1.0 - delay)).clamp(0.0, 1.0))
}

/// Cards fade and rise in one after another.
pub fn stagger(id: impl Into<ElementId>, index: usize, el: AnyElement) -> AnyElement {
    // A light cascade: short, so content never feels like it is waiting.
    if motion::reduced_motion() || index >= 8 {
        return el;
    }
    let delay = (index as f32 * motion::STAGGER).min(0.25);
    div()
        .relative()
        .flex_none()
        .child(el)
        .with_animation(id, Animation::new(motion::PANEL).with_easing(delayed(delay)), |d, t| d.opacity(t).top(sc((1.0 - t) * 10.0)))
        .into_any_element()
}

impl AppView {
    /// Colour of the playing cover, cross-faded when the track changes.
    pub fn ambient(&mut self, cx: &mut Context<Self>) -> Rgba {
        // Until the new cover's colour is known, keep fading toward the old
        // one (instead of jumping to a fallback colour and back).
        let target = match self.now.as_ref().and_then(|n| n.track.cover(640)).and_then(|u| images::color(&u, cx)) {
            Some(c) => c,
            None if self.now.is_some() && self.ambient_seen => self.ambient_to,
            None => theme::accent(),
        };
        self.ambient_seen |= self.now.is_some();
        if target != self.ambient_to {
            let current = self.ambient_now();
            self.ambient_from = current;
            self.ambient_to = target;
            self.ambient_at = Instant::now();
        }
        self.ambient_now()
    }

    fn ambient_now(&self) -> Rgba {
        let t = motion::ease_out(motion::progress(self.ambient_at, AMBIENT_FADE));
        lerp(self.ambient_from, self.ambient_to, t)
    }

    pub fn ambient_animating(&self) -> bool {
        !motion::reduced_motion() && self.ambient_at.elapsed() < AMBIENT_FADE
    }

    /// Size multiplier for a heart that was just liked (bouncy pop).
    pub fn heart_scale(&self, track_id: u64) -> f32 {
        match self.heart_pop {
            Some((id, at)) if id == track_id => {
                let t = motion::progress(at, HEART_POP);
                1.0 + 0.55 * (std::f32::consts::PI * t).sin() * (1.0 - t * 0.6)
            }
            _ => 1.0,
        }
    }
}

impl AppView {
    /// Colours of the playing cover (three entries; the ambient colour while loading).
    pub fn palette(&mut self, cx: &mut Context<Self>) -> Vec<Rgba> {
        let url = self.now.as_ref().and_then(|n| n.track.cover(640));
        let pal = url.map(|u| images::palette(&u, cx)).unwrap_or_default();
        // The new palette (or the last one, while the new one is computed)
        // fades in over the ambient fade's duration.
        let target = if pal.len() >= 3 {
            pal
        } else if self.palette_to.len() >= 3 {
            self.palette_to.clone()
        } else {
            let a = self.ambient(cx);
            vec![a, a, a]
        };
        if target != self.palette_to {
            let t = motion::ease_out(motion::progress(self.palette_at, AMBIENT_FADE));
            self.palette_from = if self.palette_to.len() >= 3 {
                self.palette_from.iter().zip(&self.palette_to).map(|(a, b)| lerp(*a, *b, t)).collect()
            } else {
                target.clone()
            };
            self.palette_to = target;
            self.palette_at = Instant::now();
        }
        let t = motion::ease_out(motion::progress(self.palette_at, AMBIENT_FADE));
        if t < 1.0 {
            motion::want_frame("ambient");
        }
        self.palette_from.iter().zip(&self.palette_to).map(|(a, b)| lerp(*a, *b, t)).collect()
    }
}

/// Big soft colour glows that drift slowly, like the official Now Playing
/// background. `t` is the looping animation phase (0..1); `vis` the smoothed
/// audio bands, which breathe the glows (brightness, size and a little drift).
pub fn blobs(colors: &[Rgba], t: f32, strength: f32, vis: [f32; 3]) -> Vec<AnyElement> {
    // (x, y, phase, radius in px at 100 % zoom, colour slot, band)
    const SPEC: [(f32, f32, f32, f32, usize, usize); 5] = [
        (0.10, 0.20, 0.0, 420.0, 0, 0),
        (0.90, 0.12, 1.7, 380.0, 1, 1),
        (0.62, 0.95, 3.3, 460.0, 2, 0),
        (0.04, 0.88, 4.8, 340.0, 1, 2),
        (0.96, 0.70, 2.4, 360.0, 0, 1),
    ];
    let tau = std::f32::consts::TAU;
    SPEC.iter()
        .map(|(x, y, ph, r, slot, band)| {
            let c = colors[slot % colors.len().max(1)];
            let v = vis[*band];
            let dx = 0.17 * (t * tau + ph).sin() + 0.05 * (v - 0.5);
            let dy = 0.13 * (t * tau + ph * 1.3).cos() + 0.03 * (v - 0.5);
            // Two stacked, very wide shadows give a smooth falloff without a visible edge.
            div()
                .absolute()
                .left(gpui::relative(x + dx))
                .top(gpui::relative(y + dy))
                .size(sc(4.))
                .rounded(sc(2.))
                .shadow(vec![
                    BoxShadow {
                        color: Hsla::from(with_alpha(c, 0.34 * strength * (0.7 + 0.7 * v))),
                        offset: gpui::point(sc(0.), sc(0.)),
                        blur_radius: sc(*r * 1.6),
                        spread_radius: sc(*r * 0.55 * (0.9 + 0.2 * v)),
                    },
                    BoxShadow {
                        color: Hsla::from(with_alpha(c, 0.30 * strength * (0.6 + 0.9 * v))),
                        offset: gpui::point(sc(0.), sc(0.)),
                        blur_radius: sc(*r * 1.1),
                        spread_radius: sc(*r * 0.18),
                    },
                ])
                .into_any_element()
        })
        .collect()
}

// ---------------------------------------------------------------- rotating background
//
// Taken from the official app's player background (its `rotator` code): the blurred
// cover turns about its centre at 360 degrees per minute while playing; on pause it
// coasts 30 degrees further over 1.5 s (ease-out cubic) and stops; while the progress
// bar is dragged the angle follows the bar (180 degrees across the whole track).

pub const SPIN_PERIOD_S: f32 = 60.0;
const COAST_DEG: f32 = 30.0;
const COAST_S: f32 = 1.5;

#[derive(Clone, Copy)]
pub enum BgSpin {
    Stopped(f32),
    Spinning { start: f32, at: Instant },
    Coasting { start: f32, at: Instant },
    Scrub { base: f32, frac0: f32 },
}

impl BgSpin {
    fn angle(self, now: Instant, frac: f32) -> f32 {
        match self {
            BgSpin::Stopped(a) => a,
            BgSpin::Spinning { start, at } => start + now.duration_since(at).as_secs_f32() / SPIN_PERIOD_S * 360.0,
            BgSpin::Coasting { start, at } => {
                let t = (now.duration_since(at).as_secs_f32() / COAST_S).clamp(0.0, 1.0);
                start + COAST_DEG * (1.0 - (1.0 - t).powi(3))
            }
            BgSpin::Scrub { base, frac0 } => base + (frac - frac0) * 180.0,
        }
    }
}

impl AppView {
    /// Angle (degrees) of the rotating background right now; updates the state machine.
    /// Asks for another frame while the background is moving.
    pub fn bg_angle(&mut self, window: &mut gpui::Window) -> f32 {
        if motion::reduced_motion() {
            return 0.0;
        }
        let now = Instant::now();
        let playing = self.now.is_some() && !self.snap.paused && !self.ended && !self.snap.buffering;
        let scrubbing = self.drag == Some(super::Drag::Progress);
        let frac = self.drag_value;
        let cur = self.bg_spin.angle(now, frac);
        self.bg_spin = match (scrubbing, playing, self.bg_spin) {
            (true, _, s @ BgSpin::Scrub { .. }) => s,
            (true, _, _) => BgSpin::Scrub { base: cur, frac0: frac },
            (false, true, s @ BgSpin::Spinning { .. }) => s,
            (false, true, _) => BgSpin::Spinning { start: cur, at: now },
            (false, false, BgSpin::Spinning { .. } | BgSpin::Scrub { .. }) => BgSpin::Coasting { start: cur, at: now },
            (false, false, BgSpin::Coasting { start, at }) => {
                if now.duration_since(at).as_secs_f32() >= COAST_S {
                    BgSpin::Stopped(start + COAST_DEG)
                } else {
                    BgSpin::Coasting { start, at }
                }
            }
            (false, false, s) => s,
        };
        if matches!(self.bg_spin, BgSpin::Spinning { .. } | BgSpin::Coasting { .. }) {
            super::motion::want_frame("bg-spin");
        }
        let _ = window;
        self.bg_spin.angle(now, frac).rem_euclid(360.0)
    }
}

/// The turning cover: two neighbouring pre-rotated frames cross-faded, so the motion is
/// continuous between samples. Fills its parent.
pub fn rotating_backdrop(frames: &std::sync::Arc<Vec<std::sync::Arc<gpui::RenderImage>>>, angle: f32) -> AnyElement {
    let n = frames.len().max(1);
    let pos = angle / images::FRAME_STEP_DEG;
    let i = (pos.floor() as usize) % n;
    let j = (i + 1) % n;
    let frac = pos - pos.floor();
    let layer = |f: &std::sync::Arc<gpui::RenderImage>| {
        gpui::img(gpui::ImageSource::Render(f.clone())).absolute().top(sc(0.)).left(sc(0.)).size_full().object_fit(gpui::ObjectFit::Cover)
    };
    div()
        .absolute()
        .top(sc(0.))
        .left(sc(0.))
        .size_full()
        .overflow_hidden()
        .child(layer(&frames[i]))
        .child(layer(&frames[j]).opacity(frac))
        .into_any_element()
}
