//! Liquid glass surfaces: a pane that bends, splits and reflects light.
//!
//! The glass refracts the *scene*, a picture lying behind the whole window:
//! the artwork in view, the featured item on Home, or a soft field of the
//! accent colour on pages without artwork. The player bar uses a moving
//! field of the playing cover's colours. [`update_scene`] picks the page
//! scene once per view. A worker prepares the artwork or its palette as a
//! mip chain (`images::glass_scene`). The page shows the same scene as a wash at its top
//! ([`page_wash`]), so what the glass bends is what the eye sees around it.
//!
//! [`glass`] draws the pane behind any content (the content stays on top and
//! interactive): one GPU draw per surface, see `gpu::glass` for the optics.
//! The pointer light follows the cursor while it is over the pane and eases
//! in and out (GPUI's `glass.rs`), requesting frames while the reflection is
//! moving. The player bar schedules its own ambient motion at 30 fps.
//!
//! # Roles
//!
//! Glass is kept for the layer that floats over content: navigation and
//! controls, headers and cards, menus and popovers. Never on dense rows, and
//! never glass on glass, except a prominent action on its panel. One radius,
//! optics and shadow per role (bigger surfaces read thicker: a wider lens and
//! a deeper shadow):
//!
//! | role      | used by                                  | radius        | optics      | shadow           | pointer |
//! |-----------|------------------------------------------|---------------|-------------|------------------|---------|
//! | panel     | page headers, featured card, inline album (14), Replay (no shadow) | [`PANEL_R`] 18 | `PANEL` | `popover_shadow` | no |
//! | over art  | Now Playing's queue / similar / credits  | frame `PANEL_R` | `OVER_ART` | `popover_shadow` | no |
//! | structure | Settings navigation, menus, info, dialogs, tooltips | 18 / 12 / 16 / 16 / 7 | `POPOVER` | `popover_shadow` | no |
//! | bar       | the player bar                           | frame radius  | `BAR`       | none (framed)    | no      |
//! | control   | top bar groups and search, Now Playing tabs | capsule ([`control`]) | `CONTROL` | `control_shadow` | yes |
//! | prominent | a page's Play, the featured card's action | capsule      | `PROMINENT`, tint from [`prominent`] | `control_shadow` | yes |
//!
//! *Pointer*: only controls react to the pointer ([`Glass::interactive`]:
//! the light that follows it, the glint on the rim, the press). Panels hold
//! still, so hovering a header never flares its edges, and moving the mouse
//! over them costs no frames. Neighbouring controls share one pane
//! ([`control`] around the group, as a `GlassEffectContainer` would merge
//! them) rather than a row of separate pills. The top bar's glass sits on the
//! plain chrome ([`Glass::on_chrome`]): it refracts no picture, only its rim
//! and sheen catch light. Secondary text on glass uses
//! `theme::text_secondary_on_glass` (vibrancy) where the flat grey would fall
//! under AA.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{mouse, overlay, Clipboard, Shell, Widget};
use iced::widget::shader;
use iced::{window, Border, Color, Element, Event, Length, Point, Rectangle, Shadow, Size, Vector};

pub use super::gpu::glass::Optics;
use super::gpu::glass::{GlassPrimitive, Kind, Light, PageLight, Scene, SceneImage};
use super::{images, theme, App};
use crate::shared::{Load, Page};

// ------------------------------------------------------------------ the scene

static SCENE: Mutex<Option<Scene>> = Mutex::new(None);

/// The scene every glass surface refracts now.
pub fn current_scene() -> Option<Scene> {
    SCENE.lock().ok().and_then(|g| g.clone())
}

/// The cover the current page is about, if any.
pub fn scene_url(app: &App) -> Option<String> {
    if let Some(cover) = &app.cover_view {
        return Some(cover.clone());
    }
    if app.now_playing_open {
        return app.now.as_ref().and_then(|n| n.track.cover(320));
    }
    match &app.page {
        Page::Album(id) => app.albums.get(id).and_then(Load::ready).and_then(|(a, _)| a.cover(320)),
        Page::MyAlbums if app.lib_grid("albums") => app.expanded_album.and_then(|id| {
            app.lib_shown.iter().filter_map(|&i| app.lib_albums.get(i)).find(|album| album.id == id).and_then(|album| album.cover(320))
        }),
        Page::Playlist(uuid) => app.playlists.get(uuid).and_then(Load::ready).and_then(|(p, _)| p.cover(320)),
        Page::Mix(id) => app.mixes.get(id).and_then(Load::ready).and_then(|(m, _)| m.image.clone()),
        Page::Artist(id) => app.artists.get(id).and_then(Load::ready).and_then(|a| a.artist.picture(320)),
        Page::Home => match &app.home {
            Load::Ready(rows) => super::home::lead_art(rows),
            _ => None,
        },
        _ => None,
    }
}

/// The accent field, made once per accent colour.
fn accent_scene() -> Scene {
    /// The accent it was made for and the field.
    type Made = Option<(u32, Arc<SceneImage>)>;
    static ACCENT: Mutex<Made> = Mutex::new(None);
    let rgb = theme::accent_rgb();
    let cell = &ACCENT;
    let mut g = cell.lock().unwrap_or_else(|e| e.into_inner());
    let image = match g.as_ref() {
        Some((key, img)) if *key == rgb => img.clone(),
        _ => {
            let img = Arc::new(SceneImage::accent(theme::rgb(rgb)));
            *g = Some((rgb, img.clone()));
            img
        }
    };
    Scene { key: format!("accent:{rgb:06x}"), image }
}

/// While a cover loads, show the accent field until the correct artwork is ready.
pub fn update_scene(app: &App) {
    // Off album pages no banner shines through the panes.
    if wash_lights(app).is_none() {
        if let Ok(mut light) = PAGE_LIGHT.lock() {
            *light = None;
        }
    }
    let url = scene_url(app);
    let palette_only = app.cover_view.is_some() || (!app.now_playing_open && matches!(app.page, Page::Album(_) | Page::MyAlbums));
    let scene = url.and_then(|url| images::glass_scene(&url, palette_only)).unwrap_or_else(accent_scene);
    let Ok(mut g) = SCENE.lock() else { return };
    if g.as_ref().is_none_or(|s| s.key != scene.key) {
        *g = Some(scene);
    }
}

// ------------------------------------------------------------------ painting

/// Builds the primitive for Iced's `shader` widget (which owns the
/// `draw_primitive` call), at whatever bounds it is laid out at.
struct Paint {
    scene: Option<Scene>,
    kind: Kind,
    light_theme: bool,
}

