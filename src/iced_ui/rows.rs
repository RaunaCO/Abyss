//! Track lists: the columns shared by a list's header and its rows, the rows
//! themselves, and dragging to reorder the user's own playlists. Same columns
//! and breakpoints as the GPUI reference (`src/ui/widgets.rs`).

use std::sync::Arc;

use iced::widget::{button, column, container, mouse_area, row, text, Space};
use iced::{alignment, Alignment, Border, Element, Length, Task};

use super::icons::Icon;
use super::library::SortKey;
use super::widgets::{self, cover, explicit_badge, fmt_date, fmt_time, icon_box, line, scale};
use super::{theme, App, MenuTarget, Message};
use crate::shared::{Load, Page};
use crate::tidal::Track;

const PAD: f32 = 12.0;
const COVER: f32 = 36.0;
const COVER_GAP: f32 = 10.0;
const NUM: f32 = 28.0;
const DATE: f32 = 94.0;
const TIME: f32 = 50.0;
const BPM: f32 = 36.0;
const RATING: f32 = 84.0;
/// Height of the column header with its margin.
pub const HEADER_H: f32 = 39.0;
/// Height of a track row with the 2 px above and below it.
pub const ROW_PITCH: f32 = theme::ROW_H + 4.0;

/// Where the rows of a list come from. A page's list is only copied when it
/// is played, not every time the page is drawn.
#[derive(Debug, Clone)]
pub enum Source {
    Page,
    List(Arc<Vec<Track>>),
}

/// The optional columns of a list.
#[derive(Debug, Clone, Copy)]
pub struct RowStyle {
    pub cover: bool,
    pub album: bool,
    pub number: bool,
    pub date: bool,
    pub bpm: bool,
    pub rating: bool,
    pub(super) compact: bool,
    pub(super) tiny: bool,
    /// The Library section whose order the column headers control.
    pub sortable: Option<&'static str>,
}

impl RowStyle {
    pub const fn new(cover: bool, album: bool, number: bool) -> Self {
        Self { cover, album, number, date: false, bpm: false, rating: true, compact: false, tiny: false, sortable: None }
    }

    /// Drop optional columns (BPM, then date added, then rating) when the list
    /// is `width` design px wide, so the title always keeps readable room.
    pub fn fit(mut self, width: f32) -> Self {
        self.bpm &= width >= 1110.0;
        self.date &= width >= 1030.0;
        self.rating &= width >= 840.0;
        self.album &= width >= 700.0;
        self.compact = width < 510.0;
        self.tiny = width < 360.0;
        self
    }

    fn gap(self) -> f32 {
        if self.compact {
            8.0
        } else {
            12.0
        }
    }

    fn actions_width(self) -> f32 {
        if self.tiny {
            28.0
        } else {
            84.0
        }
    }

    /// Shares of the flexible space: title, artist, album.
    fn portions(self) -> (u16, u16, u16) {
        if self.album {
            (40, 18, 18)
        } else {
            (48, 26, 0)
        }
    }
}

