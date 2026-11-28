//! Home's composition (featured item beside the recent albums, a collection
//! in two columns, then TIDAL's rows) and the quick-search menu.

use iced::widget::{button, column, container, mouse_area, row, scrollable, text, Space};
use iced::{alignment, Alignment, Border, Element, Length};

use super::icons::{icon, Icon};
use super::rows::Source;
use super::widgets::{self, cover, item_text, line, pill};
use super::{theme, App, MenuTarget, Message};
use crate::shared::{Load, Page};
use crate::tidal::{Item, Row, RowKind};

/// Most entries of the recent list beside the featured item.
const RECENT: usize = 6;

/// Home's parts, taken from the feed and the albums played here.
#[derive(Debug, Default, PartialEq)]
struct Composition {
    lead: Option<(String, Item)>,
    recent: Vec<Item>,
    collection: Option<(String, Vec<Item>)>,
    rows: Vec<Row>,
}

fn compose(feed: &[Row], mut recent: Vec<Item>) -> Composition {
    let mut rows: Vec<Row> = feed.iter().filter(|r| r.kind != RowKind::Shortcuts && !r.items.is_empty()).cloned().collect();
    if let Some(i) = rows.iter().position(|r| r.kind == RowKind::Featured) {
        let featured = rows.remove(i);
        rows.insert(0, featured);
    }
    let lead = rows.iter_mut().find(|r| matches!(r.kind, RowKind::Featured | RowKind::Cards)).map(|r| (r.title.clone(), r.items.remove(0)));
    rows.retain(|r| !r.items.is_empty());
    if let Some(shortcuts) = feed.iter().find(|r| r.kind == RowKind::Shortcuts) {
        for item in &shortcuts.items {
            let shown = matches!(item, Item::Album(a) if recent.iter().any(|x| matches!(x, Item::Album(b) if b.id == a.id)));
            if !shown && recent.len() < RECENT {
                recent.push(item.clone());
            }
        }
    }
    let mut collection = None;
    if let Some(shelf) = rows.iter_mut().find(|r| r.kind == RowKind::Cards && r.items.len() >= 4) {
        let count = shelf.items.len().min(6);
        collection = Some((shelf.title.clone(), shelf.items.drain(..count).collect()));
        if !shelf.items.is_empty() {
            shelf.title = crate::i18n::text("continued", std::slice::from_ref(&shelf.title));
        }
    }
    rows.retain(|r| !r.items.is_empty());
    Composition { lead, recent, collection, rows }
}

/// The featured item's artwork (the lead of [`compose`], without composing):
/// the scene behind Home's glass.
pub fn lead_art(feed: &[Row]) -> Option<String> {
    let rows = || feed.iter().filter(|r| r.kind != RowKind::Shortcuts && !r.items.is_empty());
    let row = rows().find(|r| r.kind == RowKind::Featured).or_else(|| rows().find(|r| r.kind == RowKind::Cards))?;
    widgets::item_text(&row.items[0]).2
}

pub fn view<'a>(app: &'a App, feed: &[Row], width: f32) -> Element<'a, Message> {
    let played = app.recents.iter().take(RECENT).map(|a| Item::Album(a.to_album())).collect();
    let home = compose(feed, played);
    let wide = width >= 940.0;
    let side_w = if wide { (width * 0.29).clamp(280.0, 380.0) } else { width };
    let lead_w = if wide { width - side_w - 20.0 } else { width };
    // GPUI pads the featured panel by the column's width, not the panel's.
    let featured = featured(home.lead, lead_w, if width < 520.0 { 14.0 } else { 20.0 });
    let listening = listening(home.recent, side_w, wide);
    let top: Element<'a, Message> =
        if wide { row![featured, listening].spacing(20).into() } else { column![featured, listening].spacing(20).into() };
    // GPUI's `home_lead` ends 24 px above the next block; each block below
    // carries its own space.
    let mut page = column![container(top).padding(iced::Padding { bottom: 24.0, ..Default::default() })];
    if let Some((title, items)) = home.collection {
        page = page.push(collection(title, items, width));
    }
    page.extend(super::pages::feed_rows(&home.rows, width, true)).into()
}

