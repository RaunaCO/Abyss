//! Page content (as virtualised list blocks), side panels, overlays, login.

use super::sc;
use gpui::{
    AnimationExt,
    anchored, deferred, div, list, point, prelude::*, px, relative, AnyElement, ClipboardItem, Context, Corner, Focusable, FontWeight,
    ScrollHandle, SharedString, Window,
};

use super::icons::Icon;
use super::widgets::{self, action, card, cover, fmt_hms, fmt_long, fmt_time, icon_button, message, pill, section_title, track_row, Cols, RowStyle};
use super::sort::SortKey;
use super::{theme, AppView, Load, LoginState, MenuTarget, Page};
use crate::config::Quality;
use crate::tidal::{Album, Artist, Item, Mix, Playlist, Row, RowKind, Track};

/// Which list a track row belongs to (to play the whole list from it).
#[derive(Clone, PartialEq)]
pub enum TrackSrc {
    Album(u64),
    Playlist(String),
    Mix(String),
    Favorites,
    ArtistTop(u64),
    Search(String),
}

#[derive(Clone)]
pub enum Block {
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
    /// Text tabs under the top of a page; the active one is underlined.
    Tabs(Vec<SharedString>, usize),
    Chips(SharedString, Vec<Item>),
    Links(Vec<Item>),
    Featured(SharedString, Vec<Item>),
    TrackHeader(RowStyle),
    Track(TrackSrc, usize, RowStyle),
    AlbumHeader(Album, u32),
    PlaylistHeader(Playlist, u32, u64),
    MixHeader(Mix, u32),
    ArtistHeader(Artist),
    TopHit(Item),
    Text(SharedString),
    Message(SharedString),
    Recent(Vec<String>),
    Settings,
    NewPlaylist,
    Spacer(f32),
}

impl AppView {
    pub fn tracks_of(&self, src: &TrackSrc) -> Option<&[Track]> {
        match src {
            TrackSrc::Album(id) => self.albums.get(id)?.ready().map(|(_, t)| t.as_slice()),
            TrackSrc::Playlist(u) => self.playlists.get(u)?.ready().map(|(_, t)| t.as_slice()),
            TrackSrc::Mix(m) => self.mixes.get(m)?.ready().map(|(_, t)| t.as_slice()),
            TrackSrc::Favorites => self.fav_tracks.ready().map(|t| t.as_slice()),
            TrackSrc::ArtistTop(id) => self.artists.get(id)?.ready().map(|a| a.top_tracks.as_slice()),
            TrackSrc::Search(q) => self.search.get(q)?.ready().map(|s| s.tracks.as_slice()),
        }
    }

