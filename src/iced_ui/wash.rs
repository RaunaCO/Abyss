//! Eased hover, press and selection for any surface, as GPUI's
//! `interact::hover` / `interact::press` / `interact::follow`: the values glide
//! with GPUI's timings (hover in 91 ms, out 143 ms; press in 52 ms, out
//! 143 ms; selection 143 ms) and its quartic ease-out, instead of switching on
//! the frame the pointer arrives as Iced's `Status` does.
//!
//! The wrapper watches the cursor, paints the surface under its content (a
//! wash that fades in, or any look computed from the eased values) and, while
//! drawing the content, publishes those values through [`phase`], so style
//! closures, glyphs and text inside (which Iced evaluates at draw time) can
//! fade their colours and scale with the same transition. It asks for frames
//! (on vsync) only while something moves: nothing is drawn at rest.

use std::cell::Cell;
use std::time::Instant;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{mouse, overlay, Clipboard, Shell, Widget};
use iced::{window, Background, Border, Color, Element, Event, Length, Rectangle, Shadow, Size, Transformation, Vector};

use super::motion::{self, Anim};

/// The eased interaction values of the surface being drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Phase {
    /// 0..1 pointer over it.
    pub hover: f32,
    /// 0..1 button held on it.
    pub press: f32,
    /// 0..1 selected / on (follows [`Wash::active`]).
    pub active: f32,
    /// Size factor of the press spring (1 at rest), see [`Wash::pulse`].
    pub pulse: f32,
}

impl Phase {
    pub const REST: Phase = Phase { hover: 0.0, press: 0.0, active: 0.0, pulse: 1.0 };
}

impl Default for Phase {
    fn default() -> Self {
        Self::REST
    }
}

thread_local! {
    static CURRENT: Cell<Phase> = const { Cell::new(Phase::REST) };
    static FADE: Cell<f32> = const { Cell::new(1.0) };
}

/// Opacity of the floating surface being drawn (1 outside an [`enter`]).
/// Iced has no group opacity, so the pieces that make up menus, dialogs and
/// panels (glyphs, lines of text, washes, their own style closures) multiply
/// their colours by it: together they fade in like GPUI's `motion::enter`.
pub fn fade() -> f32 {
    FADE.with(Cell::get)
}

/// `c` at the current [`fade`].
pub fn faded(c: Color) -> Color {
    let f = fade();
    if f >= 1.0 {
        c
    } else {
        Color { a: c.a * f, ..c }
    }
}

/// The eased values of the innermost [`Wash`] being drawn (rest outside one).
/// Meant for style closures, which Iced calls while drawing.
pub fn phase() -> Phase {
    CURRENT.with(Cell::get)
}

/// Run `f` with `p` published as the current phase.
fn with_phase<R>(p: Phase, f: impl FnOnce() -> R) -> R {
    let prev = CURRENT.with(|c| c.replace(p));
    let out = f();
    CURRENT.with(|c| c.set(prev));
    out
}

/// Linear blend of two colours.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
}

/// A surface as painted for a given phase.
#[derive(Clone, Copy, Debug, Default)]
pub struct Look {
    pub background: Option<Background>,
    pub border: Border,
    pub shadow: Shadow,
}

type Paint<'a> = Box<dyn Fn(Phase) -> Look + 'a>;

pub struct Wash<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    /// Fill at full hover and the extra fill while pressed (alpha of `ink`).
    hover: Color,
    press: Color,
    rest: Color,
    radius: f32,
    strip: Option<Color>,
    paint: Option<Paint<'a>>,
    active: bool,
    pulse: bool,
}

struct State {
    hover: Anim,
    press: Anim,
    active: Anim,
    pulse: Option<Instant>,
    /// The surface springs ([`Wash::pulse`]): while held it sinks with the press.
    springy: bool,
    over: bool,
    down: bool,
}

impl State {
    fn phase(&self, now: Instant) -> Phase {
        let pulse = match self.pulse {
            _ if motion::reduced() => 1.0,
            Some(at) => motion::press_spring(motion::progress(at, now, motion::CONTROL)),
            // Held: the button sinks as the press comes in (52 ms) rather than
            // only after the release, so the finger gets an answer at once.
            None if self.springy => 1.0 - motion::SPRING_DEPTH * self.press.value_at(now).clamp(0.0, 1.0),
            None => 1.0,
        };
        Phase {
            hover: self.hover.value_at(now).clamp(0.0, 1.0),
            press: self.press.value_at(now).clamp(0.0, 1.0),
            active: self.active.value_at(now).clamp(0.0, 1.0),
            pulse,
        }
    }