fn featured<'a>(lead: Option<(String, Item)>, width: f32, pad: f32) -> Element<'a, Message> {
    let inner = (width - 2.0 * pad - 2.0).max(1.0);
    // Lit from inside by the featured artwork's colours, as the unfolded album.
    let mut glow: Option<[iced::Color; 3]> = None;
    let body: Element<'a, Message> = match lead {
        Some((section, item)) => {
            let (title, subtitle, url, _) = item_text(&item);
            glow = url.as_deref().and_then(|u| super::images::palette(u).try_into().ok());
            // As GPUI: square art beside the text when there is room; a
            // promotion keeps its wide picture across the panel.
            let square = !matches!(item, Item::Promo(_));
            let beside = square && inner >= 360.0;
            let art_w = if square { (inner * if beside { 0.56 } else { 1.0 }).min(300.0) } else { inner };
            let art_h = if square { art_w } else { (art_w * 0.53).clamp(132.0, 292.0) };
            let (label, glyph) = match &item {
                Item::Album(_) => (crate::i18n::t("view_album"), Icon::Album),
                Item::Artist(_) => (crate::i18n::t("view_artist"), Icon::Artist),
                Item::Playlist(_) => (crate::i18n::t("view_playlist"), Icon::Playlist),
                Item::Mix(_) => (crate::i18n::t("open_mix"), Icon::Mix),
                Item::Track(_) => (crate::i18n::t("play"), Icon::Play),
                _ => (crate::i18n::t("open"), Icon::Forward),
            };
            let open = Message::OpenItem(item);
            let picture: Element<'a, Message> = match url.as_deref().and_then(|u| super::images::sized(u, art_w, art_h)) {
                Some(h) => {
                    iced::widget::image(h).width(art_w).height(art_h).content_fit(iced::ContentFit::Cover).border_radius(12.0).into()
                }
                None => container(icon(Icon::Album, 42.0, theme::text_dim())).width(art_w).height(art_h).center(art_w).into(),
            };
            let picture = container(picture).width(art_w).height(art_h).style(|_| container::Style {
                background: Some(theme::field().into()),
                border: Border { radius: 12.0.into(), ..Default::default() },
                ..Default::default()
            });
            let tint = container(Space::new().width(art_w).height(art_h)).style(|_| container::Style {
                background: Some(theme::ink(0.06 * super::wash::phase().hover).into()),
                border: Border { radius: 12.0.into(), ..Default::default() },
                ..Default::default()
            });
            let art = mouse_area(super::wash::track(iced::widget::stack![picture, tint]))
                .on_press(open.clone())
                .interaction(iced::mouse::Interaction::Pointer);
            let size = if beside && inner >= 630.0 { 36.0 } else { 27.0 };
            // Two lines at most, as GPUI's `line_clamp(2)`.
            let lines = 2.0 * 1.3 * size;
            let names = column![container(text(title).size(size).font(widgets::bold()).color(theme::text())).max_height(lines).clip(true)]
                .push((!subtitle.is_empty()).then(|| line(subtitle, 13.0, theme::text_secondary())))
                .push(line(section, 11.5, theme::text_secondary()))
                .spacing(3)
                .width(Length::Fill);
            // The card's primary action on prominent glass, tinted by the art.
            let action = super::widgets::prominent_pill(label, Some(glyph), super::liquid_glass::prominent(glow), Some(open));
            if beside {
                row![art, column![names, action].spacing(12).width(Length::Fill)].spacing(24).align_y(Alignment::Center).into()
            } else {
                column![art, row![names, action].spacing(12).align_y(Alignment::Center)].spacing(14).into()
            }
        }
        None => column![
            text(crate::i18n::t("explore_music")).size(23).font(widgets::bold()).color(theme::text()),
            pill(crate::i18n::t("open_explore"), Some(Icon::Explore), true, Some(Message::Navigate(Page::Explore))),
        ]
        .spacing(14)
        .into(),
    };
    super::liquid_glass::glass(container(body).width(width).padding(pad), super::liquid_glass::PANEL_R)
        .album_light(glow)
        .shadow(theme::popover_shadow())
        .into()
}