    /// Width of the page column (centred, capped) for the current layout.
    pub fn column_width(&self, window: &Window) -> f32 {
        let mut w = f32::from(window.viewport_size().width) / super::zoom() - self.sidebar_width() - 48.0;
        if self.show_queue || self.show_lyrics {
            w -= theme::QUEUE_W;
        }
        w.min(theme::CONTENT_W).max(320.0)
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
        let cw = (w - theme::GAP * (cols as f32 - 1.0)) / cols as f32;
        (cols, cw.floor())
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
                b.push(Block::Message("Cargando…".into()));
            } else if let Some(e) = err {
                b.push(Block::Message(format!("No se pudo cargar: {e}").into()));
            }
        };
        let rows = |b: &mut Vec<Block>, rows: &[Row]| {
            for (i, r) in rows.iter().enumerate() {
                b.push(match r.kind {
                    RowKind::Cards => Block::Row(i, r.title.clone().into(), r.items.clone()),
                    RowKind::Chips => Block::Chips(r.title.clone().into(), r.items.clone()),
                    RowKind::Links => Block::Links(r.items.clone()),
                    RowKind::Featured => Block::Featured(r.title.clone().into(), r.items.clone()),
                });
            }
        };
        match &self.page {
            Page::Home | Page::Explore => {
                let (title, data) = if self.page == Page::Home { ("Inicio", &self.home) } else { ("Explorar", &self.explore) };
                if self.page == Page::Home {
                    b.push(Block::Tabs(vec!["Para ti".into()], 0));
                } else {
                    b.push(Block::Title(title.into(), None));
                }
                match data {
                    Load::Ready(r) if r.is_empty() => b.push(Block::Message("No hay nada que mostrar todavía.".into())),
                    Load::Ready(r) => rows(&mut b, r),
                    Load::Failed(e) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::Browse(path, title) => {
                b.push(Block::Title(title.clone().into(), None));
                match self.browse.get(path) {
                    Some(Load::Ready(r)) if r.is_empty() => b.push(Block::Message("No hay nada que mostrar todavía.".into())),
                    Some(Load::Ready(r)) => rows(&mut b, r),
                    Some(Load::Failed(e)) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::Search(q) => {
                if q.is_empty() {
                    b.push(Block::Title("Buscar".into(), None));
                    let recent = self.settings.lock().recent_searches.clone();
                    if !recent.is_empty() {
                        b.push(Block::Section("Búsquedas recientes".into()));
                        b.push(Block::Recent(recent));
                    }
                } else {
                    b.push(Block::Title(format!("Resultados para «{q}»").into(), None));
                    match self.search.get(q) {
                        Some(Load::Ready(r)) => {
                            if let Some(top) = &r.top {
                                b.push(Block::Section("Mejor resultado".into()));
                                b.push(Block::TopHit(top.clone()));
                            }
                            if !r.tracks.is_empty() {
                                b.push(Block::Section("Pistas".into()));
                                let style = RowStyle::new(true, true, false);
                                for i in 0..r.tracks.len().min(10) {
                                    b.push(Block::Track(TrackSrc::Search(q.clone()), i, style));
                                }
                            }
                            if !r.artists.is_empty() {
                                b.push(Block::Row(0, "Artistas".into(), r.artists.iter().cloned().map(Item::Artist).collect()));
                            }
                            if !r.albums.is_empty() {
                                b.push(Block::Row(1, "Álbumes".into(), r.albums.iter().cloned().map(Item::Album).collect()));
                            }
                            if !r.playlists.is_empty() {
                                b.push(Block::Row(2, "Playlists".into(), r.playlists.iter().cloned().map(Item::Playlist).collect()));
                            }
                            if r.top.is_none() && r.tracks.is_empty() && r.albums.is_empty() && r.artists.is_empty() {
                                b.push(Block::Message("Sin resultados.".into()));
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
                    b.push(Block::TrackHeader(style));
                    for (i, t) in tracks.iter().enumerate() {
                        if multi_disc && t.volume_number != disc {
                            disc = t.volume_number;
                            b.push(Block::Text(format!("Disco {disc}").into()));
                        }
                        b.push(Block::Track(TrackSrc::Album(*id), i, style));
                    }
                    if !album.copyright.is_empty() {
                        b.push(Block::Text(format!("{}  ·  {}", album.release_date, album.copyright).into()));
                    }
                    if let Some(Load::Ready((more, similar))) = self.album_extra.get(id) {
                        let artist = album.artist_names();
                        if !more.is_empty() {
                            b.push(Block::Row(0, format!("Más álbumes de {artist}").into(), more.iter().cloned().map(Item::Album).collect()));
                        }
                        if !similar.is_empty() {
                            b.push(Block::Row(1, "Artistas relacionados".into(), similar.iter().cloned().map(Item::Artist).collect()));
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
                    let style = RowStyle { date: true, bpm: true, ..RowStyle::new(true, true, false) };
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
                        b.push(Block::Section("Pistas principales".into()));
                        let style = RowStyle::new(true, true, false);
                        for i in 0..a.top_tracks.len() {
                            b.push(Block::Track(TrackSrc::ArtistTop(*id), i, style));
                        }
                    }
                    let mut row = 0;
                    for (title, list) in [("Álbumes", &a.albums), ("EP y sencillos", &a.singles), ("Aparece en", &a.appears_on)] {
                        if !list.is_empty() {
                            b.push(Block::Row(row, title.into(), list.iter().cloned().map(Item::Album).collect()));
                            row += 1;
                        }
                    }
                    if !a.similar.is_empty() {
                        b.push(Block::Row(row, "Artistas similares".into(), a.similar.iter().cloned().map(Item::Artist).collect()));
                    }
                    if let Some(bio) = &a.bio {
                        b.push(Block::Section("Biografía".into()));
                        b.push(Block::Text(bio.clone().into()));
                    }
                }
                Some(Load::Failed(e)) => state(&mut b, false, Some(e)),
                _ => state(&mut b, true, None),
            },
            Page::MyPlaylists => {
                b.push(Block::TitleSort("Playlists".into(), "playlists"));
                b.push(Block::NewPlaylist);
                b.push(Block::FilterBar);
                match &self.my_playlists {
                    Load::Ready(l) if l.is_empty() => b.push(Block::Message("Aún no tienes playlists.".into())),
                    Load::Ready(l) => grid(
                        &mut b,
                        l.iter()
                            .filter(|p| self.text_matches(&[&p.title, &p.creator]))
                            .cloned()
                            .map(Item::Playlist)
                            .collect(),
                    ),
                    Load::Failed(e) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::MyAlbums => {
                b.push(Block::TitleSort("Álbumes".into(), "albums"));
                b.push(Block::FilterBar);
                match &self.fav_albums {
                    Load::Ready(l) if l.is_empty() => b.push(Block::Message("Aún no has añadido álbumes.".into())),
                    Load::Ready(l) => {
                        let l: Vec<Album> = l.iter().filter(|a| self.text_matches(&[&a.title, &a.artist_names()])).cloned().collect();
                        for chunk in l.chunks(cols) {
                            b.push(Block::Cards(chunk.iter().cloned().map(Item::Album).collect()));
                            if let Some(col) = chunk.iter().position(|a| Some(a.id) == self.expanded_album) {
                                b.push(Block::AlbumInline(chunk[col].clone(), col));
                            }
                        }
                    }
                    Load::Failed(e) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::MyArtists => {
                b.push(Block::TitleSort("Artistas".into(), "artists"));
                b.push(Block::FilterBar);
                match &self.fav_artists {
                    Load::Ready(l) if l.is_empty() => b.push(Block::Message("Aún no sigues a ningún artista.".into())),
                    Load::Ready(l) => grid(&mut b, l.iter().filter(|a| self.text_matches(&[&a.name])).cloned().map(Item::Artist).collect()),
                    Load::Failed(e) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::MyMixes => {
                b.push(Block::TitleSort("Mixes y radios".into(), "mixes"));
                b.push(Block::FilterBar);
                match &self.my_mixes {
                    Load::Ready(l) if l.is_empty() => b.push(Block::Message("No hay mixes todavía.".into())),
                    Load::Ready(l) => grid(&mut b, l.iter().filter(|m| self.text_matches(&[&m.title])).cloned().map(Item::Mix).collect()),
                    Load::Failed(e) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::MyTracks => {
                b.push(Block::Title("Pistas".into(), None));
                match &self.fav_tracks {
                    Load::Ready(l) if l.is_empty() => b.push(Block::Message("Aún no tienes pistas favoritas.".into())),
                    Load::Ready(l) => {
                        let style = RowStyle { date: true, bpm: true, sortable: Some("tracks"), ..RowStyle::new(true, true, false) };
                        b.push(Block::Actions(TrackSrc::Favorites));
                        b.push(Block::FilterBar);
                        b.push(Block::TrackHeader(style));
                        for (i, t) in l.iter().enumerate() {
                            if self.track_matches(t) {
                                b.push(Block::Track(TrackSrc::Favorites, i, style));
                            }
                        }
                    }
                    Load::Failed(e) => state(&mut b, false, Some(e)),
                    _ => state(&mut b, true, None),
                }
            }
            Page::Settings => {
                b.push(Block::Title("Ajustes".into(), None));
                b.push(Block::Settings);
            }
        }
        b.push(Block::Spacer(32.));
        b
    }

    /// Each block sits in a centred column of at most `CONTENT_W` pixels; the
    /// list itself spans the whole area so the wheel works on the margins too.
    pub fn render_block(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let inner = self.render_block_inner(ix, window, cx);
        div()
            .w_full()
            .px(sc(24.))
            .flex()
            .justify_center()
            .child(div().w_full().max_w(sc(theme::CONTENT_W)).child(inner))
            .into_any_element()
    }

    fn render_block_inner(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(block) = self.current_blocks.get(ix).cloned() else {
            return div().into_any_element();
        };
        match block {
            Block::Title(t, sub) => div()
                .pt(sc(22.))
                .pb(sc(14.))
                .flex()
                .flex_col()
                .gap(sc(3.))
                .when_some(sub, |d, s| d.child(div().text_size(sc(10.5)).text_color(theme::text_dim()).font_weight(FontWeight::BOLD).child(s.to_uppercase())))
                .child(div().text_size(sc(22.)).font_weight(FontWeight::BOLD).child(t))
                .into_any_element(),
            Block::Section(t) => section_title(t),
            Block::SortBar(section) => super::sort::bar(self, section, cx),
            Block::AlbumInline(album, col) => {
                let cols = self.columns(window);
                super::album_inline::render(self, album, col, cols, cx)
            }
            Block::NewPlaylist => div()
                .pb(sc(8.))
                .flex()
                .child(pill("new-pl", "Nueva playlist", None, true, |this, _, window, cx| {
                    this.open_dialog(super::playlists::Dialog::NewPlaylist { add: vec![] }, window, cx)
                }, cx))
                .into_any_element(),
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
            Block::Row(row, title, items) => self.render_row(row, title, items, cx),
            Block::Cards(items) => {
                let (cols, cw) = self.grid_metrics(window);
                div()
                    .pb(sc(22.))
                    .flex()
                    .gap(sc(theme::GAP))
                    .children(items.iter().enumerate().map(|(i, it)| {
                        let c = card(SharedString::from(format!("g{ix}-{i}")), it, cw, true, cx);
                        super::fx::stagger(SharedString::from(format!("gs{}-{ix}-{i}", self.page_anim)), i, c)
                    }))
                    .children((items.len()..cols).map(|_| div().w(sc(cw)).flex_none()))
                    .into_any_element()
            }
            Block::TrackHeader(style) => {
                let (section, cur) = match style.sortable {
                    Some(sec) => (Some(sec), Some(self.sort_of(sec))),
                    None => (None, None),
                };
                let label = |text: &'static str, key: Option<SortKey>, cx: &mut Context<Self>| {
                    let active = key.is_some() && cur.map(|(k, _)| k) == key;
                    let arrow = match (active, cur) {
                        (true, Some((_, desc))) => if desc { " ▼" } else { " ▲" },
                        _ => "",
                    };
                    let sortable = section.is_some() && key.is_some();
                    div()
                        .id(SharedString::from(format!("th-{text}")))
                        .text_size(sc(9.5))
                        .font_weight(FontWeight::BOLD)
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
                    .px(sc(8.))
                    .h(sc(34.))
                    .flex()
                    .items_center()
                    .gap(sc(12.))
                    .child(div().w(sc(Cols::NUM)).flex_none().text_center().text_size(sc(9.5)).font_weight(FontWeight::BOLD).text_color(theme::text_dim()).child("#"))
                    .child(div().flex_1().min_w(sc(0.)).pl(sc(if style.cover { 42. } else { 0. })).child(label("TÍTULO", Some(SortKey::Name), cx)))
                    .child(div().w(relative(Cols::artist(style))).flex_none().child(label("ARTISTA", Some(SortKey::Artist), cx)))
                    .when(style.album, |d| d.child(div().w(relative(Cols::ALBUM)).flex_none().child(label("ÁLBUM", Some(SortKey::Album), cx))))
                    .when(style.date, |d| d.child(div().w(sc(Cols::DATE)).flex_none().child(label("AÑADIDA", Some(SortKey::Added), cx))))
                    .child(div().w(sc(Cols::TIME)).flex_none().child(label("TIEMPO", Some(SortKey::Duration), cx)))
                    .when(style.bpm, |d| d.child(div().w(sc(Cols::BPM)).flex_none().child(label("BPM", None, cx))))
                    .child(div().w(sc(Cols::ACTIONS)).flex_none())
                    .into_any_element()
            }
            Block::TitleSort(t, section) => div()
                .pt(sc(22.))
                .pb(sc(14.))
                .flex()
                .items_center()
                .gap(sc(8.))
                .child(div().text_size(sc(22.)).font_weight(FontWeight::BOLD).child(t))
                .child(icon_button(("sort-menu", ix), Icon::More, 18., theme::text_secondary(), move |this, ev: &gpui::ClickEvent, _, cx| {
                    this.menu = Some((ev.position(), MenuTarget::Sort(section)));
                    cx.notify();
                }, cx))
                .into_any_element(),
            Block::Actions(src) => {
                let (s1, s2) = (src.clone(), src.clone());
                div()
                    .pb(sc(16.))
                    .flex()
                    .items_center()
                    .gap(sc(10.))
                    .child(pill("act-play", "Reproducir", Some(Icon::Play), true, move |this, _, _, cx| {
                        if let Some(t) = this.tracks_of(&s1).map(|t| t.to_vec()) {
                            this.shuffle = false;
                            this.play_tracks(t, 0, cx)
                        }
                    }, cx))
                    .child(pill("act-shuffle", "Aleatorio", Some(Icon::Shuffle), false, move |this, _, _, cx| {
                        if let Some(t) = this.tracks_of(&s2).map(|t| t.to_vec()) {
                            this.play_shuffled(t, cx)
                        }
                    }, cx))
                    .into_any_element()
            }
            Block::Chips(title, items) => div()
                .pb(px(6.))
                .child(div().pt(px(26.)).pb(px(12.)).text_size(px(15.)).font_weight(FontWeight::BOLD).child(title))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(10.))
                        .children(items.into_iter().enumerate().map(|(i, it)| {
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
                                .rounded(px(8.))
                                .bg(theme::elevated())
                                .text_size(px(12.5))
                                .font_weight(FontWeight::BOLD)
                                .cursor_pointer()
                                .hover(|s| s.bg(theme::active()))
                                .active(|s| s.opacity(0.7))
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, cx| this.open_item(&target, cx)))
                        })),
                )
                .into_any_element(),
            Block::Links(items) => div()
                .pt(px(26.))
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
                        .w(relative(0.3333))
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
            Block::Featured(title, items) => div()
                .child(div().pt(px(6.)).pb(px(12.)).text_size(px(15.)).font_weight(FontWeight::BOLD).child(title))
                .child(
                    div()
                        .flex()
                        .gap(px(16.))
                        .overflow_hidden()
                        .children(items.into_iter().enumerate().map(|(i, it)| {
                            let Item::Promo(p) = it.clone() else { return div().into_any_element() };
                            let url = p.image_url();
                            let image = url.as_deref().and_then(|u| super::images::get(u, cx));
                            let card = div()
                                .id(SharedString::from(format!("promo-{ix}-{i}")))
                                .w(px(220.))
                                .flex_none()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .cursor_pointer()
                                .child(match image {
                                    Some(img) => gpui::img(img).w(px(220.)).h(px(160.)).rounded(px(4.)).object_fit(gpui::ObjectFit::Cover).into_any_element(),
                                    None => div().w(px(220.)).h(px(160.)).rounded(px(4.)).bg(theme::surface()).into_any_element(),
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
                        })),
                )
                .into_any_element(),
            Block::Tabs(tabs, active) => div()
                .pt(sc(26.))
                .mb(sc(4.))
                .flex()
                .gap(sc(20.))
                .border_b_1()
                .border_color(theme::border())
                .children(tabs.into_iter().enumerate().map(|(i, t)| {
                    div()
                        .pb(sc(9.))
                        .text_size(sc(12.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(if i == active { theme::text() } else { theme::text_dim() })
                        .when(i == active, |d| d.border_b_2().border_color(theme::text()))
                        .child(t)
                }))
                .into_any_element(),
            Block::FilterBar => div()
                .pb(sc(16.))
                .child(
                    div()
                        .h(sc(34.))
                        .px(sc(12.))
                        .flex()
                        .items_center()
                        .gap(sc(10.))
                        .rounded(sc(6.))
                        .border_1()
                        .border_color(theme::border())
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
                    style,
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
            Block::AlbumHeader(album, n) => self.album_header(album, n, cx),
            Block::PlaylistHeader(pl, n, total) => self.playlist_header(pl, n, total, cx),
            Block::MixHeader(mix, n) => {
                let id = mix.id.clone();
                let id2 = mix.id.clone();
                let tint = mix.image.clone().and_then(|u| super::images::color(&u, cx));
                self.page_tint = tint;
                header_tinted(
                    tint,
                    cover(mix.image.clone(), 178., false, cx),
                    mix.title.clone(),
                    div()
                        .flex()
                        .flex_col()
                        .gap(sc(8.))
                        .child(div().text_size(sc(12.5)).text_color(theme::text_secondary()).child(mix.subtitle.clone()))
                        .child(div().text_size(sc(9.5)).font_weight(FontWeight::BOLD).text_color(theme::text_secondary()).child(format!("{n} PISTAS")))
                        .into_any_element(),
                    String::new(),
                    vec![
                        pill("mix-play", "Reproducir", Some(Icon::Play), true, move |this, _, _, cx| {
                            if let Some(t) = this.tracks_of(&TrackSrc::Mix(id.clone())).map(|t| t.to_vec()) {
                                this.shuffle = false;
                                this.play_tracks(t, 0, cx)
                            }
                        }, cx)
                        .into_any_element(),
                        pill("mix-shuffle", "Aleatorio", Some(Icon::Shuffle), false, move |this, _, _, cx| {
                            if let Some(t) = this.tracks_of(&TrackSrc::Mix(id2.clone())).map(|t| t.to_vec()) {
                                this.play_shuffled(t, cx)
                            }
                        }, cx)
                        .into_any_element(),
                    ],
                    vec![],
                )
            }
            Block::ArtistHeader(artist) => {
                let id = artist.id;
                let tint = artist.picture(750).and_then(|u| super::images::color(&u, cx));
                self.page_tint = tint;
                header_tinted(
                    tint,
                    cover(artist.picture(750), 178., true, cx),
                    artist.name.clone(),
                    div().into_any_element(),
                    String::new(),
                    vec![
                        pill("artist-play", "Reproducir", Some(Icon::Play), true, move |this, _, _, cx| {
                            if let Some(t) = this.tracks_of(&TrackSrc::ArtistTop(id)).map(|t| t.to_vec()) {
                                this.shuffle = false;
                                this.play_tracks(t, 0, cx)
                            }
                        }, cx)
                        .into_any_element(),
                        pill("artist-radio", "Radio", Some(Icon::Radio), false, move |this, _, _, cx| {
                            if let Some(t) = this.tracks_of(&TrackSrc::ArtistTop(id)).and_then(|t| t.first()).cloned() {
                                this.start_radio(&t, cx)
                            }
                        }, cx)
                        .into_any_element(),
                        pill(
                            "artist-follow",
                            if self.fav_artist_ids.contains(&id) { "Siguiendo" } else { "Seguir" },
                            None,
                            false,
                            move |this, _, _, cx| this.toggle_follow_artist(id, cx),
                            cx,
                        )
                        .into_any_element(),
                    ],
                    vec![],
                )
            }
            Block::TopHit(item) => {
                let (title, sub, url, round) = match &item {
                    Item::Album(a) => (a.title.clone(), format!("Álbum · {}", a.artist_names()), a.cover(320), false),
                    Item::Artist(a) => (a.name.clone(), "Artista".to_string(), a.picture(320), true),
                    Item::Playlist(p) => (p.title.clone(), "Playlist".to_string(), p.cover(320), false),
                    Item::Track(t) => (t.display_title(), format!("Pista · {}", t.artist_names()), t.cover(320), false),
                    Item::Mix(m) => (m.title.clone(), "Mix".to_string(), m.image.clone(), false),
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
                            .rounded(sc(8.))
                            .bg(theme::surface())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::elevated()))
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
                        .rounded(sc(999.))
                        .bg(theme::surface())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::elevated()))
                        .child(q)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.search_input.update(cx, |i, cx| i.set_text(&qq, cx));
                            this.page = Page::Search(qq.clone());
                            this.page_changed(cx);
                        }))
                }))
                .into_any_element(),
            Block::Settings => self.settings_panel(cx),
        }
    }

    fn render_row(&mut self, row: usize, title: SharedString, items: Vec<Item>, cx: &mut Context<Self>) -> AnyElement {
        let handle = self.row_scroll.entry(row).or_insert_with(ScrollHandle::new).clone();
        let h1 = handle.clone();
        let h2 = handle.clone();
        let step = (theme::ROW_CARD_W + theme::GAP) * 4.0;
        div()
            .w_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .pt(sc(26.))
                    .pb(sc(12.))
                    .flex()
                    .items_center()
                    .child(div().flex_1().text_size(sc(15.)).font_weight(FontWeight::BOLD).child(title))
                    .child(icon_button(("row-l", row), Icon::Back, 15., theme::text_secondary(), move |_, _, _, cx| {
                        let o = h1.offset();
                        h1.set_offset(point((o.x + sc(step)).min(sc(0.)), o.y));
                        cx.notify();
                    }, cx))
                    .child(icon_button(("row-r", row), Icon::Forward, 15., theme::text_secondary(), move |_, _, _, cx| {
                        let o = h2.offset();
                        let max = h2.max_offset().width;
                        h2.set_offset(point((o.x - sc(step)).max(-max), o.y));
                        cx.notify();
                    }, cx)),
            )
            .child(
                div()
                    .id(("row", row))
                    .w_full()
                    .flex()
                    .gap(sc(theme::GAP))
                    .overflow_x_scroll()
                    .track_scroll(&handle)
                    .children(items.iter().enumerate().map(|(i, it)| {
                        let c = card(SharedString::from(format!("r{row}-{i}")), it, theme::ROW_CARD_W, false, cx);
                        super::fx::stagger(SharedString::from(format!("rs{}-{row}-{i}", self.page_anim)), i, c)
                    })),
            )
            .into_any_element()
    }

    fn album_header(&mut self, album: Album, n: u32, cx: &mut Context<Self>) -> AnyElement {
        let fav = self.fav_album_ids.contains(&album.id);
        let artist_id = album.artists.first().map(|a| a.id);
        let id = album.id;
        let a_fav = album.clone();
        let meta = format!("{n} PISTAS  ({})", fmt_hms(album.duration as u64));
        let chip = if album.tags.iter().any(|t| t == "HIRES_LOSSLESS") { Some(("MAX", theme::gold())) } else { Some(("HIGH", theme::high())) };
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
                        if let Some(a) = artist_id {
                            this.navigate(Page::Artist(a), cx)
                        }
                    })),
            )
            .child(div().text_size(sc(9.5)).font_weight(FontWeight::BOLD).text_color(theme::text_secondary()).child(meta))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(sc(8.))
                    .child(div().text_size(sc(9.5)).font_weight(FontWeight::BOLD).text_color(theme::text_secondary()).child(album.year().to_string()))
                    .children(chip.map(|(t, c)| {
                        div()
                            .px(sc(5.))
                            .py(sc(1.))
                            .rounded(sc(3.))
                            .bg(super::fx::with_alpha(c, 0.16))
                            .text_color(c)
                            .text_size(sc(8.5))
                            .font_weight(FontWeight::BOLD)
                            .child(t)
                    })),
            )
            .into_any_element();
        let tint = album.cover(640).and_then(|u| super::images::color(&u, cx));
        self.page_tint = tint;
        let share_id = album.id;
        header_tinted(
            tint,
            cover(album.cover(640), 178., false, cx),
            album.title.clone(),
            subtitle,
            String::new(),
            vec![
                pill("album-play", "Reproducir", Some(Icon::Play), true, move |this, _, _, cx| {
                    if let Some(t) = this.tracks_of(&TrackSrc::Album(id)).map(|t| t.to_vec()) {
                        this.shuffle = false;
                        this.play_tracks(t, 0, cx)
                    }
                }, cx)
                .into_any_element(),
                pill("album-shuffle", "Aleatorio", Some(Icon::Shuffle), false, move |this, _, _, cx| {
                    if let Some(t) = this.tracks_of(&TrackSrc::Album(id)).map(|t| t.to_vec()) {
                        this.play_shuffled(t, cx)
                    }
                }, cx)
                .into_any_element(),
            ],
            vec![
                action(
                    "album-fav",
                    if fav { Icon::HeartFilled } else { Icon::Heart },
                    if fav { "Añadido" } else { "Añadir" },
                    false,
                    move |this, _, _, cx| this.toggle_favorite_album(&a_fav, cx),
                    cx,
                ),
                action("album-share", Icon::Link, "Compartir", false, move |this, _, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(format!("https://tidal.com/browse/album/{share_id}")));
                    this.notify_toast("Enlace copiado", cx);
                }, cx),
                action("album-more", Icon::More, "Más", false, move |this, ev: &gpui::ClickEvent, _, cx| {
                    if let Some(t) = this.tracks_of(&TrackSrc::Album(id)).map(|t| t.to_vec()) {
                        this.menu = Some((ev.position(), MenuTarget::AddToPlaylist(t)));
                        cx.notify();
                    }
                }, cx),
            ],
        )
    }

    fn playlist_header(&mut self, pl: Playlist, n: u32, total: u64, cx: &mut Context<Self>) -> AnyElement {
        let mut right: Vec<AnyElement> = Vec::new();
        if self.is_own_playlist(&pl.uuid) {
            let (u1, u2, title) = (pl.uuid.clone(), pl.uuid.clone(), pl.title.clone());
            right.push(action("pl-edit", Icon::Edit, "Editar", false, move |this, _, window, cx| {
                this.open_dialog(super::playlists::Dialog::EditPlaylist { uuid: u1.clone() }, window, cx)
            }, cx));
            right.push(action("pl-delete", Icon::Close, "Eliminar", false, move |this, _, window, cx| {
                this.open_dialog(super::playlists::Dialog::DeletePlaylist { uuid: u2.clone(), title: title.clone() }, window, cx)
            }, cx));
        } else {
            let saved = self.is_saved_playlist(&pl.uuid);
            let p = pl.clone();
            right.push(action(
                "pl-save",
                if saved { Icon::HeartFilled } else { Icon::Heart },
                if saved { "Guardada" } else { "Guardar" },
                false,
                move |this, _, _, cx| this.toggle_saved_playlist(p.clone(), cx),
                cx,
            ));
        }
        let share = pl.uuid.clone();
        right.push(action("pl-share", Icon::Link, "Compartir", false, move |this, _, _, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(format!("https://tidal.com/browse/playlist/{share}")));
            this.notify_toast("Enlace copiado", cx);
        }, cx));
        let u3 = pl.uuid.clone();
        right.push(action("pl-more", Icon::More, "Más", false, move |this, ev: &gpui::ClickEvent, _, cx| {
            if let Some(t) = this.tracks_of(&TrackSrc::Playlist(u3.clone())).map(|t| t.to_vec()) {
                this.menu = Some((ev.position(), MenuTarget::AddToPlaylist(t)));
                cx.notify();
            }
        }, cx));
        let uuid = pl.uuid.clone();
        let uuid2 = pl.uuid.clone();
        let tint = pl.cover(640).and_then(|u| super::images::color(&u, cx));
        self.page_tint = tint;
        let mut subtitle = div().flex().flex_col().gap(sc(8.));
        if !pl.creator.is_empty() {
            subtitle = subtitle.child(div().text_size(sc(12.5)).font_weight(FontWeight::BOLD).child(pl.creator.clone()));
        }
        if !pl.description.is_empty() {
            subtitle = subtitle.child(div().max_w(sc(620.)).text_size(sc(12.)).text_color(theme::text_secondary()).line_clamp(2).child(pl.description.clone()));
        }
        subtitle = subtitle.child(
            div()
                .text_size(sc(9.5))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text_secondary())
                .child(format!("{n} PISTAS  ({})", fmt_hms(total))),
        );
        header_tinted(
            tint,
            self.playlist_cover(&pl, 178., cx),
            pl.title.clone(),
            subtitle.into_any_element(),
            String::new(),
            vec![
                pill("pl-play", "Reproducir", Some(Icon::Play), true, move |this, _, _, cx| {
                    if let Some(t) = this.tracks_of(&TrackSrc::Playlist(uuid.clone())).map(|t| t.to_vec()) {
                        this.shuffle = false;
                        this.play_tracks(t, 0, cx)
                    }
                }, cx)
                .into_any_element(),
                pill("pl-shuffle", "Aleatorio", Some(Icon::Shuffle), false, move |this, _, _, cx| {
                    if let Some(t) = this.tracks_of(&TrackSrc::Playlist(uuid2.clone())).map(|t| t.to_vec()) {
                        this.play_shuffled(t, cx)
                    }
                }, cx)
                .into_any_element(),
            ],
            right,
        )
    }

    fn settings_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.lock().clone();
        let quality_opt = |q: Quality, cx: &mut Context<Self>| {
            let selected = s.quality == q;
            div()
                .id(SharedString::from(format!("q-{}", q.label())))
                .p(sc(14.))
                .flex()
                .items_center()
                .gap(sc(14.))
                .rounded(sc(6.))
                .cursor_pointer()
                .border_1()
                .border_color(if selected { theme::accent() } else { theme::border() })
                .hover(|st| st.bg(theme::hover()))
                .child(
                    div()
                        .size(sc(18.))
                        .rounded(sc(9.))
                        .border_2()
                        .border_color(if selected { theme::accent() } else { theme::text_dim() })
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(selected, |d| d.child(div().size(sc(8.)).rounded(sc(4.)).bg(theme::accent()))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(q.label()))
                        .child(div().text_size(sc(13.)).text_color(theme::text_secondary()).child(q.description())),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_quality(q, cx)))
        };
        let toggle = |id: &'static str, label: &'static str, desc: &'static str, on: bool, f: fn(&mut AppView, &mut Context<AppView>), cx: &mut Context<Self>| {
            div()
                .id(id)
                .py(sc(12.))
                .flex()
                .items_center()
                .gap(sc(16.))
                .cursor_pointer()
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(label))
                        .child(div().text_size(sc(13.)).text_color(theme::text_secondary()).child(desc)),
                )
                .child(
                    div()
                        .w(sc(40.))
                        .h(sc(22.))
                        .rounded(sc(11.))
                        .p(sc(3.))
                        .bg(if on { theme::accent() } else { theme::elevated() })
                        .flex()
                        .when(on, |d| d.justify_end())
                        .child(div().size(sc(16.)).rounded(sc(8.)).bg(if on { theme::button_text() } else { theme::text_secondary() })),
                )
                .on_click(cx.listener(move |this, _, _, cx| f(this, cx)))
        };
        let devices = self.devices.clone();
        let current = s.output_device.clone();
        let section = |t: &'static str| div().pt(sc(28.)).pb(sc(10.)).text_size(sc(18.)).font_weight(FontWeight::BOLD).child(t);

        div()
            .max_w(sc(760.))
            .flex()
            .flex_col()
            .child(section("Calidad de streaming"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(sc(8.))
                    .child(quality_opt(Quality::Max, cx))
                    .child(quality_opt(Quality::High, cx))
                    .child(quality_opt(Quality::Low, cx)),
            )
            .child(section("Salida de audio"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(sc(2.))
                    .children(std::iter::once(None).chain(devices.into_iter().map(Some)).enumerate().map(|(i, d)| {
                        let selected = d == current;
                        let label = d.clone().unwrap_or_else(|| "Predeterminado del sistema".into());
                        let dd = d.clone();
                        div()
                            .id(("dev", i))
                            .px(sc(12.))
                            .py(sc(9.))
                            .rounded(sc(6.))
                            .flex()
                            .items_center()
                            .gap(sc(10.))
                            .cursor_pointer()
                            .when(selected, |x| x.bg(theme::active()))
                            .hover(|x| x.bg(theme::hover()))
                            .child(Icon::VolumeUp.el(16., if selected { theme::accent() } else { theme::text_dim() }))
                            .child(div().truncate().child(label))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let s = this.settings.lock().clone();
                                this.set_output(dd.clone(), s.exclusive_mode, s.force_volume, cx)
                            }))
                    })),
            )
            .child(toggle(
                "exclusive",
                "Modo exclusivo",
                "Toma el control del dispositivo (WASAPI) y envía el audio a su frecuencia original, sin pasar por el mezclador de Windows.",
                s.exclusive_mode,
                |this, cx| {
                    let s = this.settings.lock().clone();
                    this.set_output(s.output_device, !s.exclusive_mode, s.force_volume, cx)
                },
                cx,
            ))
            .child(toggle(
                "forcevol",
                "Forzar volumen",
                "En modo exclusivo, fija el volumen al 100 % para una salida bit-perfect (controla el volumen en tu DAC o amplificador).",
                s.force_volume,
                |this, cx| {
                    let s = this.settings.lock().clone();
                    this.set_output(s.output_device, s.exclusive_mode, !s.force_volume, cx)
                },
                cx,
            ))
            .child(section("Reproducción"))
            .child(toggle(
                "normalize",
                "Normalización de volumen",
                "Iguala el volumen entre pistas con los datos de ganancia de TIDAL, sin saturar. Se desactiva sola con «Forzar volumen» en modo exclusivo.",
                s.normalize,
                |this, cx| this.toggle_setting("normalize", cx),
                cx,
            ))
            .child(toggle(
                "autoplay",
                "Reproducción automática",
                "Cuando termina la cola, sigue con canciones parecidas a la última.",
                s.autoplay,
                |this, cx| this.toggle_setting("autoplay", cx),
                cx,
            ))
            .child(
                div()
                    .py(sc(8.))
                    .text_size(sc(13.))
                    .text_color(theme::text_secondary())
                    .child(if self.snap.output.is_empty() {
                        "Salida actual: se abrirá al reproducir".to_string()
                    } else {
                        format!("Salida actual: {}", self.snap.output)
                    }),
            )
            .child(section("Cuenta"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(sc(16.))
                    .child(div().flex_1().child(s.username.clone().unwrap_or_default()))
                    .child(pill("signout", "Cerrar sesión", None, false, |this, _, _, cx| this.sign_out(cx), cx)),
            )
            .into_any_element()
    }
}