impl<Message> shader::Program<Message> for Paint {
    type State = ();
    type Primitive = GlassPrimitive;

    fn draw(&self, _: &(), _: mouse::Cursor, _: Rectangle) -> GlassPrimitive {
        GlassPrimitive::new(self.scene.clone(), self.kind, self.light_theme)
    }
}

/// Height of the page wash (GPUI's page tint: 380 px).
const WASH_H: f32 = 380.0;

/// The cover's three colours when the page wash shows them as drifting glows
/// (album pages, and the album expanded in the collection).
fn wash_lights(app: &App) -> Option<[Color; 3]> {
    if app.now_playing_open || app.cover_view.is_some() || !matches!(app.page, Page::Album(_) | Page::MyAlbums) {
        return None;
    }
    let url = scene_url(app)?;
    let palette: [Color; 3] = images::palette(&url).try_into().ok()?;
    Some(super::gpu::glass::banner_palette(palette, theme::is_light()))
}

/// The album banner as it was last drawn, for the panes over it (set by the
/// banner, which is drawn first, under the page; cleared off album pages).
static PAGE_LIGHT: Mutex<Option<PageLight>> = Mutex::new(None);

/// Seconds on the clock the album banner and the panes over it share.
fn banner_clock() -> f32 {
    static START: OnceLock<Instant> = OnceLock::new();
    if crate::shared::lyrics::reduced_motion() {
        0.0
    } else {
        START.get_or_init(Instant::now).elapsed().as_secs_f32()
    }
}

/// The scene as a wash at the top of the page, fading out over 380 px and
/// scrolling away with the page. On album pages it is the cover's colours,
/// drifting slowly (a frame every 33 ms while shown). Draws nothing
/// interactive; put it first in a stack under the page.
pub fn page_wash<'a, Message: 'a>(app: &App) -> Element<'a, Message> {
    let top = -app.page_scroll.max(0.0);
    if top <= -WASH_H {
        // Scrolled past the banner: the panes below see the plain page (not
        // the palette field they would fall back to, a milky grey on a
        // black-and-white cover).
        if let (Some(colors), Ok(mut light)) = (wash_lights(app), PAGE_LIGHT.lock()) {
            let area = light.map_or(Rectangle::new(Point::ORIGIN, Size::new(1.0, 1.0)), |l| l.area);
            *light = Some(PageLight { colors, elapsed: 0.0, area, top, height: WASH_H, alpha: 0.0, bg: theme::bg() });
        }
        return iced::widget::Space::new().width(Length::Fill).height(Length::Fill).into();
    }
    shader(Wash { top, light_theme: theme::is_light(), lights: wash_lights(app) }).width(Length::Fill).height(Length::Fill).into()
}

struct Wash {
    top: f32,
    light_theme: bool,
    lights: Option<[Color; 3]>,
}

impl<Message> shader::Program<Message> for Wash {
    type State = ();
    type Primitive = GlassPrimitive;

    fn update(&self, _: &mut (), event: &Event, _: Rectangle, _: mouse::Cursor) -> Option<shader::Action<Message>> {
        match event {
            Event::Window(window::Event::RedrawRequested(now)) if self.lights.is_some() && !crate::shared::lyrics::reduced_motion() => {
                Some(shader::Action::request_redraw_at(*now + super::motion::ambient_slow_interval()))
            }
            _ => None,
        }
    }

    fn draw(&self, _: &(), _: mouse::Cursor, bounds: Rectangle) -> GlassPrimitive {
        let lights = self.lights.map(|colors| (colors, banner_clock()));
        let alpha = match (self.light_theme, lights.is_some()) {
            (true, true) => 0.70,
            (true, false) => 0.30,
            // Below 0.8 so secondary text keeps AA contrast over a light,
            // saturated cover (a pink one measured 1.3-2.5:1 at 0.92).
            (false, true) => 0.78,
            (false, false) => 0.42,
        };
        if let Ok(mut light) = PAGE_LIGHT.lock() {
            *light = lights.map(|(colors, elapsed)| PageLight {
                colors,
                elapsed,
                area: bounds,
                top: self.top,
                height: WASH_H,
                alpha,
                bg: theme::bg(),
            });
        }
        GlassPrimitive::new(current_scene(), Kind::Wash { top: self.top, height: WASH_H, alpha, lights }, self.light_theme)
    }
}

pub fn cover_backdrop<'a, Message: 'a>() -> Element<'a, Message> {
    shader(Paint { scene: current_scene(), kind: Kind::CoverBackdrop, light_theme: false }).width(Length::Fill).height(Length::Fill).into()
}

/// Depth of the scroll edge (logical px).
const EDGE: f32 = 22.0;

/// The scroll edge (for the top of the page's viewport, over the page): where
/// the page slides under the top bar it dissolves into the bar over 22 px,
/// instead of being cut by a rule. Only once the page has scrolled, and only
/// as far as it has (none at rest: nothing is under the bar then). Draws
/// nothing interactive; put it over the page in a stack.
pub fn scroll_edge<'a, Message: 'a>(app: &App) -> Element<'a, Message> {
    use iced::widget::{column, container, Space};
    let k = (app.page_scroll / EDGE).clamp(0.0, 1.0);
    let bg = theme::bg();
    let fade = container(Space::new().width(Length::Fill).height(EDGE)).style(move |_| container::Style {
        background: (k > 0.0).then(|| theme::gradient(180.0, Color { a: 0.94 * k, ..bg }, Color { a: 0.0, ..bg })),
        ..Default::default()
    });
    column![fade, Space::new().width(Length::Fill).height(Length::Fill)].into()
}

// ------------------------------------------------------------------ the widget

enum Source {
    Page,
    /// On the window's chrome (the top bar), not over the page: nothing but
    /// the plain bar is behind it, so it refracts no picture (neutral glass,
    /// lit by its rim and sheen; never a muddy cast of the cover).
    Chrome,
    /// The scene without the album banner: prominent glass, whose tint must
    /// not be thinned by the banner's light.
    Scene,
    PlayerBar(Option<[Color; 3]>),
    #[cfg(test)]
    Fixed(Scene),
}

pub struct Glass<'a, Message, Theme> {
    content: Element<'a, Message, Theme, iced::Renderer>,
    radius: f32,
    optics: Optics,
    shadow: Shadow,
    light_theme: bool,
    tint: Color,
    rim: Color,
    /// A fixed pointer light (tests and screenshots): (x, y) from -1 to 1.
    forced: Option<(f32, f32)>,
    source: Source,
    album: bool,
    /// Reacts to the pointer (controls only, see the module's roles).
    interactive: bool,
    /// How much of the pane is there (0 to 1): a selection that may move.
    presence: f32,
}