    fn moving(&self, now: Instant) -> bool {
        !self.hover.done_at(now)
            || !self.press.done_at(now)
            || !self.active.done_at(now)
            || self.pulse.is_some_and(|at| !motion::reduced() && now < at + motion::CONTROL)
    }
}

/// `content` over a wash that fades to `hover` under the pointer.
pub fn wash<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    hover: Color,
    radius: f32,
) -> Wash<'a, Message, Theme, Renderer> {
    Wash {
        content: content.into(),
        hover,
        press: Color::TRANSPARENT,
        rest: Color::TRANSPARENT,
        radius,
        strip: None,
        paint: None,
        active: false,
        pulse: false,
    }
}

/// Only track the pointer: nothing is painted, but everything inside can read
/// the eased values through [`phase`].
pub fn track<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Wash<'a, Message, Theme, Renderer> {
    wash(content, Color::TRANSPARENT, 0.0)
}

/// `content` over a surface painted by `paint` from the eased values.
pub fn surface<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    paint: impl Fn(Phase) -> Look + 'a,
) -> Wash<'a, Message, Theme, Renderer> {
    let mut w = wash(content, Color::TRANSPARENT, 0.0);
    w.paint = Some(Box::new(paint));
    w
}

impl<'a, Message, Theme, Renderer> Wash<'a, Message, Theme, Renderer> {
    /// Extra fill added while the button is held.
    pub fn press(mut self, c: Color) -> Self {
        self.press = c;
        self
    }
    /// Fill when the pointer is away.
    pub fn rest(mut self, c: Color) -> Self {
        self.rest = c;
        self
    }
    /// An accent strip on the left edge that grows with the hover.
    pub fn strip(mut self, c: Color) -> Self {
        self.strip = Some(c);
        self
    }
    /// Selected / on: [`Phase::active`] glides to 1 over GPUI's `CONTROL`
    /// when this changes between views (it starts settled the first time).
    pub fn active(mut self, on: bool) -> Self {
        self.active = on;
        self
    }
    /// Give a springy press "pop" on click ([`Phase::pulse`], GPUI's `pulse`).
    pub fn pulse(mut self) -> Self {
        self.pulse = true;
        self
    }
}