/// Page header like the original: a 178 px cover with the title and details
/// beside it, then a row with the main buttons on the left and the labelled
/// actions (add, share, more...) on the right.
fn header_tinted(
    color: Option<gpui::Rgba>,
    cover: AnyElement,
    title: String,
    subtitle: AnyElement,
    _meta: String,
    left: Vec<AnyElement>,
    right: Vec<AnyElement>,
) -> AnyElement {
    use gpui::{Animation, AnimationExt};
    div()
        .pt(sc(22.))
        .pb(sc(14.))
        .flex()
        .flex_col()
        .gap(sc(24.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(sc(24.))
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .rounded(sc(4.))
                        .map(|d| match color {
                            Some(c) => d.shadow(super::fx::glow(c, 0.7, 50.)),
                            None => d.shadow_lg(),
                        })
                        .child(cover)
                        .with_animation(
                            ("header-cover", title.len()),
                            Animation::new(std::time::Duration::from_millis(500)).with_easing(super::motion::ease_out),
                            |d, t| d.opacity(t).top(sc((1.0 - t) * 12.0)),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(sc(0.))
                        .flex()
                        .flex_col()
                        .gap(sc(10.))
                        .child(div().text_size(sc(26.)).line_height(sc(30.)).font_weight(FontWeight::BOLD).line_clamp(2).child(title))
                        .child(subtitle),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(sc(10.))
                .children(left)
                .child(div().flex_1())
                .child(div().flex().items_center().gap(sc(4.)).children(right)),
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
                    d.child(
                        div()
                            .absolute()
                            .left(sc(0.))
                            .right(sc(0.))
                            .top(gpui::px(-off))
                            .h(sc(380.))
                            .bg(super::fx::tint(c, 0.42)),
                    )
                })
                .child(
                    list(view.list.clone(), cx.processor(|this, ix: usize, window, cx| this.render_block(ix, window, cx)))
                        .flex_1()
                        .min_h(sc(0.)),
                )
                .child(super::smooth::wheel_catcher(cx.entity()))
                // Page transition: fade in while sliding up a little.
                .with_animation(
                    ("page", view.page_anim),
                    Animation::new(std::time::Duration::from_millis(200)).with_easing(super::motion::ease_out),
                    |el, t| el.opacity(t).top(sc((1.0 - t) * 14.0)),
                ),
        )
        .into_any_element()
}

