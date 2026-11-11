//! Page content (as virtualised list blocks), side panels, overlays, login.

use super::sc;
use gpui::{
    anchored, deferred, div, list, point, prelude::*, px, relative, AnimationExt, AnyElement, ClipboardItem, Context, Corner, Focusable,
    FontWeight, ScrollHandle, SharedString, Window,
};

use super::icons::Icon;
use super::sort::SortKey;
use super::widgets::{
    self, action, card, cover, fmt_hms, fmt_long, fmt_time, icon_button, message, pill, section_title, track_row, widget_panel, Cols,
    RowStyle, PANEL_GAP,
};
use super::{theme, AppView, Load, LoginState, MenuTarget, Page};
use crate::config::Quality;
use crate::tidal::{Album, Artist, Item, Mix, Playlist, Row, RowKind, Track};

/// Which list a track row belongs to (to play the whole list from it).
#[derive(Clone, PartialEq)]
pub enum TrackSrc {
    Album(u64),
    Playlist(String),
    Mix(String),
    /// Library › Tracks: TIDAL favourites and local files together.
    Library,
    ArtistTop(u64),
    Search(String),
}

#[derive(Clone)]
pub enum Block {
    /// The page's content is loading: a calm indicator centred in the view.
    Loading,
    Title(SharedString, Option<SharedString>),
    Section(SharedString),
    Row(usize, SharedString, Vec<Item>),
    Cards(Vec<Item>),
    /// Unfolded album under a grid row: (album, column of its card).
    AlbumInline(Album, usize),
    SortBar(&'static str),
    /// Page title with the "..." sort menu of its collection section.
    TitleSort(SharedString, &'static str),
    /// Play / Shuffle for a track list.
    Actions(TrackSrc),
    /// The "Filter ..." box.
    FilterBar,
    Chips(SharedString, Vec<Item>),
    Links(Vec<Item>),
    Featured(SharedString, Vec<Item>),
    HomeLead(Option<(SharedString, Item)>, Vec<Item>),
    HomeCollection(SharedString, Vec<Item>),
    TrackHeader(RowStyle),
    Shortcuts(Vec<Item>),
    ShortcutsTitled(SharedString, Vec<Item>),
    Track(TrackSrc, usize, RowStyle),
    AlbumHeader(Album, u32),
    PlaylistHeader(Playlist, u32, u64),
    MixHeader(Mix, u32),
    ArtistHeader(Artist),
    Biography(SharedString),
    TopHit(Item),
    Text(SharedString),
    Message(SharedString),
    Recent(Vec<String>),
    Settings,
    NewPlaylist,
    Spacer(f32),
    /// Library header and control bar for a category ("tracks", "albums", "artists").
    LibraryBar(&'static str),
    /// Library list views of albums and artists: (position, item).
    AlbumRow(usize, Album),
    ArtistRow(usize, Artist),
    /// One part of the Replay page.
    Replay(super::replay::Part),
    /// Small label naming where the rows below come from (Mixes & novedades).
    SourceTag(SharedString, SharedString),
}

impl AppView {
    pub fn tracks_of(&self, src: &TrackSrc) -> Option<&[Track]> {
        match src {
            TrackSrc::Album(id) => self.albums.get(id)?.ready().map(|(_, t)| t.as_slice()),
            TrackSrc::Playlist(u) => self.playlists.get(u)?.ready().map(|(_, t)| t.as_slice()),
            TrackSrc::Mix(m) => self.mixes.get(m)?.ready().map(|(_, t)| t.as_slice()),
            TrackSrc::Library => Some(&self.lib_tracks),
            TrackSrc::ArtistTop(id) => self.artists.get(id)?.ready().map(|a| a.top_tracks.as_slice()),
            TrackSrc::Search(q) => self.search.get(q)?.ready().map(|s| s.tracks.as_slice()),
        }
    }

    fn content_limit(&self) -> f32 {
        // One column width for every page, so switching pages never moves the
        // left edge; Settings stays narrower for readable rows.
        if matches!(self.page, Page::Settings) {
            theme::CONTENT_W
        } else {
            theme::WIDE_W
        }
    }

    fn page_body_width(&self, window: &Window) -> f32 {
        (f32::from(window.viewport_size().width) / super::zoom()
            - self.sidebar_width()
            - theme::FRAME_GAP
            - 2.0 / super::zoom()
            - self.side_panel_w)
            .max(1.0)
    }

    fn content_inset(&self, window: &Window) -> f32 {
        (self.page_body_width(window) * 0.025).clamp(8.0, 28.0)
    }

    /// Width used by both the content column and its responsive children.
    pub fn column_width(&self, window: &Window) -> f32 {
        (self.page_body_width(window) - 2.0 * self.content_inset(window)).clamp(1.0, self.content_limit())
    }

    /// Does `t` pass the filter box? (title, artist or album)
    pub fn track_matches(&self, t: &Track) -> bool {
        self.filter.is_empty()
            || self.text_matches(&[&t.display_title(), &t.artist_names(), t.album.as_ref().map(|a| a.title.as_str()).unwrap_or("")])
    }

    pub fn text_matches(&self, fields: &[&str]) -> bool {
        self.filter.is_empty() || fields.iter().any(|f| f.to_lowercase().contains(&self.filter))
    }

    /// (columns, card width) of a grid that fills the column exactly.
    pub fn grid_metrics(&self, window: &Window) -> (usize, f32) {
        let w = self.column_width(window);
        let cols = (((w + theme::GAP) / (theme::CARD_W + theme::GAP)).floor() as usize).max(1);
        // Exact width (not rounded down), so the last column ends on the
        // column's right edge, like the controls and panels above it.
        let cw = (w - theme::GAP * (cols as f32 - 1.0)) / cols as f32;
        (cols, cw)
    }

    fn columns(&self, window: &Window) -> usize {
        self.grid_metrics(window).0
    }

    /// The current page as a flat list of blocks.
    pub fn blocks(&self, window: &Window) -> Vec<Block> {
        let mut b = Vec::new();
        let cols = self.columns(window);
        let grid = |b: &mut Vec<Block>, items: Vec<Item>| {
            for chunk in items.chunks(cols) {
                b.push(Block::Cards(chunk.to_vec()));
            }
        };
        let state = |b: &mut Vec<Block>, loading: bool, err: Option<&String>| {
            if loading {
                b.push(Block::Loading);
            } else if let Some(e) = err {
                b.push(Block::Message(crate::i18n::text("could_not_load", &[e.to_string()]).into()));
            }
        };
        let rows = |b: &mut Vec<Block>, rows: &[Row]| {
            for (i, r) in rows.iter().enumerate() {
                b.push(match r.kind {
                    RowKind::Cards => Block::Row(i, r.title.clone().into(), r.items.clone()),
                    RowKind::Chips => Block::Chips(r.title.clone().into(), r.items.clone()),
                    RowKind::Links => Block::Links(r.items.clone()),
                    RowKind::Featured => Block::Featured(r.title.clone().into(), r.items.clone()),
                    RowKind::Shortcuts => Block::ShortcutsTitled(r.title.clone().into(), r.items.clone()),
                });
            }
        };
        match &self.page {
            Page::Home | Page::Explore => {
                let home_extra = self.home_extra.get(&self.home_tab);
                let (title, data) = if self.page == Page::Home {
                    (crate::i18n::t("home"), if self.home_tab == 0 { &self.home } else { home_extra.unwrap_or(&Load::Idle) })
                } else {
                    (crate::i18n::t("explore"), &self.explore)
                };
                b.push(Block::Title(title.into(), None));
                match data {
                    Load::Ready(r) if self.page == Page::Home => {
                        let mut ordered: Vec<Row> =
                            r.iter().filter(|x| x.kind != RowKind::Shortcuts && !x.items.is_empty()).cloned().collect();
                        if let Some(index) = ordered.iter().position(|r| r.kind == RowKind::Featured) {
                            let featured = ordered.remove(index);
                            ordered.insert(0, featured);
                        }
                        let lead = ordered
                            .iter_mut()
                            .find(|row| matches!(row.kind, RowKind::Featured | RowKind::Cards))
                            .map(|row| (row.title.clone().into(), row.items.remove(0)));
                        ordered.retain(|row| !row.items.is_empty());
                        let mut recent: Vec<Item> = self.recents.iter().take(6).map(|a| Item::Album(a.to_album())).collect();
                        if let Some(short) = r.iter().find(|x| x.kind == RowKind::Shortcuts) {
                            for it in &short.items {
                                let dup =
                                    matches!(it, Item::Album(a) if recent.iter().any(|x| matches!(x, Item::Album(b) if b.id == a.id)));
                                if !dup && recent.len() < 6 {
                                    recent.push(it.clone());
                                }
                            }
                        }
                        b.push(Block::HomeLead(lead, recent));
                        if let Some(row) = ordered.iter_mut().find(|row| row.kind == RowKind::Cards && row.items.len() >= 4) {
                            let count = row.items.len().min(6);
                            b.push(Block::HomeCollection(row.title.clone().into(), row.items.drain(..count).collect()));
                            if !row.items.is_empty() {
                                row.title = crate::i18n::text("continued", &[row.title.to_string()]);
                            }
                        }
                        ordered.retain(|row| !row.items.is_empty());
                        rows(&mut b, &ordered)
                    }
                    Load::Ready(r) if r.is_empty() => b.push(Block::Message(crate::i18n::t("nothing_to_show_yet").into())),
                    Load::Ready(r) => rows(&mut b, r),
                    Load::Failed(e) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::Browse(path, title) => {
                b.push(Block::Title(title.clone().into(), None));
                match self.browse.get(path) {
                    Some(Load::Ready(r)) if r.is_empty() => b.push(Block::Message(crate::i18n::t("nothing_to_show_yet").into())),
                    Some(Load::Ready(r)) => rows(&mut b, r),
                    Some(Load::Failed(e)) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::Search(q) => {
                if q.is_empty() {
                    b.push(Block::Title(crate::i18n::t("search").into(), None));
                    let recent = self.settings.lock().recent_searches.clone();
                    if !recent.is_empty() {
                        b.push(Block::Section(crate::i18n::t("recent_searches").into()));
                        b.push(Block::Recent(recent));
                    }
                } else {
                    b.push(Block::Title(crate::i18n::text("results_for", &[q.to_string()]).into(), None));
                    match self.search.get(q) {
                        Some(Load::Ready(r)) => {
                            if let Some(top) = &r.top {
                                b.push(Block::Section(crate::i18n::t("top_result").into()));
                                b.push(Block::TopHit(top.clone()));
                            }
                            if !r.tracks.is_empty() {
                                b.push(Block::Section(crate::i18n::t("tracks").into()));
                                let style = RowStyle::new(true, true, false);
                                for i in 0..r.tracks.len().min(10) {
                                    b.push(Block::Track(TrackSrc::Search(q.clone()), i, style));
                                }
                            }
                            if !r.artists.is_empty() {
                                b.push(Block::Row(
                                    0,
                                    crate::i18n::t("artists").into(),
                                    r.artists.iter().cloned().map(Item::Artist).collect(),
                                ));
                            }
                            if !r.albums.is_empty() {
                                b.push(Block::Row(1, crate::i18n::t("albums").into(), r.albums.iter().cloned().map(Item::Album).collect()));
                            }
                            if !r.playlists.is_empty() {
                                b.push(Block::Row(
                                    2,
                                    crate::i18n::t("playlists").into(),
                                    r.playlists.iter().cloned().map(Item::Playlist).collect(),
                                ));
                            }
                            if r.top.is_none() && r.tracks.is_empty() && r.albums.is_empty() && r.artists.is_empty() {
                                b.push(Block::Message(crate::i18n::t("no_results").into()));
                            }
                        }
                        Some(Load::Failed(e)) => state(&mut b, false, Some(e)),
                        _ => state(&mut b, true, None),
                    }
                }
            }
            Page::Album(id) => match self.albums.get(id) {
                Some(Load::Ready((album, tracks))) => {
                    b.push(Block::AlbumHeader(album.clone(), tracks.len() as u32));
                    let style = RowStyle::new(false, false, true);
                    let multi_disc = tracks.iter().any(|t| t.volume_number > 1);
                    let mut disc = 0;
                    b.push(Block::Section(crate::i18n::t("tracks").into()));
                    if tracks.is_empty() {
                        b.push(Block::Message(crate::i18n::t("this_album_has_no_available_tracks").into()));
                    } else {
                        b.push(Block::TrackHeader(style));
                    }
                    for (i, t) in tracks.iter().enumerate() {
                        if multi_disc && t.volume_number != disc {
                            disc = t.volume_number;
                            b.push(Block::Text(crate::i18n::text("disc.number", &[format!("{}", disc)]).into()));
                        }
                        b.push(Block::Track(TrackSrc::Album(*id), i, style));
                    }
                    if !album.copyright.is_empty() {
                        b.push(Block::Text(format!("{}  ·  {}", album.release_date, album.copyright).into()));
                    }
                    if let Some(Load::Ready((more, similar))) = self.album_extra.get(id) {
                        let artist = album.artist_names();
                        if !more.is_empty() {
                            b.push(Block::Row(
                                0,
                                crate::i18n::text("more_albums_by", &[artist.to_string()]).into(),
                                more.iter().cloned().map(Item::Album).collect(),
                            ));
                        }
                        if !similar.is_empty() {
                            b.push(Block::Row(
                                1,
                                crate::i18n::t("related_artists").into(),
                                similar.iter().cloned().map(Item::Artist).collect(),
                            ));
                        }
                    }
                }
                Some(Load::Failed(e)) => state(&mut b, false, Some(e)),
                _ => state(&mut b, true, None),
            },
            Page::Playlist(uuid) => match self.playlists.get(uuid) {
                Some(Load::Ready((pl, tracks))) => {
                    let total: u64 = tracks.iter().map(|t| t.duration as u64).sum();
                    b.push(Block::PlaylistHeader(pl.clone(), tracks.len() as u32, total));
                    // One lock: taking the settings lock twice in a statement deadlocks.
                    let (bpm, rating) = {
                        let st = self.settings.lock();
                        (st.audio_metadata, st.show_ratings)
                    };
                    let style = RowStyle { date: true, bpm, rating, ..RowStyle::new(true, true, false) };
                    b.push(Block::FilterBar);
                    b.push(Block::TrackHeader(style));
                    for (i, t) in tracks.iter().enumerate() {
                        if self.track_matches(t) {
                            b.push(Block::Track(TrackSrc::Playlist(uuid.clone()), i, style));
                        }
                    }
                }
                Some(Load::Failed(e)) => state(&mut b, false, Some(e)),
                _ => state(&mut b, true, None),
            },
            Page::Mix(id) => match self.mixes.get(id) {
                Some(Load::Ready((mix, tracks))) => {
                    b.push(Block::MixHeader(mix.clone(), tracks.len() as u32));
                    let style = RowStyle::new(true, true, false);
                    b.push(Block::TrackHeader(style));
                    for i in 0..tracks.len() {
                        b.push(Block::Track(TrackSrc::Mix(id.clone()), i, style));
                    }
                }
                Some(Load::Failed(e)) => state(&mut b, false, Some(e)),
                _ => state(&mut b, true, None),
            },
            Page::Artist(id) => match self.artists.get(id) {
                Some(Load::Ready(a)) => {
                    b.push(Block::ArtistHeader(a.artist.clone()));
                    if !a.top_tracks.is_empty() {
                        b.push(Block::Section(crate::i18n::t("top_tracks").into()));
                        let style = RowStyle::new(true, true, false);
                        b.push(Block::TrackHeader(style));
                        for i in 0..a.top_tracks.len() {
                            b.push(Block::Track(TrackSrc::ArtistTop(*id), i, style));
                        }
                    }
                    let mut row = 0;
                    for (title, list) in [
                        (crate::i18n::t("albums"), &a.albums),
                        (crate::i18n::t("eps_and_singles"), &a.singles),
                        (crate::i18n::t("appears_on"), &a.appears_on),
                    ] {
                        if !list.is_empty() {
                            b.push(Block::Row(row, title.into(), list.iter().cloned().map(Item::Album).collect()));
                            row += 1;
                        }
                    }
                    if !a.similar.is_empty() {
                        b.push(Block::Row(
                            row,
                            crate::i18n::t("similar_artists").into(),
                            a.similar.iter().cloned().map(Item::Artist).collect(),
                        ));
                    }
                    if let Some(bio) = a.bio.as_ref().filter(|bio| !bio.trim().is_empty()) {
                        b.push(Block::Biography(bio.clone().into()));
                    }
                }
                Some(Load::Failed(e)) => state(&mut b, false, Some(e)),
                _ => state(&mut b, true, None),
            },
            Page::MyPlaylists => {
                b.push(Block::TitleSort(crate::i18n::t("playlists").into(), "playlists"));
                b.push(Block::NewPlaylist);
                b.push(Block::FilterBar);
                match &self.my_playlists {
                    Load::Ready(l) if l.is_empty() => b.push(Block::Message(crate::i18n::t("you_have_no_playlists_yet").into())),
                    Load::Ready(l) => {
                        grid(&mut b, l.iter().filter(|p| self.text_matches(&[&p.title, &p.creator])).cloned().map(Item::Playlist).collect())
                    }
                    Load::Failed(e) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::MyAlbums => {
                b.push(Block::LibraryBar("albums"));
                // Local albums are listed at once; TIDAL's join when loaded.
                let fav = match (self.signed_in, &self.fav_albums) {
                    (false, _) | (_, Load::Ready(_)) => Load::Ready(self.lib_albums.clone()),
                    // Local files show at once, without waiting for TIDAL.
                    _ if !self.local_albums.is_empty() => Load::Ready(self.lib_albums.clone()),
                    (_, Load::Failed(e)) => Load::Failed(e.clone()),
                    _ => Load::Loading,
                };
                match &fav {
                    Load::Ready(l) if l.is_empty() => b.push(Block::Message(crate::i18n::t("you_have_not_added_any_albums_yet").into())),
                    Load::Ready(l) => {
                        let l: Vec<Album> = l.iter().filter(|a| self.text_matches(&[&a.title, &a.artist_names()])).cloned().collect();
                        if l.is_empty() {
                            b.push(Block::Message(crate::i18n::t("no_albums_match_your_search").into()));
                        } else if self.lib_grid("albums") {
                            for chunk in l.chunks(cols) {
                                b.push(Block::Cards(chunk.iter().cloned().map(Item::Album).collect()));
                                if let Some(col) = chunk.iter().position(|a| Some(a.id) == self.expanded_album) {
                                    b.push(Block::AlbumInline(chunk[col].clone(), col));
                                }
                            }
                        } else {
                            b.extend(l.into_iter().enumerate().map(|(i, a)| Block::AlbumRow(i, a)));
                        }
                    }
                    Load::Failed(e) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::MyArtists => {
                b.push(Block::LibraryBar("artists"));
                let artists = match (self.signed_in, &self.fav_artists) {
                    (false, _) | (_, Load::Ready(_)) => Load::Ready(self.lib_artists.clone()),
                    _ if !self.local_artists.is_empty() => Load::Ready(self.lib_artists.clone()),
                    (_, Load::Failed(e)) => Load::Failed(e.clone()),
                    _ => Load::Loading,
                };
                match &artists {
                    Load::Ready(l) if l.is_empty() => {
                        b.push(Block::Message(crate::i18n::t("you_are_not_following_any_artists_yet").into()))
                    }
                    Load::Ready(l) => {
                        let l: Vec<Artist> = l.iter().filter(|a| self.text_matches(&[&a.name])).cloned().collect();
                        if l.is_empty() {
                            b.push(Block::Message(crate::i18n::t("no_artists_match_your_search").into()));
                        } else if self.lib_grid("artists") {
                            grid(&mut b, l.into_iter().map(Item::Artist).collect());
                        } else {
                            b.extend(l.into_iter().enumerate().map(|(i, a)| Block::ArtistRow(i, a)));
                        }
                    }
                    Load::Failed(e) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::Replay => {
                if self.history.is_empty() {
                    b.push(Block::Title(crate::i18n::t("replay").into(), Some(crate::i18n::t("your_listening_statistics").into())));
                    b.push(Block::Message(crate::i18n::t("replay.empty_explanation").into()));
                } else {
                    use super::replay::Part;
                    for p in [Part::Header, Part::Dashboard] {
                        b.push(Block::Replay(p));
                    }
                }
                // TIDAL's own monthly summaries, if its Home feed has them (no extra query).
                if let Load::Ready(r) = &self.home {
                    for row in r.iter().filter(|x| {
                        let t = x.title.to_lowercase();
                        t.contains("historial") || t.contains("history") || t.contains("most listened")
                    }) {
                        b.push(Block::SourceTag(
                            crate::i18n::t("tidal_summaries").into(),
                            crate::i18n::t("replay.tidal_monthly_playlists").into(),
                        ));
                        b.push(Block::Row(200, row.title.clone().into(), row.items.clone()));
                    }
                }
            }
            Page::Discover => {
                b.push(Block::Title(
                    crate::i18n::t("mixes_new_releases").into(),
                    Some(crate::i18n::t("mixes_editorial_picks_and_new_releases").into()),
                ));
                let mut base = 300usize;
                let section = |b: &mut Vec<Block>, tag: &str, note: &str, data: Option<&Load<Vec<Row>>>, base: &mut usize| {
                    b.push(Block::SourceTag(tag.to_string().into(), note.to_string().into()));
                    match data {
                        Some(Load::Ready(r)) if r.is_empty() => b.push(Block::Message(crate::i18n::t("nothing_to_show_yet").into())),
                        Some(Load::Ready(r)) => {
                            for (i, row) in r.iter().enumerate() {
                                b.push(match row.kind {
                                    RowKind::Cards | RowKind::Shortcuts => {
                                        Block::Row(*base + i, row.title.clone().into(), row.items.clone())
                                    }
                                    RowKind::Chips => Block::Chips(row.title.clone().into(), row.items.clone()),
                                    RowKind::Links => Block::Links(row.items.clone()),
                                    RowKind::Featured => Block::Featured(row.title.clone().into(), row.items.clone()),
                                });
                            }
                            *base += r.len();
                        }
                        Some(Load::Failed(e)) => b.push(Block::Message(crate::i18n::text("could_not_load", &[e.to_string()]).into())),
                        _ => b.push(Block::Loading),
                    }
                };
                // The user's mixes, as one row.
                b.push(Block::SourceTag(crate::i18n::t("your_mixes").into(), crate::i18n::t("mixes_and_radio_made_for_you").into()));
                match &self.my_mixes {
                    Load::Ready(l) if l.is_empty() => b.push(Block::Message(crate::i18n::t("no_mixes_yet").into())),
                    Load::Ready(l) => {
                        b.push(Block::Row(base, crate::i18n::t("mixes_and_radio").into(), l.iter().cloned().map(Item::Mix).collect()))
                    }
                    Load::Failed(e) => b.push(Block::Message(crate::i18n::text("could_not_load", &[e.to_string()]).into())),
                    _ => b.push(Block::Loading),
                }
                base += 1;
                section(
                    &mut b,
                    crate::i18n::t("staff_picks"),
                    crate::i18n::t("picks_from_tidal_s_editorial_team"),
                    self.home_extra.get(&1),
                    &mut base,
                );
                if !self.settings.lock().hide_uploads {
                    section(&mut b, "UPLOADS", crate::i18n::t("new_releases_from_independent_artists"), self.home_extra.get(&2), &mut base);
                }
            }
            Page::Recent => {
                b.push(Block::Title(crate::i18n::t("history").into(), Some(crate::i18n::t("history.albums").into())));
                if self.recents.is_empty() {
                    b.push(Block::Message(crate::i18n::t("history.empty").into()));
                } else {
                    grid(&mut b, self.recents.iter().map(|a| Item::Album(a.to_album())).collect());
                }
            }
            Page::MyMixes => {
                b.push(Block::TitleSort(crate::i18n::t("mixes_and_radio").into(), "mixes"));
                b.push(Block::FilterBar);
                match &self.my_mixes {
                    Load::Ready(l) if l.is_empty() => b.push(Block::Message(crate::i18n::t("no_mixes_yet").into())),
                    Load::Ready(l) => grid(&mut b, l.iter().filter(|m| self.text_matches(&[&m.title])).cloned().map(Item::Mix).collect()),
                    Load::Failed(e) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::MyTracks => {
                b.push(Block::LibraryBar("tracks"));
                let fav = match (self.signed_in, &self.fav_tracks) {
                    (false, _) | (_, Load::Ready(_)) => Load::Ready(self.lib_tracks.clone()),
                    // Local files show at once, without waiting for TIDAL.
                    _ if !self.local_tracks.is_empty() => Load::Ready(self.lib_tracks.clone()),
                    (_, Load::Failed(e)) => Load::Failed(e.clone()),
                    _ => Load::Loading,
                };
                match &fav {
                    Load::Ready(l) if l.is_empty() => {
                        b.push(Block::Message(crate::i18n::text("library.empty_tracks", &[crate::i18n::t("tidal").to_string()]).into()))
                    }
                    Load::Ready(l) => {
                        let (bpm, rating) = {
                            let st = self.settings.lock();
                            (st.audio_metadata, st.show_ratings)
                        };
                        let style = RowStyle { date: true, bpm, rating, sortable: Some("tracks"), ..RowStyle::new(true, true, false) };
                        b.push(Block::Actions(TrackSrc::Library));
                        let shown: Vec<usize> = (0..l.len()).filter(|&i| self.track_matches(&l[i])).collect();
                        if shown.is_empty() {
                            b.push(Block::Message(crate::i18n::t("no_tracks_match_your_search").into()));
                        } else if self.lib_grid("tracks") {
                            grid(&mut b, shown.into_iter().map(|i| Item::Track(l[i].clone())).collect());
                        } else {
                            b.push(Block::TrackHeader(style));
                            b.extend(shown.into_iter().map(|i| Block::Track(TrackSrc::Library, i, style)));
                        }
                    }
                    Load::Failed(e) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::Settings => {
                b.push(Block::Settings);
            }
        }
        b.push(Block::Spacer(32.));
        b
    }

    /// Each block sits in the page's centred content column; the
    /// list itself spans the whole area so the wheel works on the margins too.
    pub fn render_block(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let panel = match self.current_blocks.get(ix) {
            Some(Block::Row(..)) => self.has_catalog_shelves(),
            Some(Block::Chips(..) | Block::Featured(..) | Block::Links(..) | Block::ShortcutsTitled(..)) => self.page == Page::Home,
            _ => false,
        };
        let inner = self.render_block_inner(ix, window, cx);
        let inner = if panel {
            div()
                .pb(sc(PANEL_GAP))
                .child(widget_panel(("shelf-glass", ix)).p(sc(shelf_inset(self.column_width(window)))).child(inner))
                .into_any_element()
        } else {
            inner
        };
        div()
            .w_full()
            .px(sc(self.content_inset(window)))
            .flex()
            .justify_center()
            .child(div().min_w(sc(0.)).w_full().max_w(sc(self.content_limit())).child(inner))
            .into_any_element()
    }

    fn render_block_inner(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(block) = self.current_blocks.get(ix).cloned() else {
            return div().into_any_element();
        };
        match block {
            Block::Title(t, sub) => div()
                .pt(sc(22.))
                .pb(sc(if self.page == Page::Home { 20. } else { 14. }))
                .flex()
                .flex_col()
                .gap(sc(5.))
                .child(div().text_size(sc(if self.page == Page::Home { 36. } else { 30. })).font_weight(FontWeight::BOLD).child(t))
                .when_some(sub, |d, s| d.child(div().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_secondary()).child(s)))
                .into_any_element(),
            Block::Section(t) => section_title(t),
            Block::SortBar(section) => super::sort::bar(self, section, cx),
            Block::LibraryBar(section) => super::library::bar(self, section, window, cx),
            Block::Replay(part) => super::replay::render(self, part, window, cx),
            Block::SourceTag(tag, note) => div()
                .pt(sc(18.))
                .pb(sc(4.))
                .flex()
                .items_center()
                .gap(sc(10.))
                .child(
                    div()
                        .px(sc(8.))
                        .py(sc(3.))
                        .rounded(sc(5.))
                        .bg(theme::control())
                        .text_size(sc(10.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::text_secondary())
                        .child(tag),
                )
                .child(div().text_size(sc(11.5)).text_color(theme::text_dim()).child(note))
                .child(div().flex_1().h(sc(1.)).bg(theme::divider()))
                .into_any_element(),
            Block::AlbumRow(i, album) => super::library::album_row(i, &album, self.column_width(window), cx),
            Block::ArtistRow(i, artist) => super::library::artist_row(i, &artist, cx),
            Block::AlbumInline(album, col) => {
                let (cols, card_w) = self.grid_metrics(window);
                super::album_inline::render(self, album, col, cols, card_w, cx)
            }
            Block::NewPlaylist => div()
                // Same gap as between the filter field and the grid.
                .pb(sc(16.))
                .flex()
                .child(pill(
                    "new-pl",
                    crate::i18n::t("playlist.create"),
                    None,
                    true,
                    |this, _, window, cx| this.open_dialog(super::playlists::Dialog::NewPlaylist { add: vec![] }, window, cx),
                    cx,
                ))
                .into_any_element(),
            Block::Loading => {
                // Read in `render` before the list lays out (reading the list's
                // state while it lays out its items would borrow it twice).
                super::widgets::loading(self.list_view_h / super::zoom() - 140.0)
            }
            Block::Message(t) if t.starts_with(crate::i18n::t("could_not_load")) => super::widgets::error_message(t, cx),
            Block::Message(t) => message(t),
            Block::Spacer(h) => div().h(sc(h)).into_any_element(),
            Block::Text(t) => div()
                .py(sc(10.))
                .max_w(sc(900.))
                .text_size(sc(13.))
                .line_height(sc(20.))
                .text_color(theme::text_secondary())
                .child(t)
                .into_any_element(),
            Block::Row(row, title, items) => self.render_row(row, title, items, self.column_width(window), cx),
            Block::Cards(items) => {
                let (cols, cw) = self.grid_metrics(window);
                div()
                    .pb(sc(22.))
                    .flex()
                    .gap(sc(theme::GAP))
                    .children(items.iter().enumerate().map(|(i, it)| {
                        let c = card(SharedString::from(format!("g{ix}-{i}")), it, cw, true, &*self, cx);
                        super::fx::stagger(SharedString::from(format!("gs{}-{ix}-{i}", self.page_anim)), i, c)
                    }))
                    .children((items.len()..cols).map(|_| div().w(sc(cw)).flex_none()))
                    .into_any_element()
            }
            Block::TrackHeader(style) => {
                let style = style.fit(self.column_width(window));
                let (section, cur) = match style.sortable {
                    Some(sec) => (Some(sec), Some(self.sort_of(sec))),
                    None => (None, None),
                };
                let label = |text: &'static str, key: Option<SortKey>, cx: &mut Context<Self>| {
                    let active = key.is_some() && cur.map(|(k, _)| k) == key;
                    let arrow = match (active, cur) {
                        (true, Some((_, desc))) => {
                            if desc {
                                " ▼"
                            } else {
                                " ▲"
                            }
                        }
                        _ => "",
                    };
                    let sortable = section.is_some() && key.is_some();
                    div()
                        .id(SharedString::from(format!("th-{text}")))
                        .text_size(sc(10.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if active { theme::accent() } else { theme::text_dim() })
                        .when(sortable, |d| d.cursor_pointer().hover(|s| s.text_color(theme::text())))
                        .child(format!("{text}{arrow}"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let (Some(sec), Some(k)) = (section, key) {
                                this.set_sort(sec, k, cx);
                            }
                        }))
                };
                div()
                    .px(sc(Cols::PAD))
                    .h(sc(36.))
                    .mb(sc(3.))
                    .rounded(sc(8.))
                    .border_1()
                    .border_color(theme::panel_border())
                    .bg(theme::ink(0.035))
                    .flex()
                    .items_center()
                    .gap(sc(style.column_gap()))
                    .when(!style.tiny, |d| {
                        d.child(
                            div()
                                .w(sc(Cols::NUM))
                                .flex_none()
                                .text_center()
                                .text_size(sc(9.5))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme::text_dim())
                                .child("#"),
                        )
                    })
                    .child(div().flex_1().min_w(sc(0.)).pl(sc(if style.cover { Cols::COVER + Cols::COVER_GAP } else { 0. })).child(label(
                        if style.compact { crate::i18n::t("track.column") } else { crate::i18n::t("title") },
                        Some(SortKey::Name),
                        cx,
                    )))
                    .when(!style.compact, |d| {
                        d.child(div().w(relative(Cols::artist(style))).flex_none().child(label(
                            crate::i18n::t("artist.column"),
                            Some(SortKey::Artist),
                            cx,
                        )))
                    })
                    .when(style.album, |d| {
                        d.child(div().w(relative(Cols::ALBUM)).flex_none().child(label(
                            crate::i18n::t("album.column"),
                            Some(SortKey::Album),
                            cx,
                        )))
                    })
                    .when(style.rating, |d| {
                        d.child(div().w(sc(Cols::RATING)).flex_none().child(label(crate::i18n::t("rating"), Some(SortKey::Rating), cx)))
                    })
                    .when(style.date, |d| {
                        d.child(div().w(sc(Cols::DATE)).flex_none().child(label(crate::i18n::t("added"), Some(SortKey::Added), cx)))
                    })
                    .when(!style.tiny, |d| {
                        d.child(div().w(sc(Cols::TIME)).flex_none().child(label(crate::i18n::t("time"), Some(SortKey::Duration), cx)))
                    })
                    .when(style.bpm, |d| d.child(div().w(sc(Cols::BPM)).flex_none().child(label(crate::i18n::t("bpm"), None, cx))))
                    .child(div().w(sc(style.actions_width())).flex_none())
                    .into_any_element()
            }
            Block::TitleSort(t, section) => div()
                .pt(sc(22.))
                .pb(sc(14.))
                .flex()
                .items_center()
                .gap(sc(8.))
                .child(div().text_size(sc(22.)).font_weight(FontWeight::BOLD).child(t))
                .child(icon_button(
                    ("sort-menu", ix),
                    Icon::More,
                    18.,
                    theme::text_secondary(),
                    move |this, ev: &gpui::ClickEvent, _, cx| {
                        this.menu = Some((ev.position(), MenuTarget::Sort(section)));
                        cx.notify();
                    },
                    cx,
                ))
                .into_any_element(),
            Block::Actions(src) => {
                let (s1, s2) = (src.clone(), src.clone());
                div()
                    .pb(sc(16.))
                    .flex()
                    .items_center()
                    .gap(sc(10.))
                    .child(pill(
                        "act-play",
                        crate::i18n::t("play"),
                        Some(Icon::Play),
                        true,
                        move |this, _, _, cx| {
                            if let Some(t) = this.tracks_of(&s1).map(|t| t.to_vec()) {
                                this.shuffle = false;
                                this.play_tracks(t, 0, cx)
                            }
                        },
                        cx,
                    ))
                    .child(pill(
                        "act-shuffle",
                        crate::i18n::t("shuffle"),
                        Some(Icon::Shuffle),
                        false,
                        move |this, _, _, cx| {
                            if let Some(t) = this.tracks_of(&s2).map(|t| t.to_vec()) {
                                this.play_shuffled(t, cx)
                            }
                        },
                        cx,
                    ))
                    .into_any_element()
            }
            Block::Chips(title, items) => div()
                .child(
                    div()
                        .pt(sc(if self.page == Page::Home { 0. } else { 26. }))
                        .pb(sc(12.))
                        .text_size(sc(theme::TYPE_SECTION))
                        .font_weight(FontWeight::BOLD)
                        .child(title),
                )
                .child(div().flex().flex_wrap().gap(px(10.)).children(items.into_iter().enumerate().map(|(i, it)| {
                    let (label, target) = match &it {
                        Item::Link(l) => (l.title.clone(), it.clone()),
                        _ => (String::new(), it.clone()),
                    };
                    div()
                        .id(SharedString::from(format!("chip-{ix}-{i}")))
                        .h(px(36.))
                        .px(px(16.))
                        .flex()
                        .items_center()
                        .rounded(sc(18.))
                        .bg(theme::control())
                        .text_size(px(12.5))
                        .font_weight(FontWeight::BOLD)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::active()))
                        .active(|s| s.opacity(0.7))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| this.open_item(&target, cx)))
                })))
                .into_any_element(),
            Block::Links(items) => div()
                .pt(sc(if self.page == Page::Home { 0. } else { 26. }))
                .flex()
                .flex_wrap()
                .children(items.into_iter().enumerate().map(|(i, it)| {
                    let (label, icon) = match &it {
                        Item::Link(l) => {
                            let k = format!("{} {}", l.icon, l.title).to_lowercase();
                            let icon = if k.contains("new") {
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
                            (l.title.clone(), icon)
                        }
                        _ => (String::new(), Icon::Mix),
                    };
                    div()
                        .id(SharedString::from(format!("plink-{ix}-{i}")))
                        .w(relative(if self.column_width(window) >= 720. {
                            1. / 3.
                        } else if self.column_width(window) >= 400. {
                            0.5
                        } else {
                            1.
                        }))
                        .h(px(46.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .active(|s| s.opacity(0.6))
                        .child(icon.el(18., theme::text()))
                        .child(div().text_size(px(12.5)).font_weight(FontWeight::BOLD).child(label))
                        .on_click(cx.listener(move |this, _, _, cx| this.open_item(&it, cx)))
                }))
                .into_any_element(),
            Block::HomeLead(lead, recent) => home_lead(lead, recent, self.column_width(window), cx),
            Block::HomeCollection(title, items) => home_collection(title, items, self.column_width(window), cx),
            Block::Featured(title, items) => {
                let column = self.column_width(window);
                let home = self.page == Page::Home;
                let available = if home { (column - 2. * shelf_inset(column) - 2.).max(1.) } else { column };
                let width = if home { (available * 0.42).clamp(182., 290.).min(available) } else { 220.0 };
                let height = if home { width * 0.59 } else { 160.0 };
                div()
                    .child(div().pb(sc(12.)).text_size(sc(if home { 18. } else { 15. })).font_weight(FontWeight::BOLD).child(title))
                    // A strip you can scroll sideways: items past the edge stay reachable.
                    .child(div().id(("featured", ix)).w_full().flex().gap(sc(14.)).overflow_x_scroll().children(
                        items.into_iter().enumerate().map(|(i, it)| {
                            let Item::Promo(p) = it.clone() else { return div().into_any_element() };
                            let url = p.image_url();
                            let image = url.as_deref().and_then(|u| super::images::sized(u, width, height, cx));
                            let card = div()
                                .id(SharedString::from(format!("promo-{ix}-{i}")))
                                .w(sc(width))
                                .flex_none()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .cursor_pointer()
                                .child(match image {
                                    Some(img) => gpui::img(img)
                                        .w(sc(width))
                                        .h(sc(height))
                                        .rounded(sc(12.))
                                        .object_fit(gpui::ObjectFit::Cover)
                                        .into_any_element(),
                                    None => div().w(sc(width)).h(sc(height)).rounded(sc(12.)).bg(theme::frame()).into_any_element(),
                                })
                                .child(
                                    div()
                                        .pt(px(4.))
                                        .text_size(px(8.5))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme::accent())
                                        .child(p.label.to_uppercase()),
                                )
                                .child(div().text_size(px(13.)).font_weight(FontWeight::BOLD).truncate().child(p.title.clone()))
                                .child(div().text_size(px(11.5)).text_color(theme::text_secondary()).truncate().child(p.subtitle.clone()))
                                .on_click(cx.listener(move |this, _, _, cx| this.open_item(&it, cx)));
                            super::fx::stagger(SharedString::from(format!("fs{}-{i}", self.page_anim)), i, card.into_any_element())
                        }),
                    ))
                    .into_any_element()
            }
            Block::ShortcutsTitled(title, items) => {
                let col_w = self.column_width(window);
                let home = self.page == Page::Home;
                let width = if home { (col_w - 2. * shelf_inset(col_w) - 2.).max(1.) } else { col_w };
                div()
                    .pt(sc(if home { 0. } else { 22. }))
                    .flex()
                    .flex_col()
                    .gap(sc(12.))
                    .child(div().text_size(sc(theme::TYPE_SECTION)).font_weight(FontWeight::BOLD).child(title))
                    .child(shortcut_grid(items, width, cx))
                    .into_any_element()
            }
            Block::Shortcuts(items) => shortcut_grid(items, self.column_width(window), cx),
            Block::FilterBar => div()
                .pb(sc(16.))
                .child(
                    div()
                        .h(sc(34.))
                        .px(sc(12.))
                        .flex()
                        .items_center()
                        .gap(sc(10.))
                        .relative()
                        .rounded(sc(10.))
                        .bg(theme::liquid_glass(0.))
                        .border_1()
                        .border_color(theme::glass_rim(0.))
                        .child(super::glass::sheen("filter-bar-glass", 10., 0.9))
                        .child(Icon::Search.el(14., theme::text_secondary()))
                        .child(self.filter_input.clone()),
                )
                .into_any_element(),
            Block::Track(src, i, style) => {
                let Some(track) = self.tracks_of(&src).and_then(|t| t.get(i)).cloned() else {
                    return div().into_any_element();
                };
                let s = src.clone();
                let own = match &src {
                    TrackSrc::Playlist(u) if self.is_own_playlist(u) => Some(u.clone()),
                    _ => None,
                };
                track_row(
                    i,
                    &track,
                    style.fit(self.column_width(window)),
                    move |this, ix, cx| {
                        if let Some(list) = this.tracks_of(&s).map(|t| t.to_vec()) {
                            this.play_tracks(list, ix, cx);
                        }
                    },
                    own,
                    self,
                    cx,
                )
            }
            Block::AlbumHeader(album, n) => self.album_header(album, n, self.column_width(window), cx),
            Block::PlaylistHeader(pl, n, total) => self.playlist_header(pl, n, total, self.column_width(window), cx),
            Block::MixHeader(mix, n) => {
                let id = mix.id.clone();
                let id2 = mix.id.clone();
                let tint = mix.image.clone().and_then(|u| super::images::color(&u, cx));
                self.page_tint = tint;
                let width = self.column_width(window);
                catalog_header(
                    crate::i18n::t("mix"),
                    cover(mix.image.clone(), catalog_cover_size(width), false, cx),
                    mix.title.clone(),
                    div()
                        .flex()
                        .flex_col()
                        .gap(sc(8.))
                        .child(div().text_size(sc(12.5)).text_color(theme::text_secondary()).child(mix.subtitle.clone()))
                        .child(
                            div()
                                .text_size(sc(11.5))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme::text_secondary())
                                .child(crate::i18n::text("tracks.count", &[format!("{}", n)])),
                        )
                        .into_any_element(),
                    width,
                    vec![
                        pill(
                            "mix-play",
                            crate::i18n::t("play"),
                            Some(Icon::Play),
                            true,
                            move |this, _, _, cx| {
                                if let Some(t) = this.tracks_of(&TrackSrc::Mix(id.clone())).map(|t| t.to_vec()) {
                                    this.shuffle = false;
                                    this.play_tracks(t, 0, cx)
                                }
                            },
                            cx,
                        )
                        .into_any_element(),
                        pill(
                            "mix-shuffle",
                            crate::i18n::t("shuffle"),
                            Some(Icon::Shuffle),
                            false,
                            move |this, _, _, cx| {
                                if let Some(t) = this.tracks_of(&TrackSrc::Mix(id2.clone())).map(|t| t.to_vec()) {
                                    this.play_shuffled(t, cx)
                                }
                            },
                            cx,
                        )
                        .into_any_element(),
                    ],
                    vec![],
                )
            }
            Block::ArtistHeader(artist) => self.artist_header(artist, self.column_width(window), cx),
            Block::Biography(text) => div()
                .py(sc(PANEL_GAP))
                .child(
                    widget_panel("biography-glass")
                        .p(sc(shelf_inset(self.column_width(window))))
                        .flex()
                        .flex_col()
                        .gap(sc(super::widgets::PANEL_HEAD_GAP))
                        .child(div().text_size(sc(theme::TYPE_SECTION)).font_weight(FontWeight::BOLD).child(crate::i18n::t("biography")))
                        .child(
                            div().max_w(sc(900.)).text_size(sc(13.)).line_height(sc(21.)).text_color(theme::text_secondary()).child(text),
                        ),
                )
                .into_any_element(),
            Block::TopHit(item) => {
                let (title, sub, url, round) = match &item {
                    Item::Album(a) => {
                        (a.title.clone(), crate::i18n::text("album.detail", &[a.artist_names().to_string()]), a.cover(320), false)
                    }
                    Item::Artist(a) => (a.name.clone(), crate::i18n::t("artist").to_string(), a.picture(320), true),
                    Item::Playlist(p) => (p.title.clone(), crate::i18n::t("playlist").to_string(), p.cover(320), false),
                    Item::Track(t) => {
                        (t.display_title(), crate::i18n::text("track.detail", &[t.artist_names().to_string()]), t.cover(320), false)
                    }
                    Item::Mix(m) => (m.title.clone(), crate::i18n::t("mix").to_string(), m.image.clone(), false),
                    Item::Link(l) => (l.title.clone(), String::new(), None, false),
                    Item::Promo(p) => (p.title.clone(), p.subtitle.clone(), p.image_url(), false),
                };
                let open = item.clone();
                div()
                    .child(
                        div()
                            .id("tophit")
                            .w(sc(460.))
                            .p(sc(20.))
                            .flex()
                            .gap(sc(20.))
                            .items_center()
                            .relative()
                            .rounded(sc(18.))
                            .bg(theme::liquid_glass(0.))
                            .border_1()
                            .border_color(theme::glass_rim(0.))
                            .shadow(theme::glass_shadow())
                            .cursor_pointer()
                            .child(super::glass::sheen("tophit-glass", 18., 0.8))
                            .child(cover(url, 112., round, cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(sc(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(sc(6.))
                                    .child(div().text_size(sc(22.)).font_weight(FontWeight::BOLD).truncate().child(title))
                                    .child(div().text_color(theme::text_secondary()).truncate().child(sub)),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.open_item(&open, cx))),
                    )
                    .into_any_element()
            }
            Block::Recent(list) => div()
                .flex()
                .flex_wrap()
                .gap(sc(8.))
                .children(list.into_iter().enumerate().map(|(i, q)| {
                    let qq = q.clone();
                    div()
                        .id(("recent", i))
                        .px(sc(14.))
                        .py(sc(8.))
                        .relative()
                        .rounded(sc(999.))
                        .bg(theme::liquid_glass(0.))
                        .border_1()
                        .border_color(theme::glass_rim(0.))
                        .cursor_pointer()
                        .child(super::glass::sheen(("recent-glass", i), 999., 0.9))
                        .child(q)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.search_input.update(cx, |i, cx| i.set_text(&qq, cx));
                            this.page = Page::Search(qq.clone());
                            this.page_changed(cx);
                        }))
                }))
                .into_any_element(),
            Block::Settings => self.settings_panel(self.column_width(window), cx),
        }
    }

    fn has_catalog_shelves(&self) -> bool {
        matches!(self.page, Page::Album(_) | Page::Artist(_))
    }

    fn render_row(&mut self, row: usize, title: SharedString, items: Vec<Item>, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let panel = self.has_catalog_shelves();
        let home = self.page == Page::Home;
        let available = if panel { (width - 2. * shelf_inset(width) - 2.).max(1.) } else { width };
        let card_w = if panel || home { (available * 0.17).clamp(112., 152.).min(available) } else { theme::ROW_CARD_W };
        let scrollable = items.len() as f32 * (card_w + theme::GAP) - theme::GAP > available + 1.;
        let handle = self.row_scroll.entry(row).or_default().clone();
        let h1 = handle.clone();
        let h2 = handle.clone();
        let step = (available - card_w * 0.25).max(card_w + theme::GAP);
        div()
            .w_full()
            .when(home, |d| d.pb(sc(24.)).border_t_1().border_color(theme::divider()))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .pt(sc(if panel {
                        0.
                    } else if home {
                        18.
                    } else {
                        26.
                    }))
                    .pb(sc(if panel { super::widgets::PANEL_HEAD_GAP } else { 12. }))
                    .flex()
                    .items_center()
                    .gap(sc(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(sc(0.))
                            .text_size(sc(if home {
                                19.
                            } else if panel {
                                16.
                            } else {
                                15.
                            }))
                            .font_weight(FontWeight::BOLD)
                            .child(title),
                    )
                    .when(scrollable, |d| {
                        // The buttons' own padding sits outside the line, so the
                        // arrows align with the panel's edge and the title's height.
                        d.child(
                            div()
                                .flex()
                                .gap(sc(2.))
                                .my(sc(-6.))
                                .mr(sc(-6.))
                                .child(icon_button(
                                    ("row-l", row),
                                    Icon::Back,
                                    15.,
                                    theme::text_secondary(),
                                    move |_, _, _, cx| {
                                        let o = h1.offset();
                                        h1.set_offset(point((o.x + sc(step)).min(sc(0.)), o.y));
                                        cx.notify();
                                    },
                                    cx,
                                ))
                                .child(icon_button(
                                    ("row-r", row),
                                    Icon::Forward,
                                    15.,
                                    theme::text_secondary(),
                                    move |_, _, _, cx| {
                                        let o = h2.offset();
                                        let max = h2.max_offset().width;
                                        h2.set_offset(point((o.x - sc(step)).max(-max), o.y));
                                        cx.notify();
                                    },
                                    cx,
                                )),
                        )
                    }),
            )
            .child(div().id(("row", row)).w_full().flex().gap(sc(theme::GAP)).overflow_x_scroll().track_scroll(&handle).children(
                items.iter().enumerate().map(|(i, it)| {
                    let c = card(SharedString::from(format!("r{row}-{i}")), it, card_w, false, &*self, cx);
                    super::fx::stagger(SharedString::from(format!("rs{}-{row}-{i}", self.page_anim)), i, c)
                }),
            ))
            .into_any_element()
    }

    fn album_header(&mut self, album: Album, n: u32, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let fav = self.fav_album_ids.contains(&album.id);
        let artist_id = album.artists.first().map(|a| a.id);
        let id = album.id;
        let a_fav = album.clone();
        // Albums made from local files: no TIDAL actions (favourite, share,
        // artist page); they can be removed from the Library instead.
        let local = crate::local::is_local_id(id);
        let mut meta = vec![if n == 1 { crate::i18n::t("1_track").into() } else { crate::i18n::text("tracks.count", &[format!("{}", n)]) }];
        if !album.year().is_empty() {
            meta.push(album.year().to_string());
        }
        if album.duration > 0 {
            meta.push(fmt_hms(album.duration as u64));
        }
        let subtitle = div()
            .flex()
            .flex_col()
            .gap(sc(8.))
            .child(
                div()
                    .id("album-artist")
                    .flex()
                    .items_center()
                    .gap(sc(6.))
                    .text_size(sc(12.5))
                    .font_weight(FontWeight::BOLD)
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(Icon::Artist.el(14., theme::text_secondary()))
                    .child(album.artist_names())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(a) = artist_id.filter(|a| *a != 0) {
                            this.navigate(Page::Artist(a), cx)
                        }
                    })),
            )
            .child(div().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_secondary()).child(meta.join(" · ")))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(sc(8.))
                    .children(widgets::quality_badge(&album.tags))
                    .when(album.explicit, |d| d.child(widgets::explicit_badge())),
            )
            .child(widgets::album_stars("hdr", id, 17., self, cx))
            .into_any_element();
        let share_id = album.id;
        catalog_header(
            if local { crate::i18n::t("local_album") } else { crate::i18n::t("album") },
            cover(album.cover(640), catalog_cover_size(width), false, cx),
            album.title.clone(),
            subtitle,
            width,
            vec![
                pill(
                    "album-play",
                    crate::i18n::t("play"),
                    Some(Icon::Play),
                    true,
                    move |this, _, _, cx| {
                        if let Some(t) = this.tracks_of(&TrackSrc::Album(id)).map(|t| t.to_vec()) {
                            this.shuffle = false;
                            this.play_tracks(t, 0, cx)
                        }
                    },
                    cx,
                )
                .into_any_element(),
                pill(
                    "album-shuffle",
                    crate::i18n::t("shuffle"),
                    Some(Icon::Shuffle),
                    false,
                    move |this, _, _, cx| {
                        if let Some(t) = this.tracks_of(&TrackSrc::Album(id)).map(|t| t.to_vec()) {
                            this.play_shuffled(t, cx)
                        }
                    },
                    cx,
                )
                .into_any_element(),
            ],
            if local {
                vec![action(
                    "album-local-settings",
                    Icon::Settings,
                    crate::i18n::t("settings"),
                    false,
                    move |this, _, _, cx| this.open_local_settings(cx),
                    cx,
                )]
            } else {
                vec![
                    action(
                        "album-fav",
                        if fav { Icon::HeartFilled } else { Icon::Heart },
                        if fav { crate::i18n::t("sort.added") } else { crate::i18n::t("add") },
                        false,
                        move |this, _, _, cx| this.toggle_favorite_album(&a_fav, cx),
                        cx,
                    ),
                    action(
                        "album-share",
                        Icon::Link,
                        crate::i18n::t("share"),
                        false,
                        move |this, _, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(crate::tidal::share_url("album", &share_id.to_string())));
                            this.notify_toast(crate::i18n::t("link_copied"), cx);
                        },
                        cx,
                    ),
                    action(
                        "album-more",
                        Icon::More,
                        crate::i18n::t("more"),
                        false,
                        move |this, ev: &gpui::ClickEvent, _, cx| {
                            if let Some(t) = this.tracks_of(&TrackSrc::Album(id)).map(|t| t.to_vec()) {
                                this.menu = Some((ev.position(), MenuTarget::AddToPlaylist(t)));
                                cx.notify();
                            }
                        },
                        cx,
                    ),
                ]
            },
        )
    }

    fn artist_header(&mut self, artist: Artist, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let id = artist.id;
        let n = self.tracks_of(&TrackSrc::ArtistTop(id)).map_or(0, |tracks| tracks.len());
        let details = div()
            .text_size(sc(theme::TYPE_LABEL))
            .text_color(theme::text_secondary())
            .when(n > 0, |d| {
                d.child(if n == 1 {
                    crate::i18n::t("1_featured_track").into()
                } else {
                    crate::i18n::text("featured_tracks", &[format!("{}", n)])
                })
            })
            .into_any_element();
        let mut actions = Vec::new();
        if n > 0 {
            actions.push(
                pill(
                    "artist-play",
                    crate::i18n::t("play"),
                    Some(Icon::Play),
                    true,
                    move |this, _, _, cx| {
                        if let Some(t) = this.tracks_of(&TrackSrc::ArtistTop(id)).map(|t| t.to_vec()) {
                            this.shuffle = false;
                            this.play_tracks(t, 0, cx);
                        }
                    },
                    cx,
                )
                .into_any_element(),
            );
            if !crate::local::is_local_id(id) {
                actions.push(
                    pill(
                        "artist-radio",
                        crate::i18n::t("radio"),
                        Some(Icon::Radio),
                        false,
                        move |this, _, _, cx| {
                            if let Some(t) = this.tracks_of(&TrackSrc::ArtistTop(id)).and_then(|t| t.first()).cloned() {
                                this.start_radio(&t, cx);
                            }
                        },
                        cx,
                    )
                    .into_any_element(),
                );
            }
        }
        if !crate::local::is_local_id(id) {
            actions.push(
                pill(
                    "artist-follow",
                    if self.fav_artist_ids.contains(&id) { crate::i18n::t("following") } else { crate::i18n::t("follow") },
                    None,
                    false,
                    move |this, _, _, cx| this.toggle_follow_artist(id, cx),
                    cx,
                )
                .into_any_element(),
            );
        }
        catalog_header(
            crate::i18n::t("artist"),
            cover(artist.picture(750), catalog_cover_size(width), true, cx),
            artist.name,
            details,
            width,
            actions,
            Vec::new(),
        )
    }

    fn playlist_header(&mut self, pl: Playlist, n: u32, total: u64, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let mut right: Vec<AnyElement> = Vec::new();
        if self.is_own_playlist(&pl.uuid) {
            let (u1, u2, title) = (pl.uuid.clone(), pl.uuid.clone(), pl.title.clone());
            right.push(action(
                "pl-edit",
                Icon::Edit,
                crate::i18n::t("edit"),
                false,
                move |this, _, window, cx| this.open_dialog(super::playlists::Dialog::EditPlaylist { uuid: u1.clone() }, window, cx),
                cx,
            ));
            right.push(action(
                "pl-delete",
                Icon::Close,
                crate::i18n::t("delete"),
                false,
                move |this, _, window, cx| {
                    this.open_dialog(super::playlists::Dialog::DeletePlaylist { uuid: u2.clone(), title: title.clone() }, window, cx)
                },
                cx,
            ));
        } else {
            let saved = self.is_saved_playlist(&pl.uuid);
            let p = pl.clone();
            right.push(action(
                "pl-save",
                if saved { Icon::HeartFilled } else { Icon::Heart },
                if saved { crate::i18n::t("saved") } else { crate::i18n::t("save") },
                false,
                move |this, _, _, cx| this.toggle_saved_playlist(p.clone(), cx),
                cx,
            ));
        }
        let share = pl.uuid.clone();
        right.push(action(
            "pl-share",
            Icon::Link,
            crate::i18n::t("share"),
            false,
            move |this, _, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(crate::tidal::share_url("playlist", &share)));
                this.notify_toast(crate::i18n::t("link_copied"), cx);
            },
            cx,
        ));
        let u3 = pl.uuid.clone();
        right.push(action(
            "pl-more",
            Icon::More,
            crate::i18n::t("more"),
            false,
            move |this, ev: &gpui::ClickEvent, _, cx| {
                if let Some(t) = this.tracks_of(&TrackSrc::Playlist(u3.clone())).map(|t| t.to_vec()) {
                    this.menu = Some((ev.position(), MenuTarget::AddToPlaylist(t)));
                    cx.notify();
                }
            },
            cx,
        ));
        let uuid = pl.uuid.clone();
        let uuid2 = pl.uuid.clone();
        let tint = pl.cover(640).and_then(|u| super::images::color(&u, cx));
        self.page_tint = tint;
        let mut subtitle = div().flex().flex_col().gap(sc(8.));
        if !pl.creator.is_empty() {
            subtitle = subtitle.child(div().text_size(sc(12.5)).font_weight(FontWeight::BOLD).child(pl.creator.clone()));
        }
        if !pl.description.is_empty() {
            subtitle = subtitle.child(
                div()
                    .max_w(sc(620.))
                    .text_size(sc(theme::TYPE_LABEL))
                    .text_color(theme::text_secondary())
                    .line_clamp(2)
                    .child(pl.description.clone()),
            );
        }
        subtitle = subtitle.child(
            div()
                .text_size(sc(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_secondary())
                .child(crate::i18n::text("tracks.count_detail", &[format!("{}", n), fmt_hms(total).to_string()])),
        );
        catalog_header(
            crate::i18n::t("playlist"),
            self.playlist_cover(&pl, catalog_cover_size(width), cx),
            pl.title.clone(),
            subtitle.into_any_element(),
            width,
            vec![
                pill(
                    "pl-play",
                    crate::i18n::t("play"),
                    Some(Icon::Play),
                    true,
                    move |this, _, _, cx| {
                        if let Some(t) = this.tracks_of(&TrackSrc::Playlist(uuid.clone())).map(|t| t.to_vec()) {
                            this.shuffle = false;
                            this.play_tracks(t, 0, cx)
                        }
                    },
                    cx,
                )
                .into_any_element(),
                pill(
                    "pl-shuffle",
                    crate::i18n::t("shuffle"),
                    Some(Icon::Shuffle),
                    false,
                    move |this, _, _, cx| {
                        if let Some(t) = this.tracks_of(&TrackSrc::Playlist(uuid2.clone())).map(|t| t.to_vec()) {
                            this.play_shuffled(t, cx)
                        }
                    },
                    cx,
                )
                .into_any_element(),
            ],
            right,
        )
    }
}

fn home_lead(lead: Option<(SharedString, Item)>, recent: Vec<Item>, width: f32, cx: &mut Context<AppView>) -> AnyElement {
    let wide = width >= 820.;
    let gap = 20.;
    let rail_w = if wide { (width * 0.29).clamp(250., 340.) } else { width };
    let lead_w = if wide { width - rail_w - gap } else { width };
    let pad = shelf_inset(width);
    let inner_w = (lead_w - 2. * pad - 2.).max(1.);
    let featured = match lead {
        Some((section, item)) => {
            let (title, subtitle, url, _) = super::widgets::item_text(&item);
            let square = !matches!(item, Item::Promo(_));
            let beside = square && inner_w >= 360.;
            let art_w = if square { (inner_w * if beside { 0.56 } else { 1. }).min(300.) } else { inner_w };
            let art_h = if square { art_w } else { (art_w * 0.53).clamp(132., 292.) };
            let image = url.as_deref().and_then(|url| super::images::sized(url, art_w, art_h, cx));
            let key = "home-feature-art";
            let hover = super::interact::hover(key);
            let (action, icon) = match &item {
                Item::Album(_) => (crate::i18n::t("view_album"), Icon::Album),
                Item::Artist(_) => (crate::i18n::t("view_artist"), Icon::Artist),
                Item::Playlist(_) => (crate::i18n::t("view_playlist"), Icon::Playlist),
                Item::Mix(_) => (crate::i18n::t("open_mix"), Icon::Mix),
                Item::Track(_) => (crate::i18n::t("play"), Icon::Play),
                _ => (crate::i18n::t("open"), Icon::Forward),
            };
            let artwork = div()
                .id(key)
                .relative()
                .w(sc(art_w))
                .h(sc(art_h))
                .flex_none()
                .rounded(sc(12.))
                .overflow_hidden()
                .bg(theme::field())
                .cursor_pointer()
                .child(match image {
                    Some(image) => gpui::img(image).size_full().rounded(sc(12.)).object_fit(gpui::ObjectFit::Cover).into_any_element(),
                    None => div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::Album.el(42., theme::text_dim()))
                        .into_any_element(),
                })
                .child(div().absolute().top(sc(0.)).left(sc(0.)).size_full().bg(theme::ink(0.06 * hover)))
                .on_click(cx.listener({
                    let item = item.clone();
                    move |this, _, _, cx| this.open_item(&item, cx)
                }));
            div()
                .flex()
                .flex_col()
                .gap(sc(12.))
                .child(
                    div()
                        .flex()
                        .when(beside, |d| d.items_center())
                        .when(!beside, |d| d.flex_col())
                        .gap(sc(if beside { 24. } else { 14. }))
                        .child(super::interact::track(artwork, key, cx))
                        .child(
                            div()
                                .flex_1()
                                .min_w(sc(0.))
                                .flex()
                                .gap(sc(12.))
                                .when(beside, |d| d.flex_col().items_start())
                                .when(!beside, |d| d.items_center())
                                .child(
                                    div()
                                        .min_w(sc(0.))
                                        .when(beside, |d| d.w_full())
                                        .when(!beside, |d| d.flex_1())
                                        .flex()
                                        .flex_col()
                                        .gap(sc(3.))
                                        .child(
                                            div()
                                                .text_size(sc(if beside && inner_w >= 630. { 36. } else { 27. }))
                                                .font_weight(FontWeight::BOLD)
                                                .line_clamp(2)
                                                .child(title),
                                        )
                                        .when(!subtitle.is_empty(), |d| {
                                            d.child(
                                                div().text_size(sc(13.)).text_color(theme::text_secondary()).line_clamp(1).child(subtitle),
                                            )
                                        })
                                        .child(div().text_size(sc(11.5)).text_color(theme::text_secondary()).line_clamp(1).child(section)),
                                )
                                .child(super::widgets::pill(
                                    "home-feature-open",
                                    action,
                                    Some(icon),
                                    true,
                                    move |this, _, _, cx| this.open_item(&item, cx),
                                    cx,
                                )),
                        ),
                )
                .into_any_element()
        }
        None => div()
            .flex()
            .flex_col()
            .items_start()
            .gap(sc(14.))
            .child(div().text_size(sc(23.)).font_weight(FontWeight::BOLD).child(crate::i18n::t("explore_music")))
            .child(super::widgets::pill(
                "home-explore",
                crate::i18n::t("open_explore"),
                Some(Icon::Explore),
                true,
                |this, _, _, cx| this.navigate(Page::Explore, cx),
                cx,
            ))
            .into_any_element(),
    };
    let listening = div()
        .w(sc(rail_w))
        .flex_none()
        .flex()
        .flex_col()
        .when(wide, |d| d.pl(sc(8.)))
        .pt(sc(4.))
        .gap(sc(4.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(sc(8.))
                .pb(sc(9.))
                .text_size(sc(18.))
                .font_weight(FontWeight::BOLD)
                .child(Icon::Clock.el(16., theme::text_secondary()))
                .child(crate::i18n::t("recently_played")),
        )
        .when(recent.is_empty(), |d| {
            d.child(
                div()
                    .py(sc(10.))
                    .text_size(sc(theme::TYPE_LABEL))
                    .text_color(theme::text_secondary())
                    .child(crate::i18n::t("your_history_will_appear_here")),
            )
        })
        .children(recent.into_iter().enumerate().map(|(i, item)| home_item("home-recent", i, item, false, cx)))
        .child(div().flex().justify_start().pt(sc(3.)).child(super::widgets::pill(
            "home-history",
            crate::i18n::t("view_history"),
            Some(Icon::Clock),
            false,
            |this, _, _, cx| this.navigate(Page::Recent, cx),
            cx,
        )));
    div()
        .w_full()
        .pb(sc(24.))
        .flex()
        .gap(sc(gap))
        .when(!wide, |d| d.flex_col())
        .child(widget_panel("home-feature").w(sc(lead_w)).min_w(sc(0.)).p(sc(pad)).child(featured))
        .child(listening)
        .into_any_element()
}

fn home_collection(title: SharedString, items: Vec<Item>, width: f32, cx: &mut Context<AppView>) -> AnyElement {
    let cols = if width >= 640. { 2 } else { 1 };
    let gap = 16.;
    let col = (width - gap * (cols - 1) as f32) / cols as f32;
    div()
        .w_full()
        .border_t_1()
        .border_color(theme::divider())
        .pt(sc(18.))
        .pb(sc(24.))
        .child(div().pb(sc(12.)).text_size(sc(20.)).font_weight(FontWeight::BOLD).child(title))
        .child(
            div().flex().flex_wrap().gap_x(sc(gap)).gap_y(sc(4.)).children(
                items
                    .into_iter()
                    .enumerate()
                    .map(|(i, item)| div().w(sc(col)).flex_none().child(home_item("home-collection", i, item, true, cx))),
            ),
        )
        .into_any_element()
}

/// A compact catalogue row keeps titles readable beside the artwork.
fn home_item(section: &'static str, i: usize, item: Item, collection: bool, cx: &mut Context<AppView>) -> AnyElement {
    let (title, subtitle, url, round) = widgets::item_text(&item);
    let key = format!("{section}-{i}");
    let hover = super::interact::hover(&key);
    let press = super::interact::press(&key);
    let play = item.clone();
    let can_play = collection && !matches!(item, Item::Link(_) | Item::Promo(_));
    let menu = match &item {
        Item::Album(a) if collection => Some(MenuTarget::Album(a.clone())),
        Item::Artist(a) if collection => Some(MenuTarget::Artist(a.clone())),
        Item::Track(t) if collection => Some(MenuTarget::Track(t.clone())),
        _ => None,
    };
    let row = div()
        .id((section, i))
        .min_w(sc(0.))
        .h(sc(if collection { 64. } else { 40. }))
        .px(sc(if collection { 8. } else { 4. }))
        .flex()
        .items_center()
        .gap(sc(10.))
        .rounded(sc(9.))
        .cursor_pointer()
        .bg(theme::ink((if collection { 0.032 } else { 0. }) + 0.06 * hover + 0.04 * press))
        .child(cover(url, if collection { 48. } else { 32. }, round, cx))
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .flex()
                .flex_col()
                .gap(sc(3.))
                .child(div().text_size(sc(13.)).font_weight(FontWeight::SEMIBOLD).truncate().child(title))
                .when(!subtitle.is_empty(), |d| {
                    d.child(div().text_size(sc(11.5)).text_color(theme::text_secondary()).truncate().child(subtitle))
                }),
        )
        .when(can_play, |d| {
            d.child(
                div()
                    .id("play")
                    .size(sc(32.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(sc(16.))
                    .bg(theme::ink(0.05))
                    .hover(|d| d.bg(theme::ink(0.12)))
                    .tooltip(super::sidebar::tip(crate::i18n::t("play")))
                    .child(Icon::Play.el(14., theme::text()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.play_item(&play, cx);
                    })),
            )
        })
        .when(!collection, |d| {
            d.child(Icon::Forward.el_scaled(13., super::interact::mix(theme::text_dim(), theme::text(), hover), 1. + 0.08 * hover))
        })
        .when_some(menu, |d, target| {
            d.on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    this.menu = Some((event.position, target.clone()));
                    cx.notify();
                }),
            )
        })
        .on_click(cx.listener(move |this, _, _, cx| this.open_item(&item, cx)));
    super::interact::track(row, key, cx).into_any_element()
}

