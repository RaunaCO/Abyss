//! The main panel: the top bar, every page's content, the side panel (queue
//! or lyrics) and the full-window overlays (search, cover, info, dialogs).
//!
//! Long lists (Library tracks, big playlists, album grids) are virtualised:
//! only the rows near the visible part of the page are built.

use std::sync::Arc;

use iced::widget::{button, column, container, mouse_area, row, scrollable, stack, text, text_input, Space};
use iced::{alignment, Alignment, Border, Color, Element, Length, Padding};

use super::icons::Icon;
use super::library::lib_section;
use super::rows::{track_row, RowStyle, Source};
use super::widgets::{self, card, cover, error_message, fmt_hms, fmt_long, icon_button, line, loading, pill, scale, section_title};
use super::{theme, App, Dialog, MenuTarget, Message, RemoveKind};
use crate::shared::{Load, Page};
use crate::tidal::{share_url, *};

pub const PAGE_SCROLL: &str = "abyss-page";
pub const DIALOG_TITLE_ID: &str = "abyss-dialog-title";

/// Height of a track row including its margins.
const TRACK_ROW_H: f32 = super::rows::ROW_PITCH;
/// Space above every page's content column (GPUI: 22 px with the title's own 4).
pub const PAGE_TOP: f32 = 18.0;

pub fn scroll_style(_: &iced::Theme, status: scrollable::Status) -> scrollable::Style {
    let (horizontal, vertical) = match status {
        scrollable::Status::Hovered { is_horizontal_scrollbar_hovered, is_vertical_scrollbar_hovered, .. } => {
            (is_horizontal_scrollbar_hovered, is_vertical_scrollbar_hovered)
        }
        scrollable::Status::Dragged { is_horizontal_scrollbar_dragged, is_vertical_scrollbar_dragged, .. } => {
            (is_horizontal_scrollbar_dragged, is_vertical_scrollbar_dragged)
        }
        _ => (false, false),
    };
    let rail = |hovered| scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: theme::ink(if hovered { 0.50 } else { 0.25 }).into(),
            border: Border { radius: 3.0.into(), ..Default::default() },
        },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail(vertical),
        horizontal_rail: rail(horizontal),
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: theme::popover().into(),
            border: Border::default(),
            shadow: iced::Shadow::default(),
            icon: theme::text(),
        },
    }
}

fn thin_scrollbar() -> scrollable::Scrollbar {
    scrollable::Scrollbar::new().width(6).scroller_width(6).margin(2)
}

/// Width of the page body: the window less the rail, the frame and the
/// panel's 1 px borders, and the side panel (GPUI's `page_body_width`).
fn page_body_width(app: &App) -> f32 {
    let rail = if app.signed_in { super::sidebar::RAIL_W } else { theme::FRAME_GAP };
    let side = if app.show_queue || app.show_lyrics { side_panel_width(app) } else { 0.0 };
    (app.viewport.width - rail - theme::FRAME_GAP - 2.0 - side).max(1.0)
}

/// Width available to the page's content column (design px), as GPUI's
/// `column_width`: the body less its inset on each side, up to the page's limit.
pub fn content_width(app: &App) -> f32 {
    let body = page_body_width(app);
    let limit =
        if app.page == Page::Settings { (body * 0.62).clamp(theme::CONTENT_W, 1280.0) } else { (body * 0.8).clamp(theme::WIDE_W, 1900.0) };
    (body - 2.0 * page_inset(body)).clamp(1.0, limit)
}

/// GPUI's `content_inset`.
fn page_inset(body: f32) -> f32 {
    (body * 0.025).clamp(8.0, 28.0)
}

fn side_panel_width(app: &App) -> f32 {
    let body = side_body(app);
    let (min, max) = side_limits(app);
    let s = app.settings.lock();
    s.side_panel_width.map_or_else(
        || if s.wide_side_panel { (theme::QUEUE_W * 1.55).min(0.58 * body) } else { theme::QUEUE_W.min(0.44 * body) },
        |w| w.clamp(min, max),
    )
}

fn side_body(app: &App) -> f32 {
    let rail = if app.signed_in { super::sidebar::RAIL_W } else { 0.0 };
    (app.viewport.width - rail - theme::FRAME_GAP).max(0.0)
}

/// Narrowest and widest the side panel may be dragged to.
pub fn side_limits(app: &App) -> (f32, f32) {
    let body = side_body(app);
    (240.0_f32.min(body * 0.44), body * 0.64)
}

fn grid_metrics(width: f32) -> (usize, f32) {
    let gap = theme::GAP;
    let cols = (((width + gap) / (theme::CARD_W + gap)).floor() as usize).max(1);
    let card_w = (width - gap * (cols as f32 - 1.0)) / cols as f32;
    (cols, card_w)
}

/// The visible range of `count` rows `row_h` tall that start `y0` px into the page.
fn visible_rows(app: &App, y0: f32, row_h: f32, count: usize) -> (usize, usize) {
    let top = (app.page_scroll - y0 - 400.0).max(0.0);
    // Until the page is first scrolled its height is unknown: the window's is enough.
    let bottom = app.page_scroll + app.page_view_h.max(app.viewport.height) + 400.0 - y0;
    let first = ((top / row_h).floor() as usize).min(count);
    let last = ((bottom / row_h).ceil().max(0.0) as usize).min(count);
    (first, last.max(first))
}

/// Rows `first..last` of a list with spacers for the rest.
fn virtual_list<'a>(
    count: usize,
    row_h: f32,
    (first, last): (usize, usize),
    mut build: impl FnMut(usize) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let mut col = column![].width(Length::Fill);
    if first > 0 {
        col = col.push(Space::new().height(first as f32 * row_h));
    }
    for i in first..last {
        col = col.push(container(build(i)).height(row_h).width(Length::Fill));
    }
    if last < count {
        col = col.push(Space::new().height((count - last) as f32 * row_h));
    }
    col.into()
}

/// The page is still loading its main content.
pub fn page_loading(app: &App) -> bool {
    match &app.page {
        Page::Home if app.home_tab == 0 => matches!(app.home, Load::Loading),
        Page::Home => matches!(app.home_extra.get(&app.home_tab), Some(Load::Loading)),
        Page::Explore => matches!(app.explore, Load::Loading),
        Page::Album(id) => matches!(app.albums.get(id), Some(Load::Loading)),
        Page::Artist(id) => matches!(app.artists.get(id), Some(Load::Loading)),
        Page::Playlist(u) => matches!(app.playlists.get(u), Some(Load::Loading)),
        Page::Mix(m) => matches!(app.mixes.get(m), Some(Load::Loading)),
        Page::Search(q) => matches!(app.search.get(q), Some(Load::Loading)),
        Page::MyTracks => matches!(app.fav_tracks, Load::Loading) && app.local_tracks.is_empty(),
        Page::MyAlbums => matches!(app.fav_albums, Load::Loading) && app.local_albums.is_empty(),
        Page::MyArtists => matches!(app.fav_artists, Load::Loading) && app.local_artists.is_empty(),
        Page::MyPlaylists => matches!(app.my_playlists, Load::Loading),
        Page::MyMixes | Page::Discover => matches!(app.my_mixes, Load::Loading),
        _ => false,
    }
}

// ----------------------------------------------------------------- top bar

fn crumbs(app: &App) -> Vec<(String, Option<Page>)> {
    let here = |label: &str| (label.to_string(), None);
    let t = |key: &str| crate::i18n::t(key);
    let home = (t("home").to_string(), Some(Page::Home));
    let for_you = (t("for_you").to_string(), Some(Page::Replay));
    match &app.page {
        Page::Home => vec![here(t("home"))],
        Page::MyTracks | Page::MyAlbums | Page::MyArtists => vec![home, here(t("library"))],
        Page::Recent => vec![home, here(t("history"))],
        Page::Explore => vec![home, here(t("explore"))],
        Page::Browse(_, title) => vec![home, here(title)],
        Page::Search(q) if q.is_empty() => vec![home, here(t("search"))],
        Page::Search(q) => vec![home, (crate::i18n::text("results_for", std::slice::from_ref(q)), None)],
        Page::Album(id) => vec![home, here(app.albums.get(id).and_then(Load::ready).map_or(t("album"), |(a, _)| a.title.as_str()))],
        Page::Artist(id) => vec![home, here(app.artists.get(id).and_then(Load::ready).map_or(t("artist"), |a| a.artist.name.as_str()))],
        Page::Mix(id) => vec![for_you, here(app.mixes.get(id).and_then(Load::ready).map_or(t("mix"), |(m, _)| m.title.as_str()))],
        Page::Replay => vec![for_you, here(t("replay"))],
        Page::Discover => vec![for_you, here(t("mixes_new_releases"))],
        Page::MyMixes => vec![for_you, here(t("mixes_and_radio"))],
        Page::MyPlaylists => vec![here(t("playlists"))],
        Page::Playlist(uuid) => {
            let known = app.playlists.get(uuid).and_then(Load::ready).map(|(p, _)| p.title.as_str());
            let listed = || app.my_playlists.ready().and_then(|l| l.iter().find(|p| &p.uuid == uuid)).map(|p| p.title.as_str());
            vec![(t("playlists").to_string(), Some(Page::MyPlaylists)), here(known.or_else(listed).unwrap_or(t("playlist")))]
        }
        Page::Settings => vec![here(t("settings"))],
    }
}

/// GPUI's title bar: 16 px inset, 30 px tool buttons 8 px apart.
const TOP_PADDING: f32 = 16.0;
const TOP_GAP: f32 = 12.0;
const TOOL: f32 = 30.0;
const TOOL_GAP: f32 = 8.0;
/// Back and forward (Iced only; GPUI relies on the mouse's side buttons).
const TOP_LEFT_MIN: f32 = 2.0 * 28.0 + 2.0 + 4.0;
const TOP_RIGHT_MIN: f32 = 3.0 * TOOL + 3.0 * TOOL_GAP + super::chrome::CAPTION_WIDTH;

/// Both the text field and its results use the same anchor in window coordinates.
/// As GPUI's `titlebar::layout`: up to 380 px, centred in the bar, leaving the
/// tools' width (and 26 px) free on each side.
pub(super) fn search_anchor(viewport_width: f32) -> (f32, f32) {
    let bar_x = super::sidebar::RAIL_W;
    let bar_w = (viewport_width - bar_x - theme::FRAME_GAP).max(0.0);
    let actions_w = 3.0 * (TOOL + TOOL_GAP + 34.0) + 8.0 + 2.0;
    // Below 640 px GPUI moves the search to a row of its own; Iced keeps it in
    // the bar and gives it the room between the tools instead.
    let wanted = if bar_w < 640.0 { 380.0 } else { (bar_w - 2.0 - 2.0 * (actions_w + TOP_PADDING + 10.0)).clamp(1.0, 380.0) };
    let left = bar_x + TOP_PADDING + TOP_LEFT_MIN + TOP_GAP;
    let right = viewport_width - theme::FRAME_GAP - TOP_PADDING - TOP_RIGHT_MIN - TOP_GAP;
    let width = wanted.min(right - left).max(0.0);
    let x = (bar_x + (bar_w - width) * 0.5).clamp(left, (right - width).max(left));
    (x, width)
}

/// A tool of the title bar (GPUI's `toolbar_button`, 15 px glyph): a round
/// 28 px button with no fill or edge of its own, since the tools share one
/// capsule of glass (see `top_bar`); its wash fades in under the pointer.
fn toolbar_button<'a>(i: Icon, label: &'a str, msg: Message) -> Element<'a, Message> {
    let side = TOOL - 2.0;
    let b = button(container(widgets::soft_glyph(i, 15.0, theme::text_secondary())).center(side))
        .padding(0)
        .style(widgets::still(side / 2.0))
        .on_press(msg);
    widgets::tip(widgets::fading(b, side / 2.0), label)
}

/// How far the page has slid under the top bar, 0 to 1 over its first 22 px
/// (the scroll edge: the bar's rule shows only once something is under it).
fn under_bar(app: &App) -> f32 {
    (app.page_scroll / 22.0).clamp(0.0, 1.0)
}

pub fn top_bar(app: &App) -> Element<'_, Message> {
    let mut crumb_row = row![].spacing(6).align_y(Alignment::Center);
    let parts = crumbs(app);
    let n = parts.len();
    for (i, (label, target)) in parts.into_iter().enumerate() {
        if i > 0 {
            crumb_row = crumb_row.push(text("\u{203a}").size(theme::TYPE_BODY).color(theme::text_dim()));
        }
        let last = i + 1 == n;
        crumb_row = crumb_row.push(match target {
            Some(p) if !last => widgets::text_link(line(label, 12.5, theme::text_secondary()), Message::Navigate(p)),
            _ => Element::from(line(label, 12.5, theme::text()).font(widgets::semibold())),
        });
    }
    // Signed out there is no sidebar to carry the mark.
    // Back and forward share one capsule of glass (a group, not two pills).
    let nav: Element<'_, Message> = if app.signed_in {
        super::liquid_glass::control(
            container(
                row![
                    icon_button(Icon::Back, 16.0, if app.can_go_back() { theme::text() } else { theme::text_dim() }, Message::Back),
                    icon_button(
                        Icon::Forward,
                        16.0,
                        if app.can_go_forward() { theme::text() } else { theme::text_dim() },
                        Message::Forward
                    ),
                ]
                .spacing(2),
            )
            .padding(2),
        )
        .on_chrome()
        .into()
    } else {
        iced::widget::image(super::images::logo()).width(28).height(28).into()
    };
    let (search_x, search_w) = search_anchor(app.viewport.width);
    // GPUI: 34 px tall, 12 px inset, a 14 px glyph 8 px from the text; a
    // capsule of glass (a control: it follows the pointer), the field itself
    // clear but for a faint wash and an edge while focused.
    let search = text_input(crate::i18n::t("search"), &app.search_query)
        .id(widgets::SEARCH_ID)
        .on_input(Message::SearchInput)
        .on_submit(Message::SearchSubmit)
        .size(12.5)
        .padding(Padding { top: 8.0, bottom: 8.0, left: 12.0, right: 12.0 })
        .icon(text_input::Icon {
            font: super::INTER,
            code_point: '\u{2315}',
            size: Some(14.0.into()),
            spacing: 8.0,
            side: text_input::Side::Left,
        })
        .style(|_, status| {
            let focused = matches!(status, text_input::Status::Focused { .. });
            text_input::Style {
                background: if focused { theme::ink(0.06).into() } else { Color::TRANSPARENT.into() },
                border: Border { color: if focused { theme::text_dim() } else { Color::TRANSPARENT }, width: 1.0, radius: 17.0.into() },
                icon: theme::text_secondary(),
                placeholder: theme::text_dim(),
                value: theme::text(),
                selection: theme::selection(),
            }
        })
        .width(search_w);
    let search = super::liquid_glass::control(search).on_chrome();
    let light = theme::is_light();
    let tools = row![
        toolbar_button(
            if light { Icon::Moon } else { Icon::Sun },
            if light { crate::i18n::t("switch_to_dark_mode") } else { crate::i18n::t("switch_to_light_mode") },
            Message::SetLight(!light),
        ),
        toolbar_button(Icon::Settings, crate::i18n::t("settings"), Message::Navigate(Page::Settings)),
        toolbar_button(Icon::Upload, crate::i18n::t("add_local_files"), Message::Local(super::local::LocalMsg::OpenSettings)),
    ]
    .spacing(2)
    .align_y(Alignment::Center);
    // The three tools are one group on one capsule of glass; the window's
    // caption buttons sit on a pane of their own beside it.
    let tools = super::liquid_glass::control(container(tools).padding(2)).on_chrome();
    let caption = super::liquid_glass::glass(super::chrome::caption_buttons(app), 9.0)
        .optics(super::liquid_glass::Optics::CONTROL)
        .on_chrome()
        .interactive()
        .shadow(theme::control_shadow());
    let navigation = row![
        nav,
        super::chrome::drag_area(
            container(crumb_row).width(Length::Fill).height(theme::TOP_H - 1.0).align_y(alignment::Vertical::Center).clip(true)
        )
    ]
    .spacing(TOP_GAP)
    .align_y(Alignment::Center);
    let nav_width =
        if app.signed_in { Length::Fixed((search_x - super::sidebar::RAIL_W - TOP_PADDING - TOP_GAP).max(0.0)) } else { Length::Fill };
    let bar = row![navigation.width(nav_width)]
        .push(app.signed_in.then_some(search))
        .push(
            container(row![tools, caption].spacing(TOOL_GAP).align_y(Alignment::Center))
                .align_x(alignment::Horizontal::Right)
                .width(Length::Fill),
        )
        .spacing(TOP_GAP)
        .align_y(Alignment::Center)
        .padding([0.0, TOP_PADDING])
        .height(theme::TOP_H - 1.0);
    // GPUI closes the bar with a rule. Here it is a scroll edge: there only
    // once the page slides under the bar (iOS 26's edge effect, not a fixed
    // divider), fading in over the first pixels of scroll.
    let k = under_bar(app);
    let edge = container(Space::new().width(Length::Fill).height(1)).style(move |_| container::Style {
        background: Some(theme::with_alpha(theme::divider(), theme::divider().a * k).into()),
        ..Default::default()
    });
    column![bar, edge].width(Length::Fill).into()
}