/// Right panel: play queue or lyrics.
pub fn side_panel(view: &mut AppView, cx: &mut Context<AppView>) -> AnyElement {
    let body: AnyElement = if view.show_lyrics {
        lyrics_panel(view, cx)
    } else {
        queue_panel(view, cx)
    };
    div()
        .w(sc(theme::QUEUE_W))
        .flex_none()
        .h_full()
        .flex()
        .flex_col()
        .bg(theme::sidebar())
        .border_l_1()
        .border_color(theme::border())
        .child(
            div()
                .h(sc(64.))
                .px(sc(20.))
                .flex()
                .items_center()
                .child(div().flex_1().text_size(sc(18.)).font_weight(FontWeight::BOLD).child(if view.show_lyrics { "Letra" } else { "Cola de reproducción" }))
                .child(icon_button("close-panel", Icon::Close, 20., theme::text_secondary(), |this, _, _, cx| {
                    this.show_queue = false;
                    this.show_lyrics = false;
                    cx.notify();
                }, cx)),
        )
        .child(body)
        .into_any_element()
}

fn queue_panel(view: &mut AppView, cx: &mut Context<AppView>) -> AnyElement {
    let cur = view.now.as_ref().map(|n| n.index);
    let items: Vec<(usize, crate::ui::QItem)> = view.queue.iter().cloned().enumerate().filter(|(i, _)| cur.map(|c| *i >= c).unwrap_or(true)).collect();
    div()
        .id("queue-list")
        .flex_1()
        .min_h(sc(0.))
        .overflow_y_scroll()
        .px(sc(12.))
        .children(items.into_iter().map(|(i, q)| {
            let current = Some(i) == cur;
            let t = q.track.clone();
            div()
                .id(("q", q.uid as usize))
                .h(sc(56.))
                .px(sc(8.))
                .flex()
                .items_center()
                .gap(sc(12.))
                .rounded(sc(4.))
                .cursor_pointer()
                .when(current, |d| d.bg(theme::active()))
                .hover(|s| s.bg(theme::hover()))
                .child(cover(t.cover(80), 40., false, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w(sc(0.))
                        .flex()
                        .flex_col()
                        .child(div().truncate().text_color(if current { theme::accent() } else { theme::text() }).child(t.display_title()))
                        .child(div().truncate().text_size(sc(12.)).text_color(theme::text_secondary()).child(t.artist_names())),
                )
                .child(div().text_size(sc(12.)).text_color(theme::text_dim()).child(fmt_time(t.duration as f64)))
                .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                    if ev.click_count() >= 2 {
                        this.play_index(i, 0.0, false, cx)
                    }
                }))
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                        this.menu = Some((ev.position, MenuTarget::QueueItem(i)));
                        cx.notify();
                    }),
                )
        }))
        .into_any_element()
}