fn shelf_inset(width: f32) -> f32 {
    super::widgets::panel_pad(width)
}

fn shortcut_grid(items: Vec<Item>, width: f32, cx: &mut Context<AppView>) -> AnyElement {
    let gap = 8.;
    let cols = if width >= 720. {
        3.
    } else if width >= 430. {
        2.
    } else {
        1.
    };
    let tile_w = (width - (cols - 1.) * gap) / cols;
    div()
        .flex()
        .flex_wrap()
        .gap(sc(gap))
        .children(items.iter().enumerate().map(|(i, item)| widgets::shortcut(("shortcut", i), item, tile_w, cx)))
        .into_any_element()
}

fn catalog_cover_size(width: f32) -> f32 {
    (if width < 480. { width * 0.43 } else { width * 0.21 }).clamp(42., 200.)
}

fn catalog_header(
    kind: &'static str,
    art: AnyElement,
    title: String,
    details: AnyElement,
    width: f32,
    primary: Vec<AnyElement>,
    secondary: Vec<AnyElement>,
) -> AnyElement {
    let stacked = width < 480.;
    let portrait = kind == crate::i18n::t("artist");
    let inset = if portrait { 0. } else { super::widgets::panel_pad(width) };
    let surface = if portrait { div() } else { widget_panel("detail-header-glass") };
    div()
        .pt(sc(22.))
        .pb(sc(PANEL_GAP))
        .child(
            surface
                .p(sc(inset))
                .flex()
                .flex_col()
                .gap(sc(18.))
                .child(
                    div()
                        .flex()
                        .gap(sc(18.))
                        .when(stacked, |d| d.flex_col().items_start())
                        .when(!stacked, |d| d.items_center())
                        .child(div().flex_none().rounded(sc(12.)).overflow_hidden().child(art))
                        .child(
                            div()
                                .min_w(sc(0.))
                                .flex()
                                .flex_col()
                                .items_start()
                                .when(stacked, |d| d.w_full())
                                .when(!stacked, |d| d.flex_1())
                                .gap(sc(8.))
                                .child(div().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_secondary()).child(kind))
                                .child(
                                    div()
                                        .w_full()
                                        .text_size(sc(if stacked { 28. } else { 40. }))
                                        .line_height(sc(if stacked { 32. } else { 46. }))
                                        .font_weight(FontWeight::BOLD)
                                        .line_clamp(3)
                                        .child(title),
                                )
                                .child(details),
                        ),
                )
                .child(
                    div()
                        .pt(sc(12.))
                        .border_t_1()
                        .border_color(theme::divider())
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap(sc(8.))
                        .child(div().flex().flex_wrap().items_center().gap(sc(8.)).children(primary))
                        .when(!secondary.is_empty(), |d| d.child(div().flex().flex_wrap().items_center().gap(sc(3.)).children(secondary))),
                ),
        )
        .into_any_element()
}