/// Corner radius of the large panes (headers, cards, dialogs).
pub const PANEL_R: f32 = 18.0;
/// Any radius of at least half the height: the shader clamps it to a capsule.
pub const CAPSULE: f32 = 999.0;

/// `content` on a liquid glass pane with corners of `radius` (logical px).
/// The pane is exactly the content's bounds; the layout is the content's.
pub fn glass<'a, Message, Theme>(
    content: impl Into<Element<'a, Message, Theme, iced::Renderer>>,
    radius: f32,
) -> Glass<'a, Message, Theme> {
    Glass {
        content: content.into(),
        radius,
        optics: Optics::PANEL,
        shadow: Shadow::default(),
        light_theme: theme::is_light(),
        tint: Color { a: 1.0, ..theme::glass_top() },
        rim: theme::glass_rim(0.0),
        forced: None,
        source: Source::Page,
        album: false,
        interactive: false,
        presence: 1.0,
    }
}

/// A capsule of glass holding one control or a group of them (back and
/// forward, the bar's tools): one pane for the group, so neighbours read as a
/// single piece of glass; it follows the pointer. `content` brings its own
/// padding (2 px around a row of buttons keeps their washes concentric).
pub fn control<'a, Message, Theme>(content: impl Into<Element<'a, Message, Theme, iced::Renderer>>) -> Glass<'a, Message, Theme> {
    glass(content, CAPSULE).optics(Optics::CONTROL).interactive().shadow(theme::control_shadow())
}

/// The tint and label colour of prominent glass for a page's primary action,
/// from its cover's palette: the most colourful of the three, its hue and
/// chroma kept as they are (chroma only capped, never raised), its lightness
/// moved just far enough for the label to read (AA, 4.5:1) and the pill to
/// stand out of its panel: light with a dark label on the dark theme; deep
/// with a white one on the light theme, but for warm hues (orange to
/// yellow-green), which stay light since darkened they turn brown or olive.
/// A grey cover, or none, gives the neutral prominent pill (white on dark,
/// ink on light).
pub fn prominent(palette: Option<[Color; 3]>) -> (Color, Color) {
    let neutral = (theme::button(), theme::button_text());
    let Some(colors) = palette else { return neutral };
    let lead = colors.into_iter().map(theme::to_oklch).max_by(|a, b| a.1.total_cmp(&b.1));
    let Some((l, c, h)) = lead.filter(|(_, c, _)| *c >= 0.035) else { return neutral };
    let warm = (35.0..135.0).contains(&h);
    let (tint, ink) = if warm || !theme::is_light() {
        // Light, with a dark label: on the dark theme it stands out of the
        // header as the white pill did; a warm hue never darkens to brown.
        (theme::oklch(l.clamp(0.78, 0.88), c.min(0.15), h), theme::rgb(0x14141a))
    } else {
        (theme::oklch(l.clamp(0.42, 0.52), c.min(0.15), h), Color::WHITE)
    };
    // Whatever the gamut did to it, the label must read.
    let other = if ink == Color::WHITE { theme::rgb(0x14141a) } else { Color::WHITE };
    if theme::contrast(ink, tint) >= theme::contrast(other, tint) {
        (tint, ink)
    } else {
        (tint, other)
    }
}

impl<Message, Theme> Glass<'_, Message, Theme> {
    /// How the pane bends and lights the scene (default [`Optics::PANEL`]).
    pub fn optics(mut self, optics: Optics) -> Self {
        self.optics = optics;
        self
    }
    /// An album's pane, lit from inside by its cover's colours (given with
    /// their banner roles, see `banner_palette`).
    pub fn album_light(mut self, palette: Option<[Color; 3]>) -> Self {
        if let Some(colors) = palette {
            self.tint = theme::bg();
            self.album = true;
            self.source = Source::PlayerBar(Some(super::gpu::glass::banner_palette(colors, self.light_theme)));
        }
        self
    }
    pub fn player_bar(mut self, palette: Option<[Color; 3]>) -> Self {
        self.optics = Optics::BAR;
        self.tint = theme::bg();
        self.source = Source::PlayerBar(palette);
        self
    }
    /// React to the pointer: a light that follows it, a glint along the rim
    /// and a press. For controls only (buttons, fields, tabs); panels hold still.
    /// Show the pane at `amount` (0 to 1) with its content always drawn: a
    /// selection (a sidebar's current row) keeps one widget tree whether it
    /// is selected or not, and an absent pane costs nothing.
    pub fn presence(mut self, amount: f32) -> Self {
        self.presence = amount.clamp(0.0, 1.0);
        self
    }
    pub fn interactive(mut self) -> Self {
        self.interactive = true;
        self
    }
    /// Prominent glass tinted `tint` (see [`prominent`]): a primary action.
    pub fn prominent(mut self, tint: Color) -> Self {
        self.tint = Color { a: 1.0, ..tint };
        self.optics = Optics::PROMINENT;
        self.source = Source::Scene;
        self
    }
    /// The pane sits on the window's chrome (the top bar), not over the page.
    pub fn on_chrome(mut self) -> Self {
        self.source = Source::Chrome;
        self
    }
    /// A drop shadow under the pane.
    pub fn shadow(mut self, shadow: Shadow) -> Self {
        self.shadow = shadow;
        self
    }
    /// Light the pane as if the pointer rested at (x, y) (-1..1 from the centre).
    #[cfg(test)]
    pub fn force_light(mut self, x: f32, y: f32) -> Self {
        self.forced = Some((x, y));
        self
    }
    /// Refract `scene` rather than the app's.
    #[cfg(test)]
    pub fn with_scene(mut self, scene: Scene) -> Self {
        self.source = Source::Fixed(scene);
        self
    }
    /// The light theme's glass, whatever the palette in use.
    #[cfg(test)]
    pub fn light_palette(mut self) -> Self {
        self.light_theme = true;
        self.tint = Color::from_rgb(0.965, 0.965, 0.97);
        self.rim = Color { a: 0.12, ..theme::rgb(0x14141a) };
        self
    }
}

#[derive(Debug, Clone, Copy)]
struct State {
    x: f32,
    y: f32,
    hover: f32,
    press: f32,
    over: bool,
    down: bool,
    last: Option<Instant>,
}

impl Default for State {
    fn default() -> Self {
        Self { x: 0.0, y: -0.5, hover: 0.0, press: 0.0, over: false, down: false, last: None }
    }
}