fn lyrics_panel(view: &mut AppView, cx: &mut Context<AppView>) -> AnyElement {
    let Some(now) = &view.now else { return message("No se está reproduciendo nada.") };
    let pos = view.display_position();
    let id = now.track.id;
    let provider = |p: &str| {
        div()
            .pt(sc(28.))
            .text_size(sc(11.))
            .text_color(theme::text_dim())
            .child(format!("Letra: {p}"))
    };
    match view.lyrics.get(&id) {
        Some(Load::Ready(l)) if !l.synced.is_empty() => {
            let l = l.clone();
            // Timing correction: positive = lyrics later.
            let offset = view.lyrics_offset.get(&id).copied().unwrap_or(0.0);
            let current = l.synced.iter().rposition(|(t, _)| *t + offset <= pos + 0.2);
            let follow = view.lyrics_follow;
            // Follow the song: bring the current line into view when it changes.
            if current != view.lyrics_line {
                view.lyrics_line = current;
                if follow {
                    if let Some(c) = current {
                        view.lyrics_scroll.scroll_to_item((c + 3).min(l.synced.len() - 1));
                    }
                }
            }
            let step = |label: &'static str, delta: f64, cx: &mut Context<AppView>| {
                div()
                    .id(label)
                    .px(sc(10.))
                    .py(sc(4.))
                    .rounded(sc(999.))
                    .bg(theme::surface())
                    .text_size(sc(12.))
                    .text_color(theme::text_secondary())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::elevated()).text_color(theme::text()))
                    .active(|s| s.opacity(0.6))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let o = this.lyrics_offset.entry(id).or_insert(0.0);
                        *o = ((*o + delta) * 10.0).round() / 10.0;
                        cx.notify();
                    }))
            };
            let toolbar = div()
                .px(sc(20.))
                .pb(sc(10.))
                .flex()
                .items_center()
                .gap(sc(6.))
                .child(
                    div()
                        .id("lyrics-sync")
                        .px(sc(14.))
                        .py(sc(5.))
                        .rounded(sc(999.))
                        .text_size(sc(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .bg(if follow { theme::surface() } else { theme::accent() })
                        .text_color(if follow { theme::text_dim() } else { theme::button_text() })
                        .hover(|s| s.opacity(0.85))
                        .active(|s| s.opacity(0.6))
                        .child(if follow { "Sincronizada" } else { "Sincronizar" })
                        .on_click(cx.listener(|this, _, _, cx| {
                            // Back to following the song from the current line.
                            this.lyrics_follow = true;
                            if let Some(c) = this.lyrics_line {
                                this.lyrics_scroll.scroll_to_item(c + 3);
                            }
                            cx.notify();
                        })),
                )
                .child(div().flex_1())
                .child(step("−0.5 s", -0.5, cx))
                .child(
                    div()
                        .w(sc(52.))
                        .text_center()
                        .text_size(sc(12.))
                        .text_color(if offset == 0.0 { theme::text_dim() } else { theme::accent() })
                        .child(format!("{:+.1} s", offset)),
                )
                .child(step("+0.5 s", 0.5, cx));
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
                    let at = *t + offset;
                    div()
                        .id(("lyric", i))
                        .px(sc(8.))
                        .py(sc(5.))
                        .rounded(sc(6.))
                        .cursor_pointer()
                        .text_size(sc(20.))
                        .line_height(sc(28.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(if active {
                            theme::text()
                        } else if past {
                            theme::text_secondary()
                        } else {
                            theme::text_dim()
                        })
                        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                        .child(if line.is_empty() { "♪".to_string() } else { line.clone() })
                        // Click a line to jump there (and follow again).
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.lyrics_follow = true;
                            this.seek(at.max(0.0), cx)
                        }))
                }))
                .child(provider(&l.provider));
            div().flex_1().min_h(sc(0.)).flex().flex_col().child(toolbar).child(lines).into_any_element()
        }
        Some(Load::Ready(l)) if !l.text.is_empty() => div()
            .id("lyrics")
            .flex_1()
            .min_h(sc(0.))
            .overflow_y_scroll()
            .px(sc(24.))
            .text_size(sc(16.))
            .line_height(sc(26.))
            .child(l.text.clone())
            .child(provider(&format!("{} (sin sincronizar)", l.provider)))
            .into_any_element(),
        Some(Load::Loading) => message("Buscando letra…"),
        _ => message("Ningún proveedor tiene letra para esta pista."),
    }
}