/// The default wash: `rest` to `hover` with the hover, plus `press` while held.
fn fill(rest: Color, hover: Color, press: Color, p: Phase) -> Color {
    let mut fill = mix(rest, hover, p.hover);
    if p.press > 0.0 && press.a > 0.0 {
        let base = if fill.a > 0.0 { fill } else { Color { a: 0.0, ..press } };
        fill = mix(base, Color { a: (base.a + press.a).min(1.0), ..press }, p.press);
    }
    fill
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Wash<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            hover: Anim::still(0.0),
            press: Anim::still(0.0),
            active: Anim::still(f32::from(u8::from(self.active))),
            pulse: None,
            springy: self.pulse,
            over: false,
            down: false,
        })
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        if tree.tag == tree::Tag::of::<State>() {
            let state = tree.state.downcast_mut::<State>();
            state.active.go(f32::from(u8::from(self.active)), motion::CONTROL, Instant::now());
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
        {
            let state = tree.state.downcast_mut::<State>();
            let now = Instant::now();
            state.springy = self.pulse;
            if matches!(event, Event::Mouse(_) | Event::Keyboard(_) | Event::Touch(_)) {
                // Someone is using the window: the ambient drift keeps going.
                motion::note_input();
            }
            let over = cursor.is_over(layout.bounds()) && cursor.is_over(*viewport);
            if over != state.over {
                state.over = over;
                shell.request_redraw();
                if over {
                    state.hover.go(1.0, motion::HOVER_IN, now);
                } else {
                    state.hover.go(0.0, motion::HOVER_OUT, now);
                    state.press.go(0.0, motion::PRESS_OUT, now);
                    state.down = false;
                }
            }
            match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if over => {
                    state.down = true;
                    // A new press takes over from a spring still settling.
                    state.pulse = None;
                    // Immediate: the frame after the click already shows most of the press.
                    state.press.go_ahead(1.0, motion::PRESS_IN, now, motion::PRESS_LEAD);
                    shell.request_redraw();
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    if state.down {
                        shell.request_redraw();
                    }
                    if state.down && over && self.pulse {
                        // Spring back from how far it had sunk while held.
                        let depth = state.press.value_at(now).clamp(0.0, 1.0);
                        let back = motion::CONTROL.mul_f32(motion::spring_resume(depth));
                        state.pulse = Some(now.checked_sub(back).unwrap_or(now));
                    }
                    state.down = false;
                    state.press.go(0.0, motion::PRESS_OUT, now);
                }
                _ => {}
            }
            let at = match event {
                Event::Window(window::Event::RedrawRequested(at)) => *at,
                _ => now,
            };
            if state.moving(at) {
                shell.request_redraw();
            } else if state.pulse.is_some() {
                state.pulse = None;
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
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let p = state.phase(Instant::now());
        match &self.paint {
            Some(paint) => {
                let mut look = with_phase(p, || paint(p));
                let f = fade();
                if f < 1.0 {
                    look.background = look.background.map(|b| b.scale_alpha(f));
                    look.border.color.a *= f;
                    look.shadow.color.a *= f;
                }
                if look.background.is_some() || look.border.width > 0.0 || look.shadow.color.a > 0.0 {
                    renderer.fill_quad(
                        Quad { bounds, border: look.border, shadow: look.shadow, ..Default::default() },
                        look.background.unwrap_or(Background::Color(Color::TRANSPARENT)),
                    );
                }
            }
            None => {
                let fill = faded(fill(self.rest, self.hover, self.press, p));
                if fill.a > 0.0 {
                    renderer.fill_quad(
                        Quad { bounds, border: Border { radius: self.radius.into(), ..Default::default() }, ..Default::default() },
                        fill,
                    );
                }
            }
        }
        if let Some(c) = self.strip.filter(|_| p.hover > 0.0) {
            // GPUI: 2 px wide, 9 px from the top and bottom, fading in.
            let strip = Rectangle { x: bounds.x, y: bounds.y + 9.0, width: 2.0, height: (bounds.height - 18.0).max(0.0) };
            renderer.fill_quad(
                Quad { bounds: strip, border: Border { radius: 1.0.into(), ..Default::default() }, ..Default::default() },
                faded(Color { a: c.a * 0.85 * p.hover, ..c }),
            );
        }
        with_phase(p, || self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport));
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

impl<'a, Message: 'a, Theme: 'a, Renderer> From<Wash<'a, Message, Theme, Renderer>> for Element<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + 'a,
{
    fn from(w: Wash<'a, Message, Theme, Renderer>) -> Self {
        Element::new(w)
    }
}

// ------------------------------------------------------------------ entrance

/// The point of a surface it grows from, as fractions of its size (CSS's
/// `transform-origin`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pivot {
    pub x: f32,
    pub y: f32,
}

impl Pivot {
    /// Dialogs and other centred surfaces.
    pub const CENTER: Pivot = Pivot { x: 0.5, y: 0.5 };

    pub fn new(x: f32, y: f32) -> Self {
        Self { x: x.clamp(0.0, 1.0), y: y.clamp(0.0, 1.0) }
    }

    fn at(self, r: Rectangle) -> iced::Point {
        iced::Point::new(r.x + r.width * self.x, r.y + r.height * self.y)
    }
}

/// A floating surface that fades in when it appears, as GPUI's
/// `with_animation(.., motion::animation(dur), motion::enter)`: opacity 0 to 1
/// over `dur` with the quartic ease-out, growing from [`motion::ENTER_SCALE`]
/// around its centre (modals appear centred). The clock starts when the
/// widget is first built, so it plays once per opening. With reduced motion
/// only the fade remains; opened by a key press it is there at once.
pub fn enter<'a, Message: 'a, Theme: 'a, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    dur: std::time::Duration,
) -> Element<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + 'a,
{
    enter_from(content, dur, Pivot::CENTER)
}