/// Exponential approach of `value` to `target` at `rate` per second, snapping
/// when close (GPUI's glass light).
fn approach(value: f32, target: f32, rate: f32, dt: f32) -> f32 {
    if crate::shared::lyrics::reduced_motion() {
        return target;
    }
    let next = target + (value - target) * (-rate * dt).exp();
    if (next - target).abs() < 0.004 {
        target
    } else {
        next
    }
}

fn normalized(offset: f32, size: f32) -> f32 {
    (2.0 * offset / size.max(1.0)).clamp(-1.0, 1.0)
}

impl State {
    /// One step of the light toward the pointer. True while still moving.
    fn step(&mut self, pointer: Option<Point>, bounds: Rectangle, dt: f32) -> bool {
        let hovered = self.over;
        let mut moving = false;
        if let (true, Some(p)) = (hovered, pointer) {
            let c = bounds.center();
            let (tx, ty) = (normalized(p.x - c.x, bounds.width), normalized(p.y - c.y, bounds.height));
            if self.hover == 0.0 {
                (self.x, self.y) = (tx, ty);
            } else {
                self.x = approach(self.x, tx, 46.0, dt);
                self.y = approach(self.y, ty, 46.0, dt);
            }
            moving = self.x != tx || self.y != ty;
        }
        let hover = if hovered { 1.0 } else { 0.0 };
        let press = if hovered && self.down { 1.0 } else { 0.0 };
        self.hover = approach(self.hover, hover, if hovered { 23.0 } else { 10.0 }, dt);
        self.press = approach(self.press, press, if press > 0.0 { 36.0 } else { 13.0 }, dt);
        moving || self.hover != hover || self.press != press
    }
}

fn shader_tree() -> Tree {
    Tree { tag: tree::Tag::of::<()>(), state: tree::State::new(()), children: Vec::new() }
}