/// Context menu, toast and quality popover.
pub fn overlays(view: &mut AppView, _window: &mut Window, cx: &mut Context<AppView>) -> Vec<AnyElement> {
    let mut out = Vec::new();
    if let Some((pos, target)) = view.menu.clone() {
        type Action = Box<dyn Fn(&mut AppView, &mut Window, &mut Context<AppView>)>;
        let mut entries: Vec<(SharedString, Action)> = Vec::new();
        let mut push = |label: &str, f: Action| entries.push((SharedString::from(label.to_string()), f));
        let track_entries = |t: Track, push: &mut dyn FnMut(&str, Action), fav: bool| {
            let (a, b, c, d, e, f, g, h) = (t.clone(), t.clone(), t.clone(), t.clone(), t.clone(), t.clone(), t.clone(), t.clone());
            push("Reproducir a continuación", Box::new(move |v, _, cx| v.add_to_queue(a.clone(), true, cx)));
            push("Añadir a la cola", Box::new(move |v, _, cx| v.add_to_queue(b.clone(), false, cx)));
            push("Añadir a playlist…", Box::new(move |v, _, cx| {
                // Re-open the menu at the same spot with the playlist list.
                if let Some(pos) = v.last_menu_pos {
                    v.menu = Some((pos, MenuTarget::AddToPlaylist(vec![h.clone()])));
                    cx.notify();
                }
            }));
            push("Iniciar radio de la pista", Box::new(move |v, _, cx| v.start_radio(&c, cx)));
            push(
                if fav { "Quitar de Mi colección" } else { "Añadir a Mi colección" },
                Box::new(move |v, _, cx| v.toggle_favorite(&d, cx)),
            );
            if e.album.is_some() {
                push("Ir al álbum", Box::new(move |v, _, cx| {
                    if let Some(al) = &e.album {
                        v.navigate(Page::Album(al.id), cx)
                    }
                }));
            }
            if !f.artists.is_empty() {
                push("Ir al artista", Box::new(move |v, _, cx| v.navigate(Page::Artist(f.artists[0].id), cx)));
            }
            push("Copiar enlace", Box::new(move |v, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(format!("https://tidal.com/browse/track/{}", g.id)));
                v.notify_toast("Enlace copiado", cx);
            }));
        };
        view.last_menu_pos = Some(pos);
        match target {
            MenuTarget::Track(t) => {
                let fav = view.fav_ids.contains(&t.id);
                track_entries(t, &mut push, fav);
            }
            MenuTarget::PlaylistTrack { uuid, index, track } => {
                let fav = view.fav_ids.contains(&track.id);
                track_entries(track, &mut push, fav);
                push("Quitar de esta playlist", Box::new(move |v, _, cx| v.remove_playlist_track(uuid.clone(), index, cx)));
            }
            MenuTarget::Sort(section) => {
                let (cur, desc) = view.sort_of(section);
                for k in super::sort::options(section).iter().copied() {
                    let mark = if k == cur { if desc { "▼ " } else { "▲ " } } else { "    " };
                    push(&format!("{mark}{}", k.label()), Box::new(move |v, _, cx| v.set_sort(section, k, cx)));
                }
            }
            MenuTarget::QueueItem(i) => {
                push("Reproducir", Box::new(move |v, _, cx| v.play_index(i, 0.0, false, cx)));
                push("Quitar de la cola", Box::new(move |v, _, cx| v.remove_from_queue(i, cx)));
            }
            MenuTarget::AddToPlaylist(tracks) => {
                let ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
                push("+ Nueva playlist…", Box::new(move |v, window, cx| {
                    v.open_dialog(super::playlists::Dialog::NewPlaylist { add: ids.clone() }, window, cx)
                }));
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
                    div()
                        .id("menu")
                        .occlude()
                        .w(sc(240.))
                        .py(sc(6.))
                        .rounded(sc(8.))
                        .bg(theme::elevated())
                        .border_1()
                        .border_color(theme::border())
                        .shadow_lg()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.menu = None;
                            cx.notify();
                        }))
                        .children((0..n).map(|i| {
                            let e = entries.clone();
                            div()
                                .id(("menu-item", i))
                                .px(sc(14.))
                                .py(sc(8.))
                                .cursor_pointer()
                                .hover(|s| s.bg(theme::hover()))
                                .truncate()
                                .child(e[i].0.clone())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.menu = None;
                                    (e[i].1)(this, window, cx);
                                    cx.notify();
                                }))
                        }))
                        // Menus drop in with a quick fade.
                        .with_animation(
                            SharedString::from(format!("menu-{:.0}-{:.0}", f32::from(pos.x), f32::from(pos.y))),
                            gpui::Animation::new(std::time::Duration::from_millis(160)).with_easing(super::motion::ease_out),
                            |d, t| d.opacity(t).mt(sc((1.0 - t) * -8.0)),
                        ),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        );
    }
    if view.show_quality_info {
        let info = view.now.as_ref().and_then(|n| n.info.clone());
        let src = match (&info, view.snap.source) {
            (Some(i), Some((bits, rate))) if !i.codec.eq_ignore_ascii_case("aac") => {
                format!("{} {}-bit / {} kHz", i.codec.to_uppercase(), bits, crate::audio::output::fmt_khz(rate))
            }
            (Some(i), _) => i.detail(),
            _ => "—".into(),
        };
        let path = if view.snap.output.contains("exclusivo") {
            if view.settings.lock().force_volume { "Bit-perfect (sin procesar)" } else { "Exclusivo con control de volumen" }
        } else if view.snap.source.map(|s| s.1) == Some(view.snap.output_rate) {
            "Sin remuestreo"
        } else {
            "Remuestreo sinc de alta calidad"
        };
        out.push(
            deferred(
                div()
                    .absolute()
                    .right(sc(24.))
                    .bottom(sc(theme::PLAYER_H + 12.))
                    .child(
                        div()
                            .id("quality-pop")
                            .occlude()
                            .w(sc(320.))
                            .p(sc(16.))
                            .rounded(sc(8.))
                            .bg(theme::elevated())
                            .border_1()
                            .border_color(theme::border())
                            .shadow_lg()
                            .flex()
                            .flex_col()
                            .gap(sc(10.))
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.show_quality_info = false;
                                cx.notify();
                            }))
                            .child(div().font_weight(FontWeight::BOLD).child("Calidad de audio"))
                            .child(kv("Origen", src))
                            .child(kv("Salida", if view.snap.output.is_empty() { "—".into() } else { view.snap.output.clone() }))
                            .child(kv("Proceso", path.into()))
                            .child(kv("Ajuste", view.settings.lock().quality.description().into())),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        );
    }
    if let Some((msg, _)) = &view.toast {
        out.push(
            deferred(
                div().absolute().left(sc(0.)).right(sc(0.)).bottom(sc(theme::PLAYER_H + 16.)).flex().justify_center().child(
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
                            gpui::Animation::new(std::time::Duration::from_millis(280)).with_easing(super::motion::ease_out),
                            |d, t| d.opacity(t).mb(sc((1.0 - t) * -24.0)),
                        ),
                ),
            )
            .into_any_element(),
        );
    }
    out
}

