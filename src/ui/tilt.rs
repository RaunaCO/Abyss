//! The Now Playing cover follows the mouse, copied from the official app's
//! `tilt` component: up to 12° of tilt toward the pointer (strength fading to 0
//! within 15 % of the edges), a soft glare that sits opposite the pointer, all
//! driven by a spring (stiffness 200, damping 30), and back to rest on leave.
//!
//! GPUI has no 3D transforms, so the tilt is rendered with what it can do: the
//! cover shifts a little toward the near side, its shadow slides the other way,
//! the far side gets a shade, and the glare is a soft light blob.

use std::time::Instant;

use super::sc;
use gpui::{div, prelude::*, AnyElement, BoxShadow, Hsla, Rgba};

const MAX_TILT: f32 = 12.0;
const EDGE: f32 = 0.15;
const STIFFNESS: f32 = 200.0;
const DAMPING: f32 = 30.0;

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
        (self.target - self.x).abs() < 0.01 && self.v.abs() < 0.01
    }
}

/// rx, ry (degrees), glare position (0..1) and glare opacity.
pub struct Tilt {
    rx: Spring,
    ry: Spring,
    gx: Spring,
    gy: Spring,
    glow: Spring,
    last: Instant,
    moving: bool,
}

impl Default for Tilt {
    fn default() -> Self {
        Self {
            rx: Spring::at(0.0),
            ry: Spring::at(0.0),
            gx: Spring::at(0.5),
            gy: Spring::at(0.5),
            glow: Spring::at(0.0),
            last: Instant::now(),
            moving: false,
        }
    }
}

impl Tilt {
    /// Pointer at (x, y) as fractions of the cover (0..1).
    pub fn point(&mut self, x: f32, y: f32) {
        let edge = x.min(1.0 - x).min(y).min(1.0 - y).max(0.0);
        let f = (edge / EDGE).min(1.0);
        self.rx.target = (0.5 - y) * MAX_TILT * 2.0 * f;
        self.ry.target = (x - 0.5) * MAX_TILT * 2.0 * f;
        self.gx.target = 1.0 - x;
        self.gy.target = 1.0 - y;
        self.glow.target = f;
    }

    pub fn leave(&mut self) {
        self.rx.target = 0.0;
        self.ry.target = 0.0;
        self.glow.target = 0.0;
    }

    /// Advance the springs; true while still moving.
    pub fn step(&mut self) -> bool {
        let now = Instant::now();
        let mut dt = now.duration_since(self.last).as_secs_f32().min(0.05);
        self.last = now;
        // Small fixed sub-steps keep the spring stable at low frame rates.
        while dt > 0.0 {
            let h = dt.min(1.0 / 240.0);
            for s in [&mut self.rx, &mut self.ry, &mut self.gx, &mut self.gy, &mut self.glow] {
                s.step(h);
            }
            dt -= h;
        }
        self.moving = ![self.rx, self.ry, self.gx, self.gy, self.glow].iter().all(Spring::settled);
        self.moving
    }

    /// Offset (px) to move the cover by, toward the side that comes closer.
    pub fn shift(&self) -> (f32, f32) {
        (self.ry.x / MAX_TILT * 4.0, -self.rx.x / MAX_TILT * 4.0)
    }

    /// Shadow for the cover: it slides away from the tilt.
    pub fn shadow(&self, color: Rgba) -> Vec<BoxShadow> {
        let (dx, dy) = self.shift();
        vec![
            BoxShadow {
                color: Hsla::from(super::fx::with_alpha(color, 0.5)),
                offset: gpui::point(sc(-dx * 1.8), sc(18.0 - dy * 1.8)),
                blur_radius: sc(80.),
                spread_radius: sc(-6.),
            },
            BoxShadow {
                color: Hsla::from(gpui::rgba(0x00000099)),
                offset: gpui::point(sc(-dx * 1.2), sc(10.0 - dy * 1.2)),
                blur_radius: sc(30.),
                spread_radius: sc(0.),
            },
        ]
    }

    /// Shade on the far side and the glare, as layers over a `size` px cover.
    pub fn overlays(&self, size: f32) -> Vec<AnyElement> {
        let mut out = Vec::new();
        let g = self.glow.x.clamp(0.0, 1.0);
        if g > 0.01 {
            // Glare: radial white (0.15 at the centre, gone at 80 %), opposite the pointer.
            let d = size * 0.9;
            out.push(
                div()
                    .absolute()
                    .left(sc(self.gx.x * size - 3.0))
                    .top(sc(self.gy.x * size - 3.0))
                    .size(sc(6.))
                    .rounded(sc(3.))
                    .shadow(vec![BoxShadow {
                        color: Hsla::from(super::fx::with_alpha(gpui::rgb(0xffffff), 0.16 * g)),
                        offset: gpui::point(sc(0.), sc(0.)),
                        blur_radius: sc(d * 0.55),
                        spread_radius: sc(d * 0.18),
                    }])
                    .into_any_element(),
            );
        }
        out
    }
}

