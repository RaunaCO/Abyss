//! Synchronised lyrics drawn on the GPU (see `gpu::lyrics`).
//!
//! Every line keeps the same size. The active line is sharp, the next one
//! sharpens progressively until its cue, and the others stay softly blurred.
//! The text is laid out here (wrapping with cosmic-text, in logical px) and
//! rasterised once per visual line by the GPU pipeline, which keeps it in an
//! atlas and blurs it there, so a frame only draws quads: the rasterised text
//! is reused for as long as the layout stays the same. Only lines near the
//! visible area are drawn or rasterised (long lyrics stay cheap).
//!
//! The widget owns its scroll position and follows the playback clock by
//! itself (extrapolated from the engine's last reading), asking for redraws
//! while something moves, so the rest of the interface is not rebuilt every
//! frame.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use iced::advanced::graphics::text::cosmic_text as ct;
use iced::mouse;
use iced::widget::shader::{self, Action};
use iced::{Color, Rectangle};

use super::gpu::lyrics::{Instance, LineKey, LyricsPrimitive};
use super::Message;
use crate::shared::lyrics::{edge_fade, follow_step, follow_target, LyricTiming};

static RESET_GEN: AtomicU64 = AtomicU64::new(0);
static DOC_GEN: AtomicU64 = AtomicU64::new(0);

/// Put every lyrics view back at the top (track change).
pub fn reset_scroll() {
    RESET_GEN.fetch_add(1, Ordering::Relaxed);
}

/// The lyrics of one track, as both presentations use them.
#[derive(Debug)]
pub struct LyricsDoc {
    pub id: u64,
    /// Tells two builds apart (lyrics loaded again, pronunciation toggled).
    generation: u64,
    pub provider: String,
    /// (cue, line, romanised reading)
    pub lines: Vec<(f64, String, Option<String>)>,
    pub timing: Vec<(f64, String)>,
}

impl LyricsDoc {
    pub fn new(id: u64, l: &crate::tidal::Lyrics, romanize: bool) -> Self {
        let lines = l
            .synced
            .iter()
            .enumerate()
            .map(|(i, (at, line))| (*at, line.clone(), l.roman.get(i).cloned().flatten().filter(|_| romanize)))
            .collect();
        Self { id, generation: DOC_GEN.fetch_add(1, Ordering::Relaxed), provider: l.provider.clone(), lines, timing: l.synced.clone() }
    }
}

/// The two presentations: fullscreen Now Playing and the side panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    NowPlaying,
    Side,
}

/// Sizes of a presentation (logical px; `scale` already applied).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub kind: Kind,
    pub font: f32,
    pub line_h: f32,
    pub weight: u16,
    pub sec_font: f32,
    pub sec_line_h: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub gap: f32,
    pub stanza_gap: f32,
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub follow_fraction: f32,
    pub edge_top: f32,
    pub edge_bottom: f32,
    pub blur_weight: f32,
    /// Radius of the halo around the current line (0: none).
    pub halo: f32,
}

impl Style {
    /// Now Playing: text follows the window height (`scale`), and the scroll
    /// box bleeds `bleed` px into the margins so the halo is not cut.
    pub fn now_playing(scale: f32, bleed: f32) -> Self {
        const GUTTER: f32 = 36.0;
        Self {
            kind: Kind::NowPlaying,
            font: 44.0 * scale,
            line_h: 58.0 * scale,
            weight: 600,
            sec_font: 26.0 * scale,
            sec_line_h: 34.0 * scale,
            pad_x: 10.0,
            pad_y: 8.0,
            gap: 0.0,
            stanza_gap: 26.0,
            left: GUTTER + bleed,
            right: 36.0 + bleed,
            top: 32.0,
            bottom: 64.0,
            follow_fraction: 0.32,
            edge_top: 60.0,
            edge_bottom: 130.0,
            blur_weight: 1.0,
            halo: (46.0 * scale).min((GUTTER + bleed) / 2.2),
        }
    }

    pub fn side() -> Self {
        Self {
            kind: Kind::Side,
            font: 20.0,
            line_h: 28.0,
            weight: 700,
            sec_font: 14.0,
            sec_line_h: 20.0,
            pad_x: 8.0,
            pad_y: 5.0,
            gap: 6.0,
            stanza_gap: 0.0,
            left: 24.0,
            right: 24.0,
            top: 0.0,
            bottom: 40.0,
            follow_fraction: 0.38,
            edge_top: 30.0,
            edge_bottom: 70.0,
            blur_weight: 0.75,
            halo: 0.0,
        }
    }
}

