//! Track lists: the columns shared by a list's header and its rows, the rows
//! themselves, and dragging to reorder the user's own playlists. Same columns
//! and breakpoints as the GPUI reference (`src/ui/widgets.rs`).

use std::sync::Arc;

use iced::widget::{button, column, container, mouse_area, row, text, Space};
use iced::{alignment, Alignment, Border, Element, Length, Task};

use super::icons::Icon;
use super::library::SortKey;
use super::widgets::{self, cover, explicit_badge, fmt_date, fmt_time, icon_box, line};
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
}

impl App {
    pub(super) fn row_update(&mut self, msg: RowMsg) -> Task<Message> {
        match msg {
            RowMsg::Grab(uuid, from) => self.row_drag = Some(RowDrag { uuid, from, over: None }),
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

/// The column names over a list; in the Library they change its order.
pub fn header<'a>(app: &App, style: RowStyle) -> Element<'a, Message> {
    let current = style.sortable.map(|section| app.sort_of(section));
    let label = |name: &'static str, key: Option<SortKey>| -> Element<'a, Message> {
        let active = key.is_some() && current.map(|(k, _)| k) == key;
        let arrow = match (active, current) {
            (true, Some((_, true))) => " ▼",
            (true, Some((_, false))) => " ▲",
            _ => "",
        };
        let color = if active { theme::accent() } else { theme::text_dim() };
        let shown = line(format!("{name}{arrow}"), 10.5, color)
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
    let mut r = row![].spacing(style.gap()).padding([0.0, PAD]).height(HEADER_H - 3.0).align_y(Alignment::Center);
    if !style.tiny {
        let hash = text("#").size(9.5).font(widgets::bold()).color(theme::text_dim());
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
    let bar = container(r).width(Length::Fill).style(|_| container::Style {
        background: Some(theme::ink(0.035).into()),
        border: Border { color: theme::panel_border(), width: 1.0, radius: 8.0.into() },
        ..Default::default()
    });
    container(bar).height(HEADER_H).into()
}

/// A track line. Double click (or the play button in place of its number)
/// plays from this row, middle click queues it, right click opens its menu;
/// in the user's own playlists it can be dragged to another place.
pub fn track_row<'a>(app: &App, ix: usize, track: &Track, style: RowStyle, source: &Source, menu: MenuTarget) -> Element<'a, Message> {
    let playing = app.now.as_ref().map(|n| n.track.id) == Some(track.id);
    let fade = |c: iced::Color| if track.streamable { c } else { theme::with_alpha(c, 0.4) };
    let title_color = fade(if playing { theme::accent() } else { theme::text() });
    let secondary = fade(theme::text_secondary());
    // As GPUI: the artist and album columns and the numbers on the right are
    // semibold in the text colour; under the title (compact) the artist is quiet.
    let strong = fade(theme::text());
    let play = Message::PlayFrom(source.clone(), ix);
    let link = |label: String, page: Option<Page>, column: bool| -> Element<'a, Message> {
        let shown = if column { line(label, 11.5, strong).font(widgets::semibold()) } else { line(label, 11.5, secondary) };
        match page {
            // GPUI `nav_link`: a link rests a little softer and turns full text,
            // underlined, while hovered (fading with the pointer).
            Some(page) => {
                let rest = if column { theme::lerp(theme::text(), theme::text_secondary(), 0.35) } else { theme::text_secondary() };
                let rest = fade(rest);
                let full = fade(theme::text());
                let shown = shown.live(move |_| theme::lerp(rest, full, super::wash::phase().hover)).underline_on_hover();
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
            text(n.to_string()).size(11).color(theme::text_dim()).into()
        };
        r = r.push(container(number).center(NUM));
        let glyph = if playing && app.is_playing() { Icon::Pause } else { Icon::Play };
        let hover = container(icon_box(glyph, 15.0, NUM, theme::text(), play.clone())).center(NUM).style(move |_| container::Style {
            background: Some(if playing { theme::active() } else { theme::hover() }.into()),
            border: Border { radius: 6.0.into(), ..Default::default() },
            ..Default::default()
        });
        hover_play = Some(hover);
    }
    // The badge keeps its 14 px (and the 6 before it) when the title is cut.
    let name = line(track.display_title(), 13.0, title_color).font(widgets::semibold());
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
        r = r.push(cell(line(track.added.as_deref().map(fmt_date).unwrap_or_default(), 11.5, strong).font(widgets::semibold()), DATE));
    }
    if !style.tiny {
        r = r.push(cell(line(fmt_time(track.duration as f64), 11.5, strong).font(widgets::semibold()), TIME));
    }
    if style.bpm {
        r = r.push(cell(line(track.bpm.map_or_else(|| "-".to_string(), |b| b.to_string()), 11.5, strong).font(widgets::semibold()), BPM));
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
    let (edge, width) = if dropping {
        (theme::accent(), 2.0)
    } else if playing {
        (theme::with_alpha(theme::accent(), 0.35), 1.0)
    } else {
        (theme::divider(), 1.0)
    };
    let boxed = container(r).width(Length::Fill).style(move |_| container::Style {
        background: playing.then(|| theme::accent_dim().into()),
        border: Border { color: edge, width, radius: 7.0.into() },
        ..Default::default()
    });
    // As GPUI: the hover fades in (with an accent strip growing on the left)
    // and pressing deepens it; "more" shows over the row while hovered.
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
    let washed = super::wash::wash(iced::widget::hover(boxed, row_actions), theme::with_alpha(theme::hover(), 0.8), 7.0)
        .press(theme::active())
        .strip(theme::accent());
    let mut area = mouse_area(container(washed).padding([2, 0]))
        .on_double_click(play)
        .on_middle_press(Message::AddToQueue(track.clone(), false))
        .on_right_press(Message::OpenMenu(menu));
    if let Some(uuid) = own {
        area = area.on_press(Message::Row(RowMsg::Grab(uuid, ix)));
        if app.row_drag.is_some() {
            area = area.on_enter(Message::Row(RowMsg::Enter(ix))).on_exit(Message::Row(RowMsg::Leave(ix)));
        }
    }
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
