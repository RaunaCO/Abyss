//! Visual effects: ambient colour from the playing cover, staggered entrances,
//! the "pop" of a liked heart, a pulse around the play button.

use super::sc;
use std::time::{Duration, Instant};

use gpui::{div, linear_color_stop, linear_gradient, prelude::*, px, Animation, AnimationExt, AnyElement, BoxShadow, Context, ElementId, Hsla, Rgba};

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
    linear_gradient(
        180.,
        linear_color_stop(with_alpha(color, strength), 0.),
        linear_color_stop(with_alpha(theme::bg(), 0.0), 1.),
    )
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
    let delay = (index as f32 * 0.07).min(0.6);
    div()
        .relative()
        .flex_none()
        .child(el)
        .with_animation(
            id,
            Animation::new(Duration::from_millis(520)).with_easing(delayed(delay)),
            |d, t| d.opacity(t).top(sc((1.0 - t) * 22.0)),
        )
        .into_any_element()
}

impl AppView {
    /// Colour of the playing cover, cross-faded when the track changes.
    pub fn ambient(&mut self, cx: &mut Context<Self>) -> Rgba {
        let target = self
            .now
            .as_ref()
            .and_then(|n| n.track.cover(640))
            .and_then(|u| images::color(&u, cx))
            .unwrap_or(theme::accent());
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
        .with_animation(
            "play-pulse",
            Animation::new(Duration::from_millis(1800)).repeat().with_easing(motion::ease_out),
            |d, t| {
                let grow = 18.0 * t;
                d.size(sc(40.0 + grow)).top(sc(-grow / 2.0)).left(sc(-grow / 2.0)).rounded(sc(20.0 + grow / 2.0)).opacity(1.0 - t)
            },
        )
        .into_any_element()
}