/// A row of one of the user's playlists being dragged to another place.
#[derive(Debug, Clone)]
pub struct RowDrag {
    uuid: String,
    from: usize,
    pub over: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum RowMsg {
    Grab(String, usize),
    Enter(usize),
    Leave(usize),
    Release,
    /// A click on row `.0` (track id `.1`) of the page's list selects it.
    Select(usize, u64),
    /// A right click: select the row, then open its menu.
    Menu(usize, u64, MenuTarget),
}

/// The selected row of the page's track list: the one clicked last (or
/// moved to with the arrow keys), drawn with a neutral ring so it never reads
/// as the playing row (accent tint, accent title, moving bars).
#[derive(Debug, Clone, PartialEq)]
struct Selected {
    page: Page,
    ix: usize,
    id: u64,
}

thread_local! {
    // TODO(state): belongs in `App` (requested from rendimiento in
    // team/requests.md); a thread-local until then. Only the UI thread
    // builds views and handles messages, so it is never shared.
    static SELECTED: std::cell::RefCell<Option<Selected>> = const { std::cell::RefCell::new(None) };
    /// Where the page's track list starts (page px) and its display order, as
    /// last drawn: the arrow keys move along what is shown and scroll to it.
    static LIST: std::cell::RefCell<(f32, Vec<usize>)> = const { std::cell::RefCell::new((0.0, Vec::new())) };
}

/// The index (into the page's tracks) of the selected row, if it is on `page`.
fn selected_on(page: &Page) -> Option<(usize, u64)> {
    SELECTED.with(|s| s.borrow().as_ref().filter(|s| &s.page == page).map(|s| (s.ix, s.id)))
}

fn select(page: Page, ix: usize, id: u64) {
    SELECTED.with(|s| *s.borrow_mut() = Some(Selected { page, ix, id }));
}

/// Note where the list drawn now starts and which rows it shows, in order
/// (`None`: all of them), for [`App::row_key`].
pub(super) fn note_list(y0: f32, count: usize, shown: Option<&[usize]>) {
    LIST.with(|l| {
        let mut l = l.borrow_mut();
        l.0 = y0;
        l.1.clear();
        match shown {
            Some(shown) => l.1.extend_from_slice(shown),
            None => l.1.extend(0..count),
        }
    });
}

impl App {
    /// Keyboard over the page's track list: Up / Down move the selection
    /// (Home / End to the ends), Enter plays from it, Escape clears it. `None`
    /// when the key is not for the list (no selection on this page, a
    /// modifier held, a field or a floating surface has the keyboard).
    // Called from `on_key_inner` once rendimiento wires it (team/requests.md).
    #[allow(dead_code)]
    pub(super) fn row_key(&mut self, key: &iced::keyboard::Key, mods: iced::keyboard::Modifiers) -> Option<Task<Message>> {
        use iced::keyboard::{key::Named, Key};
        if mods.command() || mods.alt() || self.menu.is_some() || self.dialog.is_some() || self.search_open || self.now_playing_open {
            return None;
        }
        let (ix, _) = selected_on(&self.page)?;
        let Key::Named(named) = key else { return None };
        let (y0, order) = LIST.with(|l| l.borrow().clone());
        let pos = order.iter().position(|&i| i == ix);
        let to = match named {
            Named::Escape => {
                SELECTED.with(|s| *s.borrow_mut() = None);
                return Some(Task::none());
            }
            Named::Enter => return Some(Task::done(Message::PlayFrom(Source::Page, ix))),
            Named::ArrowDown => pos.map_or(0, |p| (p + 1).min(order.len().saturating_sub(1))),
            Named::ArrowUp => pos.map_or(0, |p| p.saturating_sub(1)),
            Named::Home => 0,
            Named::End => order.len().saturating_sub(1),
            _ => return None,
        };
        let Some(&next) = order.get(to) else { return Some(Task::none()) };
        let id = self.page_tracks().get(next).map_or(0, |t| t.id);
        select(self.page.clone(), next, id);
        // Keep the row in view, with a row of context above or below it.
        let top = y0 + to as f32 * ROW_PITCH;
        let view = self.page_view_h.max(1.0);
        let target = if top - ROW_PITCH < self.page_scroll {
            Some((top - ROW_PITCH).max(0.0))
        } else if top + 2.0 * ROW_PITCH > self.page_scroll + view {
            Some(top + 2.0 * ROW_PITCH - view)
        } else {
            None
        };
        Some(match target {
            Some(y) => iced::widget::operation::scroll_to(
                super::pages::PAGE_SCROLL,
                iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: y + super::pages::PAGE_TOP },
            ),
            None => Task::none(),
        })
    }