/// Colours the lyrics are drawn with.
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub text: Color,
    pub dim: Color,
    pub past: Color,
    pub hover: Color,
    pub glow: Color,
    pub provider: Color,
}

/// One visual row of text inside a block.
#[derive(Debug, Clone)]
struct Row {
    text: String,
    x: f32,
    y: f32,
    line_h: f32,
    font: f32,
    weight: u16,
    alpha: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BlockKind {
    Dots,
    Line(usize),
    Provider,
}

#[derive(Debug, Clone)]
struct Block {
    kind: BlockKind,
    top: f32,
    height: f32,
    width: f32,
    rows: Vec<Row>,
}

#[derive(Debug)]
struct Layout {
    key: (u64, u32, u32, u32),
    blocks: Vec<Block>,
    content_h: f32,
}

#[derive(Default)]
pub struct State {
    layout: RefCell<Option<Layout>>,
    offset: f32,
    last: Option<Instant>,
    hover: Option<usize>,
    doc: u64,
    reset_gen: u64,
    active: Option<usize>,
    previous: Option<usize>,
    changed_at: Option<Instant>,
    following: bool,
    /// Where a wheel notch is taking the scroll (it glides there as the
    /// pages and the queue do, instead of jumping 99 px).
    wheel_to: Option<f32>,
}

/// The shader program: what to show, and the clock to follow.
pub struct Lyrics {
    pub doc: Arc<LyricsDoc>,
    pub style: Style,
    pub colors: Colors,
    /// Playback position at `at`, and whether it is advancing.
    pub position: f64,
    pub at: Instant,
    pub playing: bool,
    pub duration: f64,
    pub follow: bool,
    /// The primary row is the reading (Now Playing) or the original (side).
    pub reading_first: bool,
    pub provider_label: String,
}

impl Lyrics {
    fn now_position(&self) -> f64 {
        let p = if self.playing { self.position + self.at.elapsed().as_secs_f64() } else { self.position };
        if self.duration > 0.0 {
            p.min(self.duration)
        } else {
            p
        }
    }

    fn layout_key(&self, width: f32) -> (u64, u32, u32, u32) {
        (
            self.doc.generation,
            (width * 4.0) as u32,
            (self.style.font * 100.0) as u32,
            u32::from(self.reading_first) | ((self.style.kind == Kind::Side) as u32) << 1,
        )
    }

    fn build_layout(&self, width: f32) -> Layout {
        let st = &self.style;
        let max_text_w = (width - st.left - st.right - 2.0 * st.pad_x).max(40.0);
        let mut fs = iced::advanced::graphics::text::font_system().write().expect("font system");
        let fs = fs.raw();
        let mut blocks = Vec::with_capacity(self.doc.lines.len() + 2);
        let mut y = st.top;
        if st.kind == Kind::NowPlaying {
            blocks.push(Block { kind: BlockKind::Dots, top: y, height: 54.0, width: 3.0 * 9.0 + 2.0 * 9.0 + 20.0, rows: Vec::new() });
            y += 54.0;
        }
        for (i, (at, line, reading)) in self.doc.lines.iter().enumerate() {
            if i > 0 {
                y += st.gap;
                if st.stanza_gap > 0.0 && (*at - self.doc.lines[i - 1].0) > 5.5 {
                    y += st.stanza_gap;
                }
            }
            let original = if line.is_empty() { "♪".to_string() } else { line.clone() };
            let (primary, secondary) = match (reading, self.reading_first) {
                (Some(r), true) => (r.clone(), Some(original)),
                (Some(r), false) => (original, Some(r.clone())),
                (None, _) => (original, None),
            };
            let mut rows = Vec::new();
            let mut h = st.pad_y;
            let mut w: f32 = 0.0;
            for (text, font, line_h, weight, alpha) in std::iter::once((primary, st.font, st.line_h, st.weight, 1.0))
                .chain(secondary.map(|s| (s, st.sec_font, st.sec_line_h, 500u16, 0.8)))
            {
                for (row_text, row_w) in wrap(fs, &text, font, line_h, weight, max_text_w) {
                    rows.push(Row { text: row_text, x: st.pad_x, y: h, line_h, font, weight, alpha });
                    w = w.max(row_w);
                    h += line_h;
                }
            }
            h += st.pad_y;
            blocks.push(Block { kind: BlockKind::Line(i), top: y, height: h, width: w + 2.0 * st.pad_x, rows });
            y += h;
        }
        // The provider's name under the lines.
        let pt = if st.kind == Kind::NowPlaying { 24.0 } else { 28.0 };
        let prov_rows: Vec<Row> = wrap(fs, &self.provider_label, 11.0, 15.0, 400, max_text_w)
            .into_iter()
            .enumerate()
            .map(|(k, (text, _))| Row { text, x: st.pad_x, y: pt + k as f32 * 15.0, line_h: 15.0, font: 11.0, weight: 400, alpha: 1.0 })
            .collect();
        let ph = pt + prov_rows.len() as f32 * 15.0;
        blocks.push(Block { kind: BlockKind::Provider, top: y, height: ph, width: max_text_w, rows: prov_rows });
        y += ph + st.bottom;
        Layout { key: self.layout_key(width), blocks, content_h: y }
    }