const PERSPECTIVE: f32 = 1000.0;
/// Grid cells per side: painted directly, so many are cheap.
const TILES: usize = 64;

impl Tilt {
    /// Is the cover visibly tilted (then the 3D version is drawn)?
    pub fn tilted(&self) -> bool {
        self.rx.x.abs() + self.ry.x.abs() > 0.05
    }

    /// The cover in real perspective, like CSS `perspective(1000px) rotateX() rotateY()`.
    /// GPUI cannot transform an image, so the surface is cut into a fine grid and
    /// painted directly (no layout): each cell draws the whole image, scaled and
    /// placed where the tilted surface projects it, clipped to the cell. Cells are
    /// small (under a pixel of error at 12°), so lines stay straight and edges clean.
    pub fn cover_3d(&self, image: gpui::ImageSource, size: f32) -> AnyElement {
        let (th, ph) = (self.rx.x.to_radians(), self.ry.x.to_radians());
        let z = super::zoom();
        gpui::canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let render = match &image {
                    gpui::ImageSource::Render(r) => r.clone(),
                    gpui::ImageSource::Image(i) => match i.clone().use_render_image(window, cx) {
                        Some(r) => r,
                        None => return,
                    },
                    _ => return,
                };
                let (st, ct, sp, cp) = (th.sin(), th.cos(), ph.sin(), ph.cos());
                let size_px = size * z;
                let half = size_px / 2.0;
                let persp = PERSPECTIVE * z;
                let (ox, oy) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
                let project = |u: f32, v: f32| {
                    let (x, y) = ((u - 0.5) * size_px, (v - 0.5) * size_px);
                    let (x1, z1) = (x * cp, -x * sp);
                    let (y2, z2) = (y * ct - z1 * st, y * st + z1 * ct);
                    let s = persp / (persp - z2);
                    (ox + x1 * s + half, oy + y2 * s + half, z2)
                };
                let n = TILES;
                let nf = n as f32;
                let grid: Vec<Vec<(f32, f32, f32)>> =
                    (0..=n).map(|j| (0..=n).map(|i| project(i as f32 / nf, j as f32 / nf)).collect()).collect();
                for j in 0..n {
                    for i in 0..n {
                        let (a, b, c, d) = (grid[j][i], grid[j][i + 1], grid[j + 1][i], grid[j + 1][i + 1]);
                        // Cell box from the shared edge lines (neighbours meet exactly).
                        let left = (a.0 + c.0) / 2.0;
                        let right = (b.0 + d.0) / 2.0;
                        let top = (a.1 + b.1) / 2.0;
                        let bottom = (c.1 + d.1) / 2.0;
                        let (w, h) = (right - left, bottom - top);
                        if w <= 0.0 || h <= 0.0 {
                            continue;
                        }
                        let (sx, sy) = (w * nf / size_px, h * nf / size_px);
                        let (u0, v0) = (i as f32 / nf, j as f32 / nf);
                        let image_bounds = gpui::Bounds::new(
                            gpui::point(gpui::px(left - u0 * size_px * sx), gpui::px(top - v0 * size_px * sy)),
                            gpui::size(gpui::px(size_px * sx), gpui::px(size_px * sy)),
                        );
                        // A hair of overlap so no background shows between cells.
                        let cell = gpui::Bounds::new(
                            gpui::point(gpui::px(left - 0.25), gpui::px(top - 0.25)),
                            gpui::size(gpui::px(w + 0.5), gpui::px(h + 0.5)),
                        );
                        let depth = (a.2 + b.2 + c.2 + d.2) / 4.0 / half;
                        let shade = (-depth * 0.26).clamp(0.0, 0.24);
                        window.with_content_mask(Some(gpui::ContentMask { bounds: cell }), |window| {
                            let _ = window.paint_image(image_bounds, gpui::Corners::default(), render.clone(), 0, false);
                            if shade > 0.004 {
                                window.paint_quad(gpui::fill(cell, super::fx::with_alpha(gpui::rgb(0x000000), shade)));
                            }
                        });
                    }
                }
            },
        )
        .absolute()
        .top(sc(0.))
        .left(sc(0.))
        .size(sc(size))
        .into_any_element()
    }
}