/// The middle column: navigation bar on top, page list below.
pub fn content(view: &mut AppView, _window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    use gpui::{Animation, AnimationExt};
    let top = view.list.logical_scroll_top();
    let wash_offset = if top.item_ix == 0 { Some(f32::from(top.offset_in_item)) } else { None };
    let wash = view.page_tint.zip(wash_offset);
    div()
        .flex_1()
        .min_w(sc(0.))
        .h_full()
        .flex()
        .flex_col()
        .child(
            div()
                .relative()
                .flex_1()
                .min_h(sc(0.))
                .flex()
                .flex_col()
                // Colour wash of the page's cover, fading out below the header.
                .when_some(wash, |d, (c, off)| {
                    d.child(div().absolute().left(sc(0.)).right(sc(0.)).top(gpui::px(-off)).h(sc(380.)).bg(super::fx::tint(c, 0.42)))
                })
                .child(
                    list(view.list.clone(), cx.processor(|this, ix: usize, window, cx| this.render_block(ix, window, cx)))
                        .flex_1()
                        .min_h(sc(0.))
                        // Content (or an error) eases in when loading finishes.
                        .opacity(view.content_alpha()),
                )
                .child(super::smooth::wheel_catcher(cx.entity()))
                .children(super::scrollbar::render(view, cx))
                // Page transition: fade in while sliding in from the side you are
                // heading to (from the right for a new page, from the left going back).
                .with_animation(("page", view.page_anim), super::motion::animation(super::motion::PANEL), {
                    let dir = view.nav_dir;
                    move |el, t| el.opacity(t).left(sc((1.0 - t) * 26.0 * dir))
                }),
        )
        .into_any_element()
}

