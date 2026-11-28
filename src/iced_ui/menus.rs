//! Right-click menus: tracks, queue entries, albums, artists, sort options,
//! sound outputs and the application menu. Shown at the pointer, kept inside
//! the window, closed by a click anywhere else or Esc.

use std::sync::Arc;

use iced::widget::{button, column, container, mouse_area, pin, row, scrollable, stack, text, Space};
use iced::{Alignment, Color, Element, Length, Point};

use super::icons::{icon, Icon};
use super::info::InfoTarget;
use super::library::sort_options;
use super::widgets::{self, cover, line};
use super::{theme, App, Dialog, MenuTarget, Message, RemoveKind};
use crate::shared::{Load, Page};
use crate::tidal::{share_url, Item};

/// GPUI's `ctxmenu::W` (the output menu is 360 px wide).
const MENU_W: f32 = 286.0;
/// GPUI's plain menus (sort, queue entries, playlists): 240 px, 14 px text.
const PLAIN_W: f32 = 240.0;

/// A menu entry and its height (design px), so the menu knows how tall it
/// will be before it is laid out and can be placed to fit the window.
struct Entry(Element<'static, Message>, f32);

/// Height of the menu header: 12 + a 44 px picture + 10.
const HEADER_H: f32 = 66.0;
/// Height of an option row (rich menus).
const OPTION_H: f32 = 34.0;
/// Height of a plain entry: a 14 px line (1.3) and 8 px above and below.
const PLAIN_H: f32 = 14.0 * 1.3 + 16.0;
/// Height of a separator: 5 px, the rule, 5 px.
const SEP_H: f32 = 11.0;

/// The item a menu is about: its picture (5 px corners), title and detail.
fn header(art: Element<'static, Message>, title: String, sub: String) -> Entry {
    let names = column![line(title, 13.5, theme::text()).font(widgets::semibold())]
        .push((!sub.is_empty()).then(|| line(sub, 11.5, theme::text_secondary())))
        .spacing(2);
    let r = row![art, container(names).width(Length::Fill).clip(true)].spacing(12).align_y(Alignment::Center).padding(iced::Padding {
        top: 12.0,
        bottom: 10.0,
        left: 12.0,
        right: 12.0,
    });
    Entry(container(r).height(HEADER_H).align_y(iced::alignment::Vertical::Center).into(), HEADER_H)
}

fn sep() -> Entry {
    let rule = container(
        container(Space::new().width(Length::Fill).height(1))
            .style(|_| container::Style { background: Some(super::wash::faded(theme::divider()).into()), ..Default::default() }),
    )
    .padding([5, 10]);
    Entry(rule.into(), SEP_H)
}

/// A vertical option (GPUI's `ctxmenu::option`): 34 px, 6 px in from the
/// edges, a 15 px glyph 10 px from 13 px text. Its wash fades in under the
/// pointer (ink 7 %, 5 % more pressed; the danger colour for Remove).
fn option(i: Icon, label: impl Into<String>, msg: Message, danger: bool) -> Entry {
    let color = if danger { theme::danger() } else { theme::text() };
    let body = row![
        icon(i, 15.0, if danger { color } else { theme::text_secondary() }),
        container(line(label.into(), 13.0, color)).width(Length::Fill).clip(true)
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let radius = theme::concentric(12.0, 7.0);
    let b = button(container(body).height(OPTION_H).align_y(iced::alignment::Vertical::Center))
        .width(Length::Fill)
        .padding([0, 10])
        .style(widgets::still(radius))
        .on_press(msg);
    let tint = if danger { theme::danger() } else { theme::ink(1.0) };
    let washed = super::wash::wash(b, Color { a: tint.a * if danger { 0.12 } else { 0.07 }, ..tint }, radius)
        .press(Color { a: tint.a * if danger { 0.06 } else { 0.05 }, ..tint });
    Entry(container(washed).padding([0, 6]).into(), OPTION_H)
}

/// An entry of a plain menu (GPUI's list menu): 14 px text, 14 x 8 padding.
fn plain(label: impl Into<String>, msg: Message) -> Entry {
    marked(None, label, msg)
}

/// A plain entry with a mark column before its label (the current order's
/// arrow in the sort menu): every label starts at the same place, marked or not.
fn marked(mark: Option<Icon>, label: impl Into<String>, msg: Message) -> Entry {
    let mark = container(mark.map(|i| icon(i, 13.0, theme::accent()))).width(16).center_y(PLAIN_H - 16.0);
    let body = row![mark, line(label.into(), theme::TYPE_BODY, theme::text())].spacing(6).align_y(Alignment::Center);
    let b = button(container(body).height(PLAIN_H - 16.0).align_y(iced::alignment::Vertical::Center))
        .width(Length::Fill)
        .padding([8, 10])
        .style(widgets::still(theme::concentric(12.0, 6.0)))
        .on_press(msg);
    // The wash fades in like the rich menus' options (ink 7 %, 5 % more
    // pressed), inset like them so it never meets the menu's rounded edge.
    let washed = super::wash::wash(b, theme::ink(0.07), theme::concentric(12.0, 6.0)).press(theme::ink(0.05));
    Entry(container(washed).padding([0, 6]).into(), PLAIN_H)
}

/// A small title over a menu's list.
fn caption(label: &'static str) -> Entry {
    let c = container(text(label).size(12).font(widgets::semibold()).style(widgets::faded_text(theme::text_secondary())))
        .padding([6, 16])
        .height(28);
    Entry(c.into(), 28.0)
}

/// A device in the output menu: speaker icon, name, a trailing check when chosen.
fn device_option(label: String, selected: bool, msg: Message) -> Entry {
    let tone = if selected { theme::accent() } else { theme::text_secondary() };
    let body = row![icon(Icon::VolumeUp, 15.0, tone), container(line(label, 13.0, theme::text())).width(Length::Fill).clip(true)]
        .push(selected.then(|| icon(Icon::Check, 15.0, theme::accent())))
        .spacing(10)
        .align_y(Alignment::Center);
    let radius = theme::concentric(12.0, 7.0);
    let b = button(container(body).height(OPTION_H).align_y(iced::alignment::Vertical::Center))
        .width(Length::Fill)
        .padding([0, 10])
        .style(widgets::still(radius))
        .on_press(msg);
    let rest = if selected { theme::ink(0.06) } else { Color::TRANSPARENT };
    Entry(container(super::wash::wash(b, theme::ink(0.07), radius).rest(rest).press(theme::ink(0.05))).padding([0, 6]).into(), OPTION_H)
}

fn go_to(rows: &mut Vec<Entry>, t: &crate::tidal::Track) {
    if let Some(a) = t.album.as_ref().filter(|a| a.id != 0) {
        rows.push(option(Icon::Album, crate::i18n::t("view_album"), Message::Navigate(Page::Album(a.id)), false));
    }
    if let Some(a) = t.artists.first().filter(|a| a.id != 0) {
        rows.push(option(Icon::Artist, crate::i18n::t("view_artist"), Message::Navigate(Page::Artist(a.id)), false));
    }
}

fn entries(app: &App, target: &MenuTarget) -> Vec<Entry> {
    let mut rows: Vec<Entry> = Vec::new();
    match target {
        MenuTarget::Track(t) | MenuTarget::PlaylistTrack { track: t, .. } => {
            let local = t.local_path.is_some();
            let sub = if local { crate::i18n::t("local_file").to_string() } else { t.artist_names() };
            rows.push(header(widgets::cover_rounded(t.cover(160), 44.0, 5.0), t.display_title(), sub));
            rows.push(sep());
            // A local file's menu starts with where it lives, as in GPUI.
            if local {
                go_to(&mut rows, t);
            }
            rows.push(option(Icon::Play, crate::i18n::t("play"), Message::PlayTracks(Arc::new(vec![t.clone()]), 0), false));
            rows.push(option(Icon::Queue, crate::i18n::t("play_next"), Message::AddToQueue(t.clone(), true), false));
            rows.push(option(Icon::Queue, crate::i18n::t("add_to_queue"), Message::AddToQueue(t.clone(), false), false));
            if !local {
                rows.push(option(Icon::Radio, crate::i18n::t("start_radio"), Message::StartRadio(t.clone()), false));
                let fav = app.fav_ids.contains(&t.id);
                rows.push(option(
                    if fav { Icon::HeartFilled } else { Icon::Heart },
                    if fav { crate::i18n::t("remove_from_your_collection") } else { crate::i18n::t("add_to_favorites") },
                    if fav {
                        Message::OpenDialog(Dialog::ConfirmRemove(RemoveKind::FavTrack(t.clone())))
                    } else {
                        Message::ToggleFavorite(t.clone())
                    },
                    fav,
                ));
                rows.push(option(
                    Icon::Plus,
                    crate::i18n::t("add_to_playlist"),
                    Message::OpenMenu(MenuTarget::AddToPlaylist(vec![t.clone()])),
                    false,
                ));
            }
            rows.push(sep());
            if !local {
                go_to(&mut rows, t);
            }
            if !local {
                rows.push(option(Icon::Track, crate::i18n::t("song_info"), Message::OpenInfo(InfoTarget::Song(t.clone())), false));
                rows.push(option(Icon::Link, crate::i18n::t("share"), Message::CopyLink(share_url("track", &t.id.to_string())), false));
            } else {
                rows.push(option(
                    Icon::Edit,
                    crate::i18n::t("local.edit_metadata"),
                    Message::Local(super::local::LocalMsg::Edit(t.id)),
                    false,
                ));
            }
            if let MenuTarget::PlaylistTrack { uuid, index, .. } = target {
                rows.push(option(
                    Icon::Close,
                    crate::i18n::t("remove_from_playlist"),
                    Message::OpenDialog(Dialog::ConfirmRemove(RemoveKind::PlaylistTrack {
                        uuid: uuid.clone(),
                        index: *index,
                        title: t.display_title(),
                    })),
                    true,
                ));
            }
        }
        MenuTarget::QueueItem(uid) => {
            if let Some(q) = app.queue.iter().find(|q| q.uid == *uid) {
                let t = &q.track;
                rows.push(header(widgets::cover_rounded(t.cover(160), 44.0, 5.0), t.display_title(), t.artist_names()));
                rows.push(sep());
                rows.push(option(Icon::Play, crate::i18n::t("play"), Message::PlayQueued(*uid), false));
                rows.push(option(Icon::Close, crate::i18n::t("remove_from_queue"), Message::RemoveQueued(*uid), true));
                rows.push(option(Icon::Close, crate::i18n::t("clear_queue"), Message::ClearQueue, true));
                if let Some(a) = t.album.as_ref().filter(|a| a.id != 0) {
                    rows.push(option(Icon::Album, crate::i18n::t("view_album"), Message::Navigate(Page::Album(a.id)), false));
                }
            }
        }
        MenuTarget::Album(a) => {
            let local = crate::local::is_local_id(a.id);
            rows.push(header(widgets::cover_rounded(a.cover(160), 48.0, 5.0), a.title.clone(), a.artist_names()));
            rows.push(sep());
            rows.push(option(Icon::Play, crate::i18n::t("play"), Message::PlayItem(Item::Album(a.clone())), false));
            if let Some((_, tracks)) = app.albums.get(&a.id).and_then(Load::ready) {
                rows.push(option(Icon::Shuffle, crate::i18n::t("shuffle"), Message::PlayShuffled(Arc::new(tracks.clone())), false));
                if local && !tracks.is_empty() {
                    rows.push(option(Icon::Queue, crate::i18n::t("play_next"), Message::QueueAlbum(a.id, true), false));
                    rows.push(option(Icon::Queue, crate::i18n::t("add_to_queue"), Message::QueueAlbum(a.id, false), false));
                }
            }
            rows.push(option(Icon::Album, crate::i18n::t("album_info"), Message::OpenInfo(InfoTarget::Album(a.clone())), false));
            if !local {
                rows.push(option(
                    Icon::Plus,
                    crate::i18n::t("add_to_playlist"),
                    Message::OpenMenu(MenuTarget::AddAlbumToPlaylist(a.id)),
                    false,
                ));
            }
            if !local {
                let fav = app.fav_album_ids.contains(&a.id);
                rows.push(option(
                    if fav { Icon::HeartFilled } else { Icon::Heart },
                    if fav { crate::i18n::t("remove_from_your_collection") } else { crate::i18n::t("add_to_favorites") },
                    if fav {
                        Message::OpenDialog(Dialog::ConfirmRemove(RemoveKind::FavAlbum(a.clone())))
                    } else {
                        Message::ToggleFavoriteAlbum(a.clone())
                    },
                    fav,
                ));
            }
            rows.push(option(Icon::Album, crate::i18n::t("view_album"), Message::Navigate(Page::Album(a.id)), false));
            if let Some(ar) = a.artists.first().filter(|x| x.id != 0) {
                rows.push(option(Icon::Artist, crate::i18n::t("view_artist"), Message::Navigate(Page::Artist(ar.id)), false));
            }
            if local {
                rows.push(option(Icon::Upload, crate::i18n::t("local_files"), Message::Local(super::local::LocalMsg::OpenSettings), false));
                rows.push(option(Icon::Close, crate::i18n::t("remove_from_library"), Message::RemoveLocalAlbum(a.id), true));
            } else {
                rows.push(option(Icon::Link, crate::i18n::t("share"), Message::CopyLink(share_url("album", &a.id.to_string())), false));
            }
        }
        MenuTarget::Artist(a) => {
            let local = crate::local::is_local_id(a.id);
            rows.push(header(cover(a.picture(160), 44.0, true), a.name.clone(), crate::i18n::t("artist").into()));
            rows.push(sep());
            rows.push(option(Icon::Play, crate::i18n::t("play"), Message::PlayItem(Item::Artist(a.clone())), false));
            rows.push(option(Icon::Artist, crate::i18n::t("view_artist"), Message::Navigate(Page::Artist(a.id)), false));
            rows.push(option(Icon::Lyrics, crate::i18n::t("artist_info"), Message::OpenInfo(InfoTarget::Artist(a.clone())), false));
            if local {
                rows.push(option(Icon::Upload, crate::i18n::t("local_files"), Message::Local(super::local::LocalMsg::OpenSettings), false));
            } else {
                let follows = app.fav_artist_ids.contains(&a.id);
                rows.push(option(
                    if follows { Icon::HeartFilled } else { Icon::Heart },
                    if follows { crate::i18n::t("unfollow") } else { crate::i18n::t("follow") },
                    if follows {
                        Message::OpenDialog(Dialog::ConfirmRemove(RemoveKind::Unfollow(a.id, a.name.clone())))
                    } else {
                        Message::ToggleFavoriteArtist(a.clone())
                    },
                    follows,
                ));
                rows.push(option(Icon::Link, crate::i18n::t("share"), Message::CopyLink(share_url("artist", &a.id.to_string())), false));
            }
        }
        MenuTarget::AddToPlaylist(tracks) => {
            // GPUI's plain list: "New playlist" first, then the user's playlists.
            let ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
            rows.push(plain(crate::i18n::t("playlist.create_with_icon"), Message::OpenDialog(Dialog::NewPlaylist { add: ids })));
            for p in app.own_playlists().into_iter().take(40) {
                rows.push(plain(p.title.clone(), Message::AddToPlaylist(p.uuid.clone(), tracks.clone())));
            }
        }
        MenuTarget::AddAlbumToPlaylist(id) => {
            rows.push(caption(crate::i18n::t("add_to_playlist")));
            let lists = app.own_playlists();
            if lists.is_empty() {
                rows.push(Entry(
                    container(
                        text(crate::i18n::t("you_have_no_playlists_of_your_own")).size(12).style(widgets::faded_text(theme::text_dim())),
                    )
                    .padding([8, 16])
                    .into(),
                    12.0 * 1.3 + 16.0,
                ));
            }
            for playlist in lists {
                rows.push(option(Icon::Playlist, playlist.title.clone(), Message::AlbumToPlaylist(*id, playlist.uuid.clone()), false));
            }
        }
        MenuTarget::Sort(section) => {
            let (cur, desc) = app.sort_of(section);
            // GPUI's plain list, the current order marked before its name.
            for &k in sort_options(section) {
                let mark = (k == cur).then_some(if desc { Icon::ChevDown } else { Icon::ChevUp });
                rows.push(marked(mark, k.label(), Message::SetSort(section, k.code().to_string())));
            }
        }
        MenuTarget::Output => {
            // GPUI `ctxmenu::output_menu`: a header with what is playing on now,
            // then the devices with a speaker icon and a check on the chosen one.
            // No exclusive-mode entry there: it lives in Settings (an entry with a
            // "+" when off and a check when on read as inverted here).
            let s = app.settings.lock().clone();
            let playing_on =
                if app.snap.output.is_empty() { crate::i18n::t("system_default").to_string() } else { app.snap.output.to_string() };
            let art = container(icon(Icon::VolumeUp, 17.0, theme::accent())).center(36).style(|_| container::Style {
                background: Some(super::wash::faded(theme::ink(0.07)).into()),
                border: iced::Border { radius: 10.0.into(), ..Default::default() },
                ..Default::default()
            });
            rows.push(header(art.into(), crate::i18n::t("output.menu_title").to_string(), playing_on));
            rows.push(sep());
            let devices = std::iter::once(None).chain(app.devices.iter().cloned().map(Some));
            for device in devices {
                let selected = device == s.output_device;
                let label = device.clone().unwrap_or_else(|| crate::i18n::t("system_default").to_string());
                rows.push(device_option(label, selected, Message::SetOutput(device, s.exclusive_mode, s.force_volume)));
            }
        }
    }
    rows
}

pub const SORT_ANCHOR: &str = "abyss-sort-button";

#[derive(Default)]
struct SortAnchor {
    bounds: Option<iced::Rectangle>,
    scroll: iced::Vector,
}

impl iced::advanced::widget::Operation<iced::Rectangle> for SortAnchor {
    fn traverse(&mut self, children: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<iced::Rectangle>)) {
        if self.bounds.is_none() {
            children(self);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::advanced::widget::Id>,
        _: iced::Rectangle,
        _: iced::Rectangle,
        translation: iced::Vector,
        _: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&iced::advanced::widget::Id::new(super::pages::PAGE_SCROLL)) {
            self.scroll = translation;
        }
    }

    fn container(&mut self, id: Option<&iced::advanced::widget::Id>, bounds: iced::Rectangle) {
        if id == Some(&iced::advanced::widget::Id::new(SORT_ANCHOR)) {
            self.bounds = Some(bounds - self.scroll);
        }
    }

    fn finish(&self) -> iced::advanced::widget::operation::Outcome<iced::Rectangle> {
        use iced::advanced::widget::operation::Outcome;
        self.bounds.map_or(Outcome::None, Outcome::Some)
    }
}

pub(super) fn open_sort_at_button(section: &'static str) -> iced::Task<Message> {
    iced::advanced::widget::operate(SortAnchor::default()).map(move |bounds| Message::OpenSortMenu(section, bounds))
}

pub(super) fn sort_menu_origin(bounds: iced::Rectangle, window_height: f32, count: usize) -> Point {
    let height = menu_height(count, window_height);
    let below = bounds.y + bounds.height + 4.0;
    let y = if below + height + 12.0 <= window_height { below } else { bounds.y - height - 4.0 };
    Point::new(bounds.x, y)
}

fn menu_height(count: usize, window_height: f32) -> f32 {
    (count as f32 * 34.0 + 16.0).min(window_height - 24.0)
}

pub fn overlay<'a>(app: &'a App, at: Point, target: &'a MenuTarget) -> Element<'a, Message> {
    menu(app, at, target, None)
}

/// The menu that a click elsewhere just closed at `closed`, fading out over
/// [`super::motion::EXIT`]: a picture only (the click already went through).
pub fn ghost<'a>(app: &'a App, at: Point, target: &'a MenuTarget, closed: std::time::Instant) -> Element<'a, Message> {
    menu(app, at, target, Some(closed))
}

fn menu<'a>(app: &'a App, at: Point, target: &'a MenuTarget, closed: Option<std::time::Instant>) -> Element<'a, Message> {
    let entries = entries(app, target);
    // GPUI: the rich menus are 286 px (output 360) with 6 px under the last
    // option; the plain ones 240 px with 6 px above and below.
    let plain = matches!(target, MenuTarget::Sort(_) | MenuTarget::AddToPlaylist(_));
    let width = match target {
        MenuTarget::Output => 360.0,
        _ if plain => PLAIN_W,
        _ => MENU_W,
    };
    let x = at.x.min(app.viewport.width - width - 12.0).max(8.0);
    let top = if plain || matches!(target, MenuTarget::AddAlbumToPlaylist(_) | MenuTarget::Output) { 6.0 } else { 0.0 };
    // Its real height: the entries, the padding and the 1 px edges.
    let est_h = entries.iter().map(|e| e.1).sum::<f32>() + top + 6.0 + 2.0;
    let rows: Vec<Element<'a, Message>> = entries.into_iter().map(|e| e.0).collect();
    let place = place_menu(at.y, est_h, app.viewport.height, bar_top(app));
    let panel = container(scrollable(column(rows)).style(super::pages::scroll_style))
        .width(width)
        .max_height(place.max_height)
        .padding(iced::Padding { top, bottom: 6.0, ..Default::default() })
        .style(|_| container::Style { text_color: Some(theme::text()), ..Default::default() });
    let panel = super::liquid_glass::glass(panel, 12.0).optics(super::liquid_glass::Optics::POPOVER).shadow(theme::popover_shadow());
    let panel: Element<'a, Message> = match closed {
        // Leaving: faster than it came, and it no longer takes the pointer.
        Some(at) => super::wash::leave(panel, at, super::motion::EXIT),
        // A pane of liquid glass that fades in as GPUI's menus (`motion::enter`)
        // and grows from the corner at the pointer (a menu comes from where it
        // was asked for), over `CONTROL`: a small popover's 125-200 ms.
        None => iced::widget::opaque(super::wash::enter_from(panel, super::motion::CONTROL, pivot(at.x, x, width, place.anchor))),
    };
    let placed = match place.anchor {
        Anchor::Top(y) => pin(panel).x(x).y(y),
        // Grows upward from `bottom`: its real height, not the estimate, decides where its top lands.
        Anchor::Bottom(bottom) => pin(container(panel).height(bottom).align_y(iced::alignment::Vertical::Bottom)).x(x).y(0.0),
    };
    if closed.is_some() {
        return placed.into();
    }
    let back =
        mouse_area(Space::new().width(Length::Fill).height(Length::Fill)).on_press(Message::CloseMenu).on_right_press(Message::CloseMenu);
    stack![back, placed].width(Length::Fill).height(Length::Fill).into()
}