// ----------------------------------------------------------------- content

pub fn content(app: &App) -> Element<'_, Message> {
    let width = content_width(app);
    let body = page_body(app, width);
    // GPUI ends every page with a 32 px spacer.
    let column = container(body).width(width).padding(Padding { top: PAGE_TOP, bottom: 32.0, ..Default::default() });
    let page = container(column)
        .width(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .style(|_| container::Style { text_color: Some(theme::text()), ..Default::default() });
    let ready = matches!(app.page, Page::Settings) || lib_section(&app.page).is_some() || !page_loading(app);
    let content = if ready {
        super::glide::glide(
            scrollable(page)
                .id(PAGE_SCROLL)
                .on_scroll(|vp| Message::PageScrolled(vp.absolute_offset().y, vp.bounds().height))
                .direction(scrollable::Direction::Vertical(thin_scrollbar()))
                .style(scroll_style)
                .height(Length::Fill)
                .width(Length::Fill),
        )
    } else {
        // Loading: the three dots centred in the view.
        loading()
    };
    let fade = PageFade { started: app.page_at, ready };
    // As GPUI: the page fades in while sliding 26 px from the side it comes from.
    let slid = super::panel_motion::slide(content, app.page_at, PageFade::DURATION, 26.0 * app.nav_dir);
    // The scene behind the glass, as a wash at the top of the page (GPUI's page tint).
    let wash = super::liquid_glass::page_wash(app);
    // `push_under`: the page (not the wash) still sizes the stack.
    // The page softens into the glass top bar as it scrolls under it (scroll-edge effect).
    stack![slid, super::liquid_glass::scroll_edge(app), iced::widget::canvas(fade).width(Length::Fill).height(Length::Fill)]
        .push_under(wash)
        .into()
}

/// Fade the entire page against its background, including artwork and
/// explicitly styled text: when it is navigated to, and again when the
/// content of a page that was loading arrives (it used to replace the dots
/// in one frame). With reduced motion the fade stays (the slide goes).
struct PageFade {
    started: std::time::Instant,
    /// The page shows its content, not the loading dots.
    ready: bool,
}

/// What [`PageFade`] last saw: whether the page was ready, and when its
/// content arrived.
#[derive(Default)]
struct Arrival {
    ready: Option<bool>,
    at: Option<std::time::Instant>,
}

impl Arrival {
    /// Note the page's readiness at `now`: content replacing the dots starts a fade.
    fn see(&mut self, ready: bool, now: std::time::Instant) {
        if self.ready == Some(false) && ready {
            self.at = Some(now);
        }
        self.ready = Some(ready);
    }
}

impl PageFade {
    const DURATION: std::time::Duration = super::motion::PANEL;

    /// The veil of a fade that began at `from`: 1 to 0 with the quartic ease-out.
    fn veil(from: std::time::Instant, now: std::time::Instant) -> f32 {
        1.0 - super::motion::ease_out(super::motion::progress(from, now, Self::DURATION))
    }

    fn opacity(&self, now: std::time::Instant) -> f32 {
        Self::veil(self.started, now)
    }

    fn opacity_with(&self, arrival: &Arrival, now: std::time::Instant) -> f32 {
        self.opacity(now).max(arrival.at.map_or(0.0, |at| Self::veil(at, now)))
    }
}

impl iced::widget::canvas::Program<Message> for PageFade {
    type State = Arrival;

    fn update(
        &self,
        arrival: &mut Arrival,
        event: &iced::Event,
        _: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Option<iced::widget::Action<Message>> {
        let now = match event {
            iced::Event::Window(iced::window::Event::RedrawRequested(at)) => *at,
            _ => std::time::Instant::now(),
        };
        arrival.see(self.ready, now);
        if matches!(event, iced::Event::Window(iced::window::Event::RedrawRequested(_))) && self.opacity_with(arrival, now) > 0.0 {
            // Next vsync (a fixed timer judders against the refresh).
            return Some(iced::widget::Action::request_redraw());
        }
        None
    }

    fn draw(
        &self,
        arrival: &Arrival,
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        let alpha = self.opacity_with(arrival, std::time::Instant::now());
        if alpha == 0.0 {
            return Vec::new();
        }
        let mut frame = iced::widget::canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(iced::Point::ORIGIN, bounds.size(), theme::with_alpha(theme::bg(), alpha));
        vec![frame.into_geometry()]
    }
}

/// Height of [`title`] without a subtitle: 4 + a DISPLAY line + 14.
const TITLE_H: f32 = 4.0 + scale::TYPE_DISPLAY * 1.3 + 14.0;
/// Height of [`filter_box`] with the space under it.
const FILTER_H: f32 = 34.0 + 16.0;
/// Height of [`top_hit`]: a 112 px cover, 20 px around it and the edge.
const TOP_HIT_H: f32 = 112.0 + 40.0 + 2.0;

/// GPUI's `Block::Text`: a 13 px secondary line (disc headings, copyright), 10 px above and below.
fn note_text<'a>(s: String) -> Element<'a, Message> {
    container(text(s).size(13).line_height(text::LineHeight::Absolute(20.0.into())).color(theme::text_secondary()))
        .max_width(900)
        .padding([10, 0])
        .into()
}

/// The top result of a search (GPUI's `Block::TopHit`): a 460 px glass card,
/// a 112 px cover, a 22 px title and what it is.
fn top_hit<'a>(item: &Item) -> Element<'a, Message> {
    let (title, sub, url, round) = match item {
        Item::Album(a) => (a.title.clone(), crate::i18n::text("album.detail", &[a.artist_names()]), a.cover(320), false),
        Item::Artist(a) => (a.name.clone(), crate::i18n::t("artist").to_string(), a.picture(320), true),
        Item::Playlist(p) => (p.title.clone(), crate::i18n::t("playlist").to_string(), p.cover(320), false),
        Item::Track(t) => (t.display_title(), crate::i18n::text("track.detail", &[t.artist_names()]), t.cover(320), false),
        Item::Mix(m) => (m.title.clone(), crate::i18n::t("mix").to_string(), m.image.clone(), false),
        Item::Link(l) => (l.title.clone(), String::new(), None, false),
        Item::Promo(p) => (p.title.clone(), p.subtitle.clone(), p.image_url(), false),
    };
    let names = column![
        line(title, scale::TYPE_HEADING, theme::text()).font(widgets::semibold()),
        line(sub, theme::TYPE_BODY, theme::text_secondary())
    ]
    .spacing(6);
    let body = row![cover(url, 112.0, round), container(names).width(Length::Fill).clip(true)].spacing(20).align_y(Alignment::Center);
    // `theme::liquid_glass` is a real pane of glass now (see `liquid_glass`).
    // A card that opens as a whole: a control, so its glass follows the pointer.
    let card = super::liquid_glass::glass(container(body).width(460).padding(20), super::liquid_glass::PANEL_R)
        .interactive()
        .shadow(theme::popover_shadow());
    mouse_area(card).on_press(Message::OpenItem(item.clone())).interaction(iced::mouse::Interaction::Pointer).into()
}

/// A page's title: DISPLAY, the same on every page (Home, Explore,
/// Playlists, Search...), with its subtitle under it.
fn title<'a>(t: impl text::IntoFragment<'a>, sub: Option<String>) -> Element<'a, Message> {
    title_sized(t, sub, scale::TYPE_DISPLAY, 14.0)
}

fn title_sized<'a>(t: impl text::IntoFragment<'a>, sub: Option<String>, size: f32, below: f32) -> Element<'a, Message> {
    column![text(t).size(size).font(widgets::bold()).color(theme::text())]
        .push(sub.map(|s| text(s).size(theme::TYPE_LABEL).color(theme::text_secondary())))
        .spacing(5)
        // With the column's own 18 px, 22 px above, as in GPUI.
        .padding(Padding { top: 4.0, bottom: below, ..Default::default() })
        .into()
}

fn load_state<'a, T>(l: Option<&Load<T>>) -> Option<Element<'a, Message>> {
    match l {
        Some(Load::Ready(_)) => None,
        Some(Load::Failed(e)) => Some(page_error(e, Message::RetryPage)),
        _ => Some(container(widgets::loading_dots()).width(Length::Fill).height(240).center(Length::Fill).into()),
    }
}

/// Room under a sideways strip for its thin scrollbar (GPUI draws none).
const STRIP_BAR: f32 = 10.0;

/// A strip of cards that scrolls sideways.
fn strip<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    scrollable(container(content).padding(Padding { bottom: STRIP_BAR, ..Default::default() }))
        .direction(scrollable::Direction::Horizontal(thin_scrollbar()))
        .style(scroll_style)
        .into()
}

/// A titled horizontal row of cards outside Home (GPUI's `render_row`:
/// 15 px title 26 px below the block above, 130 px cards 12 px apart).
pub(super) fn card_row<'a>(t: String, items: &[Item]) -> Element<'a, Message> {
    let cards = row(items.iter().map(|i| card(i, theme::ROW_CARD_W))).spacing(theme::GAP);
    let heading = container(text(t).size(scale::TYPE_HEADING).font(widgets::semibold()).color(theme::text())).padding(Padding {
        top: scale::SPACE_XL,
        bottom: scale::SPACE_MD,
        ..Default::default()
    });
    column![heading, strip(cards)].into()
}

/// A row of cards on Home: under a rule, 19 px title, cards sized from the width.
fn home_row<'a>(t: String, items: &[Item], width: f32) -> Element<'a, Message> {
    let card_w = (width * 0.17).clamp(112.0, 152.0).min(width);
    let cards = row(items.iter().map(|i| card(i, card_w))).spacing(theme::GAP);
    let heading = container(text(t).size(scale::TYPE_HEADING).font(widgets::semibold()).color(theme::text())).padding(Padding {
        top: scale::SPACE_LG,
        bottom: scale::SPACE_MD,
        ..Default::default()
    });
    column![rule(), heading, strip(cards), Space::new().height((24.0 - STRIP_BAR).max(0.0))].into()
}

/// A 1 px divider across the column.
fn rule<'a>() -> Element<'a, Message> {
    container(Space::new().width(Length::Fill).height(1))
        .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() })
        .into()
}

/// The flat panel GPUI puts around Home's chips, links, promos and shortcuts
/// and around the shelves of album and artist pages.
fn flat_panel<'a>(content: impl Into<Element<'a, Message>>, width: f32) -> Element<'a, Message> {
    container(container(content).padding(head_pad(width)).width(Length::Fill).style(|_| container::Style {
        background: Some(theme::ink(0.04).into()),
        border: Border { color: theme::ink(0.09), width: 1.0, radius: 18.0.into() },
        ..Default::default()
    }))
    .padding(Padding { bottom: 12.0, ..Default::default() })
    .into()
}

/// A shelf of cards inside a flat panel, as GPUI draws them on album and
/// artist pages: 16 px title, cards sized from the available width.
pub(super) fn shelf_panel<'a>(t: String, items: &[Item], width: f32) -> Element<'a, Message> {
    let inset = head_pad(width);
    let available = (width - 2.0 * inset - 2.0).max(1.0);
    let card_w = (available * 0.17).clamp(112.0, 152.0).min(available);
    let cards = row(items.iter().map(|i| card(i, card_w))).spacing(theme::GAP);
    let inner =
        column![text(t).size(scale::TYPE_SECTION).font(widgets::semibold()).color(theme::text()), strip(cards)].spacing(scale::SPACE_MD);
    flat_panel(inner, width)
}

/// Chips (GPUI's `Block::Chips`): 36 px pills 10 px apart under a 16 px title.
fn chips<'a>(t: String, items: &[Item], home: bool) -> Element<'a, Message> {
    let chips = items.iter().map(|i| {
        let label = match i {
            Item::Link(l) => l.title.clone(),
            _ => String::new(),
        };
        let b = button(
            container(text(label).size(scale::TYPE_SMALL).font(widgets::semibold())).height(36).align_y(alignment::Vertical::Center),
        )
        .padding([0, 16])
        // The hover fades in with the eased phase of the `track` below.
        .style(|_, _| button::Style {
            background: Some(super::wash::mix(theme::control(), theme::active(), super::wash::phase().hover).into()),
            text_color: theme::text(),
            border: Border { radius: 18.0.into(), ..Default::default() },
            ..Default::default()
        })
        .on_press(Message::OpenItem(i.clone()));
        super::wash::track(b).into()
    });
    let heading = container(text(t).size(theme::TYPE_SECTION).font(widgets::semibold()).color(theme::text())).padding(Padding {
        top: if home { 0.0 } else { scale::SPACE_XL },
        bottom: 12.0,
        ..Default::default()
    });
    column![heading, row(chips).spacing(10).wrap().vertical_spacing(10)].into()
}

