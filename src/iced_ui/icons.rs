//! Abyss's icon family as Iced SVG handles (see `crate::art`).

use std::collections::HashMap;
use std::sync::OnceLock;

use iced::widget::svg::{self, Handle};
use iced::{Color, Element, Length};
use parking_lot::Mutex;

pub use crate::art::Icon;

/// The SVG handle for `icon` drawn `size` design px tall (its stroke is picked
/// for that size), turned `deg` degrees. Handles are cached, so Iced keeps
/// the rasterised glyph.
pub fn handle(icon: Icon, size: f32, deg: f32) -> Handle {
    #[allow(clippy::type_complexity)]
    static CACHE: OnceLock<Mutex<HashMap<(&'static str, u32, i32), Handle>>> = OnceLock::new();
    let stroke = crate::art::stroke_for(size);
    let deg_key = (deg * 10.0).round() as i32;
    let key = (icon.name(), stroke, deg_key);
    let mut cache = CACHE.get_or_init(Default::default).lock();
    cache
        .entry(key)
        .or_insert_with(|| {
            let doc = crate::art::document(icon.name(), stroke as f32 / 10.0, deg).unwrap_or_default();
            Handle::from_memory(doc.into_bytes())
        })
        .clone()
}

/// The icon as an element, `size` design px square, tinted `color`.
pub fn icon<'a, M: 'a>(icon: Icon, size: f32, color: Color) -> Element<'a, M> {
    turned(icon, size, color, 0.0)
}

/// The icon turned `deg` degrees (the spinning sync glyph), in 7.5 degree steps.
pub fn turned<'a, M: 'a>(icon: Icon, size: f32, color: Color, deg: f32) -> Element<'a, M> {
    let deg = (deg / 7.5).round() * 7.5;
    svg::Svg::new(handle(icon, size, deg.rem_euclid(360.0)))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_, _| svg::Style { color: Some(super::wash::faded(color)) })
        .into()
}

// ------------------------------------------------------------------ glyph

use std::time::Instant;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{tree, Tree, Widget};
use iced::advanced::{mouse, renderer, Clipboard, Shell};
use iced::{window, Event, Rectangle, Size};

use super::motion;
use super::wash::{phase, Phase};

/// An icon drawn by hand, so everything about it can change between frames
/// without rebuilding the view (GPUI `el_scaled` and `icons::swap`):
/// its colour and paint scale can follow the enclosing transition
/// ([`super::wash::phase`]), it can pop (the heart after a like) and, when the
/// glyph changes (play / pause, heart, repeat), the old one dissolves while the
/// new one resolves.
pub struct Glyph<'a> {
    icon: Icon,
    size: f32,
    color: Color,
    live: Option<Box<dyn Fn(Phase, Color) -> Color + 'a>>,
    scale: Option<Box<dyn Fn(Phase) -> f32 + 'a>>,
    swap: bool,
    clock: Option<Clock>,
}

/// A timed effect: from `at` over `len`, `f(t)` gives (scale, opacity).
type Clock = (Instant, std::time::Duration, fn(f32) -> (f32, f32));

/// `icon` as a [`Glyph`], `size` design px square.
pub fn glyph<'a>(icon: Icon, size: f32, color: Color) -> Glyph<'a> {
    Glyph { icon, size, color, live: None, scale: None, swap: false, clock: None }
}

/// The big heart over the Now Playing cover after a double-click (GPUI
/// `haptics::heart_overlay`): it pops past full size, settles, then fades.
pub fn burst_curve(t: f32) -> (f32, f32) {
    let s = if t < 0.25 { 1.25 * (1.0 - (1.0 - t / 0.25).powi(3)) } else { 1.0 + 0.25 * (1.0 - ((t - 0.25) / 0.2).min(1.0)) };
    let alpha = if t < 0.6 { 1.0 } else { (1.0 - (t - 0.6) / 0.4).max(0.0) };
    (s, if t >= 1.0 { 0.0 } else { alpha })
}