fn kv(k: &'static str, v: String) -> AnyElement {
    div()
        .flex()
        .gap(sc(12.))
        .text_size(sc(13.))
        .child(div().w(sc(70.)).flex_none().text_color(theme::text_secondary()).child(k))
        .child(div().flex_1().child(v))
        .into_any_element()
}

pub fn login_screen(view: &mut AppView, cx: &mut Context<AppView>) -> AnyElement {
    let body: AnyElement = match &view.login {
        LoginState::Waiting(code) => div()
            .flex()
            .flex_col()
            .items_center()
            .gap(sc(16.))
            .child(div().text_color(theme::text_secondary()).child("Confirma este código en la página que se abrió en tu navegador:"))
            .child(div().text_size(sc(40.)).font_weight(FontWeight::BOLD).text_color(theme::accent()).child(code.user_code.clone()))
            .child(div().text_size(sc(13.)).text_color(theme::text_dim()).child(code.verification_uri.clone()))
            .child({
                let uri = code.verification_uri.clone();
                pill("reopen", "Abrir de nuevo el navegador", None, false, move |_, _, _, _| {
                    let _ = open::that(&uri);
                }, cx)
            })
            .child(div().text_size(sc(13.)).text_color(theme::text_secondary()).child("Esperando confirmación…"))
            .into_any_element(),
        LoginState::Web(login) => {
            let url = login.url.clone();
            let busy = view.login_busy;
            div()
                .w(sc(620.))
                .flex()
                .flex_col()
                .gap(sc(14.))
                .child(div().font_weight(FontWeight::SEMIBOLD).child("1. Inicia sesión en la página de TIDAL que se abrió en tu navegador."))
                .child(div().text_color(theme::text_secondary()).text_size(sc(13.)).child(
                    "2. Al terminar, TIDAL te lleva a una página que no carga (es normal). Copia la dirección completa de la barra del navegador.",
                ))
                .child(div().font_weight(FontWeight::SEMIBOLD).child("3. Pégala aquí y pulsa Enter:"))
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
                        .gap(sc(10.))
                        .child(pill("login-continue", if busy { "Comprobando…" } else { "Continuar" }, None, true, |this, _, _, cx| {
                            let text = this.login_input.read(cx).text();
                            this.finish_web_login(text, cx)
                        }, cx))
                        .child(pill("login-reopen", "Abrir de nuevo el navegador", None, false, move |_, _, _, _| {
                            let _ = open::that(&url);
                        }, cx))
                        .child(pill("login-cancel", "Cancelar", None, false, |this, _, _, cx| {
                            this.login = LoginState::Idle;
                            cx.notify();
                        }, cx)),
                )
                .into_any_element()
        }
        other => {
            let err = if let LoginState::Failed(e) = other { Some(e.clone()) } else { None };
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(sc(16.))
                .child(div().text_color(theme::text_secondary()).child("Inicia sesión con tu cuenta de TIDAL para empezar."))
                .child(pill("login", "Iniciar sesión", None, true, |this, _, _, cx| this.start_login(cx), cx))
                .when_some(err, |d, e| d.child(div().text_color(theme::danger()).text_size(sc(13.)).child(e)))
                .into_any_element()
        }
    };
    div()
        .size_full()
        .bg(theme::bg())
        .text_color(theme::text())
        .font_family(theme::FONT)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(sc(28.))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(sc(6.))
                .child(gpui::img(super::images::logo()).size(sc(96.)).rounded(sc(22.)).mb(sc(10.)))
                .child(div().text_size(sc(34.)).font_weight(FontWeight::BOLD).child("TIDE"))
                .child(div().text_size(sc(13.)).text_color(theme::text_dim()).child("Cliente nativo no oficial · Rust + GPUI")),
        )
        .child(body)
        .into_any_element()
}