/// [`enter`] growing from `pivot`: a menu from the corner at the pointer, a
/// dropdown from the edge it hangs from.
pub fn enter_from<'a, Message: 'a, Theme: 'a, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    dur: std::time::Duration,
    pivot: Pivot,
) -> Element<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + 'a,
{
    Element::new(Enter { content: content.into(), dur, key: 0, floor: 0.0, shift: Vector::ZERO, scale: motion::ENTER_SCALE, pivot })
}

/// [`enter`] that plays again whenever `key` changes (a new tab, a new
/// track), from opacity `floor` and drawn `dx` px to the side, easing back
/// into place (GPUI's player bar: `opacity(0.7 + 0.3 t).left((1 - t) * 3)`).
pub fn appear<'a, Message: 'a, Theme: 'a, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    dur: std::time::Duration,
    key: u64,
    floor: f32,
    dx: f32,
) -> Element<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + 'a,
{
    Element::new(Enter { content: content.into(), dur, key, floor, shift: Vector::new(dx, 0.0), scale: 1.0, pivot: Pivot::CENTER })
}

/// [`appear`] from `dy` px below, fully transparent: a toast rising into
/// place, once per `key` (a new message).
pub fn rise<'a, Message: 'a, Theme: 'a, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    dur: std::time::Duration,
    key: u64,
    dy: f32,
) -> Element<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + 'a,
{
    Element::new(Enter { content: content.into(), dur, key, floor: 0.0, shift: Vector::new(0.0, dy), scale: 1.0, pivot: Pivot::CENTER })
}

struct Enter<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    dur: std::time::Duration,
    key: u64,
    floor: f32,
    /// Where it is drawn from, easing back to where it rests.
    shift: Vector,
    /// Size factor at the start, 1 for none.
    scale: f32,
    pivot: Pivot,
}

/// When the surface appeared, for which key, and from where.
struct Appeared {
    at: Instant,
    key: u64,
    /// Eased progress (0..1) the entrance starts from: 0 normally; where the
    /// previous one had got to when a new key took over mid-way (fast track
    /// skips carry on instead of dipping back to the floor each time); the
    /// opacity of the ghost of the same surface still fading out (a menu
    /// opened again where it was just closed grows back from there).
    from: f32,
    /// The first frame has looked for a ghost to continue from.
    matched: bool,
}

impl Appeared {
    /// Starting now, or already over when a key press brought it (see
    /// [`motion::keyed`]).
    fn now(key: u64, dur: std::time::Duration) -> Self {
        Self::at(key, dur, Instant::now())
    }

    fn at(key: u64, dur: std::time::Duration, now: Instant) -> Self {
        let at = if motion::by_keyboard() { now.checked_sub(dur).unwrap_or(now) } else { now };
        Appeared { at, key, from: 0.0, matched: false }
    }

    /// Eased progress 0..1 at `now`.
    fn t(&self, now: Instant, dur: std::time::Duration) -> f32 {
        self.from + (1.0 - self.from) * motion::ease_out(motion::progress(self.at, now, dur))
    }

    /// A new key at `now`: a settled surface enters anew; one still entering
    /// carries on from where it is, never back to the start.
    fn rekey(&mut self, key: u64, dur: std::time::Duration, now: Instant) {
        let t = self.t(now, dur);
        let mut next = Appeared::at(key, dur, now);
        next.matched = true;
        if t < 1.0 && next.t(now, dur) < 1.0 {
            next.from = t;
        }
        *self = next;
    }
}

// ------------------------------------------------------------------ traces

/// What a floating surface looked like on its last frame, so its entrance
/// and its exit ghost (separate widgets at the same place) hand over without
/// a jump: a menu closed while still growing in fades out from the opacity
/// and size it had reached, not from full, and one opened again where its
/// ghost is still fading grows back from there.
#[derive(Clone, Copy, Debug)]
struct Trace {
    bounds: Rectangle,
    alpha: f32,
    transform: Option<Transformation>,
    leaving: bool,
    at: Instant,
}

/// How recent a trace must be to hand over (a couple of frames at 60 Hz).
const TRACE_FRESH: std::time::Duration = std::time::Duration::from_millis(100);