    fn ensure_layout(&self, state: &State, width: f32) {
        let key = self.layout_key(width);
        let stale = state.layout.borrow().as_ref().is_none_or(|l| l.key != key);
        if stale {
            *state.layout.borrow_mut() = Some(self.build_layout(width));
        }
    }

    /// Emphasis of line `i` (0..1): eases in when it becomes current, out when it stops.
    fn emphasis(&self, state: &State, i: usize, reduced: bool) -> f32 {
        let target = if state.active == Some(i) { 1.0 } else { 0.0 };
        let Some(at) = state.changed_at else { return target };
        if reduced {
            return target;
        }
        let e = super::motion::ease_out_soft(super::motion::progress(at, Instant::now(), super::motion::PANEL));
        if state.active == Some(i) {
            e
        } else if state.previous == Some(i) {
            1.0 - e
        } else {
            0.0
        }
    }

    fn block_at(&self, state: &State, bounds: Rectangle, p: iced::Point) -> Option<usize> {
        let layout = state.layout.borrow();
        let layout = layout.as_ref()?;
        let y = p.y - bounds.y + state.offset;
        let x = p.x - bounds.x - self.style.left;
        layout.blocks.iter().find_map(|b| match b.kind {
            BlockKind::Line(i) if y >= b.top && y < b.top + b.height && x >= 0.0 && x <= b.width => Some(i),
            _ => None,
        })
    }

    /// The scroll offset the follow mode aims at now.
    fn target_offset(&self, layout: &Layout, timing: &LyricTiming, height: f32, reduced: bool) -> f32 {
        let dots = usize::from(self.style.kind == Kind::NowPlaying);
        let index_of = |line: Option<usize>| line.map(|c| c + dots).unwrap_or(0);
        let block = |i: usize| layout.blocks.get(i).map(|b| b.top);
        let current = index_of(timing.current);
        let mut top = block(current).unwrap_or(0.0);
        if !reduced {
            if let Some((next, blend)) = timing.next_scroll {
                if let Some(next_top) = block(next + dots) {
                    top += (next_top - top) * blend;
                }
            }
        }
        follow_target(top, height, self.style.follow_fraction, (layout.content_h - height).max(0.0))
    }
}

/// Lines of `text` wrapped at `max_w` (logical px): (row text, row width).
fn wrap(fs: &mut ct::FontSystem, text: &str, font: f32, line_h: f32, weight: u16, max_w: f32) -> Vec<(String, f32)> {
    let mut buffer = ct::Buffer::new(fs, ct::Metrics::new(font, line_h));
    buffer.set_wrap(fs, ct::Wrap::WordOrGlyph);
    buffer.set_size(fs, Some(max_w), None);
    let attrs = ct::Attrs::new().family(ct::Family::Name("Inter")).weight(ct::Weight(weight));
    buffer.set_text(fs, text, &attrs, ct::Shaping::Advanced, None);
    buffer.shape_until_scroll(fs, false);
    let mut out = Vec::new();
    for run in buffer.layout_runs() {
        let (Some(first), Some(last)) = (run.glyphs.first(), run.glyphs.last()) else {
            continue;
        };
        let (a, b) = (first.start.min(last.start), first.end.max(last.end));
        let slice = run.text.get(a..b).unwrap_or(run.text).trim_end().to_string();
        if !slice.is_empty() {
            out.push((slice, run.line_w));
        }
    }
    if out.is_empty() {
        out.push((text.to_string(), 0.0));
    }
    out
}

impl shader::Program<Message> for Lyrics {
    type State = State;
    type Primitive = LyricsPrimitive;

