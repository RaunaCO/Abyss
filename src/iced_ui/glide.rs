//! Wheel glide, as in the GPUI interface: a notch becomes a short eased
//! movement in step with the screen. Iced's scrollable jumps once per notch.
//!
//! The wrapped widget keeps doing the scrolling: on each frame it is handed the
//! next piece of the distance as a pixel delta, so its limits, its `on_scroll`
//! and lists nested inside it behave as with a touchpad.

use std::time::Instant;

use iced::advanced::widget::{tree, Operation, Tree, Widget};
use iced::advanced::{layout, overlay, renderer, Clipboard, Layout, Shell};
use iced::{keyboard, mouse, window, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

use super::Message;

/// Distance of a wheel notch (3 lines of 33 px in GPUI).
pub(super) const NOTCH_PX: f32 = 99.0;
/// Time constant: 63 % of the distance is covered in this time.
const TAU_S: f32 = 0.075;

/// How much of `rest` to cover after `dt` seconds (also the lyrics' wheel).
pub(super) fn step_of(rest: f32, dt: f32, reduced: bool) -> f32 {
    if reduced || rest.abs() <= 0.5 {
        rest
    } else {
        rest * (1.0 - (-dt / TAU_S).exp())
    }
}

struct State {
    pending: f32,
    last: Instant,
    at: Point,
    shift: bool,
    /// The last nudge taken (see [`glide_nudged`]).
    nudged: u64,
    /// A nudge is waiting to learn where the list is (it scrolls at its centre).
    centre: bool,
}

struct Glide<'a> {
    content: Element<'a, Message>,
    nudge: Option<(u64, f32)>,
}

/// `content` (something that scrolls) with a gliding wheel.
pub fn glide<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(Glide { content: content.into(), nudge: None })
}

/// [`glide`] that also glides by `nudge.1` px (down for positive) each time
/// `nudge.0` changes, as a wheel notch would: the queue following the
/// playing track moves one row instead of jumping to it.
pub fn glide_nudged<'a>(content: impl Into<Element<'a, Message>>, nudge: (u64, f32)) -> Element<'a, Message> {
    Element::new(Glide { content: content.into(), nudge: Some(nudge) })
}

impl Widget<Message, Theme, Renderer> for Glide<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        let nudged = self.nudge.map_or(0, |(key, _)| key);
        tree::State::new(State { pending: 0.0, last: Instant::now(), at: Point::ORIGIN, shift: false, nudged, centre: false })
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        if let Some((key, by)) = self.nudge {
            let state = tree.state.downcast_mut::<State>();
            if state.nudged != key {
                state.nudged = key;
                // A wheel's pixels: positive scrolls up.
                state.pending = -by;
                state.last = Instant::now();
                state.centre = true;
            }
        }
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
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
        let state = tree.state.downcast_mut::<State>();
        if let Event::Keyboard(keyboard::Event::ModifiersChanged(mods)) = event {
            state.shift = mods.shift();
        }
        if let Event::Mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x, y } }) = event {
            if !shell.is_event_captured() && !state.shift && *y != 0.0 && y.abs() >= x.abs() {
                if let Some(at) = cursor.position_over(layout.bounds()) {
                    let distance = y * NOTCH_PX;
                    // Turning round cancels the rest of the previous glide.
                    if state.pending == 0.0 || state.pending.signum() != distance.signum() {
                        state.pending = 0.0;
                        state.last = Instant::now();
                    }
                    state.pending += distance;
                    state.at = at;
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
            }
        }
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);

        let Event::Window(window::Event::RedrawRequested(now)) = event else { return };
        if state.pending == 0.0 {
            return;
        }
        if std::mem::take(&mut state.centre) {
            state.at = layout.bounds().center();
        }
        let dt = now.saturating_duration_since(state.last).as_secs_f32().min(0.05);
        state.last = state.last.max(*now);
        let step = step_of(state.pending, dt, crate::shared::lyrics::reduced_motion());
        if step != 0.0 {
            // A shell of its own: a capture of this made-up event must not count
            // as a capture of the redraw, which the other widgets still need.
            let mut messages = Vec::new();
            let mut inner = Shell::new(&mut messages);
            let delta = mouse::ScrollDelta::Pixels { x: 0.0, y: step };
            let moved = Event::Mouse(mouse::Event::WheelScrolled { delta });
            let pointer = mouse::Cursor::Available(state.at);
            self.content.as_widget_mut().update(&mut tree.children[0], &moved, layout, pointer, renderer, clipboard, &mut inner, viewport);
            let (taken, relayout) = (inner.is_event_captured(), inner.is_layout_invalid());
            if relayout {
                shell.invalidate_layout();
            }
            for message in messages {
                shell.publish(message);
            }
            // Not taken: an end was reached, drop the rest.
            state.pending = if taken { state.pending - step } else { 0.0 };
            if state.pending.abs() < 0.01 {
                state.pending = 0.0;
            }
        }
        shell.request_redraw();
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
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
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
    use super::{step_of, NOTCH_PX};

    #[test]
    fn a_notch_glides_its_whole_distance_at_any_refresh_rate() {
        for hz in [60.0_f32, 144.0, 240.0] {
            let (mut rest, mut moved, mut frames) = (NOTCH_PX, 0.0, 0);
            while rest != 0.0 && frames < 1000 {
                let step = step_of(rest, 1.0 / hz, false);
                assert!(step > 0.0);
                moved += step;
                rest -= step;
                frames += 1;
            }
            assert!((moved - NOTCH_PX).abs() < 0.01, "{hz}: {moved}");
            assert!((frames as f32 / hz) < 0.5, "{hz}: {frames}");
        }
    }

    #[test]
    fn reduced_motion_jumps_the_whole_distance() {
        assert_eq!(step_of(-NOTCH_PX, 0.001, true), -NOTCH_PX);
    }
}