/// Links (GPUI's `Block::Links`): 46 px rows with an icon, in up to three columns.
fn links<'a>(items: &[Item], width: f32, home: bool) -> Element<'a, Message> {
    let cols = if width >= 720.0 {
        3
    } else if width >= 400.0 {
        2
    } else {
        1
    };
    let cell = width / cols as f32;
    let entries = items.iter().map(|it| {
        let (label, glyph) = match it {
            Item::Link(l) => {
                let k = format!("{} {}", l.icon, l.title).to_lowercase();
                let glyph = if k.contains("new") {
                    Icon::Calendar
                } else if k.contains("top") {
                    Icon::Trophy
                } else if k.contains("video") {
                    Icon::VideoBox
                } else if k.contains("clean") || k.contains("explicit") {
                    Icon::Explicit
                } else {
                    Icon::Mix
                };
                (l.title.clone(), glyph)
            }
            _ => (String::new(), Icon::Mix),
        };
        let body =
            row![super::icons::icon(glyph, 18.0, theme::text()), line(label, scale::TYPE_SMALL, theme::text()).font(widgets::semibold())]
                .spacing(12)
                .align_y(Alignment::Center);
        widgets::soft(
            button(container(body).height(46).align_y(alignment::Vertical::Center))
                .width(cell)
                .padding([0, 10])
                .on_press(Message::OpenItem(it.clone())),
            8.0,
        )
    });
    container(row(entries).wrap()).padding(Padding { top: if home { 0.0 } else { 26.0 }, ..Default::default() }).into()
}

/// Promotions (GPUI's `Block::Featured`): wide images with a label, a title and a line.
fn featured<'a>(t: String, items: &[Item], width: f32, home: bool) -> Element<'a, Message> {
    let available = if home { (width - 2.0 * head_pad(width) - 2.0).max(1.0) } else { width };
    let w = if home { (available * 0.42).clamp(182.0, 290.0).min(available) } else { 220.0 };
    let h = if home { w * 0.59 } else { 160.0 };
    let promos = items.iter().filter_map(|it| {
        let Item::Promo(p) = it else { return None };
        let url = p.image_url();
        let art: Element<'a, Message> = match url.as_deref().and_then(|u| super::images::sized(u, w, h)) {
            Some(img) => iced::widget::image(img).width(w).height(h).content_fit(iced::ContentFit::Cover).border_radius(12.0).into(),
            None => container(Space::new().width(w).height(h))
                .style(|_| container::Style {
                    background: Some(theme::frame().into()),
                    border: Border { radius: 12.0.into(), ..Default::default() },
                    ..Default::default()
                })
                .into(),
        };
        let body = column![
            art,
            container(text(p.label.to_uppercase()).size(scale::TYPE_MICRO).font(widgets::semibold()).color(theme::accent()))
                .padding(Padding { top: 4.0, ..Default::default() }),
            line(p.title.clone(), scale::TYPE_SMALL, theme::text()).font(widgets::semibold()),
            line(p.subtitle.clone(), 11.5, theme::text_secondary()),
        ]
        .spacing(6)
        .width(w);
        Some(mouse_area(body).on_press(Message::OpenItem(it.clone())).interaction(iced::mouse::Interaction::Pointer).into())
    });
    let heading = container(
        text(t).size(if home { scale::TYPE_SECTION } else { scale::TYPE_HEADING }).font(widgets::semibold()).color(theme::text()),
    )
    .padding(Padding { bottom: 12.0, ..Default::default() });
    column![heading, strip(row(promos).spacing(14))].into()
}

/// Quick links (GPUI's `shortcut_grid`): 48 px tiles in up to three columns.
fn shortcuts<'a>(t: String, items: &[Item], width: f32, home: bool) -> Element<'a, Message> {
    let inner = if home { (width - 2.0 * head_pad(width) - 2.0).max(1.0) } else { width };
    let cols = if inner >= 720.0 {
        3.0
    } else if inner >= 430.0 {
        2.0
    } else {
        1.0
    };
    let tile_w = (inner - (cols - 1.0) * 8.0) / cols;
    let tiles = items.iter().map(|it| {
        let (title, subtitle, url, _) = widgets::item_text(it);
        let art: Element<'a, Message> = match url {
            Some(_) => cover(url, 46.0, false),
            None => container(super::icons::icon(Icon::Heart, 18.0, theme::text()))
                .center(46)
                .style(|_| container::Style { background: Some(theme::accent().into()), ..Default::default() })
                .into(),
        };
        let names = column![line(title, scale::TYPE_LABEL, theme::text()).font(widgets::semibold())]
            .push((!subtitle.is_empty()).then(|| line(subtitle, 10.0, theme::text_dim())))
            .spacing(2);
        let body = row![art, container(names).width(Length::Fill).clip(true)]
            .spacing(10)
            .align_y(Alignment::Center)
            .padding(Padding { right: 12.0, ..Default::default() });
        let b = button(container(body).height(46).align_y(alignment::Vertical::Center))
            .width(tile_w)
            .padding(0)
            // The hover fades in with the eased phase of the `track` below.
            .style(|_, _| {
                let h = super::wash::phase().hover;
                button::Style {
                    background: Some(theme::ink(0.05 + 0.04 * h).into()),
                    text_color: theme::text(),
                    border: Border { color: theme::ink(0.09 + 0.07 * h), width: 1.0, radius: 16.0.into() },
                    ..Default::default()
                }
            })
            .on_press(Message::OpenItem(it.clone()));
        super::wash::track(b).into()
    });
    let heading = text(t).size(theme::TYPE_SECTION).font(widgets::semibold()).color(theme::text());
    column![heading, row(tiles).spacing(8).wrap().vertical_spacing(8)]
        .spacing(12)
        .padding(Padding { top: if home { 0.0 } else { 22.0 }, ..Default::default() })
        .into()
}

/// A feed's rows as GPUI lays them out on any page but Home.
pub(super) fn rows_of<'a>(rows: &[Row], width: f32) -> Vec<Element<'a, Message>> {
    feed_rows(rows, width, false)
}

/// A feed's rows; on Home card rows sit under a rule and the rest in flat panels.
pub(super) fn feed_rows<'a>(rows: &[Row], width: f32, home: bool) -> Vec<Element<'a, Message>> {
    rows.iter()
        .filter(|r| !r.items.is_empty())
        .map(|r| {
            let block = match r.kind {
                RowKind::Cards if home => return home_row(r.title.clone(), &r.items, width),
                RowKind::Cards => return card_row(r.title.clone(), &r.items),
                RowKind::Chips => chips(r.title.clone(), &r.items, home),
                RowKind::Links => links(&r.items, if home { (width - 2.0 * head_pad(width) - 2.0).max(1.0) } else { width }, home),
                RowKind::Featured => featured(r.title.clone(), &r.items, width, home),
                RowKind::Shortcuts => shortcuts(r.title.clone(), &r.items, width, home),
            };
            if home {
                flat_panel(block, width)
            } else {
                block
            }
        })
        .collect()
}

fn grid<'a>(items: Vec<Item>, width: f32) -> Element<'a, Message> {
    let (cols, card_w) = grid_metrics(width);
    // As GPUI's `Block::Cards`: 12 px between cards, 22 px under each row.
    let mut col = column![].spacing(22);
    for chunk in items.chunks(cols) {
        col = col.push(row(chunk.iter().map(|i| card(i, card_w))).spacing(theme::GAP));
    }
    col.into()
}

fn page_body(app: &App, width: f32) -> Element<'_, Message> {
    match &app.page {
        Page::Home => home(app, width),
        Page::Explore => {
            let mut c = column![title(crate::i18n::t("explore"), None)];
            match &app.explore {
                Load::Ready(r) => c = c.extend(rows_of(r, width)),
                other => c = c.push(load_state(Some(other))),
            }
            c.into()
        }
        Page::Browse(path, t) => {
            let mut c = column![title(t.clone(), None)];
            match app.browse.get(path) {
                Some(Load::Ready(r)) if r.is_empty() => c = c.push(widgets::notice(Icon::Explore, crate::i18n::t("nothing_to_show_yet"))),
                Some(Load::Ready(r)) => c = c.extend(rows_of(r, width)),
                other => c = c.push(load_state(other)),
            }
            c.into()
        }
        Page::Search(q) => search_page(app, q, width),
        Page::Album(id) => album_page(app, *id, width),
        Page::Artist(id) => artist_page(app, *id, width),
        Page::Playlist(uuid) => playlist_page(app, uuid, width),
        Page::Mix(id) => mix_page(app, id, width),
        Page::MyTracks | Page::MyAlbums | Page::MyArtists => library_page(app, width),
        Page::MyPlaylists => {
            // As GPUI: title with its sort menu, "New playlist", the filter, the grid.
            let new =
                pill(crate::i18n::t("playlist.create"), None, true, Some(Message::OpenDialog(Dialog::NewPlaylist { add: Vec::new() })));
            let mut c = column![
                title_sort(crate::i18n::t("playlists"), "playlists"),
                container(new).padding(Padding { bottom: 16.0, ..Default::default() }),
                filter_box(app)
            ];
            match &app.my_playlists {
                Load::Ready(l) if l.is_empty() => c = c.push(widgets::notice(Icon::Playlist, crate::i18n::t("you_have_no_playlists_yet"))),
                Load::Ready(l) => {
                    c = c.push(grid(
                        l.iter().filter(|p| app.text_matches(&[&p.title, &p.creator])).cloned().map(Item::Playlist).collect(),
                        width,
                    ))
                }
                other => c = c.push(load_state(Some(other))),
            }
            c.into()
        }
        Page::MyMixes => {
            let mut c = column![title_sort(crate::i18n::t("mixes_and_radio"), "mixes"), filter_box(app)];
            match &app.my_mixes {
                Load::Ready(l) if l.is_empty() => c = c.push(widgets::notice(Icon::Mix, crate::i18n::t("no_mixes_yet"))),
                Load::Ready(l) => {
                    c = c.push(grid(l.iter().filter(|m| app.text_matches(&[&m.title])).cloned().map(Item::Mix).collect(), width))
                }
                other => c = c.push(load_state(Some(other))),
            }
            c.into()
        }
        Page::Recent => {
            let c = column![title(crate::i18n::t("history"), Some(crate::i18n::t("history.albums").into()))];
            if app.recents.is_empty() {
                c.push(widgets::notice(Icon::Clock, crate::i18n::t("history.empty"))).into()
            } else {
                c.push(grid(app.recents.iter().map(|a| Item::Album(a.to_album())).collect(), width)).into()
            }
        }
        Page::Discover => discover(app, width),
        Page::Replay => super::replay::view(app, width),
        Page::Settings => super::settings::view(app, width),
    }
}

fn home(app: &App, width: f32) -> Element<'_, Message> {
    // GPUI has no tabs here: Staff picks and Uploads are sections of
    // "Mixes & new releases" (see `discover`).
    let mut c = column![title_sized(crate::i18n::t("home"), None, scale::TYPE_DISPLAY, 20.0)];
    let data = Some(&app.home);
    match data {
        Some(Load::Ready(rows)) => c = c.push(super::home::view(app, rows, width)),
        other => c = c.push(load_state(other)),
    }
    c.into()
}

/// A small label naming where the rows below come from, a note and a rule
/// (GPUI's `Block::SourceTag`).
pub(super) fn source_tag<'a>(tag: String, note: String) -> Element<'a, Message> {
    let chip =
        container(text(tag).size(scale::TYPE_MICRO).font(widgets::semibold()).color(theme::text_secondary())).padding([3, 8]).style(|_| {
            container::Style {
                background: Some(theme::control().into()),
                border: Border { radius: 5.0.into(), ..Default::default() },
                ..Default::default()
            }
        });
    let rule = container(Space::new().width(Length::Fill).height(1))
        .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() });
    row![chip, text(note).size(11.5).color(theme::text_dim()), rule]
        .spacing(10)
        .align_y(Alignment::Center)
        .padding(Padding { top: 18.0, bottom: 4.0, ..Default::default() })
        .into()
}

fn discover(app: &App, width: f32) -> Element<'_, Message> {
    let mut c = column![title(crate::i18n::t("mixes_new_releases"), Some(crate::i18n::t("mixes_editorial_picks_and_new_releases").into()))];
    // GPUI keeps the loading dots of each section in a 72 px band.
    let waiting =
        || -> Element<'_, Message> { container(widgets::loading_dots()).width(Length::Fill).height(72).center(Length::Fill).into() };
    let failed = |e: &String| widgets::message(crate::i18n::text("could_not_load", std::slice::from_ref(e)));
    c = c.push(source_tag(crate::i18n::t("your_mixes").into(), crate::i18n::t("mixes_and_radio_made_for_you").into()));
    match &app.my_mixes {
        Load::Ready(l) if l.is_empty() => c = c.push(widgets::notice(Icon::Mix, crate::i18n::t("no_mixes_yet"))),
        Load::Ready(l) => {
            c = c.push(card_row(crate::i18n::t("mixes_and_radio").into(), &l.iter().cloned().map(Item::Mix).collect::<Vec<_>>()))
        }
        Load::Failed(e) => c = c.push(failed(e)),
        _ => c = c.push(waiting()),
    }
    let uploads = !app.settings.lock().hide_uploads;
    for (tab, name, note) in [
        (1usize, crate::i18n::t("staff_picks"), crate::i18n::t("picks_from_tidal_s_editorial_team")),
        (2, "UPLOADS", crate::i18n::t("new_releases_from_independent_artists")),
    ] {
        if tab == 2 && !uploads {
            continue;
        }
        c = c.push(source_tag(name.into(), note.into()));
        match app.home_extra.get(&tab) {
            Some(Load::Ready(r)) if r.is_empty() => c = c.push(widgets::notice(Icon::Explore, crate::i18n::t("nothing_to_show_yet"))),
            Some(Load::Ready(r)) => {
                // Here GPUI shows quick links as an ordinary row of cards.
                for row in r.iter().filter(|r| !r.items.is_empty()) {
                    c = c.push(match row.kind {
                        RowKind::Cards | RowKind::Shortcuts => card_row(row.title.clone(), &row.items),
                        _ => rows_of(std::slice::from_ref(row), width).pop().unwrap_or_else(|| Space::new().into()),
                    });
                }
            }
            Some(Load::Failed(e)) => c = c.push(failed(e)),
            _ => c = c.push(waiting()),
        }
    }
    c.into()
}

/// The "Filter ..." box (GPUI's `Block::FilterBar`): the column's width, 34 px
/// tall with a search glyph, 16 px above what it filters.
fn filter_box(app: &App) -> Element<'_, Message> {
    let hint = match app.page {
        Page::MyTracks => crate::i18n::t("filter_tracks"),
        Page::MyAlbums => crate::i18n::t("filter_albums"),
        Page::MyArtists => crate::i18n::t("filter_artists"),
        Page::MyPlaylists => crate::i18n::t("filter_playlists"),
        Page::MyMixes => crate::i18n::t("filter_mixes"),
        _ => crate::i18n::t("filter_by_title_artist_or_album"),
    };
    let field = text_input(hint, &app.filter).on_input(Message::Filter).size(theme::TYPE_LABEL).padding(0).style(bare_field_style);
    let bar =
        container(row![super::icons::icon(Icon::Search, 14.0, theme::text_secondary()), field].spacing(10).align_y(Alignment::Center))
            .height(34)
            .width(Length::Fill)
            .padding([0, 12])
            .align_y(alignment::Vertical::Center)
            .style(|_| container::Style {
                background: Some(theme::ink(0.05).into()),
                border: Border { color: theme::ink(0.10), width: 1.0, radius: 10.0.into() },
                ..Default::default()
            });
    container(bar).padding(Padding { bottom: 16.0, ..Default::default() }).into()
}

/// A text field drawn by the box around it.
pub fn bare_field_style(_: &iced::Theme, _: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Color::TRANSPARENT.into(),
        border: Border::default(),
        icon: theme::text_secondary(),
        placeholder: theme::text_dim(),
        value: theme::text(),
        selection: theme::selection(),
    }
}

