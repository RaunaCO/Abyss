//! `ABYSS_PERF=1` (or `TIDE_PERF=1`): once a second, log what the interface
//! did — views rebuilt and what they cost, frames drawn (each one presented),
//! the messages that caused the rebuilds, and *who asked for the frames*.
//!
//! Frames are asked for by widgets (`Shell::request_redraw[_at]`) during the
//! `RedrawRequested` pass. [`probe`] wraps a part of the view under a name and
//! records what that part asked for, isolated from its siblings; the root
//! probe counts the frames drawn. A line looks like:
//!
//! ```text
//! perf: 1.0 views/s (avg 2.10 ms, max 2.10) | 5.0 frames/s (draw avg 1.90 ms) | msgs Tick=1 |
//!       frames asked by player_bar=5 (at ~200 ms) | driver player_bar=5 | idle poll 1000 ms ...
//! ```
//!
//! - `frames asked by X=n (next k, at ~t ms)`: in n of the frames drawn, X
//!   asked for another one (k of them for the very next vsync, the rest
//!   at a time, t the shortest delay asked).
//! - `driver X=n`: X asked for the earliest redraw in n frames, i.e. X is what
//!   keeps the window drawing. `(unnamed)` is a part of the view outside every probe.
//!
//! With the variable unset nothing is wrapped and nothing is counted: [`probe`]
//! returns the element unchanged and every note is one relaxed load.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{overlay, renderer, Clipboard, Shell, Widget};
use iced::window::RedrawRequest;
use iced::{mouse, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

/// 0 unknown, 1 off, 2 on.
static STATE: AtomicU8 = AtomicU8::new(0);

pub fn enabled() -> bool {
    match STATE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let on = std::env::var_os("ABYSS_PERF").is_some() || std::env::var_os("TIDE_PERF").is_some();
            STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
    }
}

/// For tests: count whatever the environment says.
#[cfg(test)]
pub fn force(on: bool) {
    STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
}

#[derive(Default)]
struct Ask {
    name: &'static str,
    frames: u32,
    next: u32,
    min_ms: Option<f32>,
    driver: u32,
}

struct Acc {
    start: Instant,
    views: u32,
    view_ms: f64,
    view_max: f64,
    frames: u32,
    draw_ms: f64,
    draw_max: f64,
    messages: Vec<(String, u32)>,
    asks: Vec<Ask>,
    /// What each probe asked for in the frame being handled.
    frame: Vec<(&'static str, RedrawRequest)>,
    /// What the App reported at its last view (state that explains the numbers).
    context: String,
}

impl Default for Acc {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            views: 0,
            view_ms: 0.0,
            view_max: 0.0,
            frames: 0,
            draw_ms: 0.0,
            draw_max: 0.0,
            messages: Vec::new(),
            asks: Vec::new(),
            frame: Vec::new(),
            context: String::new(),
        }
    }
}

thread_local!(static ACC: RefCell<Acc> = RefCell::new(Acc::default()));

// Every ask since the last `take_asks`, whatever the log already printed (tests).
#[cfg(test)]
thread_local!(static TOTAL: RefCell<Vec<(&'static str, u32, f32)>> = const { RefCell::new(Vec::new()) });

/// A message reached `App::update` (named by its variant).
pub fn note_message(kind: impl FnOnce() -> String) {
    if !enabled() {
        return;
    }
    let kind = kind();
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        match a.messages.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, n)) => *n += 1,
            None => a.messages.push((kind, 1)),
        }
    });
}

/// A view was built in `spent`; `context` describes the App's state.
pub fn note_view(spent: Duration, context: impl FnOnce() -> String) {
    if !enabled() {
        return;
    }
    let context = context();
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        let ms = spent.as_secs_f64() * 1000.0;
        a.views += 1;
        a.view_ms += ms;
        a.view_max = a.view_max.max(ms);
        a.context = context;
    });
    flush_if_due();
}