    pub(super) fn row_update(&mut self, msg: RowMsg) -> Task<Message> {
        match msg {
            RowMsg::Select(ix, id) => select(self.page.clone(), ix, id),
            RowMsg::Menu(ix, id, menu) => {
                select(self.page.clone(), ix, id);
                return Task::done(Message::OpenMenu(menu));
            }
            RowMsg::Grab(uuid, from) => {
                let id = self.page_tracks().get(from).map_or(0, |t| t.id);
                select(self.page.clone(), from, id);
                self.row_drag = Some(RowDrag { uuid, from, over: None });
            }
            RowMsg::Enter(i) => {
                if let Some(drag) = &mut self.row_drag {
                    drag.over = Some(i);
                }
            }
            RowMsg::Leave(i) => {
                if let Some(drag) = self.row_drag.as_mut().filter(|d| d.over == Some(i)) {
                    drag.over = None;
                }
            }
            RowMsg::Release => {
                if let Some(RowDrag { uuid, from, over: Some(to) }) = self.row_drag.take() {
                    if self.page == Page::Playlist(uuid.clone()) {
                        return self.move_playlist_track(uuid, from, to);
                    }
                }
            }
        }
        Task::none()
    }

    fn move_playlist_track(&mut self, uuid: String, from: usize, to: usize) -> Task<Message> {
        if from == to {
            return Task::none();
        }
        // Optimistic: move it now, reload the list if TIDAL refuses.
        if let Some(Load::Ready((_, list))) = self.playlists.get_mut(&uuid) {
            if from < list.len() && to < list.len() {
                let track = list.remove(from);
                list.insert(to, track);
            }
        }
        let u = uuid.clone();
        self.fetch(
            move |t| t.move_in_playlist(&u, from, to),
            move |app, r| match r {
                Ok(()) => Task::none(),
                Err(e) => {
                    app.playlists.remove(&uuid);
                    app.notify_toast(crate::i18n::text("could_not_move", &[format!("{e:#}")]));
                    app.load_page_data()
                }
            },
        )
    }

    pub(super) fn source_tracks(&self, source: &Source) -> Vec<Track> {
        match source {
            Source::Page => self.page_tracks(),
            Source::List(list) => list.as_ref().clone(),
        }
    }
}

fn cell<'a>(content: impl Into<Element<'a, Message>>, width: impl Into<Length>) -> Element<'a, Message> {
    container(content).width(width).clip(true).into()
}

//// Colours of a track row's states (art-audit.md §4): playing is the accent
/// (title, moving bars, a faint accent tint), selected is neutral (a soft fill
/// and a ring), so both read at once on the same row and neither looks like
/// the hover wash.
// TODO(theme): row state tokens; move to theme.rs with arte's.
mod tone {
    use super::theme;
    use iced::Color;

    pub fn playing_fill() -> Color {
        theme::with_alpha(theme::accent(), if theme::is_light() { 0.11 } else { 0.10 })
    }
    pub fn selected_fill() -> Color {
        theme::ink(if theme::is_light() { 0.05 } else { 0.06 })
    }
    pub fn selected_ring() -> Color {
        theme::ink(if theme::is_light() { 0.34 } else { 0.30 })
    }
    pub fn hover() -> Color {
        theme::ink(if theme::is_light() { 0.05 } else { 0.06 })
    }
    pub fn press() -> Color {
        theme::ink(0.04)
    }
    /// The strip that grows on the left with the hover: neutral, since the
    /// accent marks the playing row.
    pub fn strip() -> Color {
        theme::ink(0.45)
    }
}