/// A collection page's title with the "..." menu of its order beside it
/// (GPUI's `Block::TitleSort`, 22 above and 14 below). At the size of every
/// other page title ([`title`], 30 px; GPUI's 22 px made Playlists and
/// Mixes look like sections of a page rather than pages).
fn title_sort<'a>(t: &'a str, section: &'static str) -> Element<'a, Message> {
    let more = icon_button(Icon::More, 18.0, theme::text_secondary(), Message::OpenMenu(MenuTarget::Sort(section)));
    row![text(t).size(scale::TYPE_DISPLAY).font(widgets::bold()).color(theme::text()), container(more).id(super::menus::SORT_ANCHOR)]
        .spacing(8)
        .align_y(Alignment::Center)
        // With the column's own 18 px, 22 px above.
        .padding(Padding { top: 4.0, bottom: 14.0, ..Default::default() })
        .into()
}

pub fn field_style(_: &iced::Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: theme::field().into(),
        border: Border { color: if focused { theme::text_dim() } else { theme::control_border() }, width: 1.0, radius: 8.0.into() },
        icon: theme::text_secondary(),
        placeholder: theme::text_dim(),
        value: theme::text(),
        selection: theme::selection(),
    }
}

/// Header of a collection page (album, playlist, mix, artist), as GPUI's
/// `catalog_header`: a glass panel with the cover, the kind, a large title and
/// the details beside it, and under a rule the primary pills on the left and
/// the labelled actions on the right (artists too, round portrait). Its height
/// depends only on the width ([`header_h`]) so the tracks below can be
/// virtualised.
/// With the column's own 18 px, 22 px above the panel, as in GPUI.
const HEAD_TOP: f32 = 4.0;
const HEAD_BOTTOM: f32 = 12.0;
const HEAD_INFO: f32 = 190.0;
const HEAD_ACTIONS: f32 = 44.0;

fn head_pad(width: f32) -> f32 {
    if width < 520.0 {
        14.0
    } else {
        20.0
    }
}

/// The metrics of a catalogue header: from the column's width and, in a short
/// window (under 720 design px, e.g. 1366x768 at 125 % or 960x600), a compact
/// variant (smaller cover and title, tighter insides) so the first tracks show
/// under the header instead of a whole screen of header.
#[derive(Debug, Clone, Copy)]
pub(super) struct Head {
    width: f32,
    short: bool,
}

impl Head {
    pub(super) fn of(app: &App, width: f32) -> Self {
        Self { width, short: app.viewport.height < 720.0 }
    }

    fn pad(self) -> f32 {
        if self.short {
            head_pad(self.width).min(16.0)
        } else {
            head_pad(self.width)
        }
    }

    /// The cover's side.
    pub(super) fn cover(self) -> f32 {
        let max = if self.short { 148.0 } else { 200.0 };
        (if self.width < 480.0 { self.width * 0.43 } else { self.width * 0.21 }).clamp(42.0, max)
    }

    /// Height of the cover row: the cover, or the details beside it if taller.
    fn row_h(self) -> f32 {
        self.cover().max(if self.short { 152.0 } else { HEAD_INFO })
    }

    /// Space between the cover row and the rule over the actions.
    fn gap(self) -> f32 {
        if self.short {
            12.0
        } else {
            18.0
        }
    }

    /// The title's size and line height (at most three lines).
    fn title(self) -> (f32, f32) {
        if self.width < 480.0 {
            (28.0, 32.0)
        } else if self.short {
            (34.0, 40.0)
        } else {
            (scale::TYPE_DISPLAY, 46.0)
        }
    }

    /// Height of the whole header: the tracks below it are virtualised from it.
    pub(super) fn h(self) -> f32 {
        HEAD_TOP + 2.0 * self.pad() + self.row_h() + self.gap() + 13.0 + HEAD_ACTIONS + HEAD_BOTTOM
    }
}

#[allow(clippy::too_many_arguments)]
fn header<'a>(
    art: Element<'a, Message>,
    kind: String,
    name: String,
    details: Element<'a, Message>,
    head: Head,
    primary: Vec<Element<'a, Message>>,
    secondary: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let row_h = head.row_h();
    // GPUI: 40 px on a 46 px line (28 on 32 when the header stacks), at most three lines.
    let (size, lead) = head.title();
    let title = text(name)
        .size(size)
        .line_height(text::LineHeight::Absolute(lead.into()))
        .font(widgets::bold())
        .color(theme::text())
        .wrapping(text::Wrapping::WordOrGlyph);
    let info = column![
        text(kind).size(theme::TYPE_LABEL).color(theme::text_secondary()),
        container(title).max_height(3.0 * lead).clip(true),
        details,
    ]
    .spacing(if head.short { 6 } else { 8 });
    let top = row![art, container(info).width(Length::Fill).height(row_h).align_y(alignment::Vertical::Center).clip(true)]
        .spacing(18)
        .align_y(Alignment::Center)
        .height(row_h);
    let rule = container(Space::new().width(Length::Fill).height(1))
        .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() });
    let bottom = row![
        row(primary).spacing(8).align_y(Alignment::Center),
        Space::new().width(Length::Fill),
        row(secondary).spacing(3).align_y(Alignment::Center)
    ]
    .align_y(Alignment::Center)
    .height(HEAD_ACTIONS);
    let inner = column![top, column![rule, Space::new().height(12), bottom]].spacing(head.gap());
    let surface = container(inner).padding(head.pad()).width(Length::Fill);
    let surface = super::liquid_glass::glass(surface, super::liquid_glass::PANEL_R).shadow(theme::popover_shadow());
    container(surface).padding(Padding { top: HEAD_TOP, bottom: HEAD_BOTTOM, ..Default::default() }).height(head.h()).into()
}

/// A labelled action under a header: icon with a small caption (GPUI's `action`).
fn head_action<'a>(i: Icon, label: &'a str, active: bool, msg: Message) -> Element<'a, Message> {
    let (glyph, caption) = if active { (theme::accent(), theme::accent()) } else { (theme::text(), theme::text_secondary()) };
    let b = button(
        column![super::icons::icon(i, 18.0, glyph), text(label).size(scale::TYPE_CAPTION).font(widgets::medium()).color(caption)]
            .spacing(5)
            .align_x(Alignment::Center),
    )
    .width(56)
    .padding([4, 0])
    .style(widgets::still(8.0))
    .on_press(msg);
    widgets::fading(b, 8.0)
}

/// The prominent glass of the page's primary action: tinted with the colour
/// of the cover the page is about (see `liquid_glass::prominent`).
fn page_tint(app: &App) -> (Color, Color) {
    let palette = super::liquid_glass::scene_url(app).and_then(|url| super::images::palette(&url).try_into().ok());
    super::liquid_glass::prominent(palette)
}

/// Play and Shuffle for a page's tracks: Play on prominent glass tinted by
/// the cover, Shuffle a plain pill sharing the header's glass.
fn play_pills<'a>(app: &App, has: bool) -> Vec<Element<'a, Message>> {
    vec![
        widgets::prominent_pill(crate::i18n::t("play"), Some(Icon::Play), page_tint(app), has.then_some(Message::PlayAll(Source::Page))),
        pill(crate::i18n::t("shuffle"), Some(Icon::Shuffle), false, has.then_some(Message::ShuffleAll(Source::Page))),
    ]
}

fn actions_row<'a>(source: Source, has: bool, extra: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    row![
        pill(crate::i18n::t("play"), Some(Icon::Play), true, has.then(|| Message::PlayAll(source.clone()))),
        pill(crate::i18n::t("shuffle"), Some(Icon::Shuffle), false, has.then(|| Message::ShuffleAll(source.clone()))),
    ]
    .extend(extra)
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

/// The page's tracks under their column names, virtualised; `y0` is where the
/// list starts in the page. `shown` maps a visible row to the track's index.
fn track_list<'a>(
    app: &App,
    tracks: &[Track],
    shown: Option<&[usize]>,
    y0: f32,
    style: RowStyle,
    menu: impl Fn(usize, &Track) -> MenuTarget,
) -> Element<'a, Message> {
    let count = shown.map_or(tracks.len(), <[usize]>::len);
    super::rows::note_list(y0 + super::rows::HEADER_H, tracks.len(), shown);
    let range = visible_rows(app, y0 + super::rows::HEADER_H, TRACK_ROW_H, count);
    let list = virtual_list(count, TRACK_ROW_H, range, |k| {
        let i = shown.map_or(k, |shown| shown[k]);
        track_row(app, i, &tracks[i], style, &Source::Page, menu(i, &tracks[i]))
    });
    column![super::rows::header(app, style), list].into()
}

/// An album with several discs: a heading before each one.
fn disc_list<'a>(app: &App, tracks: &[Track], y0: f32, style: RowStyle) -> Element<'a, Message> {
    // The arrow keys move along the rows (the disc headings shift them a little).
    super::rows::note_list(y0 + super::rows::HEADER_H, tracks.len(), None);
    let mut list = column![super::rows::header(app, style)];
    let mut disc = 0;
    for (i, track) in tracks.iter().enumerate() {
        if track.volume_number != disc {
            disc = track.volume_number;
            list = list.push(disc_heading(disc, disc > 1));
        }
        list = list.push(track_row(app, i, track, style, &Source::Page, MenuTarget::Track(track.clone())));
    }
    list.into()
}

/// A disc's heading in an album's list: a dim caption in capitals over the
/// number column, as the unfolded album's (more air above a later disc).
fn disc_heading<'a>(disc: u32, later: bool) -> Element<'a, Message> {
    let label = crate::i18n::text("disc.heading", &[disc.to_string()]);
    container(text(label).size(widgets::scale::TYPE_CAPTION).font(widgets::semibold()).color(theme::text_dim()))
        .height(if later { 40.0 } else { 28.0 })
        .padding(Padding { left: 12.0, bottom: 6.0, ..Default::default() })
        .align_y(alignment::Vertical::Bottom)
        .into()
}

fn album_page(app: &App, id: u64, width: f32) -> Element<'_, Message> {
    let Some(Load::Ready((album, tracks))) = app.albums.get(&id) else { return load_state(app.albums.get(&id)).unwrap_or_else(loading) };
    let total: u64 = tracks.iter().map(|t| t.duration as u64).sum();
    let count = if tracks.len() == 1 {
        crate::i18n::t("1_track").to_string()
    } else {
        crate::i18n::text("tracks.count", &[tracks.len().to_string()])
    };
    let meta = [count, album.year().to_string(), if total > 0 { fmt_hms(total) } else { String::new() }]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let local = crate::local::is_local_id(id);
    let fav = app.fav_album_ids.contains(&id);
    let artist_name = row![
        super::icons::icon(Icon::Artist, 14.0, theme::text_secondary()),
        line(album.artist_names(), scale::TYPE_SMALL, theme::text()).font(widgets::semibold()).underline_on_hover()
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let artist: Element<'_, Message> = match album.artists.first().filter(|a| a.id != 0) {
        Some(a) => mouse_area(super::wash::track(artist_name))
            .on_press(Message::Navigate(Page::Artist(a.id)))
            .interaction(iced::mouse::Interaction::Pointer)
            .into(),
        None => artist_name.into(),
    };
    let mut badges = row![].spacing(8).align_y(Alignment::Center);
    if let Some(b) = widgets::quality_badge(&album.tags) {
        badges = badges.push(b);
    }
    if album.explicit {
        badges = badges.push(widgets::explicit_badge());
    }
    let mut details = column![artist, text(meta).size(theme::TYPE_LABEL).color(theme::text_secondary()), badges].spacing(8);
    if app.settings.lock().show_ratings {
        details = details.push(widgets::stars_sized(app, id, true, 17.0));
    }
    let mut secondary: Vec<Element<'_, Message>> = Vec::new();
    if local {
        let open = Message::Local(super::local::LocalMsg::OpenSettings);
        secondary.push(head_action(Icon::Settings, crate::i18n::t("settings"), false, open));
    } else {
        let (glyph, label) = if fav { (Icon::HeartFilled, "sort.added") } else { (Icon::Heart, "add") };
        secondary.push(head_action(glyph, crate::i18n::t(label), false, Message::ToggleFavoriteAlbum(album.clone())));
        secondary.push(head_action(Icon::Link, crate::i18n::t("share"), false, Message::CopyLink(share_url("album", &id.to_string()))));
    }
    secondary.push(head_action(Icon::More, crate::i18n::t("more"), false, Message::OpenMenu(MenuTarget::Album(album.clone()))));
    let metrics = Head::of(app, width);
    let art = mouse_area(widgets::cover_rounded(album.cover(640), metrics.cover(), scale::R_MD))
        .on_press(Message::OpenCover(album.cover(1280).unwrap_or_default()));
    let head = header(
        art.into(),
        (if local { crate::i18n::t("local_album") } else { crate::i18n::t("album") }).to_string(),
        album.title.clone(),
        details.into(),
        metrics,
        play_pills(app, !tracks.is_empty()),
        secondary,
    );
    let head_h = metrics.h();
    let style = RowStyle::new(false, false, true).fit(width);
    let list = if tracks.is_empty() {
        widgets::notice(Icon::Album, crate::i18n::t("this_album_has_no_available_tracks"))
    } else if tracks.iter().any(|t| t.volume_number > 1) {
        disc_list(app, tracks, head_h + widgets::SECTION_H, style)
    } else {
        track_list(app, tracks, None, head_h + widgets::SECTION_H, style, |_, t| MenuTarget::Track(t.clone()))
    };
    let mut c = column![head, section_title(crate::i18n::t("tracks")), list];
    if !album.copyright.is_empty() {
        c = c.push(note_text(format!("{}  ·  {}", album.release_date, album.copyright)));
    }
    if let Some(Load::Ready((more, similar))) = app.album_extra.get(&id) {
        if !more.is_empty() {
            c = c.push(shelf_panel(
                crate::i18n::text("more_albums_by", &[album.artist_names()]),
                &more.iter().cloned().map(Item::Album).collect::<Vec<_>>(),
                width,
            ));
        }
        if !similar.is_empty() {
            c = c.push(shelf_panel(
                crate::i18n::t("related_artists").into(),
                &similar.iter().cloned().map(Item::Artist).collect::<Vec<_>>(),
                width,
            ));
        }
    }
    c.into()
}