/// The corner of a menu at the pointer (`at_x`), for a menu `width` wide put
/// at `x`: it hangs from its top when it opens downward, rises from its
/// bottom when it opens upward.
fn pivot(at_x: f32, x: f32, width: f32, anchor: Anchor) -> super::wash::Pivot {
    let fx = if width > 0.0 { (at_x - x) / width } else { 0.0 };
    super::wash::Pivot::new(fx, if matches!(anchor, Anchor::Bottom(_)) { 1.0 } else { 0.0 })
}

/// The top of the player bar, which menus never cover (the window's bottom in
/// the full-screen player, which has no bar).
fn bar_top(app: &App) -> f32 {
    if app.now_playing_open {
        app.viewport.height
    } else {
        app.viewport.height - theme::FRAME_GAP - theme::PLAYER_H
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Anchor {
    /// The menu's top edge, going down.
    Top(f32),
    /// The menu's bottom edge, going up.
    Bottom(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Placement {
    anchor: Anchor,
    max_height: f32,
}

/// Where a menu opened at height `y` (estimated `est_h` tall) goes: below the
/// pointer when it fits above the player bar, else above it (from the bar's
/// top when opened on the bar, as the volume's output menu), else wherever
/// there is more room, scrolling inside.
fn place_menu(y: f32, est_h: f32, window_h: f32, bar_top: f32) -> Placement {
    const MARGIN: f32 = 8.0;
    let floor = (bar_top - 6.0).min(window_h - 12.0);
    let on_bar = y > floor;
    let below = floor - y;
    let up_from = if on_bar { floor } else { y };
    let above = up_from - MARGIN;
    if !on_bar && est_h <= below {
        Placement { anchor: Anchor::Top(y.max(MARGIN)), max_height: below }
    } else if est_h <= above {
        Placement { anchor: Anchor::Bottom(up_from), max_height: above }
    } else if est_h <= floor - MARGIN {
        // Fits neither side of the pointer but fits the window: slide it up
        // until it rests on the floor, whole, instead of scrolling a half.
        Placement { anchor: Anchor::Bottom(floor), max_height: floor - MARGIN }
    } else {
        // Taller than the window: as tall as it can be, scrolling inside.
        Placement { anchor: Anchor::Top(MARGIN), max_height: (floor - MARGIN).max(40.0) }
    }
}

#[cfg(test)]
mod placement_tests {
    use super::*;

    #[test]
    fn menus_open_below_the_pointer_when_they_fit() {
        let p = place_menu(100.0, 300.0, 900.0, 826.0);
        assert_eq!(p.anchor, Anchor::Top(100.0));
    }

    #[test]
    fn menus_near_the_bar_open_upward_instead_of_covering_it() {
        let p = place_menu(700.0, 300.0, 900.0, 826.0);
        assert_eq!(p.anchor, Anchor::Bottom(700.0));
    }

    #[test]
    fn menus_grow_from_the_corner_at_the_pointer() {
        let down = pivot(700.0, 700.0, 286.0, Anchor::Top(380.0));
        assert_eq!((down.x, down.y), (0.0, 0.0));
        // Pushed left by the window's edge: it still grows from the pointer.
        let pushed = pivot(1400.0, 1114.0, 286.0, Anchor::Bottom(820.0));
        assert_eq!((pushed.x, pushed.y), (1.0, 1.0));
    }

    #[test]
    fn a_menu_opened_on_the_bar_rises_from_its_top() {
        // Right click on the volume icon, inside the player bar.
        let p = place_menu(860.0, 200.0, 900.0, 826.0);
        assert_eq!(p.anchor, Anchor::Bottom(820.0));
        assert!(p.max_height <= 812.0);
    }
}