/// Right panel: play queue or lyrics.
pub fn side_panel(view: &mut AppView, width: f32, cx: &mut Context<AppView>) -> AnyElement {
    // While closing (both flags off) keep drawing whichever panel was open.
    let lyrics = view.show_lyrics || (!view.show_queue && view.side_last_lyrics);
    let (icon, title, detail) = if lyrics {
        (Icon::Lyrics, crate::i18n::t("lyrics"), view.now.as_ref().map(|n| n.track.display_title().into()))
    } else {
        let next = view.now.as_ref().map_or(0, |n| n.index + 1);
        let count = view.queue.len().saturating_sub(next);
        (Icon::Queue, crate::i18n::t("play_queue"), Some(crate::i18n::text("queued", &[format!("{}", count)]).into()))
    };
    let body: AnyElement = if lyrics { lyrics_panel(view, cx) } else { queue_panel(view, "queue", cx) };
    div()
        .w(sc(width))
        .flex_none()
        .h_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(theme::surface())
        .border_l_1()
        .border_color(theme::divider())
        .child(
            div()
                .h(sc(66.))
                .flex_none()
                .px(sc(12.))
                .flex()
                .items_center()
                .gap(sc(6.))
                .border_b_1()
                .border_color(theme::divider())
                .child(widgets::panel_heading(icon, title, detail))
                .child(icon_button(
                    "resize-panel",
                    if view.settings.lock().wide_side_panel { Icon::WinRestore } else { Icon::WinMax },
                    20.,
                    theme::text_secondary(),
                    |this, _, _, cx| {
                        let mut settings = this.settings.lock();
                        settings.side_panel_width = None;
                        settings.wide_side_panel = !settings.wide_side_panel;
                        settings.save();
                        cx.notify();
                    },
                    cx,
                ))
                .child(icon_button(
                    "close-panel",
                    Icon::Close,
                    20.,
                    theme::text_secondary(),
                    |this, _, _, cx| {
                        this.show_queue = false;
                        this.show_lyrics = false;
                        cx.notify();
                    },
                    cx,
                )),
        )
        .child(body)
        .into_any_element()
}