fn listening<'a>(recent: Vec<Item>, width: f32, wide: bool) -> Element<'a, Message> {
    let head = row![
        icon(Icon::Clock, 16.0, theme::text_secondary()),
        text(crate::i18n::t("recently_played")).size(widgets::scale::TYPE_HEADING).font(widgets::semibold()).color(theme::text()),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    // GPUI: 4 px above, 8 px on the left beside the featured panel, 9 px under the heading.
    let inner = if wide { width - 8.0 } else { width };
    let mut list = column![container(head).padding(iced::Padding { bottom: 9.0, ..Default::default() })].spacing(4).width(inner);
    if recent.is_empty() {
        list = list.push(
            container(text(crate::i18n::t("your_history_will_appear_here")).size(theme::TYPE_LABEL).color(theme::text_secondary()))
                .padding([10, 0]),
        );
    }
    for item in recent {
        list = list.push(home_item(item, inner, false));
    }
    let list = list.push(Space::new().height(3)).push(pill(
        crate::i18n::t("view_history"),
        Some(Icon::Clock),
        false,
        Some(Message::Navigate(Page::Recent)),
    ));
    container(list).width(width).padding(iced::Padding { top: 4.0, left: if wide { 8.0 } else { 0.0 }, ..Default::default() }).into()
}

// A titled collection in two columns, under a rule (GPUI's `home_collection`).
fn collection<'a>(title: String, items: Vec<Item>, width: f32) -> Element<'a, Message> {
    let cols = if width >= 640.0 { 2 } else { 1 };
    let cell = (width - 16.0 * (cols - 1) as f32) / cols as f32;
    let heading = container(text(title).size(widgets::scale::TYPE_HEADING).font(widgets::semibold()).color(theme::text()))
        .padding(iced::Padding { bottom: 8.0, ..Default::default() });
    let mut list = column![heading].spacing(4);
    for pair in items.chunks(cols) {
        list = list.push(row(pair.iter().map(|item| home_item(item.clone(), cell, true))).spacing(16));
    }
    let rule = container(Space::new().width(Length::Fill).height(1))
        .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() });
    column![rule, container(list).padding(iced::Padding { top: 18.0, bottom: 24.0, ..Default::default() })].into()
}