/// The variant's name of a `Debug` value (`Chrome(Edge(East))` → `Chrome`).
pub fn variant(debug: &str) -> String {
    let end = debug.find(['(', ' ', '{']).unwrap_or(debug.len());
    debug[..end].to_string()
}

fn ms_until(request: RedrawRequest, now: Instant) -> Option<f32> {
    match request {
        RedrawRequest::NextFrame => Some(0.0),
        RedrawRequest::At(at) => Some(at.saturating_duration_since(now).as_secs_f32() * 1000.0),
        RedrawRequest::Wait => None,
    }
}

fn record_frame(total: RedrawRequest, now: Instant) {
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        let frame = std::mem::take(&mut a.frame);
        let earliest = frame.iter().map(|(_, r)| *r).min().unwrap_or(RedrawRequest::Wait);
        let mut entries = frame;
        // A request no probe made comes from a part of the view outside all of them.
        if total < earliest {
            entries.push(("(unnamed)", total));
        }
        let earliest = earliest.min(total);
        for (name, request) in entries {
            let Some(ms) = ms_until(request, now) else { continue };
            let i = match a.asks.iter().position(|x| x.name == name) {
                Some(i) => i,
                None => {
                    a.asks.push(Ask { name, ..Default::default() });
                    a.asks.len() - 1
                }
            };
            #[cfg(test)]
            TOTAL.with(|t| {
                let mut t = t.borrow_mut();
                match t.iter_mut().find(|x| x.0 == name) {
                    Some(x) => {
                        x.1 += 1;
                        x.2 = x.2.min(ms);
                    }
                    None => t.push((name, 1, ms)),
                }
            });
            let ask = &mut a.asks[i];
            ask.frames += 1;
            if request == RedrawRequest::NextFrame {
                ask.next += 1;
            }
            ask.min_ms = Some(ask.min_ms.map_or(ms, |m| m.min(ms)));
            if request == earliest {
                ask.driver += 1;
            }
        }
    });
}

fn note_draw(spent: Duration) {
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        let ms = spent.as_secs_f64() * 1000.0;
        a.frames += 1;
        a.draw_ms += ms;
        a.draw_max = a.draw_max.max(ms);
    });
    flush_if_due();
}

fn flush_if_due() {
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        let secs = a.start.elapsed().as_secs_f64();
        if secs < 1.0 {
            return;
        }
        log::info!("{}", line(&a, secs));
        let context = std::mem::take(&mut a.context);
        *a = Acc { context, ..Acc::default() };
    });
}

fn line(a: &Acc, secs: f64) -> String {
    use std::fmt::Write;
    let rate = |n: u32| n as f64 / secs;
    let mut s = format!(
        "perf: {:.1} views/s (avg {:.2} ms, max {:.2}) | {:.1} frames/s (draw avg {:.2} ms, max {:.2}) over {:.1} s | msgs",
        rate(a.views),
        if a.views > 0 { a.view_ms / a.views as f64 } else { 0.0 },
        a.view_max,
        rate(a.frames),
        if a.frames > 0 { a.draw_ms / a.frames as f64 } else { 0.0 },
        a.draw_max,
        secs,
    );
    let mut messages: Vec<_> = a.messages.iter().collect();
    messages.sort_by_key(|x| std::cmp::Reverse(x.1));
    for (k, n) in messages {
        let _ = write!(s, " {k}={n}");
    }
    s.push_str(" | frames asked by");
    let mut asks: Vec<_> = a.asks.iter().collect();
    asks.sort_by_key(|x| std::cmp::Reverse(x.frames));
    if asks.is_empty() {
        s.push_str(" nobody");
    }
    for x in &asks {
        let _ = write!(s, " {}={} (next {}, at ~{:.0} ms)", x.name, x.frames, x.next, x.min_ms.unwrap_or(0.0));
    }
    s.push_str(" | driver");
    for x in asks.iter().filter(|x| x.driver > 0) {
        let _ = write!(s, " {}={}", x.name, x.driver);
    }
    if !a.context.is_empty() {
        let _ = write!(s, " | {}", a.context);
    }
    s
}