fn queue_meter(_uid: u64, playing: bool) -> AnyElement {
    let color = theme::accent();
    let bars = move |d: gpui::Div, phase: f32| {
        d.children((0..3).map(|i| {
            let wave = (std::f32::consts::TAU * (phase + i as f32 / 3.)).sin();
            div().w(sc(4.)).h(sc(10. + 5. * wave)).rounded(sc(2.)).bg(color)
        }))
    };
    let meter = div().w(sc(19.)).h(sc(16.)).flex_none().flex().items_end().justify_between();
    if playing {
        super::motion::looping(meter, std::time::Duration::from_millis(1150), "eq-bars", bars).into_any_element()
    } else {
        bars(meter.opacity(0.6), 0.).into_any_element()
    }
}

/// The queue list; `key` names its scroll glide (one per place it is shown).
pub fn queue_panel(view: &mut AppView, key: &'static str, cx: &mut Context<AppView>) -> AnyElement {
    let cur = view.now.as_ref().map(|n| n.index).filter(|&i| i < view.queue.len());
    let playing = view.queue_open_view == Some(key)
        && view.now.as_ref().is_some_and(|n| view.snap.key == Some(n.key))
        && !view.snap.paused
        && !view.snap.buffering
        && !view.ended;
    let handle = super::smooth::handle(key);
    // Use the measured panel height, shared by both queue presentations.
    let height = view.queue_heights.get(key).copied().unwrap_or(0.);
    let content_height = view.queue.len() as f32 * 60. + if cur.is_some() { 18. } else { 0. };
    let scrolling = height > 0. && content_height > height;
    let spacer = ((height - 78.) / 2.).max(0.);
    if !scrolling {
        super::smooth::stop(key);
        handle.set_offset(gpui::point(sc(0.), sc(0.)));
    }
    let center = if scrolling && view.queue_open_view == Some(key) && view.queue_center_pending {
        cur.map(|i| {
            view.queue_center_pending = false;
            super::smooth::stop(key);
            i + 1 // The first layout child is the leading scroll spacer.
        })
    } else {
        None
    };
    let list = div()
        .id(key)
        .size_full()
        .flex()
        .flex_col()
        .min_h(sc(0.))
        .when(scrolling, |d| d.overflow_y_scroll().track_scroll(&handle))
        .when(!scrolling, |d| d.overflow_hidden().justify_center())
        .px(sc(12.))
        .when(view.queue.is_empty(), |d| d.child(message(crate::i18n::t("add_music_to_the_queue_to_see_it_here"))))
        // Leave room to center the first or last track too.
        .when(scrolling && cur.is_some(), |d| d.child(div().flex_none().h(sc(spacer))))
        .children(view.queue.iter().enumerate().map(|(i, q)| {
            let current = Some(i) == cur;
            let previous = cur.is_some_and(|index| i < index);
            let uid = q.uid;
            let t = &q.track;
            div()
                .id(("q", uid as usize))
                .flex_none()
                .h(sc(60.))
                .border_b_1()
                .border_color(theme::divider())
                .px(sc(8.))
                .flex()
                .items_center()
                .gap(sc(12.))
                .rounded(sc(9.))
                .cursor_pointer()
                .when(current, |d| d.my(sc(9.)).bg(theme::active()))
                .hover(|s| s.bg(theme::hover()))
                .child(div().flex_none().opacity(if previous { 0.55 } else { 1.0 }).child(cover(t.cover(80), 40., false, cx)))
                .child(
                    div()
                        .flex_1()
                        .min_w(sc(0.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if current {
                                    theme::accent()
                                } else if previous {
                                    theme::text_dim()
                                } else {
                                    theme::text()
                                })
                                .child(t.display_title()),
                        )
                        .child(
                            div().truncate().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_secondary()).child(t.artist_names()),
                        ),
                )
                .child(div().flex_none().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_dim()).child(fmt_time(t.duration as f64)))
                .when(current && key == "queue", |d| d.child(queue_meter(uid, playing)))
                .when(!current, |d| {
                    d.child(icon_button(
                        ("remove-queued", uid as usize),
                        Icon::Close,
                        18.,
                        theme::text_secondary(),
                        move |this, _, _, cx| {
                            if let Some(index) = this.queue.iter().position(|item| item.uid == uid) {
                                this.remove_from_queue(index, cx);
                            }
                        },
                        cx,
                    ))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(index) = this.queue.iter().position(|item| item.uid == uid) {
                        if this.now.as_ref().is_none_or(|now| now.index != index) {
                            this.play_index(index, 0.0, false, cx);
                        }
                    }
                }))
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                        this.menu = Some((ev.position, MenuTarget::QueueItem(uid)));
                        cx.notify();
                    }),
                )
        }))
        .when(scrolling && cur.is_some(), |d| d.child(div().flex_none().h(sc(spacer))))
        .when(scrolling, |d| d.child(super::smooth::glide(key)))
        .when_some(center, |d, index| {
            d.child(
                gpui::canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        if let Some(row) = handle.bounds_for_item(index) {
                            let viewport = handle.bounds();
                            let target = row.top() - viewport.top() + (row.size.height - viewport.size.height) / 2.;
                            let offset = handle.offset();
                            handle.set_offset(gpui::point(offset.x, -target.clamp(sc(0.), handle.max_offset().height)));
                            window.request_animation_frame();
                        }
                    },
                )
                .absolute()
                .size(sc(0.)),
            )
        });
    div()
        .flex_1()
        .min_h(sc(0.))
        .relative()
        .overflow_hidden()
        .my(sc(8.))
        .child(list)
        .child(
            gpui::canvas(
                {
                    let entity = cx.entity();
                    move |bounds, _, cx| {
                        let height = f32::from(bounds.size.height) / super::zoom();
                        entity.update(cx, |view, cx| {
                            if view.queue_heights.get(key).is_none_or(|old| (old - height).abs() > 0.5) {
                                view.queue_heights.insert(key, height);
                                view.queue_center_pending = true;
                                cx.notify();
                            }
                        });
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .into_any_element()
}

fn lyrics_panel(view: &mut AppView, cx: &mut Context<AppView>) -> AnyElement {
    let Some(now) = &view.now else { return message(crate::i18n::t("nothing_is_playing")) };
    let pos = view.display_position();
    let show_roman = view.settings.lock().lyrics_romanize;
    let id = now.track.id;
    let provider = |p: &str| {
        div().pt(sc(28.)).text_size(sc(11.)).text_color(theme::text_dim()).child(crate::i18n::text("lyrics.provider", &[p.to_string()]))
    };
    match view.lyrics.get(&id) {
        Some(Load::Ready(l)) if !l.synced.is_empty() => {
            let l = l.clone();
            let current = l.synced.iter().rposition(|(t, _)| *t <= pos + 0.15);
            view.lyrics_line = current;
            if view.lyrics_follow {
                // Keep the current line a little above the middle, gliding.
                super::now_playing::follow_lyrics(
                    view,
                    current.unwrap_or(0),
                    0.38,
                    super::now_playing::upcoming(&l.synced, current, pos, 0),
                );
            }
            super::smooth::register("lyrics", &view.lyrics_scroll);
            let lines = div()
                .id("lyrics")
                .flex_1()
                .min_h(sc(0.))
                .overflow_y_scroll()
                .track_scroll(&view.lyrics_scroll)
                // Scrolling by hand pauses the auto-follow until "Sincronizar".
                .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                    if this.lyrics_follow {
                        this.lyrics_follow = false;
                        this.lyrics_sync_at = None;
                        cx.notify();
                    }
                }))
                .px(sc(24.))
                .pb(sc(40.))
                .flex()
                .flex_col()
                .gap(sc(6.))
                .children(l.synced.iter().enumerate().map(|(i, (t, line))| {
                    let active = Some(i) == current;
                    let past = current.is_some_and(|c| i < c);
                    let emphasis = super::now_playing::line_emphasis(id, i, active);
                    let at = *t;
                    let edge = super::now_playing::edge_fade(&view.lyrics_scroll, i, 30.0, 70.0);
                    div()
                        .id(("lyric", i))
                        .opacity(edge)
                        .px(sc(8.))
                        .py(sc(5.))
                        .rounded(sc(6.))
                        .cursor_pointer()
                        .text_size(sc(20.))
                        .line_height(sc(28.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(super::interact::mix(
                            if past { theme::text_secondary() } else { theme::text_dim() },
                            theme::text(),
                            emphasis,
                        ))
                        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                        .child(if line.is_empty() { "♪".to_string() } else { line.clone() })
                        .when_some(l.roman.get(i).and_then(|r| r.as_ref()).filter(|_| show_roman), |d, reading| {
                            d.child(
                                div()
                                    .text_size(sc(14.))
                                    .line_height(sc(20.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .opacity(0.8)
                                    .child(reading.clone()),
                            )
                        })
                        // Click a line to jump there (and follow again).
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.lyrics_follow = true;
                            this.seek(at.max(0.0), cx)
                        }))
                }))
                .child(provider(&l.provider))
                .child(super::smooth::glide("lyrics"));
            super::widgets::with_sync_button(lines.into_any_element(), view, "side", true, cx)
        }
        Some(Load::Ready(l)) if !l.text.is_empty() => div()
            .id("lyrics")
            .flex_1()
            .min_h(sc(0.))
            .overflow_y_scroll()
            .track_scroll(&super::smooth::handle("lyrics-text"))
            .child(super::smooth::glide("lyrics-text"))
            .px(sc(24.))
            .text_size(sc(theme::TYPE_SECTION))
            .line_height(sc(26.))
            .children(l.text.lines().enumerate().map(|(i, line)| {
                div()
                    .child(if line.is_empty() { "\u{a0}".into() } else { line.to_owned() })
                    .when_some(l.roman.get(i).and_then(|r| r.as_ref()).filter(|_| show_roman), |d, reading| {
                        d.child(div().text_size(sc(13.)).opacity(0.8).child(reading.clone()))
                    })
            }))
            .child(provider(&crate::i18n::text("unsynced", &[l.provider.to_string()])))
            .into_any_element(),
        Some(Load::Loading) => message(crate::i18n::t("searching_for_lyrics")),
        _ => super::now_playing::missing_lyrics(cx),
    }
}

/// Context menu and toast.
pub fn overlays(view: &mut AppView, _window: &mut Window, cx: &mut Context<AppView>) -> Vec<AnyElement> {
    let mut out = Vec::new();
    if view.menu.is_none() {
        // Any menu opens with "Add to playlist" folded.
        view.ctx_pl_open = false;
    }
    if let Some((pos, target)) = view.menu.clone() {
        type Action = Box<dyn Fn(&mut AppView, &mut Window, &mut Context<AppView>)>;
        let mut entries: Vec<(SharedString, Action)> = Vec::new();
        let mut push = |label: &str, f: Action| entries.push((SharedString::from(label.to_string()), f));
        // A new menu starts with "Add to playlist" folded.
        if view.last_menu_pos != Some(pos) {
            view.ctx_pl_open = false;
        }
        view.last_menu_pos = Some(pos);
        if let Some(panel) = super::ctxmenu::panel(view, &target, cx) {
            out.push(
                deferred(anchored().position(pos).anchor(Corner::TopLeft).snap_to_window().child(div().child(panel).with_animation(
                    SharedString::from(format!("cmenu-{:.0}-{:.0}", f32::from(pos.x), f32::from(pos.y))),
                    super::motion::animation(super::motion::FAST),
                    |d, t| d.opacity(t),
                )))
                .with_priority(2)
                .into_any_element(),
            );
            return out;
        }
        match target {
            MenuTarget::Sort(section) => {
                let (cur, desc) = view.sort_of(section);
                for k in super::sort::options(section).iter().copied() {
                    let mark = if k == cur {
                        if desc {
                            "▼ "
                        } else {
                            "▲ "
                        }
                    } else {
                        "    "
                    };
                    push(&format!("{mark}{}", k.label()), Box::new(move |v, _, cx| v.set_sort(section, k, cx)));
                }
            }
            MenuTarget::QueueItem(uid) => {
                push(
                    crate::i18n::t("play"),
                    Box::new(move |v, _, cx| {
                        if let Some(index) = v.queue.iter().position(|item| item.uid == uid) {
                            v.play_index(index, 0.0, false, cx);
                        }
                    }),
                );
                if view.now.as_ref().and_then(|now| view.queue.get(now.index)).is_none_or(|item| item.uid != uid) {
                    push(
                        crate::i18n::t("remove_from_queue"),
                        Box::new(move |v, _, cx| {
                            if let Some(index) = v.queue.iter().position(|item| item.uid == uid) {
                                v.remove_from_queue(index, cx);
                            }
                        }),
                    );
                }
            }
            MenuTarget::Album(_) | MenuTarget::Artist(_) | MenuTarget::Track(_) | MenuTarget::PlaylistTrack { .. } => {}
            MenuTarget::AddToPlaylist(tracks) => {
                let ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
                push(
                    crate::i18n::t("playlist.create_with_icon"),
                    Box::new(move |v, window, cx| v.open_dialog(super::playlists::Dialog::NewPlaylist { add: ids.clone() }, window, cx)),
                );
                for pl in view.own_playlists() {
                    let (uuid, t) = (pl.uuid.clone(), tracks.clone());
                    push(&pl.title, Box::new(move |v, _, cx| v.add_tracks_to_playlist(uuid.clone(), t.clone(), cx)));
                }
            }
        }
        let entries = std::rc::Rc::new(entries);
        let n = entries.len();
        out.push(
            deferred(
                anchored().position(pos).anchor(Corner::TopLeft).snap_to_window().child(
                    widgets::floating_panel(12.)
                        .id("menu")
                        .occlude()
                        .w(sc(240.))
                        .py(sc(6.))
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.menu = None;
                            cx.notify();
                        }))
                        .children((0..n).map(|i| {
                            let e = entries.clone();
                            let k = format!("mi-{i}");
                            let h = super::interact::hover(&k);
                            let el = div()
                                .id(("menu-item", i))
                                .px(sc(14.))
                                .py(sc(8.))
                                .cursor_pointer()
                                .bg(super::fx::with_alpha(theme::hover(), 0.9 * h))
                                .truncate()
                                .child(e[i].0.clone())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.menu = None;
                                    (e[i].1)(this, window, cx);
                                    cx.notify();
                                }));
                            super::interact::track(el, k, cx)
                        }))
                        // Menus drop in with a quick fade.
                        .with_animation(
                            SharedString::from(format!("menu-{:.0}-{:.0}", f32::from(pos.x), f32::from(pos.y))),
                            super::motion::animation(super::motion::FAST),
                            |d, t| d.opacity(t).mt(sc((1.0 - t) * -8.0)),
                        ),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        );
    }
    if let Some((msg, _)) = &view.toast {
        out.push(
            deferred(
                div()
                    .absolute()
                    .left(sc(0.))
                    .right(sc(0.))
                    .bottom(sc(theme::PLAYER_H + theme::FRAME_GAP + 16.))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .px(sc(18.))
                            .py(sc(10.))
                            .rounded(sc(8.))
                            .bg(theme::text())
                            .text_color(theme::bg())
                            .font_weight(FontWeight::SEMIBOLD)
                            .shadow_lg()
                            .child(msg.clone())
                            // Toasts rise from the player bar.
                            .with_animation(
                                SharedString::from(format!("toast-{}", msg.len())),
                                super::motion::animation(super::motion::PANEL),
                                |d, t| d.opacity(t).mb(sc((1.0 - t) * -24.0)),
                            ),
                    ),
            )
            .into_any_element(),
        );
    }
    out
}