/// An entry of Home's lists, as GPUI's `home_item`: in the recent list a
/// 40 px row with a small cover and a chevron; in a collection a 64 px tile
/// with a faint fill and a play button.
fn home_item<'a>(item: Item, width: f32, collection: bool) -> Element<'a, Message> {
    let (title, subtitle, url, round) = item_text(&item);
    let mut names = column![line(title, 13.0, theme::text()).font(widgets::semibold())].spacing(3);
    if !subtitle.is_empty() {
        names = names.push(line(subtitle, 11.5, theme::text_secondary()));
    }
    let can_play = collection && !matches!(item, Item::Link(_) | Item::Promo(_));
    let hover_play = collection && matches!(item, Item::Album(_));
    let mut body = row![cover(url, if collection { 48.0 } else { 32.0 }, round), container(names).width(Length::Fill).clip(true)]
        .spacing(10)
        .align_y(Alignment::Center)
        .height(if collection { 64 } else { 40 })
        .padding([0, if collection { 8 } else { 4 }]);
    let play = can_play.then(|| {
        let play = button(container(icon(Icon::Play, 14.0, theme::text())).center(32))
            .padding(0)
            .style(widgets::still(16.0))
            .on_press(Message::PlayItem(item.clone()));
        let play = super::wash::wash(play, theme::ink(0.12), 16.0).rest(theme::ink(0.05));
        widgets::tip(play, crate::i18n::t("play"))
    });
    let mut hover_layer = None;
    if hover_play {
        body = body.push(Space::new().width(32).height(32));
        hover_layer = play.map(|play| {
            container(play)
                .width(width)
                .height(64)
                .padding([0, 8])
                .align_x(alignment::Horizontal::Right)
                .align_y(alignment::Vertical::Center)
        });
    } else {
        body = body.push(play);
    }
    if !collection {
        // GPUI: the arrow brightens and grows 8 % with the row's hover.
        body = body.push(
            super::icons::glyph(Icon::Forward, 13.0, theme::text_dim())
                .live(|p, c| super::wash::mix(c, theme::text(), p.hover))
                .scale(|p| 1.0 + 0.08 * p.hover),
        );
    }
    let menu = match &item {
        Item::Album(a) if collection => Some(MenuTarget::Album(a.clone())),
        Item::Artist(a) if collection => Some(MenuTarget::Artist(a.clone())),
        Item::Track(t) if collection => Some(MenuTarget::Track(t.clone())),
        _ => None,
    };
    // No slab at rest (art-audit §4): the hover wash only.
    let rest = 0.0;
    // Hover and press fade like GPUI's (`wash`), over the resting fill.
    let body: Element<'a, Message> = match hover_layer {
        Some(layer) => iced::widget::hover(body, layer),
        None => body.into(),
    };
    let b = button(body)
        .width(width)
        .padding(0)
        .on_press(Message::OpenItem(item))
        .style(|_, _| button::Style { text_color: theme::text(), ..Default::default() });
    let b = super::wash::wash(b, theme::ink(rest + 0.06), widgets::scale::R_MD).rest(theme::ink(rest)).press(theme::ink(0.04));
    match menu {
        Some(menu) => mouse_area(b).on_right_press(Message::OpenMenu(menu)).into(),
        None => b.into(),
    }
}

