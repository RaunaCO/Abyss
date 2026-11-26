//! One line of text that ends in "…" when it does not fit, like GPUI's
//! `truncate()`. Iced 0.14 has no ellipsis: a `Text` without wrapping simply
//! overflows its parent and paints over its neighbours (album titles in a
//! carousel, long track names in a row).
//!
//! The cut is found by binary search over character boundaries, measuring
//! with the renderer's own paragraph, and is cached in the widget state until
//! the text, its style or the available width change.

use iced::advanced::layout::{self, Layout};
use iced::advanced::text::{self as core_text, Paragraph as _};
use iced::advanced::widget::{tree, Tree, Widget};
use iced::advanced::{mouse, renderer, Text};
use iced::alignment;
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::{Color, Element, Font, Length, Pixels, Rectangle, Size};

pub struct Line<'a> {
    content: std::borrow::Cow<'a, str>,
    size: f32,
    color: Color,
    font: Option<Font>,
    width: Length,
    line_height: LineHeight,
    align_x: core_text::Alignment,
    /// Colour computed when drawn (hover fades, the sidebar's label fade).
    live: Option<Box<dyn Fn(Color) -> Color + 'a>>,
    /// Underlined while the enclosing transition is more than half hovered.
    underline: bool,
}

pub fn line<'a>(content: impl Into<std::borrow::Cow<'a, str>>, size: f32, color: Color) -> Line<'a> {
    Line {
        content: content.into(),
        size,
        color,
        font: None,
        width: Length::Shrink,
        line_height: LineHeight::default(),
        align_x: core_text::Alignment::Default,
        live: None,
        underline: false,
    }
}

impl<'a> Line<'a> {
    /// The colour as it is when drawn: `f` gets the base colour and may read
    /// the enclosing transition (`wash::phase()`) or a clock.
    pub fn live(mut self, f: impl Fn(Color) -> Color + 'a) -> Self {
        self.live = Some(Box::new(f));
        self
    }
    /// Underline the text while the enclosing `wash` is hovered (GPUI's
    /// `.when(h > 0.5, |d| d.underline())` on links and card titles).
    pub fn underline_on_hover(mut self) -> Self {
        self.underline = true;
        self
    }
    pub fn brighten_on_hover(self, full: Color) -> Self {
        self.live(move |rest| super::theme::lerp(rest, full, super::wash::phase().hover)).underline_on_hover()
    }
    pub fn font(mut self, font: Font) -> Self {
        self.font = Some(font);
        self
    }
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
    pub fn line_height(mut self, line_height: impl Into<LineHeight>) -> Self {
        self.line_height = line_height.into();
        self
    }
    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.size = size.into().0;
        self
    }
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
    pub fn center(mut self) -> Self {
        self.align_x = core_text::Alignment::Center;
        self
    }
}

/// What the cached paragraph was built for.
#[derive(PartialEq, Clone)]
struct Key {
    content: String,
    size: f32,
    font: Option<Font>,
    line_height: LineHeight,
    max_width: f32,
}

struct State<P> {
    key: Option<Key>,
    paragraph: Option<P>,
}

impl<P> Default for State<P> {
    fn default() -> Self {
        Self { key: None, paragraph: None }
    }
}

const ELLIPSIS: char = '…';