impl<Message, Theme> Widget<Message, Theme, iced::Renderer> for Glass<'_, Message, Theme> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content), shader_tree()]
    }

    fn diff(&self, tree: &mut Tree) {
        if tree.children.len() != 2 {
            tree.children = self.children();
            return;
        }
        tree.children[0].diff(&self.content);
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
        if !self.interactive {
            // A still pane: only the player bar's ambient light asks for frames.
            if let Event::Window(window::Event::RedrawRequested(now)) = event {
                if matches!(self.source, Source::PlayerBar(Some(_))) && !crate::shared::lyrics::reduced_motion() {
                    shell.request_redraw_at(
                        *now + if self.album { super::motion::ambient_slow_interval() } else { super::motion::ambient_interval() },
                    );
                }
            }
        } else {
            let bounds = layout.bounds();
            let state = tree.state.downcast_mut::<State>();
            let over = cursor.position_over(bounds).is_some();
            let mut wake = false;
            match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if over => {
                    state.down = true;
                    state.press = state.press.max(0.55);
                    wake = true;
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.down => {
                    state.down = false;
                    wake = true;
                }
                // The light follows the pointer: a frame per move over the pane only.
                Event::Mouse(mouse::Event::CursorMoved { .. }) if over || state.hover > 0.0 => wake = true,
                Event::Mouse(mouse::Event::CursorLeft) if state.hover > 0.0 => wake = true,
                Event::Window(window::Event::RedrawRequested(now)) => {
                    if matches!(self.source, Source::PlayerBar(Some(_))) && !crate::shared::lyrics::reduced_motion() {
                        shell.request_redraw_at(
                            *now + if self.album { super::motion::ambient_slow_interval() } else { super::motion::ambient_interval() },
                        );
                    }
                    let dt = state.last.map_or(1.0 / 60.0, |l| now.saturating_duration_since(l).as_secs_f32()).clamp(1.0 / 240.0, 0.05);
                    state.over = over;
                    if !over {
                        state.down = false;
                    }
                    if state.step(cursor.position(), bounds, dt) {
                        state.last = Some(*now);
                        shell.request_redraw();
                    } else {
                        state.last = None;
                    }
                }
                _ => {}
            }
            if over != state.over {
                wake = true;
            }
            if wake {
                shell.request_redraw();
            }
        }
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<State>();
        let light = match self.forced {
            Some((x, y)) => Light { x, y, hover: 1.0, press: 0.0 },
            None if !self.interactive => Light { x: 0.0, y: -0.5, hover: 0.0, press: 0.0 },
            None => {
                let h = state.hover.clamp(0.0, 1.0);
                Light { x: state.x, y: state.y, hover: h * h * (3.0 - 2.0 * h), press: state.press }
            }
        };
        // Inside a floating surface that is fading in (`wash::enter`), the
        // pane and its shadow fade with the content.
        let fade = super::wash::fade() * self.presence;
        let shadow = Shadow { color: Color { a: self.shadow.color.a * fade, ..self.shadow.color }, ..self.shadow };
        let (scene, bar_light, page_light) = match &self.source {
            Source::Page => (current_scene(), None, PAGE_LIGHT.lock().ok().and_then(|g| *g)),
            Source::Chrome => (None, None, None),
            Source::Scene => (current_scene(), None, None),
            Source::PlayerBar(palette) => (
                None,
                palette.map(|colors| {
                    static START: OnceLock<Instant> = OnceLock::new();
                    let elapsed = if self.album {
                        banner_clock()
                    } else if crate::shared::lyrics::reduced_motion() {
                        0.0
                    } else {
                        START.get_or_init(Instant::now).elapsed().as_secs_f32()
                    };
                    (colors, elapsed)
                }),
                None,
            ),
            #[cfg(test)]
            Source::Fixed(scene) => (Some(scene.clone()), None, None),
        };
        let reach = shadow.blur_radius + shadow.offset.x.abs().max(shadow.offset.y.abs()) + 2.0;
        let pane = bounds.expand(reach).intersection(viewport).filter(|_| fade > 0.0);
        if let Some(clip) = pane {
            renderer.with_layer(clip, |renderer| {
                if shadow.color.a > 0.0 {
                    renderer.fill_quad(
                        Quad { bounds, border: Border { radius: self.radius.into(), ..Default::default() }, shadow, ..Default::default() },
                        Color::TRANSPARENT,
                    );
                }
                // One pixel of margin so the antialiased edge is never cut by the viewport.
                let outer = bounds.expand(1.0);
                let node = layout::Node::new(outer.size()).move_to(outer.position());
                let paint = shader::Shader::<Message, Paint>::new(Paint {
                    scene,
                    kind: Kind::Glass {
                        radius: self.radius,
                        inset: 1.0,
                        optics: self.optics,
                        light,
                        tint: self.tint,
                        rim: self.rim,
                        opacity: fade,
                        bar_light,
                        album_light: self.album,
                        page_light,
                    },
                    light_theme: self.light_theme,
                });
                Widget::<Message, Theme, iced::Renderer>::draw(
                    &paint,
                    &tree.children[1],
                    renderer,
                    theme,
                    style,
                    Layout::new(&node),
                    cursor,
                    viewport,
                );
            });
        }
        // The content in a layer of its own, so its backgrounds are drawn over
        // the pane; with no pane drawn, no layer either.
        if pane.is_some() {
            renderer.with_layer(*viewport, |renderer| {
                self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
            });
        } else {
            self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
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
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

impl<'a, Message: 'a, Theme: 'a> From<Glass<'a, Message, Theme>> for Element<'a, Message, Theme, iced::Renderer> {
    fn from(g: Glass<'a, Message, Theme>) -> Self {
        Element::new(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_follows_the_pointer_and_settles() {
        let bounds = Rectangle { x: 0.0, y: 0.0, width: 200.0, height: 100.0 };
        let mut s = State { over: true, ..State::default() };
        let pointer = Some(Point::new(150.0, 25.0));
        let mut frames = 0;
        while s.step(pointer, bounds, 1.0 / 60.0) {
            frames += 1;
            assert!(frames < 120, "the light must settle");
        }
        assert_eq!((s.x, s.y, s.hover), (0.5, -0.5, 1.0));
        // Away: it fades out and stops asking for frames.
        s.over = false;
        while s.step(None, bounds, 1.0 / 60.0) {
            frames += 1;
            assert!(frames < 300);
        }
        assert_eq!(s.hover, 0.0);
        assert!(!s.step(None, bounds, 1.0 / 60.0), "idle: no more frames");
    }

    #[test]
    fn prominent_glass_keeps_the_cover_hue_and_a_readable_label() {
        let _theme = theme::THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for light in [false, true] {
            theme::set_light(light);
            // A grey cover (or none): the neutral prominent pill.
            assert_eq!(prominent(None), (theme::button(), theme::button_text()));
            let grey = [Color::from_rgb(0.5, 0.5, 0.5), Color::from_rgb(0.2, 0.2, 0.21), Color::from_rgb(0.9, 0.9, 0.9)];
            assert_eq!(prominent(Some(grey)), (theme::button(), theme::button_text()));
            for hue in (0..360).step_by(15) {
                for (l, c) in [(0.3, 0.06), (0.55, 0.12), (0.7, 0.2), (0.9, 0.05)] {
                    let lead = theme::oklch(l, c, hue as f32);
                    let (tint, ink) = prominent(Some([grey[0], lead, grey[1]]));
                    let ratio = theme::contrast(ink, tint);
                    assert!(ratio >= 4.5, "label {ratio:.2}:1 on hue {hue} l {l} c {c} (light {light})");
                    // Truthful: the hue of the cover, its chroma never raised.
                    let (_, c0, h0) = theme::to_oklch(lead);
                    let (tl, tc, th) = theme::to_oklch(tint);
                    let dh = (th - h0).rem_euclid(360.0).min((h0 - th).rem_euclid(360.0));
                    assert!(c0 < 0.035 || dh < 10.0, "hue {h0} became {th}");
                    assert!(tc <= c0.min(0.15) + 0.01, "chroma {c0} raised to {tc}");
                    // Warm hues never darken into brown or olive.
                    if (35.0..135.0).contains(&h0) {
                        assert!(tl >= 0.75, "warm hue {h0} at lightness {tl}");
                    }
                }
            }
        }
        theme::set_light(false);
    }

    /// A colourful, photograph-like test picture: stripes of different hues,
    /// diagonal bands, a few soft discs and a gradient sky.
    fn test_scene() -> Scene {
        let img = image::RgbaImage::from_fn(512, 512, |x, y| {
            let (u, v) = (x as f32 / 511.0, y as f32 / 511.0);
            let stripe = |period: f32, phase: f32| (0.5 + 0.5 * ((x as f32 + phase) / period * std::f32::consts::TAU).sin()).powf(0.6);
            let diag = 0.5 + 0.5 * (((x + y) as f32) / 19.0).sin();
            let mut r = 0.15 + 0.75 * stripe(24.0, 0.0) * (1.0 - v * 0.4);
            let mut g = 0.10 + 0.45 * diag * v + 0.25 * (1.0 - v);
            let mut b = 0.20 + 0.70 * stripe(24.0, 12.0) * (0.4 + 0.6 * u);
            for (cx, cy, rad, c) in
                [(0.3, 0.35, 0.12, [1.0, 0.85, 0.2]), (0.7, 0.6, 0.15, [0.2, 0.9, 0.6]), (0.5, 0.2, 0.08, [1.0, 0.3, 0.6])]
            {
                let d = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() / rad;
                let a = (1.0 - d).clamp(0.0, 1.0).powf(0.5);
                r = r * (1.0 - a) + c[0] * a;
                g = g * (1.0 - a) + c[1] * a;
                b = b * (1.0 - a) + c[2] * a;
            }
            let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0) as u8;
            image::Rgba([q(r), q(g), q(b), 255])
        });
        Scene { key: "test:stripes".into(), image: Arc::new(SceneImage::from_image(&image::DynamicImage::ImageRgba8(img))) }
    }

    const LOGICAL: Size = Size::new(640.0, 400.0);
    const SCALE: f32 = 1.25;
    const PAD: f32 = 60.0;

    fn physical() -> (u32, u32) {
        ((LOGICAL.width * SCALE) as u32, (LOGICAL.height * SCALE) as u32)
    }

    /// The scene (its sharpest level) behind the whole window and a glass
    /// pane over it, 60 px in from every side, at a scale factor of 1.25.
    fn render(
        renderer: &mut iced::Renderer,
        scene: &Scene,
        pane: impl Fn(Element<'static, ()>) -> Glass<'static, (), iced::Theme>,
    ) -> Vec<u8> {
        use iced::advanced::renderer::Headless;
        use iced::advanced::Renderer as _;
        use iced::widget::{column, container, image as picture, stack, text};
        let back = picture(picture::Handle::from_rgba(scene.image.size, scene.image.size, scene.image.levels[0].clone()))
            .content_fit(iced::ContentFit::Cover)
            .width(Length::Fill)
            .height(Length::Fill);
        let label = column![
            text("Liquid glass").size(26).color(Color::WHITE),
            text("refraction, dispersion, live rim light").size(13).color(Color::from_rgb(0.8, 0.8, 0.85)),
        ]
        .spacing(4);
        let inside: Element<'static, ()> = container(label).padding(24).width(Length::Fill).height(Length::Fill).into();
        let mut root: Element<'static, ()> =
            stack![back, container(pane(inside).shadow(theme::popover_shadow())).padding(PAD).width(Length::Fill).height(Length::Fill)]
                .into();
        let mut tree = Tree::new(root.as_widget());
        let node = root.as_widget_mut().layout(&mut tree, renderer, &layout::Limits::new(LOGICAL, LOGICAL));
        let frame = Rectangle::with_size(LOGICAL);
        renderer.reset(frame);
        root.as_widget().draw(
            &tree,
            renderer,
            &iced::Theme::Dark,
            &Default::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &frame,
        );
        let (w, h) = physical();
        Headless::screenshot(renderer, Size::new(w, h), SCALE, Color::BLACK)
    }

    fn save(name: &str, pixels: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        let (w, h) = physical();
        image::save_buffer(&path, pixels, w, h, image::ColorType::Rgba8).expect("write the PNG");
        path
    }

    /// Mean absolute difference per channel (of 255) of two shots over a block of physical px.
    fn diff(a: &[u8], b: &[u8], xs: std::ops::Range<u32>, ys: std::ops::Range<u32>) -> [f32; 3] {
        let (w, _) = physical();
        let mut sum = [0f64; 3];
        let mut n = 0f64;
        for y in ys {
            for x in xs.clone() {
                let i = ((y * w + x) * 4) as usize;
                for (c, total) in sum.iter_mut().enumerate() {
                    *total += (f64::from(a[i + c]) - f64::from(b[i + c])).abs();
                }
                n += 1.0;
            }
        }
        sum.map(|s| (s / n.max(1.0)) as f32)
    }

    #[test]
    #[ignore = "requires a wgpu adapter; renders offscreen"]
    fn glass_refracts_and_disperses_the_scene_offscreen() {
        use iced::advanced::renderer::Headless;
        let mut renderer =
            iced::futures::executor::block_on(<iced::Renderer as Headless>::new(super::super::INTER, iced::Pixels(16.0), None))
                .expect("wgpu adapter for the offscreen glass test");
        let scene = test_scene();
        let with = |optics: Optics| {
            let scene = scene.clone();
            move |c: Element<'static, ()>| glass(c, 22.0).with_scene(scene.clone()).optics(optics)
        };
        let lit = render(&mut renderer, &scene, move |c| with(Optics::PANEL)(c).force_light(0.55, -0.45));
        let light = render(&mut renderer, &scene, move |c| with(Optics::PANEL)(c).light_palette().force_light(-0.3, -0.6));
        let normal = render(&mut renderer, &scene, with(Optics::PANEL));
        let flat = render(&mut renderer, &scene, with(Optics { refraction: 0.0, dispersion: 0.0, magnify: 0.0, ..Optics::PANEL }));
        let mono = render(&mut renderer, &scene, with(Optics { dispersion: 0.0, ..Optics::PANEL }));
        let path = save("abyss-glass-test.png", &lit);
        save("abyss-glass-test-light.png", &light);
        save("abyss-glass-test-flat.png", &flat);
        eprintln!("liquid glass test images: {} (and -light.png, -flat.png)", path.display());

        // The pane in physical px: 75..725 x 75..425. The rim band is 3..16 px
        // inside the left and right edges, on rows clear of the corners and label.
        let p0 = (PAD * SCALE) as u32;
        let (x1, y1) = (((LOGICAL.width - PAD) * SCALE) as u32, ((LOGICAL.height - PAD) * SCALE) as u32);
        let rows = (p0 + 150)..(y1 - 60);
        let left = diff(&normal, &flat, (p0 + 3)..(p0 + 16), rows.clone());
        let right = diff(&normal, &flat, (x1 - 16)..(x1 - 3), rows.clone());
        let rim = (left.iter().sum::<f32>() + right.iter().sum::<f32>()) / 6.0;
        // The middle of the pane is (nearly) straight through.
        let (cx, cy) = ((p0 + x1) / 2, (p0 + 150 + y1 - 60) / 2);
        let middle = diff(&normal, &flat, (cx - 8)..(cx + 8), (cy - 8)..(cy + 8));
        let mid = middle.iter().sum::<f32>() / 3.0;
        eprintln!("refraction: rim {rim:.2} vs middle {mid:.2} (mean |difference| from unbent glass, of 255)");
        assert!(rim > 6.0, "the rim must bend the scene: {left:?} {right:?}");
        assert!(rim > mid * 3.0, "bending must be strongest at the rim: rim {rim} middle {mid}");

        // Dispersion: red and blue move apart, green (the reference) does not.
        let d = diff(&normal, &mono, (p0 + 3)..(p0 + 16), rows.clone());
        let e = diff(&normal, &mono, (x1 - 16)..(x1 - 3), rows);
        let (r, g, b) = ((d[0] + e[0]) / 2.0, (d[1] + e[1]) / 2.0, (d[2] + e[2]) / 2.0);
        eprintln!("dispersion at the rim: r {r:.2} g {g:.2} b {b:.2}");
        assert!(r > 1.5 && b > 1.5, "red and blue must separate at the rim: {r} {g} {b}");
        assert!(r > g * 2.0 && b > g * 2.0, "green is the reference channel: {r} {g} {b}");

        // Frosting follows the theme: light glass is light, dark glass dark.
        let black = vec![0u8; lit.len()];
        let mean = |px: &[u8]| diff(px, &black, (cx - 40)..(cx + 40), (cy - 20)..(cy + 20)).iter().sum::<f32>() / 3.0;
        assert!(mean(&light) > mean(&lit) + 60.0, "light {} dark {}", mean(&light), mean(&lit));

        let palette = [Color::from_rgb(1.0, 0.0, 0.0), Color::from_rgb(0.0, 1.0, 0.0), Color::from_rgb(0.0, 0.0, 1.0)];
        let bar = render(&mut renderer, &scene, |c| glass(c, 22.0).player_bar(Some(palette)));
        let left = diff(&bar, &black, (p0 + 60)..(p0 + 100), (cy - 10)..(cy + 10));
        let middle = diff(&bar, &black, (cx - 30)..(cx + 30), (cy - 10)..(cy + 10));
        let right = diff(&bar, &black, (x1 - 100)..(x1 - 60), (cy - 10)..(cy + 10));
        assert!(left[0] > left[2] + 10.0 && right[2] > right[0] + 10.0, "bar palette: {left:?} {right:?}");
        assert!(middle[1] > middle[0] + 10.0 && middle[1] > middle[2] + 10.0, "third bar colour: {middle:?}");
    }

    #[test]
    #[ignore = "requires a wgpu adapter; renders offscreen"]
    fn a_faint_rainbow_runs_along_the_rim_over_a_plain_scene_offscreen() {
        use iced::advanced::renderer::Headless;
        let mut renderer =
            iced::futures::executor::block_on(<iced::Renderer as Headless>::new(super::super::INTER, iced::Pixels(16.0), None))
                .expect("wgpu adapter for the offscreen glass test");
        let grey = image::RgbaImage::from_pixel(256, 256, image::Rgba([70, 70, 72, 255]));
        let scene = Scene { key: "test:grey".into(), image: Arc::new(SceneImage::from_image(&image::DynamicImage::ImageRgba8(grey))) };
        let s = scene.clone();
        let shot = render(&mut renderer, &scene, move |c| glass(c, 22.0).with_scene(s.clone()).optics(Optics::PANEL));
        eprintln!("rim rainbow test image: {}", save("abyss-glass-rim-test.png", &shot).display());
        // Each edge's bevel tints a different hue; on grey that is the only colour there is.
        let p0 = (PAD * SCALE) as u32;
        let (x1, y1) = (((LOGICAL.width - PAD) * SCALE) as u32, ((LOGICAL.height - PAD) * SCALE) as u32);
        let (cx, cy) = ((p0 + x1) / 2, (p0 + y1) / 2);
        let chroma = |xs: std::ops::Range<u32>, ys: std::ops::Range<u32>| {
            let (w, _) = physical();
            let (mut sum, mut n) = (0f32, 0f32);
            for y in ys {
                for x in xs.clone() {
                    let i = ((y * w + x) * 4) as usize;
                    let c = [shot[i], shot[i + 1], shot[i + 2]].map(f32::from);
                    sum += c.iter().cloned().fold(0.0, f32::max) - c.iter().cloned().fold(255.0, f32::min);
                    n += 1.0;
                }
            }
            sum / n
        };
        let rim = [
            chroma((p0 + 6)..(p0 + 30), (cy - 20)..(cy + 20)),
            chroma((x1 - 30)..(x1 - 6), (cy - 20)..(cy + 20)),
            chroma((cx - 20)..(cx + 20), (p0 + 6)..(p0 + 30)),
            chroma((cx - 20)..(cx + 20), (y1 - 30)..(y1 - 6)),
        ];
        let middle = chroma((cx - 20)..(cx + 20), (cy - 20)..(cy + 20));
        eprintln!("rim chroma {rim:?} vs middle {middle:.2}");
        let mean = rim.iter().sum::<f32>() / 4.0;
        assert!(mean > middle + 2.0, "the rim must carry a little colour: {rim:?} vs {middle}");
        assert!(rim.iter().all(|c| *c < 30.0), "but only a little: {rim:?}");
    }

    /// Draws `root` over the whole test frame.
    fn shoot(renderer: &mut iced::Renderer, mut root: Element<'static, ()>) -> Vec<u8> {
        use iced::advanced::renderer::Headless;
        use iced::advanced::Renderer as _;
        let mut tree = Tree::new(root.as_widget());
        let node = root.as_widget_mut().layout(&mut tree, renderer, &layout::Limits::new(LOGICAL, LOGICAL));
        let frame = Rectangle::with_size(LOGICAL);
        renderer.reset(frame);
        root.as_widget().draw(
            &tree,
            renderer,
            &iced::Theme::Dark,
            &Default::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &frame,
        );
        let (w, h) = physical();
        Headless::screenshot(renderer, Size::new(w, h), SCALE, Color::BLACK)
    }

    /// An album page's top (the banner, the cover, a glass panel over the
    /// banner) for real covers from a cache folder (`ABYSS_COVERS`): the
    /// greyest, the most colourful, the darkest and three in between, tiled
    /// into `%TEMP%\abyss-banner-sheet.png` for judging by eye.
    #[test]
    #[ignore = "requires a wgpu adapter and a folder of covers; renders offscreen"]
    fn album_banner_contact_sheet_offscreen() {
        use iced::advanced::renderer::Headless;
        use iced::widget::{column, container, image as picture, row, stack, text, Space};
        let Ok(dir) = std::env::var("ABYSS_COVERS") else { return };
        let mut covers: Vec<(f32, f32, Vec<u8>)> = std::fs::read_dir(dir)
            .expect("covers folder")
            .filter_map(|e| std::fs::read(e.ok()?.path()).ok())
            .filter(|b| (12_000..400_000).contains(&b.len()))
            .filter_map(|bytes| {
                let img = image::load_from_memory(&bytes).ok()?;
                if img.width() < 300 || img.width() != img.height() {
                    return None;
                }
                let small = img.thumbnail(24, 24).to_rgb8();
                let (mut s, mut v) = (0f32, 0f32);
                for p in small.pixels() {
                    let [r, g, b] = p.0.map(|c| c as f32 / 255.0);
                    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
                    s += if max > 0.0 { (max - min) / max } else { 0.0 };
                    v += max;
                }
                let n = small.pixels().len() as f32;
                Some((s / n, v / n, bytes))
            })
            .collect();
        assert!(covers.len() >= 6, "need covers");
        covers.sort_by(|a, b| a.0.total_cmp(&b.0));
        let n = covers.len();
        let darkest = (0..n).min_by(|&a, &b| covers[a].1.total_cmp(&covers[b].1)).unwrap();
        let mut picks = vec![0, n / 5, 2 * n / 5, 3 * n / 5, n - 1, darkest];
        picks.dedup();
        let mut renderer =
            iced::futures::executor::block_on(<iced::Renderer as Headless>::new(super::super::INTER, iced::Pixels(16.0), None))
                .expect("wgpu adapter");
        let (w, h) = physical();
        let mut sheet = image::RgbaImage::new(w * 2, h * 3);
        for (k, &i) in picks.iter().enumerate().take(6) {
            let bytes = covers[i].2.clone();
            let raw: [Color; 3] = super::super::artwork::palette_of(&bytes).unwrap().try_into().unwrap();
            let palette = super::super::gpu::glass::banner_palette(raw, false);
            let scene = Scene { key: format!("sheet{k}"), image: Arc::new(SceneImage::palette(raw)) };
            *SCENE.lock().unwrap() = Some(scene);
            let cover = picture(picture::Handle::from_bytes(bytes)).width(150).height(150);
            let swatches = row(raw.into_iter().chain(palette).map(|c| {
                container(Space::new().width(18).height(18))
                    .style(move |_| container::Style { background: Some(c.into()), ..Default::default() })
                    .into()
            }))
            .spacing(4);
            let head = row![
                cover,
                column![
                    text("Album title").size(28).color(theme::text()),
                    text("Artist").size(14).color(theme::text_secondary()),
                    swatches
                ]
                .spacing(8)
            ]
            .spacing(20)
            .align_y(iced::Alignment::End);
            let rows = column((1..=4).map(|t| text(format!("{t}   Track name")).size(13).color(theme::text()).into())).spacing(10);
            let panel = glass(container(rows).padding(16).width(Length::Fill), theme::PANEL_R).optics(Optics::PANEL);
            let root: Element<'static, ()> = stack![
                container(Space::new().width(Length::Fill).height(Length::Fill))
                    .style(|_| container::Style { background: Some(theme::bg().into()), ..Default::default() }),
                shader(Wash { top: 0.0, light_theme: false, lights: Some(palette) }).width(Length::Fill).height(Length::Fill),
                column![head, panel].spacing(22).padding(28),
            ]
            .into();
            let shot = shoot(&mut renderer, root);
            let tile = image::RgbaImage::from_raw(w, h, shot).unwrap();
            image::imageops::overlay(&mut sheet, &tile, i64::from((k as u32 % 2) * w), i64::from((k as u32 / 2) * h));
        }
        let path = std::env::temp_dir().join("abyss-banner-sheet.png");
        sheet.save(&path).unwrap();
        eprintln!("banner sheet: {}", path.display());
    }

    /// The banner at four moments 1.5 s apart, stacked into
    /// `%TEMP%byss-banner-flow.png`, to judge how much the liquid flow moves.
    #[test]
    #[ignore = "requires a wgpu adapter; renders offscreen"]
    fn banner_flow_frames_offscreen() {
        use iced::advanced::renderer::Headless;
        use iced::widget::{container, stack, Space};
        let mut renderer =
            iced::futures::executor::block_on(<iced::Renderer as Headless>::new(super::super::INTER, iced::Pixels(16.0), None))
                .expect("wgpu adapter");
        let raw = [Color::from_rgb(0.62, 0.40, 0.27), Color::from_rgb(0.36, 0.55, 0.72), Color::from_rgb(0.95, 0.95, 0.95)];
        let palette = super::super::gpu::glass::banner_palette(raw, false);
        let (w, h) = physical();
        let mut sheet = image::RgbaImage::new(w, h * 4);
        let mut shots = Vec::new();
        for k in 0..4u32 {
            let t = 40.0 + 1.5 * k as f32;
            let paint = Paint {
                scene: None,
                kind: Kind::Wash { top: 0.0, height: WASH_H, alpha: 0.92, lights: Some((palette, t)) },
                light_theme: false,
            };
            let root: Element<'static, ()> = stack![
                container(Space::new().width(Length::Fill).height(Length::Fill))
                    .style(|_| container::Style { background: Some(theme::bg().into()), ..Default::default() }),
                shader(paint).width(Length::Fill).height(Length::Fill),
            ]
            .into();
            let shot = shoot(&mut renderer, root);
            image::imageops::overlay(&mut sheet, &image::RgbaImage::from_raw(w, h, shot.clone()).unwrap(), 0, i64::from(k * h));
            shots.push(shot);
        }
        let path = std::env::temp_dir().join("abyss-banner-flow.png");
        sheet.save(&path).unwrap();
        let change = diff(&shots[0], &shots[1], 0..w, 0..(h / 2));
        eprintln!("banner flow frames: {} ; mean change over 1.5 s {change:?}", path.display());
        // It moves, but quietly: a few levels of 255 on average over 1.5 s.
        let mean = change.iter().sum::<f32>() / 3.0;
        assert!(mean > 0.3 && mean < 12.0, "flow change {change:?}");
    }

    #[test]
    #[ignore = "requires a wgpu adapter; renders offscreen"]
    fn album_band_shows_the_cover_colours_brightly_offscreen() {
        use iced::advanced::renderer::Headless;
        use iced::advanced::Renderer as _;
        let mut renderer =
            iced::futures::executor::block_on(<iced::Renderer as Headless>::new(super::super::INTER, iced::Pixels(16.0), None))
                .expect("wgpu adapter for the offscreen band test");
        // A dark album: deep red, teal and violet, as `palette_of` returns them.
        let palette = [Color::from_rgb(0.86, 0.22, 0.20), Color::from_rgb(0.15, 0.70, 0.66), Color::from_rgb(0.52, 0.30, 0.90)];
        let wash = Wash { top: 0.0, light_theme: false, lights: Some(palette) };
        let mut root: Element<'static, ()> = iced::widget::stack![
            iced::widget::container(iced::widget::Space::new().width(Length::Fill).height(Length::Fill))
                .style(|_| iced::widget::container::Style { background: Some(Color::BLACK.into()), ..Default::default() }),
            shader(wash).width(Length::Fill).height(Length::Fill),
        ]
        .into();
        let mut tree = Tree::new(root.as_widget());
        let node = root.as_widget_mut().layout(&mut tree, &renderer, &layout::Limits::new(LOGICAL, LOGICAL));
        let frame = Rectangle::with_size(LOGICAL);
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
        let (w, h) = physical();
        let shot = Headless::screenshot(&mut renderer, Size::new(w, h), SCALE, Color::BLACK);
        eprintln!("album band test image: {}", save("abyss-album-band-test.png", &shot).display());
        let black = vec![0u8; shot.len()];
        let top = diff(&shot, &black, 0..w, 10..60);
        let lum = (top[0] + top[1] + top[2]) / 3.0;
        eprintln!("band near the top: {top:?} (mean {lum:.1} of 255)");
        assert!(lum > 70.0, "the band must glow, not sit dark: {top:?}");
        // Its colours differ across the width (the three glows, not one tint).
        let left = diff(&shot, &black, 0..(w / 4), 20..70);
        let right = diff(&shot, &black, (3 * w / 4)..w, 20..70);
        let spread: f32 = left.iter().zip(right).map(|(a, b)| (a - b).abs()).sum();
        assert!(spread > 25.0, "left {left:?} right {right:?}");
        // And it fades out downward.
        let low = diff(&shot, &black, 0..w, (h - 20)..h);
        assert!(low.iter().sum::<f32>() < top.iter().sum::<f32>() * 0.35, "fade: top {top:?} bottom {low:?}");
    }
}
