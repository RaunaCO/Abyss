use std::time::{Duration, Instant};

use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{layout, overlay, renderer, Clipboard, Layout, Shell, Widget};
use iced::{mouse, window, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

use super::motion;

pub use motion::{CONTROL, PANEL};

#[derive(Clone, Copy)]
pub struct Reveal {
    from: f32,
    target: bool,
    started: Instant,
    duration: Duration,
}

impl Reveal {
    pub fn closed(duration: Duration) -> Self {
        Self { from: 0.0, target: false, started: Instant::now() - duration, duration }
    }

    pub fn set(&mut self, open: bool) {
        self.set_after(open, Duration::ZERO);
    }

    /// Head for `open`, but only start moving after `delay` (the sidebar
    /// folds 150 ms after the pointer left, GPUI's `CLOSE_DELAY`). Turning
    /// back during the delay leaves it where it is.
    pub fn set_after(&mut self, open: bool, delay: Duration) {
        if open != self.target {
            let now = Instant::now();
            self.from = self.progress(now, false);
            self.started = now + delay;
            self.target = open;
        }
    }

    pub fn visible(self) -> bool {
        self.target || self.fraction() > 0.0
    }

    pub fn fraction(self) -> f32 {
        self.progress(Instant::now(), crate::shared::lyrics::reduced_motion())
    }

    fn progress(self, now: Instant, reduced: bool) -> f32 {
        let target = f32::from(self.target);
        if reduced {
            return target;
        }
        let t = motion::progress(self.started, now, self.duration);
        self.from + (target - self.from) * motion::ease_out(t)
    }
}

/// A child laid out at the animated width itself, as GPUI's sidebar, whose
/// width changes continuously (rows, rounded corners and borders follow).
pub fn fluid<'a, Message: 'a>(child: Element<'a, Message>, motion: Reveal, closed: f32, expanded: f32) -> Element<'a, Message> {
    Element::new(Panel { child, motion, closed, expanded, mode: Mode::Fluid })
}

/// GPUI's side panel: the room opens with the eased fraction while the
/// fixed-width panel, anchored to the right edge, slides in with it and fades
/// with the same fraction (`.opacity(t)`). Iced has no group opacity: the fade
/// is drawn as a veil of the backdrop's colour `under`, which composes to
/// exactly the same pixels over a plain backdrop.
pub fn sliding<'a, Message: 'a>(child: Element<'a, Message>, motion: Reveal, expanded: f32, under: iced::Color) -> Element<'a, Message> {
    Element::new(Panel { child, motion, closed: 0.0, expanded, mode: Mode::Slide(under) })
}

#[derive(Clone, Copy)]
enum Mode {
    Fluid,
    Slide(iced::Color),
}

struct Panel<'a, Message> {
    child: Element<'a, Message>,
    motion: Reveal,
    closed: f32,
    expanded: f32,
    mode: Mode,
}

impl<Message> Widget<Message, Theme, Renderer> for Panel<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<bool>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(false)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.child.as_widget())]
    }

    fn diff(&self, tree: &mut Tree) {
        self.child.as_widget().diff(&mut tree.children[0]);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.closed + (self.expanded - self.closed) * self.motion.fraction()), Length::Fill)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let now = Instant::now();
        let reduced = crate::shared::lyrics::reduced_motion();
        let progress = self.motion.progress(now, reduced);
        *tree.state.downcast_mut::<bool>() = reduced || now >= self.motion.started + self.motion.duration;
        let width = Length::Fixed(self.closed + (self.expanded - self.closed) * progress);
        let visible = limits.resolve(width, Length::Fill, Size::ZERO);
        let inner = match self.mode {
            Mode::Fluid => visible,
            _ => Size::new(self.expanded, visible.height),
        };
        let mut child = self.child.as_widget_mut().layout(&mut tree.children[0], renderer, &layout::Limits::new(inner, inner));
        if let Mode::Slide(_) = self.mode {
            child = child.move_to(iced::Point::new(visible.width - inner.width, 0.0));
        }
        layout::Node::with_children(visible, vec![child])
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.child.as_widget_mut().operate(&mut tree.children[0], layout.child(0), renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if let Event::Window(window::Event::RedrawRequested(at)) = event {
            let end = self.motion.started + self.motion.duration;
            if !crate::shared::lyrics::reduced_motion() && *at < self.motion.started {
                // Waiting out a delay: one wake-up when it ends, no frames meanwhile.
                shell.request_redraw_at(self.motion.started);
            } else if !crate::shared::lyrics::reduced_motion() && *at < end {
                shell.invalidate_layout();
                // Next vsync, as GPUI: a fixed 8.3 ms timer beats against the
                // display's refresh and makes the slide judder.
                shell.request_redraw();
            } else if !*tree.state.downcast_ref::<bool>() {
                shell.invalidate_layout();
                shell.request_redraw();
            }
        }
        let Some(clip) = bounds.intersection(viewport) else { return };
        let cursor = if cursor.is_over(clip) { cursor } else { mouse::Cursor::Unavailable };
        self.child.as_widget_mut().update(&mut tree.children[0], event, layout.child(0), cursor, renderer, clipboard, shell, &clip);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        match layout.bounds().intersection(viewport).filter(|clip| cursor.is_over(*clip)) {
            Some(clip) => self.child.as_widget().mouse_interaction(&tree.children[0], layout.child(0), cursor, &clip, renderer),
            None => mouse::Interaction::None,
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let Some(clip) = layout.bounds().intersection(viewport) {
            let cursor = if cursor.is_over(clip) { cursor } else { mouse::Cursor::Unavailable };
            renderer::Renderer::with_layer(renderer, clip, |renderer| {
                self.child.as_widget().draw(&tree.children[0], renderer, theme, style, layout.child(0), cursor, &clip);
                if let Mode::Slide(under) = self.mode {
                    let veil = 1.0 - self.motion.fraction();
                    if veil > 0.002 {
                        renderer::Renderer::fill_quad(
                            renderer,
                            renderer::Quad { bounds: layout.bounds(), ..Default::default() },
                            iced::Color { a: under.a * veil, ..under },
                        );
                    }
                }
            });
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let clip = layout.bounds().intersection(viewport)?;
        self.child.as_widget_mut().overlay(&mut tree.children[0], layout.child(0), renderer, &clip, translation)
    }
}

/// Content drawn `distance` px to the side at `started`, easing back into
/// place over `duration` (GPUI's page entrance: `left((1 - t) * 26 * dir)`).
/// Only the drawing moves; layout and hit-testing stay where the page rests.
pub fn slide<'a, Message: 'a>(content: Element<'a, Message>, started: Instant, duration: Duration, distance: f32) -> Element<'a, Message> {
    Element::new(Slide { content, started, duration, distance })
}

struct Slide<'a, Message> {
    content: Element<'a, Message>,
    started: Instant,
    duration: Duration,
    distance: f32,
}

impl<Message> Slide<'_, Message> {
    fn offset(&self, now: Instant) -> f32 {
        // Reduced motion keeps the page's fade (see `pages::PageFade`) and drops the slide.
        if motion::reduced() {
            return 0.0;
        }
        (1.0 - motion::ease_out(motion::progress(self.started, now, self.duration))) * self.distance
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Slide<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(at)) = event {
            if self.offset(*at).abs() > 0.05 {
                shell.request_redraw();
            }
        }
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let dx = self.offset(Instant::now());
        if dx.abs() < 0.05 {
            self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
            return;
        }
        use iced::advanced::Renderer as _;
        renderer.with_layer(layout.bounds(), |renderer| {
            renderer.with_translation(Vector::new(dx, 0.0), |renderer| {
                self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
            });
        });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revealing_and_closing_follow_the_same_curve_and_reduced_motion_snaps() {
        let at = Instant::now();
        let open = Reveal { from: 0.0, target: true, started: at, duration: PANEL };
        assert_eq!(open.progress(at, false), 0.0);
        assert_eq!(open.progress(at, true), 1.0);
        let halfway = open.progress(at + PANEL / 2, false);
        assert!((halfway - 0.9375).abs() < 0.0001);
        let closing = Reveal { from: halfway, target: false, started: at, duration: PANEL };
        assert_eq!(closing.progress(at, false), halfway);
        assert_eq!(closing.progress(at + PANEL, false), 0.0);
        assert_eq!(closing.progress(at, true), 0.0);
    }
}