/// The column names over a list; in the Library they change its order. No
/// box: small dim captions over one hairline, so the rows carry the table.
pub fn header<'a>(app: &App, style: RowStyle) -> Element<'a, Message> {
    let current = style.sortable.map(|section| app.sort_of(section));
    let label = |name: &'static str, key: Option<SortKey>| -> Element<'a, Message> {
        let active = key.is_some() && current.map(|(k, _)| k) == key;
        let arrow = match (active, current) {
            (true, Some((_, true))) => " ▼",
            (true, Some((_, false))) => " ▲",
            _ => "",
        };
        // The order's column in the text colour (the accent means "playing").
        let color = if active { theme::text() } else { theme::text_dim() };
        let shown = line(format!("{name}{arrow}"), scale::TYPE_CAPTION, color)
            .font(widgets::semibold())
            // Room above the cap height for accented capitals (TÍTULO, ÁLBUM).
            .line_height(text::LineHeight::Relative(1.6));
        match (style.sortable, key) {
            (Some(section), Some(key)) => {
                let shown = shown.live(|c| theme::lerp(c, theme::text(), super::wash::phase().hover));
                super::wash::track(
                    button(shown).padding(0).style(widgets::still(0.0)).on_press(Message::SetSort(section, key.code().to_string())),
                )
                .into()
            }
            _ => shown.into(),
        }
    };
    let (title_w, artist_w, album_w) = style.portions();
    let mut r = row![].spacing(style.gap()).padding([0.0, PAD]).height(HEADER_H - 1.0 - scale::SPACE_XS).align_y(Alignment::Center);
    if !style.tiny {
        let hash = text("#").size(scale::TYPE_CAPTION).font(widgets::semibold()).color(theme::text_dim());
        r = r.push(container(hash).width(NUM).align_x(alignment::Horizontal::Center));
    }
    let indent = if style.cover { COVER + COVER_GAP } else { 0.0 };
    let title = label(if style.compact { crate::i18n::t("track.column") } else { crate::i18n::t("title") }, Some(SortKey::Name));
    r = r.push(cell(container(title).padding(iced::Padding { left: indent, ..Default::default() }), Length::FillPortion(title_w)));
    if !style.compact {
        r = r.push(cell(label(crate::i18n::t("artist.column"), Some(SortKey::Artist)), Length::FillPortion(artist_w)));
    }
    if style.album {
        r = r.push(cell(label(crate::i18n::t("album.column"), Some(SortKey::Album)), Length::FillPortion(album_w)));
    }
    if style.rating {
        r = r.push(cell(label(crate::i18n::t("rating"), Some(SortKey::Rating)), RATING));
    }
    if style.date {
        r = r.push(cell(label(crate::i18n::t("added"), Some(SortKey::Added)), DATE));
    }
    if !style.tiny {
        r = r.push(cell(label(crate::i18n::t("time"), Some(SortKey::Duration)), TIME));
    }
    if style.bpm {
        r = r.push(cell(label(crate::i18n::t("bpm"), None), BPM));
    }
    r = r.push(Space::new().width(style.actions_width()));
    let rule = container(Space::new().width(Length::Fill).height(1))
        .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() });
    column![r, rule, Space::new().height(scale::SPACE_XS)].height(HEADER_H).into()
}