/// The longest prefix of `content` (cut at a char boundary, trailing spaces
/// trimmed) that fits in `max` once "…" is added, measured with `fits`.
pub(crate) fn cut(content: &str, max: f32, mut width_of: impl FnMut(&str) -> f32) -> String {
    if width_of(content) <= max + 0.5 {
        return content.to_string();
    }
    let bounds: Vec<usize> = content.char_indices().map(|(i, _)| i).skip(1).collect();
    let (mut lo, mut hi) = (0usize, bounds.len());
    let candidate = |n: usize| -> String {
        let end = if n == 0 { 0 } else { bounds[n - 1] };
        format!("{}{ELLIPSIS}", content[..end].trim_end())
    };
    // Largest n whose candidate fits.
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if width_of(&candidate(mid)) <= max + 0.5 {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    candidate(lo)
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Line<'_>
where
    Renderer: core_text::Renderer<Font = Font>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Renderer::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<Renderer::Paragraph>::default())
    }

    fn size(&self) -> Size<Length> {
        Size { width: self.width, height: Length::Shrink }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let state = tree.state.downcast_mut::<State<Renderer::Paragraph>>();
        let font = self.font.unwrap_or_else(|| renderer.default_font());
        let max_width = limits.max().width;
        let key = Key { content: self.content.to_string(), size: self.size, font: self.font, line_height: self.line_height, max_width };
        let make = |s: &str| {
            Renderer::Paragraph::with_text(Text {
                content: s,
                bounds: Size::new(f32::INFINITY, f32::INFINITY),
                size: Pixels(self.size),
                line_height: self.line_height,
                font,
                align_x: self.align_x,
                align_y: alignment::Vertical::Top,
                shaping: Shaping::Advanced,
                wrapping: Wrapping::None,
            })
        };
        if state.key.as_ref() != Some(&key) || state.paragraph.is_none() {
            let shown = if max_width.is_finite() {
                cut(&self.content, max_width, |s| make(s).min_bounds().width)
            } else {
                self.content.to_string()
            };
            state.paragraph = Some(make(&shown));
            state.key = Some(key);
        }
        let measured = state.paragraph.as_ref().map(|p| p.min_bounds()).unwrap_or_default();
        let size = limits.width(self.width).height(Length::Shrink).resolve(self.width, Length::Shrink, measured);
        layout::Node::new(size)
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
        let state = tree.state.downcast_ref::<State<Renderer::Paragraph>>();
        let Some(paragraph) = &state.paragraph else { return };
        let bounds = layout.bounds();
        let x = match self.align_x {
            core_text::Alignment::Center => bounds.center_x() - paragraph.min_bounds().width / 2.0,
            _ => bounds.x,
        };
        let color = super::wash::faded(self.live.as_ref().map_or(self.color, |f| f(self.color)));
        if color.a <= 0.0 {
            return;
        }
        renderer.fill_paragraph(paragraph, iced::Point::new(x, bounds.y), color, *viewport);
        if self.underline && super::wash::phase().hover > 0.5 {
            let size = paragraph.min_bounds();
            let thickness = (self.size / 13.0).max(1.0);
            let y = bounds.y + (size.height + self.size) / 2.0 + 1.0;
            renderer.fill_quad(
                iced::advanced::renderer::Quad {
                    bounds: Rectangle { x, y: y.min(bounds.y + size.height - thickness), width: size.width, height: thickness },
                    ..Default::default()
                },
                color,
            );
        }
    }
}

impl<'a, Message: 'a, Theme: 'a, Renderer> From<Line<'a>> for Element<'a, Message, Theme, Renderer>
where
    Renderer: core_text::Renderer<Font = Font> + 'a,
{
    fn from(line: Line<'a>) -> Self {
        Element::new(line)
    }
}

#[cfg(test)]
mod tests {
    use super::cut;

    /// Every char 10 px wide, "…" too.
    fn w(s: &str) -> f32 {
        s.chars().count() as f32 * 10.0
    }

    #[test]
    fn short_text_is_kept() {
        assert_eq!(cut("Home", 100.0, w), "Home");
    }

    #[test]
    fn long_text_ends_in_ellipsis_and_fits() {
        let out = cut("Somewhere Along The Highway", 100.0, w);
        assert!(out.ends_with('…'));
        assert!(w(&out) <= 100.0);
        assert_eq!(out, "Somewhere…");
    }

    #[test]
    fn cuts_on_char_boundaries_with_accents_and_kanji() {
        let out = cut("Álbum 死へ戒る想いは", 60.0, w);
        assert!(out.ends_with('…') && w(&out) <= 60.0);
    }

    #[test]
    fn nothing_fits_gives_just_the_ellipsis() {
        assert_eq!(cut("abc", 5.0, w), "…");
    }
}