fn artist_page(app: &App, id: u64, width: f32) -> Element<'_, Message> {
    let Some(Load::Ready(a)) = app.artists.get(&id) else { return load_state(app.artists.get(&id)).unwrap_or_else(loading) };
    let tracks = &a.top_tracks;
    let follows = app.fav_artist_ids.contains(&id);
    let local = crate::local::is_local_id(id);
    // As GPUI: Play, Radio and Follow as pills. The header is a pane of glass
    // like the album, playlist and mix headers (GPUI leaves the portrait bare).
    let mut primary: Vec<Element<'_, Message>> = Vec::new();
    if !tracks.is_empty() {
        primary.push(widgets::prominent_pill(
            crate::i18n::t("play"),
            Some(Icon::Play),
            page_tint(app),
            Some(Message::PlayAll(Source::Page)),
        ));
        if let Some(first) = tracks.first().filter(|_| !local) {
            primary.push(pill(crate::i18n::t("radio"), Some(Icon::Radio), false, Some(Message::StartRadio(first.clone()))));
        }
    }
    if !local {
        primary.push(pill(
            if follows { crate::i18n::t("following") } else { crate::i18n::t("follow") },
            Some(if follows { Icon::Check } else { Icon::Plus }),
            false,
            Some(Message::ToggleFavoriteArtist(a.artist.clone())),
        ));
    }
    let secondary = vec![head_action(Icon::More, crate::i18n::t("more"), false, Message::OpenMenu(MenuTarget::Artist(a.artist.clone())))];
    let featured = match tracks.len() {
        0 => String::new(),
        1 => crate::i18n::t("1_featured_track").to_string(),
        n => crate::i18n::text("featured_tracks", &[n.to_string()]),
    };
    let metrics = Head::of(app, width);
    let head = header(
        cover(a.artist.picture(640), metrics.cover(), true),
        crate::i18n::t("artist").into(),
        a.artist.name.clone(),
        text(featured).size(theme::TYPE_LABEL).color(theme::text_secondary()).into(),
        metrics,
        primary,
        secondary,
    );
    let head_h = metrics.h();
    let mut c = column![head];
    if !tracks.is_empty() {
        let style = RowStyle::new(true, true, false).fit(width);
        c = c.push(section_title(crate::i18n::t("top_tracks")));
        c = c.push(track_list(app, tracks, None, head_h + widgets::SECTION_H, style, |_, t| MenuTarget::Track(t.clone())));
    }
    for (t, list) in [
        (crate::i18n::t("albums"), &a.albums),
        (crate::i18n::t("eps_and_singles"), &a.singles),
        (crate::i18n::t("appears_on"), &a.appears_on),
    ] {
        if !list.is_empty() {
            c = c.push(shelf_panel(t.into(), &list.iter().cloned().map(Item::Album).collect::<Vec<_>>(), width));
        }
    }
    if !a.similar.is_empty() {
        c = c.push(shelf_panel(
            crate::i18n::t("similar_artists").into(),
            &a.similar.iter().cloned().map(Item::Artist).collect::<Vec<_>>(),
            width,
        ));
    }
    if let Some(bio) = a.bio.as_ref().filter(|b| !b.trim().is_empty()) {
        // GPUI: a flat panel with a 16 px heading and 13 px text on 21 px lines, 900 px at most.
        let body = text(bio.clone()).size(13).line_height(text::LineHeight::Absolute(21.0.into())).color(theme::text_secondary());
        let inner = column![
            text(crate::i18n::t("biography")).size(theme::TYPE_SECTION).font(widgets::semibold()).color(theme::text()),
            container(body).max_width(900)
        ]
        .spacing(14);
        c = c.push(container(flat_panel(inner, width)).padding(Padding { top: 12.0, ..Default::default() }));
    }
    c.into()
}

fn playlist_page<'a>(app: &'a App, uuid: &str, width: f32) -> Element<'a, Message> {
    let Some(Load::Ready((pl, tracks))) = app.playlists.get(uuid) else {
        return load_state(app.playlists.get(uuid)).unwrap_or_else(loading);
    };
    let total: u64 = tracks.iter().map(|t| t.duration as u64).sum();
    let own = app.is_own_playlist(uuid);
    let saved = app.is_saved_playlist(uuid);
    // As GPUI: creator, up to two lines of description, then the count.
    let mut details = column![].spacing(8);
    if !pl.creator.is_empty() {
        details = details.push(line(pl.creator.clone(), scale::TYPE_SMALL, theme::text()).font(widgets::semibold()));
    }
    if !pl.description.is_empty() {
        details = details.push(
            container(text(pl.description.clone()).size(theme::TYPE_LABEL).color(theme::text_secondary()))
                .max_width(620)
                .max_height(34)
                .clip(true),
        );
    }
    details = details.push(
        text(crate::i18n::text("tracks.count_detail", &[tracks.len().to_string(), fmt_hms(total)]))
            .size(scale::TYPE_LABEL)
            .color(theme::text_secondary()),
    );
    let mut secondary: Vec<Element<'a, Message>> = Vec::new();
    if own {
        let edit = Message::OpenDialog(Dialog::EditPlaylist { uuid: uuid.to_string() });
        secondary.push(head_action(Icon::Edit, crate::i18n::t("edit"), false, edit));
        let delete = Message::OpenDialog(Dialog::DeletePlaylist { uuid: uuid.to_string(), title: pl.title.clone() });
        secondary.push(head_action(Icon::Close, crate::i18n::t("delete"), false, delete));
    } else {
        let (glyph, label) = if saved { (Icon::HeartFilled, "saved") } else { (Icon::Heart, "save") };
        secondary.push(head_action(glyph, crate::i18n::t(label), false, Message::ToggleSavedPlaylist(pl.clone())));
    }
    secondary.push(head_action(Icon::Link, crate::i18n::t("share"), false, Message::CopyLink(share_url("playlist", uuid))));
    secondary.push(head_action(Icon::More, crate::i18n::t("more"), false, Message::OpenMenu(MenuTarget::AddToPlaylist(tracks.clone()))));
    let metrics = Head::of(app, width);
    let head = header(
        widgets::cover_rounded(pl.cover(640), metrics.cover(), scale::R_MD),
        crate::i18n::t("playlist").into(),
        pl.title.clone(),
        details.into(),
        metrics,
        play_pills(app, !tracks.is_empty()),
        secondary,
    );
    let shown: Vec<usize> = (0..tracks.len()).filter(|&i| app.track_matches(&tracks[i])).collect();
    let (bpm, rating) = {
        let s = app.settings.lock();
        (s.audio_metadata, s.show_ratings)
    };
    let style = RowStyle { date: true, bpm, rating, ..RowStyle::new(true, true, false) }.fit(width);
    let u = uuid.to_string();
    let list = track_list(app, tracks, Some(&shown), metrics.h() + FILTER_H, style, move |i, t| {
        if own {
            MenuTarget::PlaylistTrack { uuid: u.clone(), index: i, track: t.clone() }
        } else {
            MenuTarget::Track(t.clone())
        }
    });
    column![head, filter_box(app), list].into()
}

fn mix_page<'a>(app: &'a App, id: &str, width: f32) -> Element<'a, Message> {
    let Some(Load::Ready((mix, tracks))) = app.mixes.get(id) else { return load_state(app.mixes.get(id)).unwrap_or_else(loading) };
    let metrics = Head::of(app, width);
    let head = header(
        widgets::cover_rounded(mix.image.clone(), metrics.cover(), scale::R_MD),
        crate::i18n::t("mix").into(),
        mix.title.clone(),
        text(mix.subtitle.clone()).size(theme::TYPE_LABEL).color(theme::text_secondary()).into(),
        metrics,
        play_pills(app, !tracks.is_empty()),
        Vec::new(),
    );
    let style = RowStyle::new(true, true, false).fit(width);
    column![head, track_list(app, tracks, None, metrics.h(), style, |_, t| MenuTarget::Track(t.clone()))].into()
}

fn search_page<'a>(app: &'a App, q: &str, width: f32) -> Element<'a, Message> {
    if q.is_empty() {
        let recent = app.settings.lock().recent_searches.clone();
        let mut c = column![title(crate::i18n::t("search"), None)];
        if !recent.is_empty() {
            c = c.push(section_title(crate::i18n::t("recent_searches")));
            // GPUI: glass capsules with the query in body text, 8 px apart.
            let chip = |q: String| -> Element<'a, Message> {
                // The capsule is a pane of liquid glass, lit by the pointer.
                let b = button(text(q.clone()).size(theme::TYPE_BODY))
                    .padding([8, 14])
                    .style(|_, _| button::Style { text_color: theme::text(), ..Default::default() })
                    .on_press(Message::SearchRecent(q));
                super::liquid_glass::glass(b, 999.0).into()
            };
            c = c.push(row(recent.into_iter().map(chip)).spacing(8).wrap().vertical_spacing(8));
        }
        return c.into();
    }
    let mut c = column![title(crate::i18n::text("results_for", &[q.to_string()]), None)];
    match app.search.get(q) {
        Some(Load::Ready(r)) => {
            if let Some(top) = &r.top {
                c = c.push(section_title(crate::i18n::t("top_result")));
                c = c.push(top_hit(top));
            }
            if !r.tracks.is_empty() {
                c = c.push(section_title(crate::i18n::t("tracks")));
                let style = RowStyle::new(true, true, false).fit(width);
                // Where the list starts: the title and, when there is one, the top result.
                let top = if r.top.is_some() { widgets::SECTION_H + TOP_HIT_H } else { 0.0 };
                let y0 = TITLE_H + top + widgets::SECTION_H;
                c = c.push(track_list(app, &r.tracks, None, y0, style, |_, t| MenuTarget::Track(t.clone())));
            }
            if !r.artists.is_empty() {
                c = c.push(card_row(crate::i18n::t("artists").into(), &r.artists.iter().cloned().map(Item::Artist).collect::<Vec<_>>()));
            }
            if !r.albums.is_empty() {
                c = c.push(card_row(crate::i18n::t("albums").into(), &r.albums.iter().cloned().map(Item::Album).collect::<Vec<_>>()));
            }
            if !r.playlists.is_empty() {
                c = c.push(card_row(
                    crate::i18n::t("playlists").into(),
                    &r.playlists.iter().cloned().map(Item::Playlist).collect::<Vec<_>>(),
                ));
            }
            if r.top.is_none() && r.tracks.is_empty() && r.albums.is_empty() && r.artists.is_empty() && r.playlists.is_empty() {
                c = c.push(widgets::notice(Icon::Search, crate::i18n::t("no_results")));
            }
        }
        other => c = c.push(load_state(other)),
    }
    c.into()
}

// ----------------------------------------------------------------- library

pub(super) const INITIAL_LIBRARY_BAR_H: f32 = 152.0;
/// Height shared by every control of the Library bar (GPUI's `CTRL_H`).
const CTRL_H: f32 = 32.0;
/// Height of a Library list row (album or artist).
const LIB_ROW_H: f32 = 62.0;
/// Height of Play / Shuffle over the Library's tracks, with the space under them.
const LIB_ACTIONS_H: f32 = 36.0 + 16.0;

fn library_bar<'a>(app: &'a App, section: &'static str) -> Element<'a, Message> {
    library_controls(section, content_width(app), app.lib_grid(section), app.sort_of(section).0, &app.filter)
}

/// A segmented control (GPUI's `library::segmented`): one 32 px surface with
/// the active option lifted inside it; each option is its content and 10 px
/// on each side (the callers size the content to GPUI's minimum widths).
fn segmented<'a>(options: Vec<(Element<'a, Message>, bool, Message)>) -> Element<'a, Message> {
    let inner = theme::concentric(9.0, 4.0);
    let parts: Vec<Element<'a, Message>> = options
        .into_iter()
        .map(|(content, on, msg)| {
            // GPUI `library` segments: the selection glides over `CONTROL`,
            // the hover wash (ink 5 %) fades in where nothing is selected.
            let b = button(container(content).center_y(Length::Fill))
                .height(Length::Fill)
                .width(Length::Shrink)
                .padding([0, 10])
                .style(move |_, _| {
                    let p = super::wash::phase();
                    button::Style {
                        background: Some(
                            super::wash::mix(theme::ink(0.05 * p.hover * (1.0 - p.active)), theme::control_active(), p.active).into(),
                        ),
                        text_color: theme::text(),
                        border: Border { radius: inner.into(), ..Default::default() },
                        ..Default::default()
                    }
                })
                .on_press(msg);
            super::wash::track(b).active(on).into()
        })
        .collect();
    container(row(parts).spacing(2).height(Length::Fill))
        .height(CTRL_H)
        .padding(3)
        .style(|_| container::Style {
            background: Some(theme::ink(0.05).into()),
            border: Border { color: theme::ink(0.10), width: 1.0, radius: 9.0.into() },
            ..Default::default()
        })
        .into()
}

/// A control of the Library bar's surface: ink fill, faint edge, 9 px
/// corners; `h` (0..1) is its eased hover.
fn control_style(h: f32) -> container::Style {
    container::Style {
        background: Some(theme::ink(0.05 + 0.05 * h).into()),
        border: Border { color: theme::ink(0.10 + 0.06 * h), width: 1.0, radius: 9.0.into() },
        ..Default::default()
    }
}

fn library_controls<'a>(
    section: &'static str,
    width: f32,
    grid: bool,
    key: super::library::SortKey,
    filter: &'a str,
) -> Element<'a, Message> {
    let cats = [("tracks", crate::i18n::t("tracks")), ("albums", crate::i18n::t("albums")), ("artists", crate::i18n::t("artists"))];
    let compact = width < 560.0;
    let medium = super::font(iced::font::Weight::Medium);
    let cat_w = if compact { 52.0 } else { 70.0 };
    let seg = segmented(
        cats.into_iter()
            .map(|(s, label)| {
                let on = s == section;
                let t = text(label)
                    .size(12.5)
                    .font(if on { widgets::semibold() } else { medium })
                    // The colour follows the segment's gliding selection.
                    .style(|_| text::Style {
                        color: Some(super::wash::mix(theme::text_secondary(), theme::text(), super::wash::phase().active)),
                    })
                    .align_x(alignment::Horizontal::Center)
                    .width(Length::Fixed(cat_w - 20.0));
                (Element::from(t), on, Message::LibraryCat(s))
            })
            .collect(),
    );
    let heading = cats.iter().find(|(s, _)| *s == section).map_or("", |(_, label)| *label);
    let sort_label = text(if width < 760.0 { key.short_label() } else { key.label() }).size(12.5).font(medium).color(theme::text());
    let chevron = super::icons::icon(Icon::ChevDown, 12.0, theme::text_secondary());
    let sort = button(container(row![sort_label, chevron].spacing(6).align_y(Alignment::Center)).center_y(Length::Fill))
        .height(CTRL_H)
        .padding([0, 12])
        .style(|_, _| {
            let p = super::wash::phase();
            let s = control_style(p.hover.max(p.press));
            button::Style { background: s.background, border: s.border, text_color: theme::text(), ..Default::default() }
        })
        .on_press(Message::OpenMenu(MenuTarget::Sort(section)));
    let sort = super::wash::track(sort);
    // The glyph's colour follows the segment's gliding selection.
    let view_icon = |i: Icon, _on: bool| -> Element<'a, Message> {
        let g = super::icons::glyph(i, 15.0, theme::text_secondary())
            .live(|p, _| super::wash::mix(theme::text_secondary(), theme::text(), p.active));
        container(g).center_x(10).into()
    };
    let views = segmented(vec![
        (view_icon(Icon::List, !grid), !grid, Message::LibGrid(section, false)),
        (view_icon(Icon::Grid, grid), grid, Message::LibGrid(section, true)),
    ]);
    let search_w = if width < 640.0 {
        110.0
    } else if width < 820.0 {
        150.0
    } else {
        190.0
    };
    let field = text_input(crate::i18n::t("search.placeholder"), filter)
        .on_input(Message::Filter)
        .size(theme::TYPE_LABEL)
        .padding(0)
        .style(bare_field_style);
    let search =
        container(row![super::icons::icon(Icon::Search, 13.0, theme::text_secondary()), field].spacing(7).align_y(Alignment::Center))
            .width(search_w)
            .height(CTRL_H)
            .padding([0, 10])
            .align_y(alignment::Vertical::Center)
            .style(|_| control_style(0.0));
    let sort = container(sort).id(super::menus::SORT_ANCHOR);
    // GPUI: the switch on the left, the rest on the right, wrapping when narrow.
    // A wrapping row gives a `Fill` gap the whole line (it would always push the
    // tools under the switch), so the gap is there only when everything fits.
    let tools: Element<'a, Message> = if width >= 720.0 {
        let right = row![sort, views, search].spacing(8).align_y(Alignment::Center);
        row![seg, Space::new().width(Length::Fill), right].spacing(10).align_y(Alignment::Center).into()
    } else {
        row![seg, sort, views, search].spacing(8).align_y(Alignment::Center).wrap().vertical_spacing(8).into()
    };
    // A slim pane of glass, as the headers of the catalogue pages: over an
    // album's banner it frosts the colour behind the controls so they stay legible.
    let tools = super::liquid_glass::glass(container(tools).width(Length::Fill).padding(8), 12.0)
        .optics(super::liquid_glass::Optics::POPOVER)
        .shadow(theme::popover_shadow());
    let header = column![
        text(heading).size(scale::TYPE_DISPLAY).font(widgets::bold()).color(theme::text()),
        text(crate::i18n::t("your_library")).size(theme::TYPE_LABEL).color(theme::text_secondary()),
    ]
    .spacing(4)
    // With the column's own 18 px, 24 px above (GPUI); 16 below.
    .padding(iced::Padding { top: 6.0, bottom: 16.0, ..Default::default() });
    let bar = column![header, tools].padding(Padding { bottom: 16.0, ..Default::default() });
    iced::widget::sensor(bar).on_resize(|size| Message::LibraryBarSized(size.height)).into()
}