    fn update(&self, state: &mut State, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Action<Message>> {
        let reduced = crate::shared::lyrics::reduced_motion();
        // A new track (or a reset): back to the top.
        let gen = RESET_GEN.load(Ordering::Relaxed);
        if state.doc != self.doc.id || state.reset_gen != gen {
            state.doc = self.doc.id;
            state.reset_gen = gen;
            state.offset = 0.0;
            state.active = None;
            state.previous = None;
            state.changed_at = None;
            state.hover = None;
            state.wheel_to = None;
        }
        match event {
            iced::Event::Window(iced::window::Event::RedrawRequested(now)) => {
                self.ensure_layout(state, bounds.width);
                let pos = self.now_position();
                let timing = LyricTiming::at_position(&self.doc.timing, pos, reduced);
                if timing.current != state.active {
                    state.previous = state.active;
                    state.active = timing.current;
                    state.changed_at = Some(*now);
                }
                let dt = state.last.map_or(0.016, |l| now.saturating_duration_since(l).as_secs_f32());
                state.last = Some(*now);
                let mut moving = false;
                if self.follow {
                    let target = state.layout.borrow().as_ref().map(|layout| self.target_offset(layout, &timing, bounds.height, reduced));
                    if let Some(target) = target {
                        let next = follow_step(state.offset, target, dt, reduced);
                        moving = (next - target).abs() > 0.01 || (next - state.offset).abs() > 0.01;
                        state.offset = next;
                    }
                }
                // Back to following the song: a notch still under way is dropped.
                if self.follow && !state.following {
                    state.wheel_to = None;
                }
                state.following = self.follow;
                if let Some(to) = state.wheel_to.filter(|_| !self.follow) {
                    let step = super::glide::step_of(to - state.offset, dt.min(0.05), reduced);
                    state.offset += step;
                    if (to - state.offset).abs() < 0.01 {
                        state.offset = to;
                        state.wheel_to = None;
                    } else {
                        moving = true;
                    }
                }
                let emph_moving = state.changed_at.is_some_and(|at| at.elapsed() < super::motion::PANEL) && !reduced;
                if moving || emph_moving {
                    return Some(Action::request_redraw());
                }
                if self.playing {
                    // The next line sharpens slowly: 30 frames a second is plenty.
                    return Some(Action::request_redraw_at(*now + std::time::Duration::from_millis(if reduced { 100 } else { 33 })));
                }
                None
            }
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_in(bounds)?;
                self.ensure_layout(state, bounds.width);
                let max = state.layout.borrow().as_ref().map_or(0.0, |l| (l.content_h - bounds.height).max(0.0));
                match delta {
                    // A notch glides (from where it is headed, so notches add up).
                    mouse::ScrollDelta::Lines { y, .. } => {
                        if state.wheel_to.is_none() {
                            // The glide's clock starts with the notch, not at the last frame drawn.
                            state.last = Some(Instant::now());
                        }
                        let from = state.wheel_to.unwrap_or(state.offset);
                        state.wheel_to = Some((from - y * super::glide::NOTCH_PX).clamp(0.0, max));
                    }
                    // A touchpad already moves smoothly: follow it 1:1.
                    mouse::ScrollDelta::Pixels { y, .. } => {
                        state.wheel_to = None;
                        state.offset = (state.offset - y).clamp(0.0, max);
                    }
                }
                let action = if self.follow { Action::publish(Message::LyricsManual) } else { Action::request_redraw() };
                Some(action.and_capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
                let hover = cursor.position_over(bounds).and_then(|p| self.block_at(state, bounds, p));
                if hover != state.hover {
                    state.hover = hover;
                    return Some(Action::request_redraw());
                }
                None
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_over(bounds)?;
                let i = self.block_at(state, bounds, p)?;
                let at = self.doc.lines.get(i)?.0;
                Some(Action::publish(Message::LyricLine(at.max(0.0))).and_capture())
            }
            _ => None,
        }
    }

    fn mouse_interaction(&self, state: &State, _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        if state.hover.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(&self, state: &State, _cursor: mouse::Cursor, bounds: Rectangle) -> LyricsPrimitive {
        let reduced = crate::shared::lyrics::reduced_motion();
        self.ensure_layout(state, bounds.width);
        let layout = state.layout.borrow();
        let Some(layout) = layout.as_ref() else { return LyricsPrimitive::empty() };
        let st = &self.style;
        let pos = self.now_position();
        let timing = LyricTiming::at_position(&self.doc.timing, pos, reduced);
        let mut lines: Vec<LineKey> = Vec::new();
        let mut instances: Vec<Instance> = Vec::new();
        let mut glows: Vec<Instance> = Vec::new();
        let h = bounds.height;
        let margin = st.line_h * 2.0;
        for block in &layout.blocks {
            let top = block.top - state.offset;
            if top + block.height < -margin || top > h + margin {
                continue;
            }
            let edge = edge_fade(top, top + block.height, h, st.edge_top, st.edge_bottom);
            if edge <= 0.0 {
                continue;
            }
            match block.kind {
                BlockKind::Dots => {
                    let color = if timing.current.is_none() { self.colors.text } else { Color { a: 0.35, ..Color::WHITE } };
                    for k in 0..3 {
                        let x = st.left + 10.0 + k as f32 * 18.0;
                        let y = top + (block.height - 9.0) / 2.0;
                        instances.push(Instance::dot(Rectangle::new(iced::Point::new(x, y), iced::Size::new(9.0, 9.0)), color, edge));
                    }
                }
                BlockKind::Provider => {
                    for row in &block.rows {
                        push_row(&mut lines, &mut instances, row, st.left, top, self.colors.provider, edge, 0.0, 1.0);
                    }
                }
                BlockKind::Line(i) => {
                    let emphasis = self.emphasis(state, i, reduced);
                    let blur = timing.blur(i) * st.blur_weight;
                    let hovered = state.hover == Some(i);
                    let past = timing.current.is_some_and(|c| i < c);
                    let color = match st.kind {
                        Kind::NowPlaying => {
                            let a = if hovered { 0.85_f32.max(0.55 + 0.45 * emphasis) } else { 0.55 + 0.45 * emphasis };
                            Color { a, ..Color::WHITE }
                        }
                        Kind::Side => {
                            let base = if past { self.colors.past } else { self.colors.dim };
                            if hovered {
                                self.colors.text
                            } else {
                                super::theme::lerp(base, self.colors.text, emphasis)
                            }
                        }
                    };
                    let rect = Rectangle::new(iced::Point::new(st.left, top), iced::Size::new(block.width, block.height));
                    if hovered && st.kind == Kind::Side {
                        glows.push(Instance::rounded(rect, self.colors.hover, edge, 6.0, 0.6));
                    }
                    if st.halo > 0.0 && emphasis > 0.001 {
                        let g = self.colors.glow;
                        let spread = 4.0 * emphasis;
                        let outer = Rectangle {
                            x: rect.x - spread,
                            y: rect.y - spread,
                            width: rect.width + 2.0 * spread,
                            height: rect.height + 2.0 * spread,
                        };
                        glows.push(Instance::glow(outer, Color { a: 0.16 * emphasis, ..g }, edge, 10.0, st.halo));
                        let spread = 2.0 * emphasis;
                        let inner = Rectangle {
                            x: rect.x - spread,
                            y: rect.y - spread,
                            width: rect.width + 2.0 * spread,
                            height: rect.height + 2.0 * spread,
                        };
                        glows.push(Instance::glow(inner, Color { a: 0.30 * emphasis, ..g }, edge, 10.0, st.halo * 0.7));
                    }
                    // Fully soft lines read a little fainter, as in the reference.
                    let fade = 1.0 - 0.18 * blur;
                    for row in &block.rows {
                        push_row(&mut lines, &mut instances, row, st.left, top, color, edge * fade, blur, row.alpha);
                    }
                }
            }
        }
        glows.extend(instances);
        LyricsPrimitive::new(lines, glows, st.kind == Kind::NowPlaying, st.font)
    }
}

#[allow(clippy::too_many_arguments)]
fn push_row(
    lines: &mut Vec<LineKey>,
    instances: &mut Vec<Instance>,
    row: &Row,
    left: f32,
    top: f32,
    color: Color,
    alpha: f32,
    blur: f32,
    row_alpha: f32,
) {
    let key = LineKey {
        text: row.text.clone(),
        font_centi: (row.font * 100.0).round() as u32,
        line_centi: (row.line_h * 100.0).round() as u32,
        weight: row.weight,
    };
    let index = match lines.iter().position(|k| *k == key) {
        Some(i) => i,
        None => {
            lines.push(key);
            lines.len() - 1
        }
    };
    let origin = iced::Point::new(left + row.x, top + row.y);
    instances.push(Instance::text(index, origin, Color { a: color.a * alpha * row_alpha, ..color }, blur));
}