/// The quick-search menu under the search box, with GPUI's `search_menu`
/// metrics: 54 px rows with a 36 px cover, 12 px headings with a rule between
/// sections, recent searches as pills and "see all" in a bar at the bottom.
pub fn search_overlay(app: &App) -> Option<Element<'_, Message>> {
    if !app.search_open || app.now_playing_open {
        return None;
    }
    let heading = |first: bool, label: &'static str| -> Element<'_, Message> {
        let t = container(text(label).size(widgets::scale::TYPE_CAPTION).font(widgets::semibold()).color(theme::text_dim()))
            .padding(iced::Padding { top: if first { 4.0 } else { 9.0 }, bottom: 6.0, left: 8.0, right: 8.0 });
        if first {
            t.into()
        } else {
            let rule = container(Space::new().width(Length::Fill).height(1))
                .style(|_| container::Style { background: Some(super::wash::faded(theme::divider()).into()), ..Default::default() });
            column![Space::new().height(6), rule, t].into()
        }
    };
    let hint = |label: String, color: iced::Color| -> Element<'_, Message> {
        container(text(label).size(theme::TYPE_LABEL).color(color)).padding(14).into()
    };
    let entry = |url: Option<String>, round: bool, title: String, sub: String, msg: Message| -> Element<'_, Message> {
        let names =
            column![line(title, 12.0, theme::text()).font(widgets::semibold()), line(sub, 10.5, theme::text_secondary())].spacing(2);
        let body = row![cover(url, 36.0, round), container(names).width(Length::Fill).clip(true)].spacing(10).align_y(Alignment::Center);
        widgets::soft(
            button(container(body).height(54).align_y(alignment::Vertical::Center)).width(Length::Fill).padding([0, 10]).on_press(msg),
            9.0,
        )
    };
    let album_sub = |artists: String| crate::i18n::text("album.detail", &[artists]);

    let q = app.search_query.trim();
    let mut list: Vec<Element<'_, Message>> = Vec::new();
    let mut footer = None;
    if q.is_empty() {
        let recent = app.settings.lock().recent_searches.clone();
        if recent.is_empty() {
            list.push(hint(crate::i18n::t("type_to_search_songs_albums_artists_or_playlists").into(), theme::text_dim()));
        } else {
            list.push(heading(true, crate::i18n::t("recent_searches")));
            // As in GPUI, a recent search fills the box; Enter opens all its results.
            let pills =
                recent.into_iter().take(8).map(|past| pill(past.clone(), Some(Icon::Clock), false, Some(Message::SearchInput(past))));
            list.push(
                container(row(pills).spacing(8).wrap().vertical_spacing(8))
                    .padding(iced::Padding { left: 8.0, right: 8.0, bottom: 6.0, top: 0.0 })
                    .into(),
            );
        }
    } else {
        match app.search.get(q) {
            Some(Load::Ready(found)) => {
                if let Some(top) = &found.top {
                    let (title, sub, url, round) = item_text(top);
                    let sub = match top {
                        Item::Album(a) => album_sub(a.artist_names()),
                        Item::Track(t) => crate::i18n::text("song", &[t.artist_names()]),
                        Item::Playlist(_) => crate::i18n::t("playlist").to_string(),
                        _ => sub,
                    };
                    list.push(heading(list.is_empty(), crate::i18n::t("top_result")));
                    list.push(entry(url, round, title, sub, Message::OpenItem(top.clone())));
                }
                if !found.tracks.is_empty() {
                    list.push(heading(list.is_empty(), crate::i18n::t("songs")));
                    let songs = std::sync::Arc::new(found.tracks.iter().take(8).cloned().collect::<Vec<_>>());
                    for (i, track) in songs.iter().enumerate() {
                        let play = Message::PlayFrom(Source::List(songs.clone()), i);
                        list.push(entry(track.cover(160), false, track.display_title(), track.artist_names(), play));
                    }
                }
                if !(found.albums.is_empty() && found.artists.is_empty() && found.playlists.is_empty()) {
                    list.push(heading(list.is_empty(), crate::i18n::t("albums_artists_and_playlists")));
                }
                for album in found.albums.iter().take(5) {
                    let open = Message::OpenItem(Item::Album(album.clone()));
                    list.push(entry(album.cover(160), false, album.title.clone(), album_sub(album.artist_names()), open));
                }
                for artist in found.artists.iter().take(4) {
                    let open = Message::OpenItem(Item::Artist(artist.clone()));
                    list.push(entry(artist.picture(160), true, artist.name.clone(), crate::i18n::t("artist").to_string(), open));
                }
                for playlist in found.playlists.iter().take(3) {
                    let open = Message::OpenItem(Item::Playlist(playlist.clone()));
                    list.push(entry(playlist.cover(160), false, playlist.title.clone(), crate::i18n::t("playlist").to_string(), open));
                }
                if list.is_empty() {
                    list.push(hint(crate::i18n::t("no_results").into(), theme::text_dim()));
                }
            }
            Some(Load::Failed(reason)) => list.push(hint(reason.clone(), theme::danger())),
            _ => list.push(container(widgets::loading_dots()).height(48).center(Length::Fill).into()),
        }
        let short = if q.chars().count() > 26 { format!("{}\u{2026}", q.chars().take(25).collect::<String>()) } else { q.to_string() };
        let all = pill(crate::i18n::text("see_all_results_for", &[short]), Some(Icon::Search), false, Some(Message::SearchSubmit));
        let rule = container(Space::new().width(Length::Fill).height(1))
            .style(|_| container::Style { background: Some(super::wash::faded(theme::divider()).into()), ..Default::default() });
        footer = Some(column![rule, container(all).width(Length::Fill).padding([10, 14]).align_x(alignment::Horizontal::Right)]);
    }
    // The Esc hint, as GPUI's key cap: 22 px, 7 px corners, 10.5 px semibold.
    let esc = container(text(crate::i18n::t("search.close_key")).size(10.5).font(widgets::semibold()).color(theme::text_dim()))
        .height(22)
        .padding([0, 8])
        .align_y(alignment::Vertical::Center)
        .style(|_| container::Style {
            background: Some(super::wash::faded(theme::ink(0.07)).into()),
            border: Border { color: super::wash::faded(theme::glass_rim(0.0)), width: 1.0, radius: 7.0.into() },
            ..Default::default()
        });
    let esc = container(esc)
        .width(Length::Fill)
        .padding(iced::Padding { right: 14.0, top: 8.0, ..Default::default() })
        .align_x(alignment::Horizontal::Right);
    let (field_x, field_w) = super::pages::search_anchor(app.viewport.width);
    // GPUI's panel scales with the window: half of it, 460 to 760 px.
    let width = (app.viewport.width * 0.5).clamp(460.0, 760.0).min(app.viewport.width - 24.0).max(0.0);
    let x =
        (field_x + (field_w - width) / 2.0).clamp(theme::FRAME_GAP, (app.viewport.width - width - theme::FRAME_GAP).max(theme::FRAME_GAP));
    let y = theme::TOP_H + theme::FRAME_GAP;
    let scroller = super::glide::glide(
        scrollable(container(column(list)).padding(iced::Padding { top: 8.0, bottom: 8.0, left: 8.0, right: 16.0 }))
            .style(super::pages::scroll_style),
    );
    let panel = container(column![esc, scroller].push(footer))
        .width(width)
        .max_height((app.viewport.height - y - theme::FRAME_GAP).clamp(0.0, app.viewport.height * 0.72));
    // GPUI's search menu: a pane of liquid glass (18 px corners) that fades
    // in over `PANEL`.
    let panel = super::liquid_glass::glass(panel, super::liquid_glass::PANEL_R)
        .optics(super::liquid_glass::Optics::POPOVER)
        .shadow(theme::popover_shadow());
    // It hangs from the search field: it grows from its top centre.
    let panel = super::wash::enter_from(panel, super::motion::CONTROL, super::wash::Pivot::new(0.5, 0.0));
    let back = iced::widget::pin(mouse_area(Space::new().width(Length::Fill).height(Length::Fill)).on_press(Message::SearchClose)).y(y);
    let placed = iced::widget::pin(iced::widget::opaque(panel)).x(x).y(y);
    Some(iced::widget::stack![back, placed].into())
}

