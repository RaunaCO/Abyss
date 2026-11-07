//! Visual effects: ambient colour from the playing cover, staggered entrances,
//! the "pop" of a liked heart, a pulse around the play button.

use super::sc;
use std::time::{Duration, Instant};

use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, Animation, AnimationExt, AnyElement, BoxShadow, Context, ElementId, Hsla, Rgba,
};

use super::{images, motion, theme, AppView};

pub const AMBIENT_FADE: Duration = Duration::from_millis(900);
pub const HEART_POP: Duration = Duration::from_millis(480);

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

/// Coloured glow under a cover.
pub fn glow(color: Rgba, strength: f32, blur: f32) -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: Hsla::from(with_alpha(color, 0.55 * strength)),
            offset: gpui::point(sc(0.), sc(18.)),
            blur_radius: sc(blur),
            spread_radius: sc(-6.),
        },
        BoxShadow {
            color: Hsla::from(gpui::rgba(0x00000099)),
            offset: gpui::point(sc(0.), sc(10.)),
            blur_radius: sc(30.),
            spread_radius: sc(0.),
        },
    ]
}

/// Delay an animation's start: the first `delay` fraction stays at 0.
pub fn delayed(delay: f32) -> impl Fn(f32) -> f32 {
    move |t| motion::ease_out(((t - delay) / (1.0 - delay)).clamp(0.0, 1.0))
}

/// Cards fade and rise in one after another.
pub fn stagger(id: impl Into<ElementId>, index: usize, el: AnyElement) -> AnyElement {
    // A light cascade: short, so content never feels like it is waiting.
    let delay = (index as f32 * 0.035).min(0.25);
    div()
        .relative()
        .flex_none()
        .child(el)
        .with_animation(id, Animation::new(Duration::from_millis(320)).with_easing(delayed(delay)), |d, t| {
            d.opacity(t).top(sc((1.0 - t) * 10.0))
        })
        .into_any_element()
}

impl AppView {
    /// Colour of the playing cover, cross-faded when the track changes.
    pub fn ambient(&mut self, cx: &mut Context<Self>) -> Rgba {
        let target = self.now.as_ref().and_then(|n| n.track.cover(640)).and_then(|u| images::color(&u, cx)).unwrap_or(theme::accent());
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
        self.ambient_at.elapsed() < AMBIENT_FADE
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

/// Soft ring that breathes around the play button while music plays.
pub fn play_pulse(color: Rgba, playing: bool) -> AnyElement {
    if !playing {
        return div().into_any_element();
    }
    div()
        .absolute()
        .top(sc(0.))
        .left(sc(0.))
        .size(sc(40.))
        .rounded(sc(20.))
        .border_2()
        .border_color(with_alpha(color, 0.6))
        .with_animation("play-pulse", Animation::new(Duration::from_millis(1800)).repeat().with_easing(motion::ease_out), |d, t| {
            let grow = 18.0 * t;
            d.size(sc(40.0 + grow)).top(sc(-grow / 2.0)).left(sc(-grow / 2.0)).rounded(sc(20.0 + grow / 2.0)).opacity(1.0 - t)
        })
        .into_any_element()
}

impl AppView {
    /// Colours of the playing cover (three entries; the ambient colour while loading).
    pub fn palette(&mut self, cx: &mut Context<Self>) -> Vec<Rgba> {
        let url = self.now.as_ref().and_then(|n| n.track.cover(640));
        let pal = url.map(|u| images::palette(&u, cx)).unwrap_or_default();
        if pal.len() >= 3 {
            pal
        } else {
            let a = self.ambient(cx);
            vec![a, a, a]
        }
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
            window.request_animation_frame();
        }
        self.bg_spin.angle(now, frac).rem_euclid(360.0)
    }
}

/// The turning cover: two neighbouring pre-rotated frames cross-faded, so the motion is
/// continuous even though frames are 2 degrees apart. Fills its parent.
pub fn rotating_backdrop(frames: &std::sync::Arc<Vec<std::sync::Arc<gpui::Image>>>, angle: f32) -> AnyElement {
    let n = frames.len().max(1);
    let pos = angle / images::FRAME_STEP_DEG;
    let i = (pos.floor() as usize) % n;
    let j = (i + 1) % n;
    let frac = pos - pos.floor();
    let layer = |f: &std::sync::Arc<gpui::Image>| {
        gpui::img(f.clone()).absolute().top(sc(0.)).left(sc(0.)).size_full().object_fit(gpui::ObjectFit::Cover)
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
