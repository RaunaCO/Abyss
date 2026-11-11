//! The Now Playing cover reacts to the mouse with light only: the picture
//! itself stays perfectly still (no tilt, drift or deformation) while a soft
//! sheen follows the pointer across it, as if a light moved over a glossy
//! print. The sheen and shade share the cover's exact rounded rectangle, so
//! they never spill past its edges. Driven by springs (stiffness 200,
//! damping 30) so the light glides and settles; it fades away on leave.
//!
//! The cover is a stack of plain elements (image layers for the track-change
//! cross-fade, then the two light gradients), all with the same size and
//! corner radius: no canvas tricks, masks or overlapping pieces.

use std::time::Instant;

use super::sc;
use gpui::{div, img, prelude::*, AnyElement, BoxShadow, Hsla, ImageSource, ObjectFit, Rgba};

/// How strongly the light reacts toward the edges (0 at the very edge).
const EDGE: f32 = 0.15;
const STIFFNESS: f32 = 200.0;
const DAMPING: f32 = 30.0;
/// Corner radius of the cover (logical px).
pub const RADIUS: f32 = 6.0;

#[derive(Clone, Copy)]
struct Spring {
    x: f32,
    v: f32,
    target: f32,
}

impl Spring {
    const fn at(x: f32) -> Self {
        Self { x, v: 0.0, target: x }
    }
    fn step(&mut self, dt: f32) {
        let a = STIFFNESS * (self.target - self.x) - DAMPING * self.v;
        self.v += a * dt;
        self.x += self.v * dt;
    }
    fn settled(&self) -> bool {
        (self.target - self.x).abs() < 0.002 && self.v.abs() < 0.002
    }
}

/// Pointer position over the cover (-1..1 from the centre) and light strength.
pub struct Tilt {
    px: Spring,
    py: Spring,
    glow: Spring,
    last: Instant,
}

impl Default for Tilt {
    fn default() -> Self {
        Self { px: Spring::at(0.0), py: Spring::at(0.0), glow: Spring::at(0.0), last: Instant::now() }
    }
}

impl Tilt {
    /// Pointer at (x, y) as fractions of the cover (0..1).
    pub fn point(&mut self, x: f32, y: f32) {
        let edge = x.min(1.0 - x).min(y).min(1.0 - y).max(0.0);
        self.px.target = (x - 0.5) * 2.0;
        self.py.target = (y - 0.5) * 2.0;
        self.glow.target = (edge / EDGE).min(1.0);
    }

    pub fn leave(&mut self) {
        self.glow.target = 0.0;
    }

    /// Advance the springs; true while still moving.
    pub fn step(&mut self) -> bool {
        if super::motion::reduced_motion() {
            for s in [&mut self.px, &mut self.py, &mut self.glow] {
                s.x = s.target;
                s.v = 0.0;
            }
            return false;
        }
        let now = Instant::now();
        let mut dt = now.duration_since(self.last).as_secs_f32().min(0.05);
        self.last = now;
        // Small fixed sub-steps keep the spring stable at low frame rates.
        while dt > 0.0 {
            let h = dt.min(1.0 / 240.0);
            for s in [&mut self.px, &mut self.py, &mut self.glow] {
                s.step(h);
            }
            dt -= h;
        }
        ![self.px, self.py, self.glow].iter().all(Spring::settled)
    }

    /// The cover, `size` logical px square: the previous picture under the
    /// current one fading in (track change), then the light. Nothing moves
    /// or deforms; only the light follows the pointer.
    pub fn surface(
        &self,
        prev: Option<ImageSource>,
        now: Option<(ImageSource, f32)>,
        size: f32,
        ambient: Rgba,
        placeholder: Rgba,
    ) -> AnyElement {
        let r = sc(RADIUS);
        let layer = |src: ImageSource, a: f32| {
            img(src).absolute().top(sc(0.)).left(sc(0.)).size(sc(size)).rounded(r).object_fit(ObjectFit::Cover).opacity(a)
        };
        let glow = self.glow.x.clamp(0.0, 1.0);
        let (x, y) = (self.px.x.clamp(-1.0, 1.0), self.py.x.clamp(-1.0, 1.0));
        // Direction from the pointer toward the opposite edge (CSS angles: 0 = up).
        let angle = if x.abs() + y.abs() > 0.001 { (-x).atan2(y).to_degrees() } else { 180.0 };
        let reach = (x * x + y * y).sqrt().min(1.0);
        let white = |a: f32| super::fx::with_alpha(gpui::rgb(0xffffff), a);
        let black = |a: f32| super::fx::with_alpha(gpui::rgb(0x000000), a);
        let light = |bg: gpui::Background| div().absolute().top(sc(0.)).left(sc(0.)).size(sc(size)).rounded(r).bg(bg);
        div()
            .relative()
            .flex_none()
            .size(sc(size))
            .rounded(r)
            // A still, soft shadow in the cover's colour.
            .shadow(vec![
                BoxShadow {
                    color: Hsla::from(super::fx::with_alpha(ambient, 0.42)),
                    offset: gpui::point(sc(0.), sc(18.)),
                    blur_radius: sc(70.),
                    spread_radius: sc(-6.),
                },
                BoxShadow {
                    color: Hsla::from(gpui::rgba(0x00000088)),
                    offset: gpui::point(sc(0.), sc(9.)),
                    blur_radius: sc(26.),
                    spread_radius: sc(0.),
                },
            ])
            .when(prev.is_none() && now.is_none(), |d| d.bg(placeholder))
            .children(prev.map(|p| layer(p, 1.0)))
            .children(now.map(|(n, a)| layer(n, a)))
            // Light under the pointer, gently darker on the far side.
            .when(glow > 0.003, |d| {
                d.child(light(gpui::linear_gradient(
                    angle,
                    gpui::linear_color_stop(white(0.16 * glow * (0.45 + 0.55 * reach)), 0.0),
                    gpui::linear_color_stop(white(0.0), 0.62),
                )))
                .child(light(gpui::linear_gradient(
                    angle,
                    gpui::linear_color_stop(black(0.0), 0.45),
                    gpui::linear_color_stop(black(0.14 * glow * reach), 1.0),
                )))
            })
            .into_any_element()
    }
}