#[cfg(test)]
mod tests {
    use super::compose;
    use crate::tidal::{Album, Item, Row, RowKind};

    fn album(id: u64) -> Item {
        Item::Album(Album {
            id,
            title: id.to_string(),
            cover: None,
            artists: Vec::new(),
            release_date: String::new(),
            number_of_tracks: 0,
            duration: 0,
            kind: String::new(),
            explicit: false,
            tags: Vec::new(),
            copyright: String::new(),
        })
    }

    #[test]
    fn home_leads_with_the_featured_item_and_never_shows_one_twice() {
        let shelf = |title: &str, kind, ids: &[u64]| Row { title: title.into(), kind, items: ids.iter().map(|&i| album(i)).collect() };
        let feed = [
            shelf("new", RowKind::Cards, &[1, 2, 3, 4, 5, 6, 7, 8]),
            shelf("hero", RowKind::Featured, &[20]),
            shelf("short", RowKind::Shortcuts, &[30, 31]),
        ];
        let home = compose(&feed, vec![album(30)]);
        assert_eq!(home.lead, Some(("hero".to_string(), album(20))));
        assert_eq!(home.recent, vec![album(30), album(31)]);
        assert_eq!(home.collection.map(|(_, items)| items.len()), Some(6));
        assert_eq!(home.rows.len(), 1);
        assert_eq!(home.rows[0].items, vec![album(7), album(8)]);
    }
}