fn library_page(app: &App, width: f32) -> Element<'_, Message> {
    let section = lib_section(&app.page).unwrap_or("tracks");
    let bar = library_bar(app, section);
    let list = &app.lib_shown;
    let body: Element<'_, Message> = match section {
        "tracks" => {
            let waiting = app.signed_in && matches!(app.fav_tracks, Load::Loading | Load::Idle) && app.local_tracks.is_empty();
            if waiting {
                loading_block()
            } else if let (true, true, Load::Failed(error)) = (app.signed_in, app.local_tracks.is_empty(), &app.fav_tracks) {
                library_failed(error)
            } else if app.lib_tracks.is_empty() {
                widgets::notice(Icon::Track, crate::i18n::text("library.empty_tracks", &[crate::i18n::t("tidal").to_string()]))
            } else {
                // As GPUI: Play and Shuffle over the tracks, as a list or a grid.
                let actions =
                    container(actions_row(Source::Page, true, Vec::new())).padding(Padding { bottom: 16.0, ..Default::default() });
                let y0 = app.lib_bar_h + LIB_ACTIONS_H;
                if list.is_empty() {
                    column![actions, widgets::notice(Icon::Search, crate::i18n::t("no_tracks_match_your_search"))].into()
                } else if app.lib_grid("tracks") {
                    column![actions, virtual_grid(app, list.len(), width, y0, |i| Item::Track(app.lib_tracks[list[i]].clone()))].into()
                } else {
                    let (bpm, rating) = {
                        let s = app.settings.lock();
                        (s.audio_metadata, s.show_ratings)
                    };
                    let style =
                        RowStyle { date: true, bpm, rating, sortable: Some("tracks"), ..RowStyle::new(true, true, false) }.fit(width);
                    column![actions, track_list(app, &app.lib_tracks, Some(list), y0, style, |_, t| MenuTarget::Track(t.clone()))].into()
                }
            }
        }
        "albums" => {
            let waiting = app.signed_in && matches!(app.fav_albums, Load::Loading | Load::Idle) && app.local_albums.is_empty();
            if waiting {
                loading_block()
            } else if let (true, true, Load::Failed(error)) = (app.signed_in, app.local_albums.is_empty(), &app.fav_albums) {
                library_failed(error)
            } else if app.lib_albums.is_empty() {
                widgets::notice(Icon::Album, crate::i18n::t("you_have_not_added_any_albums_yet"))
            } else if list.is_empty() {
                widgets::notice(Icon::Search, crate::i18n::t("no_albums_match_your_search"))
            } else if app.lib_grid("albums") {
                album_grid(app, list, width)
            } else {
                let range = visible_rows(app, app.lib_bar_h, LIB_ROW_H, list.len());
                virtual_list(list.len(), LIB_ROW_H, range, |i| {
                    let album = &app.lib_albums[list[i]];
                    let tracks = if album.number_of_tracks == 1 {
                        crate::i18n::t("1_track").to_string()
                    } else {
                        crate::i18n::text("tracks.count", &[album.number_of_tracks.to_string()])
                    };
                    let meta = [
                        (500.0, album.year().to_string(), 52.0),
                        (420.0, tracks, 76.0),
                        (320.0, fmt_long(u64::from(album.duration)), 70.0),
                    ]
                    .into_iter()
                    .filter(|(min, ..)| width >= *min)
                    .map(|(_, t, w)| (t, w))
                    .collect();
                    list_row(
                        cover(album.cover(160), 44.0, false),
                        album.title.clone(),
                        album.artist_names(),
                        meta,
                        Item::Album(album.clone()),
                    )
                })
            }
        }
        _ => {
            let waiting = app.signed_in && matches!(app.fav_artists, Load::Loading | Load::Idle) && app.local_artists.is_empty();
            if waiting {
                loading_block()
            } else if let (true, true, Load::Failed(error)) = (app.signed_in, app.local_artists.is_empty(), &app.fav_artists) {
                library_failed(error)
            } else if app.lib_artists.is_empty() {
                widgets::notice(Icon::Artist, crate::i18n::t("you_are_not_following_any_artists_yet"))
            } else if list.is_empty() {
                widgets::notice(Icon::Search, crate::i18n::t("no_artists_match_your_search"))
            } else if app.lib_grid("artists") {
                virtual_grid(app, list.len(), width, app.lib_bar_h, |i| Item::Artist(app.lib_artists[list[i]].clone()))
            } else {
                let range = visible_rows(app, app.lib_bar_h, LIB_ROW_H, list.len());
                virtual_list(list.len(), LIB_ROW_H, range, |i| {
                    let artist = &app.lib_artists[list[i]];
                    list_row(
                        cover(artist.picture(320), 44.0, true),
                        artist.name.clone(),
                        crate::i18n::t("artist").into(),
                        Vec::new(),
                        Item::Artist(artist.clone()),
                    )
                })
            }
        }
    };
    column![bar, body].into()
}

fn loading_block<'a>() -> Element<'a, Message> {
    container(widgets::loading_dots()).width(Length::Fill).height(320).center(Length::Fill).into()
}

fn library_failed(error: &str) -> Element<'_, Message> {
    page_error(error, Message::RetryPage)
}

fn page_error<'a>(error: &str, retry: Message) -> Element<'a, Message> {
    error_message(crate::i18n::text("could_not_load", &[error.to_string()]), retry)
}

/// An album or artist in the Library's list view (GPUI's `album_row` /
/// `artist_row`): 62 px with a rule under it, a 44 px picture, the name over
/// the artist and, for albums, year, tracks and length where there is room.
fn list_row<'a>(art: Element<'a, Message>, t: String, sub: String, meta: Vec<(String, f32)>, item: Item) -> Element<'a, Message> {
    let names =
        column![line(t, scale::TYPE_SMALL, theme::text()).font(widgets::medium()), line(sub, theme::TYPE_LABEL, theme::text_secondary())]
            .spacing(2);
    let mut r = row![art, container(names).width(Length::Fill).clip(true)].spacing(14).align_y(Alignment::Center);
    for (value, w) in meta {
        r = r.push(container(line(value, theme::TYPE_LABEL, theme::text_secondary())).width(w).clip(true));
    }
    let r = container(r).height(LIB_ROW_H - 1.0).padding([0, 12]).align_y(alignment::Vertical::Center);
    let r = column![
        r,
        container(Space::new().width(Length::Fill).height(1))
            .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() })
    ];
    let menu = match &item {
        Item::Album(a) => MenuTarget::Album(a.clone()),
        Item::Artist(a) => MenuTarget::Artist(a.clone()),
        _ => MenuTarget::Output,
    };
    mouse_area(widgets::soft(button(r).width(Length::Fill).padding(0).on_press(Message::OpenItem(item)), 6.0))
        .on_right_press(Message::OpenMenu(menu))
        .into()
}

fn card_height(card_w: f32) -> f32 {
    // Cover, 7 px, title, artist and the year 3 px apart, then GPUI's 22 px under a grid row.
    card_w + 7.0 + 12.0 * 1.3 + 3.0 + 11.5 * 1.3 + 3.0 + 10.0 * 1.3 + 22.0
}

/// A grid of `count` cards, virtualised by rows: `item` is asked only for those shown.
fn virtual_grid<'a>(app: &App, count: usize, width: f32, y0: f32, item: impl Fn(usize) -> Item) -> Element<'a, Message> {
    let (cols, card_w) = grid_metrics(width);
    let rows = count.div_ceil(cols);
    let h = card_height(card_w);
    let range = visible_rows(app, y0, h, rows);
    virtual_list(rows, h, range, |r| {
        row((r * cols..((r + 1) * cols).min(count)).map(|i| widgets::grid_card(app, &item(i), card_w))).spacing(theme::GAP).into()
    })
}

/// A track of the unfolded album, as GPUI's `album_inline` rows: 42 px with a
/// rule under it, number (plays), title, stars where there is room, the like
/// and the duration; a double click plays too.
fn inline_track<'a>(app: &App, i: usize, track: &Track, source: &Source, multi_disc: bool, list_w: f32) -> Element<'a, Message> {
    let current = app.now.as_ref().is_some_and(|n| n.track.id == track.id);
    let play = Message::PlayFrom(source.clone(), i);
    let number = if multi_disc { format!("{}.{}", track.volume_number, track.track_number) } else { track.track_number.to_string() };
    let resting: Element<'a, Message> = if current {
        widgets::playing_indicator(app.is_playing(), 14.0)
    } else {
        text(number).size(theme::TYPE_LABEL).color(theme::text_dim()).into()
    };
    let resting = container(resting).center_x(26).center_y(Length::Fill);
    let glyph = if current && app.is_playing() { Icon::Pause } else { Icon::Play };
    let play_button =
        container(widgets::icon_box(glyph, 15.0, 26.0, theme::text(), play.clone())).center(26).style(move |_| container::Style {
            background: Some(if current { theme::active() } else { theme::hover() }.into()),
            border: Border { radius: 6.0.into(), ..Default::default() },
            ..Default::default()
        });
    let title = line(track.display_title(), scale::TYPE_SMALL, if current { theme::accent() } else { theme::text() }).font(if current {
        widgets::semibold()
    } else {
        widgets::medium()
    });
    // Ratings give way to the title in the narrowest layout; a rated track
    // shows its stars, the like shows once given.
    let rated = app.ratings.get(track.id) > 0;
    let room_for_stars = list_w >= 300.0;
    let stars: Option<Element<'a, Message>> = room_for_stars.then(|| {
        if rated {
            widgets::stars_sized(app, track.id, false, 12.0)
        } else {
            Space::new().width(60).height(12).into()
        }
    });
    let fav = app.fav_ids.contains(&track.id);
    let can_like = track.local_path.is_none();
    let heart = |filled| {
        let (icon, tone) = if filled { (Icon::HeartFilled, theme::text()) } else { (Icon::Heart, theme::text_secondary()) };
        widgets::icon_box(icon, 14.0, 22.0, tone, Message::ToggleFavorite(track.clone()))
    };
    let heart_space = || Space::new().width(22).height(22);
    let time = widgets::fmt_time(track.duration as f64);
    let base = row![resting, container(title).width(Length::Fill).clip(true)]
        .push(stars)
        .push(if fav && can_like { heart(true) } else { heart_space().into() })
        .push(text(time.clone()).size(theme::TYPE_LABEL).color(theme::text_secondary()))
        .spacing(10)
        .height(Length::Fill)
        .align_y(Alignment::Center);
    let base = container(base).height(41).padding([0, 10]).style(move |_| container::Style {
        background: current.then(|| super::rows::tone::playing_fill().into()),
        border: Border { radius: scale::R_SM.into(), ..Default::default() },
        ..Default::default()
    });
    let hover_stars: Option<Element<'a, Message>> = room_for_stars.then(|| {
        if rated {
            Space::new().width(60).height(12).into()
        } else {
            widgets::stars_sized(app, track.id, false, 12.0)
        }
    });
    let overlay = row![play_button, Space::new().width(Length::Fill)]
        .push(hover_stars)
        .push(if !fav && can_like { heart(false) } else { heart_space().into() })
        .push(text(time).size(theme::TYPE_LABEL).color(Color::TRANSPARENT))
        .spacing(10)
        .height(Length::Fill)
        .align_y(Alignment::Center);
    let overlay = container(overlay).width(Length::Fill).height(41).padding([0, 10]);
    let body = super::wash::wash(iced::widget::hover(base, overlay), super::rows::tone::hover(), scale::R_SM)
        .press(super::rows::tone::press())
        .strip(super::rows::tone::strip());
    let body = column![body, rule()];
    mouse_area(body)
        .on_double_click(play)
        .on_right_press(Message::OpenMenu(MenuTarget::Track(track.clone())))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

/// Albums of the Library grid; the clicked one unfolds its tracks under its row.
fn album_grid<'a>(app: &'a App, shown: &[usize], width: f32) -> Element<'a, Message> {
    let (cols, card_w) = grid_metrics(width);
    let h = card_height(card_w);
    let albums = &app.lib_albums;
    let expanded = app.expanded_album.and_then(|id| shown.iter().position(|&i| albums[i].id == id));
    let Some(pos) = expanded else {
        return virtual_grid(app, shown.len(), width, app.lib_bar_h, |i| Item::Album(albums[shown[i]].clone()));
    };
    let er = pos / cols;
    let card_row = |indices: &[usize]| {
        row(indices.iter().map(|&i| widgets::grid_card(app, &Item::Album(albums[i].clone()), card_w))).spacing(theme::GAP)
    };
    let build_row = |i: usize| card_row(&shown[i * cols..((i + 1) * cols).min(shown.len())]).into();
    let before = er + 1;
    let after = shown.len().div_ceil(cols) - before;
    let start_after = app.lib_bar_h + before as f32 * h + app.expanded_album_height;
    let album = &albums[shown[pos]];
    let id = album.id;
    let grid_w = cols as f32 * card_w + cols.saturating_sub(1) as f32 * theme::GAP;
    // Centre of the clicked card, where the panel's tip points (GPUI).
    let tip_cx = (pos % cols) as f32 * (card_w + theme::GAP) + card_w / 2.0;
    let detail = iced::widget::sensor(album_inline(app, album, grid_w, tip_cx))
        .key(id)
        .anticipate(400)
        .on_resize(move |size| Message::InlineAlbumSized(id, size.height));
    column![
        virtual_list(before, h, visible_rows(app, app.lib_bar_h, h, before), build_row),
        detail,
        virtual_list(after, h, visible_rows(app, start_after, h, after), |i| build_row(i + before)),
    ]
    .into()
}