// ------------------------------------------------------------------ probes

/// `content` under `name`: what it asks for is recorded (only with `ABYSS_PERF`).
pub fn probe<'a, Message: 'a>(
    name: &'static str,
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Element<'a, Message, Theme, Renderer> {
    let content = content.into();
    if !enabled() {
        return content;
    }
    Element::new(Probe { name, root: false, content })
}

/// The whole window: counts the frames drawn as well.
pub fn root<'a, Message: 'a>(content: Element<'a, Message, Theme, Renderer>) -> Element<'a, Message, Theme, Renderer> {
    if !enabled() {
        return content;
    }
    Element::new(Probe { name: "(root)", root: true, content })
}

struct Probe<'a, Message> {
    name: &'static str,
    root: bool,
    content: Element<'a, Message, Theme, Renderer>,
}

impl<Message> Widget<Message, Theme, Renderer> for Probe<'_, Message> {
    // Transparent to the tree: the probe has no state of its own, so turning
    // the log on does not change what Iced keeps between frames.
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
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
        let t0 = Instant::now();
        self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
        if self.root {
            note_draw(t0.elapsed());
        }
    }
    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(tree, layout, renderer, operation);
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
        let Event::Window(iced::window::Event::RedrawRequested(now)) = event else {
            self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
            return;
        };
        if self.root {
            ACC.with(|a| a.borrow_mut().frame.clear());
        }
        // Isolated from what the siblings asked for, then merged back.
        let mut messages = Vec::new();
        let mut inner = Shell::new(&mut messages);
        self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, &mut inner, viewport);
        let asked = inner.redraw_request();
        shell.merge(inner, |m| m);
        if self.root {
            record_frame(asked, *now);
        } else if asked != RedrawRequest::Wait {
            ACC.with(|a| a.borrow_mut().frame.push((self.name, asked)));
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation)
    }
}

/// What the probes saw in one frame, for tests (name, ms until the redraw asked; 0 = next vsync).
#[cfg(test)]
pub fn take_asks() -> Vec<(&'static str, u32, f32)> {
    TOTAL.with(|t| std::mem::take(&mut *t.borrow_mut()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_names_drop_their_payload() {
        assert_eq!(variant("Chrome(Edge(East))"), "Chrome");
        assert_eq!(variant("Tick"), "Tick");
        assert_eq!(variant("Done(Callback)"), "Done");
    }

    #[test]
    fn the_earliest_request_drives_the_frame() {
        let now = Instant::now();
        ACC.with(|a| {
            let mut a = a.borrow_mut();
            a.asks.clear();
            a.frame = vec![("slow", RedrawRequest::At(now + Duration::from_millis(50))), ("fast", RedrawRequest::NextFrame)];
        });
        record_frame(RedrawRequest::NextFrame, now);
        ACC.with(|a| {
            let a = a.borrow();
            let fast = a.asks.iter().find(|x| x.name == "fast").unwrap();
            let slow = a.asks.iter().find(|x| x.name == "slow").unwrap();
            assert_eq!((fast.driver, fast.next), (1, 1));
            assert_eq!((slow.driver, slow.frames), (0, 1));
            assert!(a.asks.iter().all(|x| x.name != "(unnamed)"));
        });
        // Something outside every probe asked sooner.
        ACC.with(|a| a.borrow_mut().frame = vec![("slow", RedrawRequest::At(now + Duration::from_millis(50)))]);
        record_frame(RedrawRequest::At(now + Duration::from_millis(10)), now);
        ACC.with(|a| assert!(a.borrow().asks.iter().any(|x| x.name == "(unnamed)" && x.driver == 1)));
    }
}
