//! The Now Playing cover reacts to the pointer with light only, as GPUI's
//! `src/ui/tilt.rs`: the picture stays perfectly still while a soft sheen
//! follows the pointer across it (and the far side darkens a little), driven
//! by springs (stiffness 200, damping 30) so the light glides and settles; it
//! fades away when the pointer leaves. Frames are asked for (on vsync) only
//! while the springs move.

use std::time::Instant;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{mouse, overlay, Clipboard, Shell, Widget};
use iced::{window, Background, Border, Color, Element, Event, Length, Rectangle, Size, Vector};

/// How strongly the light reacts toward the edges (0 at the very edge).
const EDGE: f32 = 0.15;
const STIFFNESS: f32 = 200.0;
const DAMPING: f32 = 30.0;

#[derive(Clone, Copy, Debug)]
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
    fn snap(&mut self) {
        self.x = self.target;
        self.v = 0.0;
    }
}

/// Pointer position over the cover (-1..1 from the centre) and light strength.
#[derive(Debug)]
struct Light {
    px: Spring,
    py: Spring,
    glow: Spring,
    last: Option<Instant>,
}

impl Default for Light {
    fn default() -> Self {
        Self { px: Spring::at(0.0), py: Spring::at(0.0), glow: Spring::at(0.0), last: None }
    }
}

impl Light {
    /// Pointer at (x, y) as fractions of the cover (0..1).
    fn point(&mut self, x: f32, y: f32) {
        let edge = x.min(1.0 - x).min(y).min(1.0 - y).max(0.0);
        self.px.target = (x - 0.5) * 2.0;
        self.py.target = (y - 0.5) * 2.0;
        self.glow.target = (edge / EDGE).min(1.0);
    }

    fn leave(&mut self) {
        self.glow.target = 0.0;
    }

    fn settled(&self) -> bool {
        [self.px, self.py, self.glow].iter().all(Spring::settled)
    }

    /// Advance the springs to `now`; true while still moving.
    fn step(&mut self, now: Instant) -> bool {
        if crate::shared::lyrics::reduced_motion() {
            for s in [&mut self.px, &mut self.py, &mut self.glow] {
                s.snap();
            }
            self.last = None;
            return false;
        }
        let mut dt = self.last.map_or(0.0, |l| now.saturating_duration_since(l).as_secs_f32()).min(0.05);
        self.last = Some(now);
        // Small fixed sub-steps keep the spring stable at low frame rates.
        while dt > 0.0 {
            let h = dt.min(1.0 / 240.0);
            for s in [&mut self.px, &mut self.py, &mut self.glow] {
                s.step(h);
            }
            dt -= h;
        }
        if self.settled() {
            self.last = None;
            false
        } else {
            true
        }
    }

    /// The two gradients (light under the pointer, shade on the far side).
    fn gradients(&self) -> Option<[Background; 2]> {
        let glow = self.glow.x.clamp(0.0, 1.0);
        if glow <= 0.003 {
            return None;
        }
        let (x, y) = (self.px.x.clamp(-1.0, 1.0), self.py.x.clamp(-1.0, 1.0));
        // Direction from the pointer toward the opposite edge (CSS angles: 0 = up).
        let angle = if x.abs() + y.abs() > 0.001 { (-x).atan2(y).to_degrees() } else { 180.0 };
        let reach = (x * x + y * y).sqrt().min(1.0);
        let white = |a: f32| Color { a, ..Color::WHITE };
        let black = |a: f32| Color { a, ..Color::BLACK };
        let linear = |stops: [(f32, Color); 2]| {
            let mut g = iced::gradient::Linear::new(iced::Radians(angle.to_radians()));
            for (at, c) in stops {
                g = g.add_stop(at, c);
            }
            Background::Gradient(iced::Gradient::Linear(g))
        };
        Some([
            linear([(0.0, white(0.16 * glow * (0.45 + 0.55 * reach))), (0.62, white(0.0))]),
            linear([(0.45, black(0.0)), (1.0, black(0.14 * glow * reach))]),
        ])
    }
}

/// `content` (the cover) with the pointer light over it, `radius` its corners.
pub fn sheen<'a, Message: 'a>(content: impl Into<Element<'a, Message>>, radius: f32) -> Element<'a, Message> {
    Element::new(Sheen { content: content.into(), radius })
}

struct Sheen<'a, Message> {
    content: Element<'a, Message>,
    radius: f32,
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Sheen<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Light>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Light::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        {
            let light = tree.state.downcast_mut::<Light>();
            let bounds = layout.bounds();
            let was_settled = light.settled();
            match event {
                Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
                    match cursor.position_over(bounds) {
                        Some(p) if bounds.width > 0.0 && bounds.height > 0.0 => {
                            light.point((p.x - bounds.x) / bounds.width, (p.y - bounds.y) / bounds.height)
                        }
                        _ => light.leave(),
                    }
                    if was_settled && !light.settled() {
                        light.last = Some(Instant::now());
                        shell.request_redraw();
                    }
                }
                Event::Window(window::Event::RedrawRequested(now)) if light.step(*now) => shell.request_redraw(),
                _ => {}
            }
        }
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
        let Some(layers) = tree.state.downcast_ref::<Light>().gradients() else { return };
        use iced::advanced::Renderer as _;
        for bg in layers {
            renderer.fill_quad(
                Quad { bounds: layout.bounds(), border: Border { radius: self.radius.into(), ..Default::default() }, ..Default::default() },
                bg,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn light_follows_the_pointer_and_settles() {
        let mut l = Light::default();
        assert!(l.settled() && l.gradients().is_none());
        l.point(0.5, 0.5);
        let t0 = Instant::now();
        l.step(t0);
        let mut t = t0;
        for _ in 0..240 {
            t += Duration::from_millis(8);
            if !l.step(t) {
                break;
            }
        }
        assert!(l.settled(), "springs settle within two seconds");
        assert!((l.glow.x - 1.0).abs() < 0.01);
        assert!(l.gradients().is_some());
        l.leave();
        for _ in 0..240 {
            t += Duration::from_millis(8);
            l.step(t);
        }
        assert!(l.gradients().is_none());
    }

    #[test]
    fn edges_dim_the_light() {
        let mut l = Light::default();
        l.point(0.0, 0.5);
        assert_eq!(l.glow.target, 0.0);
        l.point(0.075, 0.5);
        assert!((l.glow.target - 0.5).abs() < 1e-4);
    }
}