thread_local! {
    static TRACES: std::cell::RefCell<Vec<Trace>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn same_place(a: Rectangle, b: Rectangle) -> bool {
    (a.x - b.x).abs() < 0.5 && (a.y - b.y).abs() < 0.5 && (a.width - b.width).abs() < 0.5 && (a.height - b.height).abs() < 0.5
}

fn trace(t: Trace) {
    TRACES.with(|traces| {
        let mut traces = traces.borrow_mut();
        traces
            .retain(|o| t.at.saturating_duration_since(o.at) < TRACE_FRESH && !(o.leaving == t.leaving && same_place(o.bounds, t.bounds)));
        if traces.len() >= 8 {
            traces.remove(0);
        }
        traces.push(t);
    });
}

/// The latest trace of a surface `leaving` (or entering) at `bounds`.
fn traced(bounds: Rectangle, leaving: bool, now: Instant) -> Option<Trace> {
    TRACES.with(|traces| {
        traces
            .borrow()
            .iter()
            .rev()
            .find(|t| t.leaving == leaving && same_place(t.bounds, bounds) && now.saturating_duration_since(t.at) < TRACE_FRESH)
            .copied()
    })
}

impl<Message, Theme, Renderer> Enter<'_, Message, Theme, Renderer> {
    /// Eased progress 0..1. Reduced motion keeps this fade (it is not
    /// movement); only the slide and the growth are dropped.
    fn opacity(&self, tree: &Tree, now: Instant) -> f32 {
        tree.state.downcast_ref::<Appeared>().t(now, self.dur)
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Enter<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Appeared>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Appeared::now(self.key, self.dur))
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<Appeared>();
        if state.key != self.key {
            state.rekey(self.key, self.dur, Instant::now());
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
        if let Event::Window(window::Event::RedrawRequested(at)) = event {
            let state = tree.state.downcast_mut::<Appeared>();
            if !std::mem::replace(&mut state.matched, true) {
                // Opened where its own ghost is still fading: grow back from it.
                if let Some(ghost) = traced(layout.bounds(), true, *at) {
                    let resumed = Appeared { at: *at, key: state.key, from: ghost.alpha.clamp(0.0, 1.0), matched: true };
                    if resumed.from > state.t(*at, self.dur) {
                        *state = resumed;
                    }
                }
            }
            if self.opacity(tree, *at) < 1.0 {
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
        let t = self.opacity(tree, Instant::now());
        let alpha = self.floor + (1.0 - self.floor) * t;
        if alpha <= 0.0 {
            return;
        }
        let prev = FADE.with(|f| f.replace(f.get() * alpha));
        let still = motion::reduced();
        let d = if still { Vector::ZERO } else { self.shift * (1.0 - t) };
        let k = if still { 1.0 } else { self.scale + (1.0 - self.scale) * t };
        let mut moved = None;
        if d.x.abs() + d.y.abs() >= 0.01 {
            moved = Some(Transformation::translate(d.x, d.y));
        }
        if (1.0 - k).abs() >= 1e-4 {
            let grow = motion::scale_about(k, self.pivot.at(layout.bounds()));
            moved = Some(moved.map_or(grow, |m| m * grow));
        }
        if t < 1.0 && self.floor == 0.0 {
            trace(Trace { bounds: layout.bounds(), alpha, transform: moved, leaving: false, at: Instant::now() });
        }
        match moved {
            Some(m) => renderer.with_transformation(m, |renderer| {
                self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
            }),
            None => self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport),
        }
        FADE.with(|f| f.set(prev));
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

// ------------------------------------------------------------------ exit

/// The last look of a floating surface that was dismissed at `started`,
/// fading out over `dur` (quartic ease-out, as everything that leaves). It
/// takes no input and offers nothing to operations: it is only a picture of
/// what was there, so the click that closed it already reaches what is under.
/// Dismissed while still entering, it leaves from the opacity and size the
/// entrance had reached (no flash back to full before fading).
pub fn leave<'a, Message: 'a, Theme: 'a, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    started: Instant,
    dur: std::time::Duration,
) -> Element<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + 'a,
{
    Element::new(Leave { content: content.into(), started, dur })
}

/// Opacity of a surface leaving since `started` (0 once it is gone).
pub fn leaving(started: Instant, now: Instant, dur: std::time::Duration) -> f32 {
    1.0 - motion::ease_out(motion::progress(started, now, dur))
}

struct Leave<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    started: Instant,
    dur: std::time::Duration,
}

/// Where a ghost starts from: the entrance's opacity and transformation
/// when it was dismissed (looked up on its first frame).
type LeftFrom = std::cell::Cell<Option<(f32, Option<Transformation>)>>;

impl<Message, Theme, Renderer> Leave<'_, Message, Theme, Renderer> {
    fn start(&self, tree: &Tree, bounds: Rectangle) -> (f32, Option<Transformation>) {
        let cell = tree.state.downcast_ref::<LeftFrom>();
        if let Some(start) = cell.get() {
            return start;
        }
        let start = traced(bounds, false, self.started).map_or((1.0, None), |t| (t.alpha.clamp(0.0, 1.0), t.transform));
        cell.set(Some(start));
        start
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Leave<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<LeftFrom>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(LeftFrom::new(None))
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

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(at)) = event {
            let _ = self.start(tree, layout.bounds());
            if leaving(self.started, *at, self.dur) > 0.0 {
                shell.request_redraw();
            }
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let (from, transform) = self.start(tree, layout.bounds());
        let alpha = from * leaving(self.started, Instant::now(), self.dur);
        if alpha <= 0.002 {
            return;
        }
        trace(Trace { bounds: layout.bounds(), alpha, transform, leaving: true, at: Instant::now() });
        let prev = FADE.with(|f| f.replace(f.get() * alpha));
        let content = &tree.children[0];
        match transform {
            Some(m) => renderer.with_transformation(m, |renderer| {
                self.content.as_widget().draw(content, renderer, theme, style, layout, mouse::Cursor::Unavailable, viewport);
            }),
            None => self.content.as_widget().draw(content, renderer, theme, style, layout, mouse::Cursor::Unavailable, viewport),
        }
        FADE.with(|f| f.set(prev));
    }
}