/// The unfolded album (GPUI's `album_inline::render`): a glass panel with the
/// cover and details on the left and the tracks in columns on the right.
fn album_inline<'a>(app: &'a App, album: &Album, grid_w: f32, tip_cx: f32) -> Element<'a, Message> {
    // As in GPUI: too narrow for two columns, the tracks go under the cover.
    let stacked = grid_w < 570.0;
    let inset = if stacked { 14.0 } else { 20.0 };
    let cover_size = (grid_w - 2.0 * inset).clamp(1.0, if stacked { 144.0 } else { 200.0 });
    let list_w = if stacked { grid_w - 2.0 * inset } else { grid_w - 200.0 - 24.0 - 2.0 * inset };
    let body: Element<'a, Message> = match app.albums.get(&album.id) {
        Some(Load::Ready((_, tracks))) => {
            let tracks = Arc::new(tracks.clone());
            // As in GPUI: as many columns as fit (260 px each), but only as many as the album needs.
            let fit = ((list_w + 16.0) / 276.0).floor().clamp(1.0, 4.0) as usize;
            let cols = fit.min(tracks.len().div_ceil(10).clamp(1, 4));
            let source = Source::List(tracks.clone());
            let multi_disc = tracks.iter().any(|t| t.volume_number > 1);
            // A flat list with a heading before each disc, cut into columns.
            let mut flat: Vec<Result<usize, u32>> = Vec::new();
            let mut disc = 0;
            for (i, track) in tracks.iter().enumerate() {
                if multi_disc && track.volume_number != disc {
                    disc = track.volume_number;
                    flat.push(Err(disc));
                }
                flat.push(Ok(i));
            }
            let per = flat.len().div_ceil(cols).max(1);
            let columns = flat.chunks(per).map(|chunk| {
                let entries = chunk.iter().map(|e| match e {
                    Ok(i) => inline_track(app, *i, &tracks[*i], &source, multi_disc, list_w),
                    Err(d) => container(
                        text(crate::i18n::text("disc.heading", &[d.to_string()]))
                            .size(scale::TYPE_CAPTION)
                            .font(widgets::semibold())
                            .color(theme::text_dim()),
                    )
                    .height(30)
                    .padding(Padding { left: 10.0, right: 10.0, bottom: 4.0, top: 0.0 })
                    .align_y(alignment::Vertical::Bottom)
                    .into(),
                });
                column(entries).width(Length::FillPortion(1)).into()
            });
            row(columns).spacing(16).into()
        }
        Some(Load::Failed(error)) => page_error(error, Message::RetryInlineAlbum(album.clone())),
        _ => container(widgets::loading_dots()).width(Length::Fill).height(64).center(Length::Fill).into(),
    };
    let tracks = app.albums.get(&album.id).and_then(Load::ready).map(|(_, t)| Arc::new(t.clone())).unwrap_or_default();
    let (count, total) = if tracks.is_empty() {
        (album.number_of_tracks as usize, u64::from(album.duration))
    } else {
        (tracks.len(), tracks.iter().map(|t| u64::from(t.duration)).sum())
    };
    let year = if album.year().is_empty() { String::new() } else { format!("{} · ", album.year()) };
    let meta = crate::i18n::text("tracks.count_duration", &[year, count.to_string(), fmt_long(total)]);
    let artist = line(album.artist_names(), 13.0, theme::text_secondary());
    let artist = match album.artists.first().filter(|a| a.id != 0) {
        Some(a) => widgets::text_link(artist, Message::Navigate(Page::Artist(a.id))),
        _ => artist.into(),
    };
    let art = mouse_area(cover(album.cover(640), cover_size, false))
        .on_press(Message::OpenCover(album.cover(1280).or_else(|| album.cover(640)).unwrap_or_default()))
        .interaction(iced::mouse::Interaction::Pointer);
    let title = mouse_area(super::wash::track(
        container(text(album.title.clone()).size(scale::TYPE_HEADING).font(widgets::bold()).style(|_| text::Style {
            color: Some(theme::lerp(theme::text(), theme::text_secondary(), 0.35 * (1.0 - super::wash::phase().hover))),
        }))
        .max_height(2.0 * scale::TYPE_HEADING * 1.3)
        .clip(true)
        .padding(Padding { top: 6.0, ..Default::default() }),
    ))
    .on_press(Message::Navigate(Page::Album(album.id)))
    .interaction(iced::mouse::Interaction::Pointer);
    let colors: Option<[Color; 3]> = album.cover(320).and_then(|url| super::images::palette(&url).try_into().ok());
    let tools = row![
        widgets::prominent_pill(
            crate::i18n::t("play"),
            Some(Icon::Play),
            super::liquid_glass::prominent(colors),
            (!tracks.is_empty()).then(|| Message::PlayAll(Source::List(tracks.clone())))
        ),
        icon_button(Icon::Shuffle, 20.0, theme::text_secondary(), Message::ShuffleAll(Source::List(tracks.clone()))),
        widgets::tip(
            icon_button(Icon::Forward, 20.0, theme::text_secondary(), Message::Navigate(Page::Album(album.id))),
            crate::i18n::t("view_album"),
        ),
        icon_button(Icon::Close, 18.0, theme::text_secondary(), Message::ToggleAlbumInline(album.clone())),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .wrap()
    .vertical_spacing(6);
    let left = column![art, title, artist, text(meta).size(theme::TYPE_LABEL).color(theme::text_dim())]
        .push(widgets::stars_sized(app, album.id, true, 15.0))
        .push(widgets::quality_badge(&album.tags))
        .push(container(tools).padding(Padding { top: 8.0, ..Default::default() }))
        .spacing(8)
        .width(if stacked { Length::Fill } else { Length::Fixed(200.0) });
    let panel: Element<'a, Message> =
        if stacked { column![left, body].spacing(24).into() } else { row![left, container(body).width(Length::Fill)].spacing(24).into() };
    // The unfolded album is a pane of liquid glass (see `liquid_glass`).
    // Lit from inside by the album's own colours (it sits below the page's banner).
    let panel = super::liquid_glass::glass(container(panel).width(Length::Fill).padding(inset), 14.0)
        .album_light(colors)
        .shadow(theme::popover_shadow());
    // GPUI leaves 11 px above for the tip pointing at the card, 18 below.
    let panel = container(panel).padding(Padding { top: 11.0, bottom: 18.0, ..Default::default() });
    // The tip continues the pane: the glass colour warmed by the album's own
    // lights (the pane is lit from inside), not the plain grey of an unlit pane.
    let fill = colors.map_or_else(theme::glass_top, |c| {
        let lights = super::gpu::glass::banner_palette(c, theme::is_light());
        let mean = |f: fn(&Color) -> f32| lights.iter().map(f).sum::<f32>() / 3.0;
        let glow = Color::from_rgb(mean(|c| c.r), mean(|c| c.g), mean(|c| c.b));
        let top = theme::glass_top();
        Color { a: top.a, ..theme::lerp(top, glow, if theme::is_light() { 0.35 } else { 0.5 }) }
    });
    let tip = iced::widget::canvas(InlineTip { left: (tip_cx - InlineTip::W / 2.0).max(14.0), fill })
        .width(Length::Fill)
        .height(InlineTip::H + 1.0);
    stack![panel, tip].into()
}

/// The tip over the unfolded album's top edge, pointing at its card (GPUI's
/// `shapes/tip.svg`): the glass colour with the rim along its two sides,
/// reaching 1 px into the panel so it covers the border under its base.
struct InlineTip {
    left: f32,
    fill: Color,
}

impl InlineTip {
    const W: f32 = 24.0;
    const H: f32 = 12.0;
}

impl iced::widget::canvas::Program<Message> for InlineTip {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, Path, Stroke};
        use iced::Point;
        let mut frame = Frame::new(renderer, bounds.size());
        let (x, w, h) = (self.left, Self::W, Self::H);
        // A slightly rounded apex, as the SVG.
        let shape = |inset: f32| {
            Path::new(|b| {
                b.move_to(Point::new(x + inset, h + 1.0));
                b.line_to(Point::new(x + w / 2.0 - 2.0, 1.0 + inset * 0.5 + 1.2));
                b.quadratic_curve_to(Point::new(x + w / 2.0, inset * 0.5), Point::new(x + w / 2.0 + 2.0, 1.0 + inset * 0.5 + 1.2));
                b.line_to(Point::new(x + w - inset, h + 1.0));
                b.close();
            })
        };
        frame.fill(&shape(0.0), self.fill);
        let sides = Path::new(|b| {
            b.move_to(Point::new(x, h));
            b.line_to(Point::new(x + w / 2.0 - 2.0, 2.2));
            b.quadratic_curve_to(Point::new(x + w / 2.0, 1.0), Point::new(x + w / 2.0 + 2.0, 2.2));
            b.line_to(Point::new(x + w, h));
        });
        frame.stroke(&sides, Stroke::default().with_width(1.0).with_color(theme::glass_rim(0.0)));
        vec![frame.into_geometry()]
    }
}

// The part of the window the page is drawn in, under the top bar (design px).
pub fn page_area(app: &App) -> iced::Rectangle {
    let gap = theme::FRAME_GAP;
    let left = if app.signed_in { super::sidebar::RAIL_W } else { gap };
    let side = side_panel_width(app) * app.side_reveal.fraction();
    let top = gap + theme::TOP_H;
    iced::Rectangle {
        x: left,
        y: top,
        width: (app.viewport.width - left - gap - side).max(0.0),
        height: (app.viewport.height - top - theme::PLAYER_H - 2.0 * gap).max(0.0),
    }
}

// ----------------------------------------------------------------- side panel

pub fn side_panel(app: &App) -> Option<Element<'_, Message>> {
    if !app.side_reveal.visible() {
        return None;
    }
    let width = side_panel_width(app);
    let lyrics = app.side_last_lyrics;
    let (i, t, detail) = if lyrics {
        (Icon::Lyrics, crate::i18n::t("lyrics"), app.now.as_ref().map(|n| n.track.display_title()))
    } else {
        let next = app.now.as_ref().map_or(0, |n| n.index + 1);
        let count = app.queue.len().saturating_sub(next);
        (Icon::Queue, crate::i18n::t("play_queue"), Some(crate::i18n::text("queued", &[format!("{count}")])))
    };
    let wide = app.settings.lock().wide_side_panel;
    let head = row![
        widgets::panel_heading(i, t, detail),
        icon_button(if wide { Icon::WinRestore } else { Icon::WinMax }, 20.0, theme::text_secondary(), Message::WideSidePanel),
        icon_button(Icon::Close, 20.0, theme::text_secondary(), Message::CloseSidePanel),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .padding([0, 12])
    // GPUI's 66 px include the rule under the heading.
    .height(65);
    // GPUI keeps 8 px above and below the queue; the lyrics run to the edges.
    let body: Element<'_, Message> = if lyrics {
        super::now_playing::lyrics_view(app, super::lyrics::Kind::Side, super::lyrics::Style::side())
    } else {
        container(super::queue::panel(app, false)).height(Length::Fill).padding([8, 0]).into()
    };
    let column = column![head, rule(), container(body).height(Length::Fill)];
    // Only a rule on the left, as GPUI's `border_l_1`.
    let edge = container(Space::new().width(1).height(Length::Fill))
        .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() });
    let panel = container(row![edge, column])
        .width(width)
        .height(Length::Fill)
        .style(|_| container::Style { background: Some(theme::surface().into()), ..Default::default() });
    // The handle lies over the panel's left edge.
    let handle = (app.show_lyrics || app.show_queue).then(|| super::chrome::side_handle(width));
    let panel = widgets::layered(panel.into(), vec![handle]);
    Some(super::panel_motion::sliding(panel, app.side_reveal, width, theme::bg()))
}

// ----------------------------------------------------------------- overlays

pub(super) fn scrim<'a>(content: Element<'a, Message>, close: Message, alpha: f32) -> Element<'a, Message> {
    // The veil fades in with the surface over it instead of popping in.
    let back = mouse_area(container(Space::new().width(Length::Fill).height(Length::Fill)).style(move |_| container::Style {
        background: Some(super::wash::faded(Color { a: alpha, ..Color::BLACK }).into()),
        ..Default::default()
    }))
    .on_press(close);
    let back = super::wash::appear(back, super::motion::PANEL, 0, 0.0, 0.0);
    stack![back, container(iced::widget::opaque(content)).center(Length::Fill)].into()
}

pub fn cover_overlay<'a>(app: &'a App, url: &'a str) -> Element<'a, Message> {
    let side = (app.viewport.width.min(app.viewport.height) - 64.0).clamp(1.0, 1280.0);
    let img: Element<'a, Message> = match super::images::fitted_cover(url, side) {
        Some(h) => iced::widget::image(h).width(side).height(side).content_fit(iced::ContentFit::Contain).into(),
        None => widgets::loading_dots(),
    };
    let back = mouse_area(super::liquid_glass::cover_backdrop()).on_press(Message::CloseCover);
    let cover = container(iced::widget::opaque(container(img).center(side))).center(Length::Fill);
    stack![back, cover].into()
}