pub fn login_screen(view: &mut AppView, window: &Window, cx: &mut Context<AppView>) -> AnyElement {
    let wide = f32::from(window.viewport_size().width) / super::zoom() >= 920.;
    let body: AnyElement = match &view.login {
        LoginState::Waiting(code) => div()
            .flex()
            .flex_col()
            .items_center()
            .gap(sc(16.))
            .child(div().text_color(theme::text_secondary()).child(crate::i18n::t("login.confirm_code")))
            .child(div().text_size(sc(40.)).font_weight(FontWeight::BOLD).text_color(theme::accent()).child(code.user_code.clone()))
            .child(div().text_size(sc(13.)).text_color(theme::text_dim()).child(code.verification_uri.clone()))
            .child({
                let uri = code.verification_uri.clone();
                pill(
                    "reopen",
                    crate::i18n::t("reopen_browser"),
                    None,
                    false,
                    move |_, _, _, _| {
                        let _ = open::that(&uri);
                    },
                    cx,
                )
            })
            .child(div().text_size(sc(13.)).text_color(theme::text_secondary()).child(crate::i18n::t("waiting_for_confirmation")))
            .into_any_element(),
        LoginState::Web(login) => {
            let url = login.url.clone();
            let busy = view.login_busy;
            div()
                .w(sc(620.))
                .max_w_full()
                .flex()
                .flex_col()
                .gap(sc(14.))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(crate::i18n::t("1_sign_in_on_the_tidal_page_opened_in_your_browser")))
                .child(div().text_color(theme::text_secondary()).text_size(sc(13.)).child(crate::i18n::t("login.copy_redirect_url")))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(crate::i18n::t("3_paste_it_here_and_press_enter")))
                .child(
                    div()
                        .h(sc(42.))
                        .px(sc(12.))
                        .flex()
                        .items_center()
                        .rounded(sc(6.))
                        .bg(theme::surface())
                        .border_1()
                        .border_color(theme::text_dim())
                        .child(view.login_input.clone()),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(sc(10.))
                        .child(pill(
                            "login-continue",
                            if busy { crate::i18n::t("checking") } else { crate::i18n::t("continue") },
                            None,
                            true,
                            |this, _, _, cx| {
                                let text = this.login_input.read(cx).text();
                                this.finish_web_login(text, cx)
                            },
                            cx,
                        ))
                        .child(pill(
                            "login-reopen",
                            crate::i18n::t("reopen_browser"),
                            None,
                            false,
                            move |_, _, _, _| {
                                let _ = open::that(&url);
                            },
                            cx,
                        ))
                        .child(pill(
                            "login-cancel",
                            crate::i18n::t("cancel"),
                            None,
                            false,
                            |this, _, _, cx| {
                                this.login = LoginState::Idle;
                                cx.notify();
                            },
                            cx,
                        )),
                )
                .into_any_element()
        }
        other => {
            let err = match other {
                LoginState::Failed(e) => Some(e.clone()),
                _ => None,
            };
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(sc(16.))
                .child(div().text_color(theme::text_secondary()).child(crate::i18n::t("sign_in_with_your_tidal_account_to_get_started")))
                .child(pill("login", crate::i18n::t("sign_in_to_tidal"), Some(Icon::Link), true, |this, _, _, cx| this.start_login(cx), cx))
                .child(pill(
                    "login-local",
                    crate::i18n::t("library.open_local"),
                    Some(Icon::Track),
                    false,
                    |this, _, _, cx| this.navigate(this.library_page(), cx),
                    cx,
                ))
                .when_some(err, |d, e| d.child(div().text_color(theme::danger()).text_size(sc(13.)).child(e)))
                .into_any_element()
        }
    };
    div()
        .size_full()
        .px(sc(20.))
        .bg(theme::bg())
        .text_color(theme::text())
        .font_family(theme::FONT)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .max_w_full()
                .flex()
                .items_center()
                .gap(sc(if wide { 32. } else { 18. }))
                .when(!wide, |d| d.flex_col())
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .when(wide, |d| d.flex_col())
                        .gap(sc(12.))
                        .child(super::widgets::brand_mark(if wide { 74. } else { 46. }))
                        .child(
                            div()
                                .when(wide, |d| d.text_center())
                                .child(div().text_size(sc(36.)).font_weight(FontWeight::BOLD).child(crate::i18n::t("app.name")))
                                .child(
                                    div()
                                        .text_size(sc(11.))
                                        .text_color(theme::text_secondary())
                                        .child(crate::i18n::t("unofficial_client_for_tidal")),
                                ),
                        ),
                )
                .child(widget_panel("login-panel").min_w(sc(0.)).max_w_full().p(sc(22.)).child(body)),
        )
        .into_any_element()
}