/// A track line. A click selects it, a double click (or the play button in
/// place of its number) plays from it, middle click queues it, right click
/// selects it and opens its menu; in the user's own playlists it can be
/// dragged to another place.
pub fn track_row<'a>(app: &App, ix: usize, track: &Track, style: RowStyle, source: &Source, menu: MenuTarget) -> Element<'a, Message> {
    let playing = app.now.as_ref().map(|n| n.track.id) == Some(track.id);
    // Selected: clicked last on this page, or the row whose menu is open.
    let menu_open = app.menu.as_ref().is_some_and(|(_, m)| match m {
        MenuTarget::Track(t) | MenuTarget::PlaylistTrack { track: t, .. } => t.id == track.id,
        _ => false,
    });
    let selected = menu_open || selected_on(&app.page) == Some((ix, track.id));
    let fade = |c: iced::Color| if track.streamable { c } else { theme::with_alpha(c, 0.4) };
    let title_color = fade(if playing { theme::accent() } else { theme::text() });
    let secondary = fade(theme::text_secondary());
    let quiet = fade(theme::text_dim());
    let play = Message::PlayFrom(source.clone(), ix);
    let link = |label: String, page: Option<Page>, column: bool| -> Element<'a, Message> {
        let size = if column { scale::TYPE_LABEL } else { scale::TYPE_CAPTION };
        let shown = line(label, size, secondary);
        match page {
            // GPUI `nav_link`: a link rests quiet and turns full text,
            // underlined, while hovered (fading with the pointer).
            Some(page) => {
                let full = fade(theme::text());
                let shown = shown.live(move |_| theme::lerp(secondary, full, super::wash::phase().hover)).underline_on_hover();
                mouse_area(super::wash::track(shown))
                    .on_press(Message::Navigate(page))
                    .interaction(iced::mouse::Interaction::Pointer)
                    .into()
            }
            None => shown.into(),
        }
    };
    let artist_page = track.artists.first().filter(|a| a.id != 0).map(|a| Page::Artist(a.id));
    let album_page = track.album.as_ref().filter(|a| a.id != 0).map(|a| Page::Album(a.id));
    let (title_w, artist_w, album_w) = style.portions();

    let mut r = row![].spacing(style.gap()).padding([0.0, PAD]).height(theme::ROW_H).align_y(Alignment::Center);
    let mut hover_play = None;
    if !style.tiny {
        let number: Element<'a, Message> = if playing {
            widgets::playing_indicator(app.is_playing(), 14.0)
        } else {
            let n = if style.number && track.track_number > 0 { track.track_number as usize } else { ix + 1 };
            text(n.to_string()).size(scale::TYPE_CAPTION).color(theme::text_dim()).into()
        };
        r = r.push(container(number).center(NUM));
        let glyph = if playing && app.is_playing() { Icon::Pause } else { Icon::Play };
        let hover = container(icon_box(glyph, 15.0, NUM, theme::text(), play.clone())).center(NUM).style(move |_| container::Style {
            background: Some(theme::ink(if playing { 0.12 } else { 0.08 }).into()),
            border: Border { radius: scale::R_SM.into(), ..Default::default() },
            ..Default::default()
        });
        hover_play = Some(hover);
    }
    // The badge keeps its 14 px (and the 6 before it) when the title is cut.
    let weight = if playing { widgets::semibold() } else { widgets::medium() };
    let name = line(track.display_title(), scale::TYPE_SMALL, title_color).font(weight);
    let name = if track.explicit { widgets::leave_room(name, 20.0) } else { name.into() };
    let title = row![name].push(track.explicit.then(explicit_badge)).spacing(6).align_y(Alignment::Center);
    let names = column![title].push(style.compact.then(|| link(track.artist_names(), artist_page.clone(), false))).spacing(2);
    let lead =
        row![].push(style.cover.then(|| cover(track.cover(80), COVER, false))).push(names).spacing(COVER_GAP).align_y(Alignment::Center);
    r = r.push(cell(lead, Length::FillPortion(title_w)));
    if !style.compact {
        r = r.push(cell(link(track.artist_names(), artist_page, true), Length::FillPortion(artist_w)));
    }
    if style.album {
        let album = track.album.as_ref().map(|a| a.title.clone()).unwrap_or_default();
        r = r.push(cell(link(album, album_page, true), Length::FillPortion(album_w)));
    }
    if style.rating {
        r = r.push(cell(widgets::stars(app, track.id, false), RATING));
    }
    if style.date {
        r = r.push(cell(line(track.added.as_deref().map(fmt_date).unwrap_or_default(), scale::TYPE_LABEL, quiet), DATE));
    }
    if !style.tiny {
        r = r.push(cell(line(fmt_time(track.duration as f64), scale::TYPE_LABEL, secondary), TIME));
    }
    if style.bpm {
        r = r.push(cell(line(track.bpm.map_or_else(|| "–".to_string(), |b| b.to_string()), scale::TYPE_LABEL, quiet), BPM));
    }
    let mut actions = row![].spacing(2).align_y(Alignment::Center);
    if !style.tiny && track.local_path.is_none() {
        let fav = app.fav_ids.contains(&track.id);
        let (glyph, color) = if fav { (Icon::HeartFilled, theme::text()) } else { (Icon::Heart, theme::text_secondary()) };
        let add = Message::OpenMenu(MenuTarget::AddToPlaylist(vec![track.clone()]));
        // GPUI: the heart pops after a like (`heart_scale`, paint only).
        let pop = app.heart_pop.filter(|(id, _)| *id == track.id).map(|(_, at)| at);
        actions = actions.push(icon_box(Icon::Plus, 15.0, 26.0, theme::text_secondary(), add)).push(widgets::glyph_box(
            widgets::soft_glyph(glyph, 15.0, color).pop(pop),
            26.0,
            Message::ToggleFavorite(track.clone()),
        ));
    }
    let more = icon_box(Icon::More, 15.0, 26.0, theme::text_secondary(), Message::OpenMenu(menu.clone()));
    // GPUI shows "more" only while the pointer is over the row (always for
    // local files and tiny rows, which have no other action).
    let hover_more = !style.tiny && track.local_path.is_none();
    let mut more = Some(more);
    actions = if hover_more { actions.push(Space::new().width(26).height(26)) } else { actions.push(more.take()) };
    r = r.push(container(actions).width(style.actions_width()).align_x(alignment::Horizontal::Right));

    let own = match &menu {
        MenuTarget::PlaylistTrack { uuid, .. } => Some(uuid.clone()),
        _ => None,
    };
    let dropping = own.is_some() && app.row_drag.as_ref().is_some_and(|d| d.over == Some(ix) && d.from != ix);
    // No box at rest: the row sits flat on the page. Playing tints it with the
    // accent; the selection (gliding in with the wash's `active`) adds a soft
    // fill and a neutral ring; a drop target shows the accent edge.
    let boxed = container(r).width(Length::Fill).style(move |_| {
        let sel = super::wash::phase().active;
        let fill = match (playing, sel > 0.0) {
            (true, true) => Some(theme::lerp(tone::playing_fill(), theme::with_alpha(theme::accent(), 0.16), sel)),
            (true, false) => Some(tone::playing_fill()),
            (false, true) => Some(theme::with_alpha(tone::selected_fill(), tone::selected_fill().a * sel)),
            (false, false) => None,
        };
        let border = if dropping {
            Border { color: theme::accent(), width: 2.0, radius: scale::R_MD.into() }
        } else {
            let ring = tone::selected_ring();
            Border { color: theme::with_alpha(ring, ring.a * sel), width: if sel > 0.0 { 1.0 } else { 0.0 }, radius: scale::R_MD.into() }
        };
        container::Style { background: fill.map(Into::into), border, ..Default::default() }
    });
    // As GPUI: the hover fades in (with a strip growing on the left) and
    // pressing deepens it; "more" shows over the row while hovered.
    let row_actions = container(
        row![]
            .push(hover_play)
            .push(Space::new().width(Length::Fill))
            .push(more)
            .align_y(Alignment::Center)
            .height(Length::Fill)
            .padding([0.0, PAD]),
    )
    .width(Length::Fill)
    .height(Length::Fill);
    let washed = super::wash::wash(iced::widget::hover(boxed, row_actions), tone::hover(), scale::R_MD)
        .press(tone::press())
        .strip(tone::strip())
        .active(selected);
    let area = mouse_area(container(washed).padding([2, 0]))
        .on_double_click(play)
        .on_middle_press(Message::AddToQueue(track.clone(), false))
        .on_right_press(Message::Row(RowMsg::Menu(ix, track.id, menu)));
    let area = match own {
        Some(uuid) => {
            let area = area.on_press(Message::Row(RowMsg::Grab(uuid, ix)));
            if app.row_drag.is_some() {
                area.on_enter(Message::Row(RowMsg::Enter(ix))).on_exit(Message::Row(RowMsg::Leave(ix)))
            } else {
                area
            }
        }
        None => area.on_press(Message::Row(RowMsg::Select(ix, track.id))),
    };
    area.into()
}

#[cfg(test)]
mod tests {
    use super::RowStyle;

    #[test]
    fn optional_columns_go_in_order_as_the_list_narrows() {
        let full = RowStyle { date: true, bpm: true, ..RowStyle::new(true, true, false) };
        let shown = |width: f32| {
            let s = full.fit(width);
            (s.bpm, s.date, s.rating, s.album, s.compact, s.tiny)
        };
        assert_eq!(shown(1200.0), (true, true, true, true, false, false));
        assert_eq!(shown(1100.0), (false, true, true, true, false, false));
        assert_eq!(shown(900.0), (false, false, true, true, false, false));
        assert_eq!(shown(750.0), (false, false, false, true, false, false));
        assert_eq!(shown(500.0), (false, false, false, false, true, false));
        assert_eq!(shown(300.0), (false, false, false, false, true, true));
    }
}