pub fn dialog_overlay<'a>(app: &'a App, dialog: &'a Dialog) -> Element<'a, Message> {
    // As GPUI's `playlists::dialog_overlay`: a 12 px label over a 40 px field.
    let field = |label: &'static str, input: Element<'a, Message>| -> Element<'a, Message> {
        column![text(label).size(theme::TYPE_LABEL).style(widgets::faded_text(theme::text_secondary())), input].spacing(6).into()
    };
    let input = |placeholder: &'static str, value: &'a str, on: fn(String) -> Message| {
        text_input(placeholder, value)
            .on_input(on)
            .on_submit(Message::DialogSubmit)
            .size(theme::TYPE_BODY)
            .padding([11, 12])
            .style(dialog_field_style)
    };
    // Texts in the dialog follow its entrance fade (see `wash::enter`).
    let note =
        |s: String| -> Element<'a, Message> { text(s).size(theme::TYPE_BODY).style(widgets::faded_text(theme::text_secondary())).into() };
    let (heading, body, confirm): (String, Element<'a, Message>, String) = match dialog {
        Dialog::NewPlaylist { .. } | Dialog::EditPlaylist { .. } => {
            let new = matches!(dialog, Dialog::NewPlaylist { .. });
            let title = input(crate::i18n::t("my_playlist"), &app.dlg_title, Message::DialogTitle).id(DIALOG_TITLE_ID);
            let desc = input(crate::i18n::t("description_optional"), &app.dlg_desc, Message::DialogDesc);
            let desc_label = if new { crate::i18n::t("description_optional") } else { crate::i18n::t("description") };
            let fields = column![field(crate::i18n::t("name"), title.into()), field(desc_label, desc.into())].spacing(14);
            let fields = match dialog {
                Dialog::NewPlaylist { add } if !add.is_empty() => {
                    let count = if add.len() == 1 {
                        crate::i18n::t("one_track_will_be_added").to_string()
                    } else {
                        crate::i18n::text("tracks_will_be_added", &[add.len().to_string()])
                    };
                    fields.push(text(count).size(13).style(widgets::faded_text(theme::text_secondary())))
                }
                _ => fields,
            };
            (
                if new { crate::i18n::t("playlist.create").into() } else { crate::i18n::t("edit_playlist").into() },
                fields.into(),
                if new { crate::i18n::t("create").into() } else { crate::i18n::t("save").into() },
            )
        }
        Dialog::DeletePlaylist { title, .. } => (
            crate::i18n::t("delete_playlist").into(),
            note(crate::i18n::text("delete_this_cannot_be_undone", std::slice::from_ref(title))),
            crate::i18n::t("delete").into(),
        ),
        Dialog::SignOut => (
            crate::i18n::t("sign_out").into(),
            note(crate::i18n::text("account.signout_explanation", &[service()])),
            crate::i18n::t("sign_out").into(),
        ),
        Dialog::ConfirmRemove(k) => {
            let (title, what, button) = match k {
                RemoveKind::PlaylistTrack { title, .. } => (
                    crate::i18n::t("remove_from_playlist"),
                    crate::i18n::text("playlist.confirm_remove", &[title.clone(), service()]),
                    crate::i18n::t("remove"),
                ),
                RemoveKind::FavTrack(t) => (
                    crate::i18n::t("remove_from_your_collection"),
                    crate::i18n::text("remove_from_your_favorite_tracks_in", &[t.display_title(), service()]),
                    crate::i18n::t("remove"),
                ),
                RemoveKind::FavAlbum(a) => (
                    crate::i18n::t("remove_from_your_collection"),
                    crate::i18n::text("remove_from_your_albums_in", &[a.title.clone(), service()]),
                    crate::i18n::t("remove"),
                ),
                RemoveKind::Unfollow(_, name) => {
                    (crate::i18n::t("unfollow"), crate::i18n::text("unfollow_in", &[name.clone(), service()]), crate::i18n::t("unfollow"))
                }
            };
            (title.into(), note(what), button.into())
        }
    };
    let rule = container(Space::new().width(Length::Fill).height(1))
        .style(|_| container::Style { background: Some(super::wash::faded(theme::divider()).into()), ..Default::default() });
    let footer = column![
        rule,
        row![
            Space::new().width(Length::Fill),
            pill(crate::i18n::t("cancel"), None, false, Some(Message::DialogClose)),
            pill(confirm, None, true, Some(Message::DialogSubmit))
        ]
        .spacing(10)
    ]
    .spacing(12);
    // GPUI: 460 px wide, 20 px padding, 18 px between parts, 16 px corners.
    let panel = container(
        column![text(heading).size(scale::TYPE_HEADING).font(widgets::semibold()).style(widgets::faded_text(theme::text())), body, footer]
            .spacing(scale::SPACE_LG),
    )
    .width(460)
    .padding(20);
    // A pane of glass, as the menus and the info panel.
    let panel = super::liquid_glass::glass(panel, 16.0).optics(super::liquid_glass::Optics::POPOVER).shadow(theme::popover_shadow());
    // GPUI: the dialog fades in over `PANEL` (`motion::enter`); the backdrop does not.
    scrim(super::wash::enter(panel, super::motion::PANEL), Message::DialogClose, 0.70)
}

/// A dialog's text field (GPUI: surface fill, a divider edge, 6 px corners).
fn dialog_field_style(_: &iced::Theme, _: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: theme::surface().into(),
        border: Border { color: theme::divider(), width: 1.0, radius: 6.0.into() },
        icon: theme::text_secondary(),
        placeholder: theme::text_dim(),
        value: theme::text(),
        selection: theme::selection(),
    }
}

fn service() -> String {
    crate::i18n::t("tidal").to_string()
}

// ----------------------------------------------------------------- session screens

pub fn login_screen(app: &App) -> Element<'_, Message> {
    // As GPUI's `login_screen`: the mark and the name beside (or over) a
    // glass panel 22 px inside, its parts 16 px apart.
    let wide = app.viewport.width >= 920.0;
    let secondary = |s: &str| text(s.to_string()).size(theme::TYPE_BODY).color(theme::text_secondary());
    let body: Element<'_, Message> = match &app.login {
        super::LoginState::Starting => container(widgets::loading_dots()).width(Length::Fill).height(64).center(Length::Fill).into(),
        super::LoginState::Waiting(code) => column![
            secondary(crate::i18n::t("login.confirm_code")),
            text(code.user_code.clone()).size(40).font(widgets::bold()).color(theme::accent()),
            text(code.verification_uri.clone()).size(13).color(theme::text_dim()),
            pill(crate::i18n::t("reopen_browser"), None, false, Some(Message::OpenUrl(code.verification_uri.clone()))),
            text(crate::i18n::t("waiting_for_confirmation")).size(13).color(theme::text_secondary()),
            pill(crate::i18n::t("cancel"), None, false, Some(Message::CancelLogin)),
        ]
        .spacing(16)
        .align_x(Alignment::Center)
        .into(),
        state => column![
            secondary(crate::i18n::t("sign_in_with_your_tidal_account_to_get_started")),
            pill(crate::i18n::t("sign_in_to_tidal"), Some(Icon::Link), true, Some(Message::StartLogin)),
            pill(crate::i18n::t("library.open_local"), Some(Icon::Track), false, Some(Message::UseLocalOnly)),
        ]
        .push(match state {
            super::LoginState::Failed(e) => Some(text(e.clone()).size(13).color(theme::danger())),
            _ => None,
        })
        .spacing(16)
        .align_x(Alignment::Center)
        .into(),
    };
    let panel = super::liquid_glass::glass(container(body).padding(22), super::liquid_glass::PANEL_R).shadow(theme::popover_shadow());
    let mark = iced::widget::image(super::images::logo()).width(if wide { 74 } else { 46 }).height(if wide { 74 } else { 46 });
    let names = column![
        text(crate::i18n::t("app.name")).size(36).font(widgets::bold()).color(theme::text()),
        text(crate::i18n::t("unofficial_client_for_tidal")).size(11).color(theme::text_secondary()),
    ]
    .align_x(if wide { Alignment::Center } else { Alignment::Start });
    let brand: Element<'_, Message> = if wide {
        column![mark, names].spacing(12).align_x(Alignment::Center).into()
    } else {
        row![mark, names].spacing(12).align_y(Alignment::Center).into()
    };
    let layout: Element<'_, Message> = if wide {
        row![brand, panel].spacing(32).align_y(Alignment::Center).into()
    } else {
        column![brand, panel].spacing(18).align_x(Alignment::Center).into()
    };
    container(layout)
        .center(Length::Fill)
        .padding([0, 20])
        .style(|_| container::Style { background: Some(theme::bg().into()), text_color: Some(theme::text()), ..Default::default() })
        .into()
}

pub fn welcome(app: &App) -> Element<'_, Message> {
    // As GPUI's `onboarding::render`: a 760 px panel (40 px inside when wide),
    // a four-part progress line, the step, a 28 px title over 16 px text, the
    // step's content and the buttons under a rule.
    let step = app.welcome_step.unwrap_or(0).min(3);
    let wide = app.viewport.width >= 850.0;
    let title_key = ["welcome.title", "welcome.service", "welcome.language", "welcome.library"][step];
    let desc_key = ["welcome.intro", "welcome.service_help", "interface_language", "welcome.library_help"][step];
    let lead = |s: String, lh: f32| text(s).size(16).line_height(text::LineHeight::Absolute(lh.into())).color(theme::text_secondary());
    // A choice card: 16 px inside, 12 px corners, a 2 px edge (accent when chosen).
    let choice = |label: &'static str, selected: bool, msg: Message| -> Element<'_, Message> {
        // The hover and the choice glide in as GPUI's `interact` (see `wash`).
        let b = button(text(label).size(theme::TYPE_BODY).font(widgets::semibold()))
            .padding(16)
            .style(|_, _| {
                let p = super::wash::phase();
                button::Style {
                    background: Some(theme::ink(0.04 + 0.06 * p.hover).into()),
                    text_color: theme::text(),
                    border: Border {
                        color: super::wash::mix(theme::divider(), theme::accent(), p.active),
                        width: 2.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }
            })
            .on_press(msg);
        super::wash::track(b).active(selected).into()
    };
    let body: Element<'_, Message> = match step {
        0 => column![
            iced::widget::image(super::images::logo()).width(100).height(100),
            lead(crate::i18n::t("about.unofficial_notice").into(), 25.0)
        ]
        .spacing(18)
        .into(),
        1 => {
            let local = app.settings.lock().preferred_service == "local";
            let choices = row![
                choice(crate::i18n::t("welcome.tidal"), !local, Message::ChooseService("tidal")),
                choice(crate::i18n::t("welcome.local"), local, Message::ChooseService("local")),
            ]
            .spacing(12)
            .wrap();
            // The account risk, shown only while TIDAL is the chosen service.
            let risk = (!local).then(|| {
                container(
                    column![
                        text(crate::i18n::t("welcome.risk_title")).size(theme::TYPE_BODY).font(widgets::semibold()).color(theme::text()),
                        text(crate::i18n::t("welcome.risk_body"))
                            .size(14)
                            .line_height(text::LineHeight::Absolute(22.0.into()))
                            .color(theme::text_secondary()),
                    ]
                    .spacing(8),
                )
                .width(Length::Fill)
                .padding(16)
                .style(|_| container::Style {
                    background: Some(theme::ink(0.04).into()),
                    border: Border { color: theme::divider(), width: 1.0, radius: 12.0.into() },
                    ..Default::default()
                })
            });
            column![choices].push(risk).spacing(18).into()
        }
        2 => {
            let selected = app.settings.lock().language.clone();
            let options = std::iter::once((None, crate::i18n::t("system")))
                .chain(crate::i18n::LANGUAGES.iter().map(|(code, name)| (Some(*code), *name)))
                .map(|(code, name)| choice(name, selected.as_deref() == code, Message::SetLanguage(code.map(str::to_string))));
            row(options).spacing(10).wrap().vertical_spacing(10).into()
        }
        _ => column![
            row![
                pill(crate::i18n::t("add_music_folder"), Some(Icon::Upload), true, Some(Message::ChooseLocal(true))),
                pill(crate::i18n::t("add_audio_files"), Some(Icon::Track), false, Some(Message::ChooseLocal(false)))
            ]
            .spacing(12)
            .wrap(),
            text(if app.local_busy {
                crate::i18n::t("adding_to_your_library").to_string()
            } else {
                crate::i18n::text("welcome.imported", &[app.local_tracks.len().to_string()])
            })
            .size(14)
            .color(theme::text_secondary()),
        ]
        .spacing(18)
        .into(),
    };
    let progress = row((0..4).map(|i| {
        container(Space::new().width(Length::Fill).height(4))
            .width(Length::FillPortion(1))
            .style(move |_| container::Style {
                background: Some(if i <= step { theme::accent() } else { theme::divider() }.into()),
                ..Default::default()
            })
            .into()
    }))
    .spacing(8);
    let rule = container(Space::new().width(Length::Fill).height(1))
        .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() });
    let buttons = row![pill(crate::i18n::t("welcome.skip"), None, false, Some(Message::WelcomeSkip)), Space::new().width(Length::Fill)]
        .push((step > 0).then(|| pill(crate::i18n::t("previous"), Some(Icon::Back), false, Some(Message::WelcomeBack))))
        .push(pill(
            if step == 3 { crate::i18n::t("welcome.finish") } else { crate::i18n::t("next") },
            Some(Icon::Forward),
            true,
            Some(Message::WelcomeNext),
        ))
        .spacing(12)
        .align_y(Alignment::Center);
    let panel = container(
        scrollable(
            column![
                progress,
                text(crate::i18n::text("welcome.step", &[(step + 1).to_string()])).size(theme::TYPE_LABEL).color(theme::text_secondary()),
                column![
                    text(crate::i18n::t(title_key)).size(28).font(widgets::bold()).color(theme::text()),
                    lead(crate::i18n::t(desc_key).into(), 24.0),
                    body
                ]
                .spacing(12),
                column![rule, buttons].spacing(20),
            ]
            .spacing(24)
            .padding(if wide { 40 } else { 24 }),
        )
        .style(scroll_style),
    )
    .max_width(760)
    .style(|_| container::Style {
        background: Some(theme::popover().into()),
        border: Border { color: theme::popover_border(), width: 1.0, radius: 20.0.into() },
        shadow: theme::popover_shadow(),
        ..Default::default()
    });
    container(panel)
        .center(Length::Fill)
        .padding(if wide { 40 } else { 16 })
        .style(|_| container::Style { background: Some(theme::frame().into()), text_color: Some(theme::text()), ..Default::default() })
        .into()
}

#[cfg(test)]
mod library_layout_tests {
    use super::*;
    #[test]
    fn navigation_fade_finishes_and_content_arriving_fades_in_too() {
        let started = std::time::Instant::now();
        let fade = PageFade { started, ready: true };
        assert_eq!(fade.opacity(started), 1.0);
        let mid = fade.opacity(started + PageFade::DURATION / 2);
        assert!((mid - 0.0625).abs() < 0.001);
        assert_eq!(fade.opacity(started + PageFade::DURATION), 0.0);
        // A page that was loading: its content fades in when it arrives...
        let later = started + PageFade::DURATION * 4;
        let mut arrival = Arrival::default();
        arrival.see(false, started);
        arrival.see(true, later);
        assert_eq!(fade.opacity_with(&arrival, later), 1.0);
        assert_eq!(fade.opacity_with(&arrival, later + PageFade::DURATION), 0.0);
        // ...but a page that was ready from the start has nothing to fade.
        let mut cached = Arrival::default();
        cached.see(true, started);
        assert_eq!(fade.opacity_with(&cached, later), 0.0);
    }
    use iced::advanced::{
        layout,
        renderer::Headless,
        widget::{self, Operation, Tree},
        Layout,
    };

    #[test]
    #[ignore = "requires a wgpu adapter; renders offscreen"]
    fn library_search_has_room_at_all_supported_widths() {
        struct InputBounds(Option<iced::Rectangle>);
        impl Operation for InputBounds {
            fn traverse(&mut self, children: &mut dyn FnMut(&mut dyn Operation)) {
                children(self);
            }

            fn text_input(&mut self, _: Option<&widget::Id>, bounds: iced::Rectangle, _: &mut dyn widget::operation::TextInput) {
                self.0 = Some(bounds);
            }
        }

        let renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(super::super::INTER, iced::Pixels(16.0), None))
            .expect("wgpu adapter for the offscreen layout test");
        for width in [168., 260., 366., 524., 640., 760., 820., 1480.] {
            let mut bar = library_controls("albums", width, true, super::super::library::SortKey::Added, "");
            let mut tree = Tree::new(bar.as_widget());
            let node =
                bar.as_widget_mut().layout(&mut tree, &renderer, &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, 1000.)));
            let mut input = InputBounds(None);
            bar.as_widget_mut().operate(&mut tree, Layout::new(&node), &renderer, &mut input);
            let bounds = input.0.expect("Library search field");
            // The 110 px box holds the glyph (13 px, 7 px from the text) and 10 px on each side.
            assert!(bounds.width >= 110.0 - 40.0, "width {width}: {bounds:?}");
            assert!(bounds.x >= 0.0 && bounds.x + bounds.width <= width + 0.01, "width {width}: {bounds:?}");
            assert!(bounds.y + bounds.height <= node.size().height + 0.01);
        }
    }
}

#[cfg(test)]
#[path = "shots.rs"]
mod shots;

#[cfg(test)]
#[path = "panel_shots.rs"]
mod panel_shots;