/// GPUI's `AMBIENT_FADE`, the length of the heart burst.
pub const BURST: std::time::Duration = motion::AMBIENT;

impl<'a> Glyph<'a> {
    /// The colour when drawn, from the enclosing transition and the base colour.
    pub fn live(mut self, f: impl Fn(Phase, Color) -> Color + 'a) -> Self {
        self.live = Some(Box::new(f));
        self
    }
    /// A paint scale from the enclosing transition (GPUI `el_scaled`).
    pub fn scale(mut self, f: impl Fn(Phase) -> f32 + 'a) -> Self {
        self.scale = Some(Box::new(f));
        self
    }
    /// Cross-fade to a new glyph when the icon changes (GPUI `icons::swap`).
    pub fn swap(mut self) -> Self {
        self.swap = true;
        self
    }
    /// The heart pop that started at `at` (GPUI `heart_scale`).
    pub fn pop(mut self, at: Option<Instant>) -> Self {
        self.clock = at.map(|at| (at, motion::HEART_POP, (|t| (motion::heart_scale(t), 1.0)) as fn(f32) -> (f32, f32)));
        self
    }
    /// The double-click heart burst that started at `at` (gone once it ends).
    pub fn burst(mut self, at: Instant) -> Self {
        self.clock = Some((at, BURST, burst_curve));
        self
    }
}

/// The glyph on screen and, while changing, the one before and when it began.
struct Shown {
    icon: Icon,
    prev: Option<(Icon, Instant)>,
}

/// How long one glyph takes to give way to another (GPUI `icons::SWAP`).
const SWAP: std::time::Duration = motion::CONTROL;

impl Glyph<'_> {
    fn busy(&self, shown: &Shown, now: Instant) -> bool {
        shown.prev.is_some_and(|(_, at)| now < at + SWAP) || self.clock.is_some_and(|(at, len, _)| now < at + len)
    }
}