// ------------------------------------------------------------------ paint offset

/// `content` drawn moved by `offset(phase())` (a switch's knob): paint only,
/// the layout and hit-testing stay where it rests.
pub fn shifted<'a, Message: 'a, Theme: 'a, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    offset: impl Fn(Phase) -> Vector + 'a,
) -> Element<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + 'a,
{
    Element::new(Scaled {
        content: content.into(),
        transform: Box::new(move |p, _| {
            let v = offset(p);
            (v.x.abs() + v.y.abs() >= 0.01).then(|| Transformation::translate(v.x, v.y))
        }),
    })
}

struct Scaled<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    transform: Box<dyn Fn(Phase, Rectangle) -> Option<Transformation> + 'a>,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Scaled<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
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
        let Some(t) = (self.transform)(phase(), layout.bounds()) else {
            self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
            return;
        };
        renderer.with_transformation(t, |renderer| {
            self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
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
    use std::time::Duration;

    #[test]
    fn fade_multiplies_colours_only_inside_an_entrance() {
        let c = Color { a: 0.8, ..Color::WHITE };
        assert_eq!(faded(c), c);
        let prev = FADE.with(|f| f.replace(0.5));
        assert!((faded(c).a - 0.4).abs() < 1e-6);
        FADE.with(|f| f.set(prev));
        assert_eq!(fade(), 1.0);
    }

    #[test]
    fn phase_is_published_only_while_drawing_inside() {
        assert_eq!(phase(), Phase::REST);
        let p = Phase { hover: 0.5, press: 0.25, active: 1.0, pulse: 0.9 };
        let seen = with_phase(p, || {
            let inner = with_phase(Phase::REST, phase);
            (phase(), inner)
        });
        assert_eq!(seen, (p, Phase::REST));
        assert_eq!(phase(), Phase::REST);
    }

    #[test]
    fn hover_glides_with_gpuis_timings() {
        motion::full_motion_for_tests();
        let t0 = Instant::now();
        let mut s = State {
            hover: Anim::still(0.0),
            press: Anim::still(0.0),
            active: Anim::still(0.0),
            pulse: None,
            springy: false,
            over: false,
            down: false,
        };
        s.hover.go(1.0, motion::HOVER_IN, t0);
        assert!(s.moving(t0));
        assert!(!s.moving(t0 + motion::HOVER_IN));
        assert_eq!(s.phase(t0 + motion::HOVER_IN).hover, 1.0);
        s.hover.go(0.0, motion::HOVER_OUT, t0 + motion::HOVER_IN);
        assert!(s.moving(t0 + motion::HOVER_IN + motion::FAST));
        assert!(!s.moving(t0 + motion::HOVER_IN + motion::HOVER_OUT));
    }

    #[test]
    fn a_springy_button_sinks_while_held_and_springs_back_from_there() {
        motion::full_motion_for_tests();
        let t0 = Instant::now();
        let mut s = State {
            hover: Anim::still(0.0),
            press: Anim::still(0.0),
            active: Anim::still(0.0),
            pulse: None,
            springy: true,
            over: true,
            down: true,
        };
        s.press.go(1.0, motion::PRESS_IN, t0);
        assert_eq!(s.phase(t0).pulse, 1.0);
        let held = t0 + motion::PRESS_IN * 2;
        let sunk = s.phase(held).pulse;
        assert!((sunk - (1.0 - motion::SPRING_DEPTH)).abs() < 1e-4, "{sunk}");
        // Released: the spring takes over at the same size, no jump back up.
        let back = motion::CONTROL.mul_f32(motion::spring_resume(1.0));
        s.pulse = Some(held - back);
        s.press.go(0.0, motion::PRESS_OUT, held);
        assert!((s.phase(held).pulse - sunk).abs() < 1e-3);
        assert!(s.moving(held));
        assert!((s.phase(held - back + motion::CONTROL).pulse - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_dismissed_surface_fades_out_quickly_and_is_gone() {
        let t0 = Instant::now();
        assert_eq!(leaving(t0, t0, motion::EXIT), 1.0);
        assert!(leaving(t0, t0 + motion::EXIT / 2, motion::EXIT) < 0.1);
        assert_eq!(leaving(t0, t0 + motion::EXIT, motion::EXIT), 0.0);
    }

    #[test]
    fn pivots_name_the_point_a_surface_grows_from() {
        let r = Rectangle { x: 10.0, y: 20.0, width: 100.0, height: 50.0 };
        assert_eq!(Pivot::CENTER.at(r), iced::Point::new(60.0, 45.0));
        assert_eq!(Pivot::new(0.5, 0.0).at(r), iced::Point::new(60.0, 20.0));
        assert_eq!(Pivot::new(2.0, -1.0).at(r), iced::Point::new(110.0, 20.0));
    }

    #[test]
    fn default_fill_adds_the_press_to_the_hover() {
        let ink = |a: f32| Color { a, ..Color::WHITE };
        let f = fill(Color::TRANSPARENT, ink(0.06), ink(0.04), Phase { hover: 1.0, press: 1.0, ..Phase::REST });
        assert!((f.a - 0.10).abs() < 1e-6);
        let f = fill(Color::TRANSPARENT, ink(0.06), ink(0.04), Phase { hover: 0.0, press: 1.0, ..Phase::REST });
        assert!((f.a - 0.04).abs() < 1e-6);
        assert_eq!(fill(Color::TRANSPARENT, ink(0.06), ink(0.04), Phase::REST).a, 0.0);
    }

    #[test]
    fn a_new_key_mid_entrance_carries_on_and_a_settled_one_enters_anew() {
        motion::full_motion_for_tests();
        let dur = motion::FAST;
        let t0 = Instant::now();
        let mut a = Appeared { at: t0, key: 1, from: 0.0, matched: true };
        let mid = t0 + dur / 4;
        let v = a.t(mid, dur);
        assert!(v > 0.0 && v < 1.0);
        // Five quick skips: never back to the start, always settling forward.
        let mut last = v;
        for k in 2..7 {
            a.rekey(k, dur, mid);
            let now_v = a.t(mid, dur);
            assert!(now_v >= last - 1e-3, "skip {k}: {now_v} < {last}");
            last = now_v;
        }
        assert!(a.t(mid + dur, dur) >= 1.0 - 1e-6);
        // Settled: the next key plays the whole entrance again.
        let mut b = Appeared { at: t0, key: 1, from: 0.0, matched: true };
        let later = t0 + dur * 3;
        b.rekey(2, dur, later);
        assert!(b.t(later, dur) < 0.05);
    }

    #[test]
    fn a_ghost_leaves_from_the_entrance_it_replaces_and_back() {
        let now = Instant::now();
        let bounds = Rectangle { x: 300.0, y: 120.0, width: 286.0, height: 240.0 };
        let grow = Transformation::scale(0.985);
        trace(Trace { bounds, alpha: 0.55, transform: Some(grow), leaving: false, at: now });
        let found = traced(bounds, false, now + Duration::from_millis(5)).expect("the entrance's last frame");
        assert!((found.alpha - 0.55).abs() < 1e-6 && found.transform == Some(grow));
        // Elsewhere, or a leaving surface, or too old: nothing to hand over.
        assert!(traced(Rectangle { x: 500.0, ..bounds }, false, now).is_none());
        assert!(traced(bounds, true, now).is_none());
        assert!(traced(bounds, false, now + TRACE_FRESH * 2).is_none());
        // One trace per surface and direction: the latest frame wins.
        trace(Trace { bounds, alpha: 0.7, transform: None, leaving: false, at: now + Duration::from_millis(5) });
        assert!((traced(bounds, false, now + Duration::from_millis(6)).unwrap().alpha - 0.7).abs() < 1e-6);
    }

    /// A glass pane with a filled box inside, growing from its top-left
    /// corner at half its size: the pane, its shadow and its content are all
    /// drawn scaled about the pivot (the glass is a shader primitive, so this
    /// checks the transformation reaches it). `%TEMP%\abyss-enter-scale.png`.
    #[test]
    #[ignore = "requires a wgpu adapter; renders offscreen"]
    fn an_entrance_scales_the_glass_and_its_content_about_the_pivot_offscreen() {
        use iced::advanced::renderer::Headless;
        use iced::advanced::Renderer as _;
        use iced::widget::{container, Space};
        let mut renderer =
            iced::futures::executor::block_on(<iced::Renderer as Headless>::new(super::super::INTER, iced::Pixels(16.0), None))
                .expect("wgpu adapter for the offscreen entrance test");
        let size = Size::new(480.0, 320.0);
        let filled = container(Space::new().width(300).height(140))
            .style(|_| iced::widget::container::Style { background: Some(Color::from_rgb(0.9, 0.2, 0.2).into()), ..Default::default() });
        let pane = super::super::liquid_glass::glass::<(), iced::Theme>(container(filled).padding(50), 16.0)
            .on_chrome()
            .shadow(super::super::theme::popover_shadow());
        let half: Element<'static, (), iced::Theme, iced::Renderer> = Element::new(Enter {
            content: pane.into(),
            dur: std::time::Duration::from_secs(60),
            key: 0,
            floor: 1.0,
            shift: Vector::ZERO,
            scale: 0.5,
            pivot: Pivot::new(0.0, 0.0),
        });
        let mut root: Element<'static, (), iced::Theme, iced::Renderer> = container(half).padding(40).into();
        let mut tree = Tree::new(root.as_widget());
        let node = root.as_widget_mut().layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let frame = Rectangle::with_size(size);
        renderer.reset(frame);
        root.as_widget().draw(
            &tree,
            &mut renderer,
            &iced::Theme::Dark,
            &Default::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &frame,
        );
        let pixels = Headless::screenshot(&mut renderer, Size::new(480, 320), 1.0, Color::BLACK);
        let path = std::env::temp_dir().join("abyss-enter-scale.png");
        image::save_buffer(&path, &pixels, 480, 320, image::ColorType::Rgba8).expect("write the PNG");
        let at = |x: u32, y: u32| {
            let i = ((y * 480 + x) * 4) as usize;
            (pixels[i], pixels[i + 1], pixels[i + 2])
        };
        let red = |x: u32, y: u32| at(x, y).0 > 120 && at(x, y).1 < 110;
        let lit = |x: u32, y: u32| {
            let (r, g, b) = at(x, y);
            r.max(g).max(b) > 25
        };
        // The pane (400 x 240 at 40, 40) drawn at half size from its corner:
        // the box inside it at (65, 65)..(215, 135), the glass around it.
        assert!(red(100, 100) && red(200, 125), "the scaled content is missing: {}", path.display());
        assert!(lit(52, 100) && !red(52, 100), "the scaled glass is missing: {}", path.display());
        // Where the full-size pane and box would be, beyond the half: empty.
        assert!(!lit(300, 220) && !lit(400, 250), "the pane was drawn at full size: {}", path.display());
    }
}