/// The glyph that gives way and when the change began, once `icon` replaces
/// `shown` at `now`. Changing back midway (play, pause, play) turns the
/// cross-fade round from the mix on screen instead of starting over: the
/// curve is symmetric, so running it from the mirrored point continues
/// every glyph's opacity and size exactly.
fn retarget(shown: Icon, prev: Option<(Icon, Instant)>, icon: Icon, now: Instant) -> (Icon, Instant) {
    match prev {
        Some((old, at)) if old == icon && now < at + SWAP => {
            let left = SWAP.saturating_sub(now.saturating_duration_since(at));
            (shown, now.checked_sub(left).unwrap_or(now))
        }
        _ => (shown, now),
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Glyph<'_>
where
    Renderer: iced::advanced::svg::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Shown>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Shown { icon: self.icon, prev: None })
    }

    fn diff(&self, tree: &mut Tree) {
        let shown = tree.state.downcast_mut::<Shown>();
        if shown.icon != self.icon {
            shown.prev = self.swap.then(|| retarget(shown.icon, shown.prev, self.icon, Instant::now()));
            shown.icon = self.icon;
        }
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.size), Length::Fixed(self.size))
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::Node::new(limits.resolve(self.size, self.size, Size::new(self.size, self.size)))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let shown = tree.state.downcast_mut::<Shown>();
            if self.busy(shown, *now) {
                shell.request_redraw();
            } else {
                shown.prev = None;
            }
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let shown = tree.state.downcast_ref::<Shown>();
        let now = Instant::now();
        let p = phase();
        let color = super::wash::faded(self.live.as_ref().map_or(self.color, |f| f(p, self.color)));
        // Reduced motion: the glyph keeps its size (no press dip, no pop) but
        // still fades, so the heart burst and the swaps are not lost.
        let still = motion::reduced();
        let mut scale = if still { 1.0 } else { self.scale.as_ref().map_or(1.0, |f| f(p)) };
        let mut fade = 1.0;
        if let Some((at, len, f)) = self.clock {
            let (s, a) = f(motion::progress(at, now, len));
            scale *= if still { 1.0 } else { s };
            fade = a;
        }
        let color = Color { a: color.a * fade, ..color };
        if color.a <= 0.002 {
            return;
        }
        let bounds = layout.bounds();
        let c = bounds.center();
        let mut draw = |icon: Icon, alpha: f32, k: f32| {
            let alpha = alpha.clamp(0.0, 1.0);
            if alpha <= 0.002 {
                return;
            }
            let side = self.size * k;
            let rect = Rectangle { x: c.x - side / 2.0, y: c.y - side / 2.0, width: side, height: side };
            let svg = iced::advanced::svg::Svg::new(handle(icon, self.size, 0.0)).color(Color { a: color.a * alpha, ..color });
            renderer.draw_svg(svg, rect, *viewport);
        };
        let swapping = shown.prev.filter(|(_, at)| now < *at + SWAP);
        let Some((old, at)) = swapping else {
            draw(shown.icon, 1.0, scale);
            return;
        };
        // Smoothstep: both glyphs ease at each end, so neither pops. Two faint
        // copies, a little larger and a little smaller, stand in for a blur.
        let s = motion::smoothstep(motion::progress(at, now, SWAP));
        if still {
            draw(old, 1.0 - s, scale);
            draw(shown.icon, s, scale);
            return;
        }
        let haze = (std::f32::consts::PI * s).sin();
        for (g, alpha, k) in [(old, 1.0 - s, 1.0 - 0.3 * s), (shown.icon, s, 0.7 + 0.3 * s)] {
            draw(g, alpha * haze * 0.32, scale * k * 1.16);
            draw(g, alpha * haze * 0.32, scale * k * 0.86);
            draw(g, alpha * (1.0 - 0.45 * haze), scale * k);
        }
    }
}

impl<'a, Message: 'a, Theme: 'a, Renderer> From<Glyph<'a>> for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::svg::Renderer + 'a,
{
    fn from(g: Glyph<'a>) -> Self {
        Element::new(g)
    }
}

#[cfg(test)]
mod glyph_tests {
    use super::{burst_curve, retarget, Icon, SWAP};
    use crate::iced_ui::motion::{progress, smoothstep};
    use std::time::Instant;

    #[test]
    fn turning_a_swap_back_midway_continues_from_the_mix_on_screen() {
        let t0 = Instant::now();
        let (play, pause) = (Icon::Play, Icon::Pause);
        // Play gives way to pause...
        let (old, at) = retarget(play, None, pause, t0);
        assert_eq!((old, at), (play, t0));
        let mid = t0 + SWAP / 4;
        let pause_alpha = smoothstep(progress(at, mid, SWAP));
        // ...and back to play a quarter of the way in.
        let (old2, at2) = retarget(pause, Some((old, at)), play, mid);
        assert_eq!(old2, pause);
        let play_alpha = smoothstep(progress(at2, mid, SWAP));
        assert!((play_alpha - (1.0 - pause_alpha)).abs() < 1e-3, "{play_alpha} vs {pause_alpha}");
        // A change to something else starts a fresh swap.
        assert_eq!(retarget(pause, Some((old, at)), Icon::Heart, mid).1, mid);
    }

    #[test]
    fn burst_pops_past_full_size_then_fades_out() {
        assert_eq!(burst_curve(0.0), (0.0, 1.0));
        let (peak, a) = burst_curve(0.25);
        assert!((peak - 1.25).abs() < 1e-5 && a == 1.0);
        let (settled, _) = burst_curve(0.5);
        assert!((settled - 1.0).abs() < 1e-5);
        assert!((burst_curve(0.8).1 - 0.5).abs() < 1e-5);
        assert_eq!(burst_curve(1.0).1, 0.0);
    }
}
