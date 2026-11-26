//! Navigation, page data, session, search, sorting, the local library and
//! playlist editing. Same rules as the GPUI reference (`src/ui/mod.rs`,
//! `search_menu.rs`, `sort.rs`, `local.rs`, `library.rs`, `playlists.rs`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::Task;

use super::{blocking, App, Callback, Dialog, LoginState, Message, RemoveKind};
use crate::shared::{into_load, Load, Page};
use crate::tidal::*;

/// Pause after typing before the quick search asks TIDAL.
const DEBOUNCE: Duration = Duration::from_millis(220);

// ------------------------------------------------------------------ sorting

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Added,
    Name,
    Artist,
    Album,
    Year,
    Duration,
    Count,
    Rating,
}

impl SortKey {
    /// Shorter label for narrow layouts (the Library selector).
    pub fn short_label(self) -> &'static str {
        match self {
            SortKey::Added => crate::i18n::t("sort.added"),
            SortKey::Name => crate::i18n::t("a_z"),
            SortKey::Year => crate::i18n::t("year"),
            SortKey::Count => crate::i18n::t("tracks"),
            other => other.label(),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Added => crate::i18n::t("date_added"),
            SortKey::Name => crate::i18n::t("alphabetical"),
            SortKey::Artist => crate::i18n::t("artist"),
            SortKey::Album => crate::i18n::t("album"),
            SortKey::Year => crate::i18n::t("release_year"),
            SortKey::Duration => crate::i18n::t("duration"),
            SortKey::Count => crate::i18n::t("track_count"),
            SortKey::Rating => crate::i18n::t("rating.label"),
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            SortKey::Added => "added",
            SortKey::Name => "name",
            SortKey::Artist => "artist",
            SortKey::Album => "album",
            SortKey::Year => "year",
            SortKey::Duration => "duration",
            SortKey::Count => "count",
            SortKey::Rating => "rating",
        }
    }
    pub fn from_code(s: &str) -> Self {
        match s {
            "name" => SortKey::Name,
            "artist" => SortKey::Artist,
            "album" => SortKey::Album,
            "year" => SortKey::Year,
            "duration" => SortKey::Duration,
            "count" => SortKey::Count,
            "rating" => SortKey::Rating,
            _ => SortKey::Added,
        }
    }
}

pub fn sort_options(section: &str) -> &'static [SortKey] {
    match section {
        "tracks" => &[SortKey::Added, SortKey::Name, SortKey::Artist, SortKey::Album, SortKey::Duration, SortKey::Rating],
        "albums" => &[SortKey::Added, SortKey::Name, SortKey::Artist, SortKey::Year, SortKey::Rating],
        "artists" => &[SortKey::Added, SortKey::Name],
        "playlists" => &[SortKey::Added, SortKey::Name, SortKey::Count],
        _ => &[SortKey::Added, SortKey::Name],
    }
}

fn text_key(s: &str) -> String {
    let s = s.trim().to_lowercase();
    s.strip_prefix("the ").map(str::to_string).unwrap_or(s)
}

/// Remember the API ("date added") order of a list the first time each item is seen.
fn rank<T>(ranks: &mut HashMap<String, i64>, list: &[T], id: impl Fn(&T) -> String) {
    let fresh: Vec<String> = list.iter().map(&id).filter(|k| !ranks.contains_key(k)).collect();
    if fresh.is_empty() {
        return;
    }
    if ranks.is_empty() {
        for (i, k) in list.iter().map(&id).enumerate() {
            ranks.insert(k, i as i64);
        }
    } else {
        // Items added later (e.g. a new favourite) count as the newest.
        let min = ranks.values().copied().min().unwrap_or(0);
        for (i, k) in fresh.into_iter().enumerate() {
            ranks.insert(k, min - 1 - i as i64);
        }
    }
}

/// The Library category of `page`, if it is one.
pub fn lib_section(page: &Page) -> Option<&'static str> {
    match page {
        Page::MyTracks => Some("tracks"),
        Page::MyAlbums => Some("albums"),
        Page::MyArtists => Some("artists"),
        _ => None,
    }
}

pub fn lib_page_of(section: &str) -> Page {
    match section {
        "albums" => Page::MyAlbums,
        "artists" => Page::MyArtists,
        _ => Page::MyTracks,
    }
}

fn playlist_is_editable(playlist: &Playlist, uid: Option<u64>) -> bool {
    Some(playlist.creator_id) == uid || playlist.kind == "USER"
}

fn library_indices<T>(items: &[T], query: &str, matches: impl Fn(&T, &str) -> bool) -> Vec<usize> {
    if query.is_empty() {
        return (0..items.len()).collect();
    }
    items.iter().enumerate().filter_map(|(i, item)| matches(item, query).then_some(i)).collect()
}

fn remove_failure<K: Eq + std::hash::Hash, T>(cache: &mut HashMap<K, Load<T>>, key: &K) {
    if matches!(cache.get(key), Some(Load::Failed(_))) {
        cache.remove(key);
    }
}

impl App {
    // -------------------------------------------------------------- session

    pub(super) fn after_login(&mut self) -> Task<Message> {
        self.search.clear();
        let session = self.fetch(|t| t.load_session(), |_, _| Task::none());
        Task::batch([session, self.load_page_data(), self.load_library()])
    }

    pub(super) fn reset_data(&mut self) {
        self.data_epoch = self.data_epoch.wrapping_add(1);
        self.home = Load::Idle;
        self.home_extra.clear();
        self.home_loaded = None;
        self.explore = Load::Idle;
        self.browse.clear();
        self.albums.clear();
        self.album_extra.clear();
        self.playlists.clear();
        self.artists.clear();
        self.mixes.clear();
        self.search.clear();
        self.search_gen = self.search_gen.wrapping_add(1);
        self.search_open = false;
        self.search_query.clear();
        self.sort_ranks.clear();
        self.fav_ids.clear();
        self.fav_artist_ids.clear();
        self.fav_album_ids.clear();
        self.fav_tracks = Load::Idle;
        self.fav_albums = Load::Idle;
        self.fav_artists = Load::Idle;
        self.my_playlists = Load::Idle;
        self.my_mixes = Load::Idle;
        self.local_changed();
    }

    pub fn start_login(&mut self) -> Task<Message> {
        if matches!(self.login, LoginState::Starting | LoginState::Waiting(_)) {
            return Task::none();
        }
        self.login = LoginState::Starting;
        let attempt = self.tidal.begin_session_attempt();
        self.fetch(
            move |t| t.start_device_login(attempt),
            |app, r| match r {
                Ok(code) => {
                    let _ = open::that(&code.verification_uri);
                    app.login = LoginState::Waiting(code.clone());
                    app.poll_login(code, Instant::now())
                }
                Err(e) => {
                    app.login = LoginState::Failed(format!("{e:#}"));
                    Task::none()
                }
            },
        )
    }

    /// One poll of the device login, then the next after `interval` seconds.
    fn poll_login(&mut self, code: DeviceCode, started: Instant) -> Task<Message> {
        let tidal = self.tidal.clone();
        let c = code.clone();
        Task::perform(
            blocking(move || {
                std::thread::sleep(Duration::from_secs(c.interval.max(1)));
                tidal.poll_device_login(&c)
            }),
            move |r| {
                Message::Done(Callback::new(move |app: &mut App| {
                    if !matches!(&app.login, LoginState::Waiting(active) if active.device_code == code.device_code) {
                        return Task::none();
                    }
                    match r {
                        Ok(PollResult::Done) => {
                            app.login = LoginState::Idle;
                            app.signed_in = true;
                            app.after_login()
                        }
                        Ok(PollResult::Pending) if started.elapsed() < Duration::from_secs(code.expires_in) => {
                            app.poll_login(code, started)
                        }
                        Ok(PollResult::Pending) => {
                            app.login = LoginState::Failed(crate::i18n::t("the_code_expired_try_again").into());
                            Task::none()
                        }
                        Err(e) => {
                            app.login = LoginState::Failed(format!("{e:#}"));
                            Task::none()
                        }
                    }
                }))
            },
        )
    }

    pub fn sign_out(&mut self) -> Task<Message> {
        self.player.send(crate::audio::Cmd::Stop);
        self.tidal.sign_out();
        self.signed_in = false;
        self.now = None;
        self.queue.clear();
        self.unshuffled = None;
        self.resume_at = None;
        self.login = LoginState::Idle;
        self.login_busy = false;
        self.reset_data();
        Task::none()
    }

    pub fn load_library(&mut self) -> Task<Message> {
        self.my_playlists = Load::Loading;
        let a = self.fetch(
            |t| t.my_playlists(),
            |app, r| {
                app.my_playlists = into_load(r);
                app.apply_sorts();
                Task::none()
            },
        );
        let b = self.fetch(
            |t| t.favorite_ids(),
            |app, r| {
                if let Ok((ids, artists)) = r {
                    app.fav_ids = ids.into_iter().collect();
                    app.fav_artist_ids = artists.into_iter().collect();
                }
                Task::none()
            },
        );
        let c = self.fetch(
            |t| t.favorite_albums(),
            |app, r| {
                if let Ok(a) = &r {
                    app.fav_album_ids = a.iter().map(|a| a.id).collect();
                }
                app.fav_albums = into_load(r);
                app.apply_sorts();
                Task::none()
            },
        );
        Task::batch([a, b, c])
    }

    // ----------------------------------------------------------- navigation

    pub fn navigate(&mut self, page: Page) -> Task<Message> {
        if !self.signed_in {
            match page {
                Page::Settings | Page::MyTracks | Page::MyAlbums | Page::MyArtists | Page::Search(_) => {}
                Page::Album(id) | Page::Artist(id) if crate::local::is_local_id(id) => {}
                Page::Home => {
                    self.player.send(crate::audio::Cmd::Pause);
                    self.snap.paused = true;
                }
                _ => return Task::none(),
            }
        }
        log::info!("navigate -> {page:?}");
        if page == self.page {
            if self.now_playing_open {
                return self.toggle_now_playing();
            }
            return Task::none();
        }
        let old = std::mem::replace(&mut self.page, page);
        self.back.push(old);
        self.forward.clear();
        self.menu = None;
        self.nav_dir = 1.0;
        self.page_changed()
    }

    pub fn go_back(&mut self) -> Task<Message> {
        if let Some(p) = self.back.pop() {
            let old = std::mem::replace(&mut self.page, p);
            self.forward.push(old);
            self.nav_dir = -1.0;
            return self.page_changed();
        }
        Task::none()
    }

    pub fn go_forward(&mut self) -> Task<Message> {
        if let Some(p) = self.forward.pop() {
            let old = std::mem::replace(&mut self.page, p);
            self.back.push(old);
            self.nav_dir = 1.0;
            return self.page_changed();
        }
        Task::none()
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }
    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    fn page_changed(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        if self.now_playing_open {
            tasks.push(self.toggle_now_playing());
        }
        self.filter.clear();
        self.page_at = Instant::now();
        self.page_scroll = 0.0;
        if lib_section(&self.page).is_some() {
            self.after_library_switch();
        }
        tasks.push(self.load_page_data());
        tasks.push(iced::widget::operation::scroll_to(
            super::pages::PAGE_SCROLL,
            iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
        ));
        Task::batch(tasks)
    }

    /// Library as last shown (category remembered between sessions).
    pub fn library_page(&self) -> Page {
        lib_page_of(&self.settings.lock().lib_last)
    }

    /// Show `section` in Library: swapped in place from inside it, an ordinary
    /// navigation from elsewhere.
    pub fn set_library_cat(&mut self, section: &'static str) -> Task<Message> {
        let target = lib_page_of(section);
        if lib_section(&self.page).is_none() {
            return self.navigate(target);
        }
        if self.page == target {
            return Task::none();
        }
        self.page = target;
        self.after_library_switch();
        self.page_scroll = 0.0;
        Task::batch([
            self.load_page_data(),
            iced::widget::operation::scroll_to(super::pages::PAGE_SCROLL, iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: 0.0 }),
        ])
    }

    fn after_library_switch(&mut self) {
        let Some(sec) = lib_section(&self.page) else { return };
        {
            let mut s = self.settings.lock();
            if s.lib_last != sec {
                s.lib_last = sec.to_string();
                s.save();
            }
        }
        // Each category keeps its own search.
        self.filter = self.lib_queries.get(sec).cloned().unwrap_or_default();
        self.refresh_lib_shown();
        self.menu = None;
        self.expanded_album = None;
    }

    /// Grid (true) or list for a category; albums and artists start as grids.
    pub fn lib_grid(&self, section: &str) -> bool {
        self.settings.lock().lib_grid.get(section).copied().unwrap_or(section != "tracks")
    }

    pub fn set_lib_grid(&mut self, section: &'static str, grid: bool) {
        if self.lib_grid(section) == grid {
            return;
        }
        let mut s = self.settings.lock();
        s.lib_grid.insert(section.to_string(), grid);
        s.save();
        drop(s);
        self.expanded_album = None;
    }

    pub fn load_page_data(&mut self) -> Task<Message> {
        match self.page.clone() {
            Page::Home if self.home_tab == 0 && self.home.is_idle() => {
                self.home = Load::Loading;
                self.home_loaded = Some(Instant::now());
                self.fetch(
                    |t| t.home_feed("static"),
                    |app, r| {
                        app.home = into_load(r);
                        Task::none()
                    },
                )
            }
            // Coming back to Home after a while: refresh quietly, keeping what is shown.
            Page::Home if self.home_tab == 0 && self.home_loaded.is_some_and(|t| t.elapsed() > Duration::from_secs(90)) => {
                self.home_loaded = Some(Instant::now());
                self.fetch(
                    |t| t.home_feed("static"),
                    |app, r| {
                        if let Ok(rows) = r {
                            app.home = Load::Ready(rows);
                        }
                        Task::none()
                    },
                )
            }
            Page::Home if self.home_tab > 0 && !self.home_extra.contains_key(&self.home_tab) => {
                let tab = self.home_tab;
                self.load_feed_tab(tab)
            }
            Page::Explore if self.explore.is_idle() => {
                self.explore = Load::Loading;
                self.fetch(
                    |t| t.page("explore"),
                    |app, r| {
                        app.explore = into_load(r);
                        Task::none()
                    },
                )
            }
            Page::Album(id) if !self.albums.contains_key(&id) && !crate::local::is_local_id(id) => {
                self.albums.insert(id, Load::Loading);
                self.fetch(
                    move |t| t.album(id),
                    move |app, r| {
                        let mut task = Task::none();
                        if let Ok((album, _)) = &r {
                            if let Some(artist) = album.artists.first().map(|a| a.id) {
                                app.album_extra.insert(id, Load::Loading);
                                task = app.fetch(
                                    move |t| t.album_extras(artist, id),
                                    move |app, r| {
                                        app.album_extra.insert(id, into_load(r));
                                        Task::none()
                                    },
                                );
                            }
                        }
                        app.albums.insert(id, into_load(r));
                        task
                    },
                )
            }
            Page::Browse(path, _) if !self.browse.contains_key(&path) => {
                self.browse.insert(path.clone(), Load::Loading);
                let p = path.clone();
                self.fetch(
                    move |t| t.page(&p),
                    move |app, r| {
                        app.browse.insert(path, into_load(r));
                        Task::none()
                    },
                )
            }
            Page::Playlist(uuid) if !self.playlists.contains_key(&uuid) => {
                self.playlists.insert(uuid.clone(), Load::Loading);
                let u = uuid.clone();
                self.fetch(
                    move |t| t.playlist(&u),
                    move |app, r| {
                        app.playlists.insert(uuid, into_load(r));
                        Task::none()
                    },
                )
            }
            Page::Artist(id) if !self.artists.contains_key(&id) && !crate::local::is_local_id(id) => {
                self.artists.insert(id, Load::Loading);
                self.fetch(
                    move |t| t.artist(id),
                    move |app, r| {
                        app.artists.insert(id, into_load(r));
                        Task::none()
                    },
                )
            }
            Page::Mix(id) if !self.mixes.contains_key(&id) => {
                self.mixes.insert(id.clone(), Load::Loading);
                let i = id.clone();
                self.fetch(
                    move |t| t.mix(&i),
                    move |app, r| {
                        app.mixes.insert(id, into_load(r));
                        Task::none()
                    },
                )
            }
            Page::Search(q) if !q.trim().is_empty() && !self.search.contains_key(&q) => self.load_search(q),
            Page::MyTracks if self.fav_tracks.is_idle() && self.signed_in => {
                self.fav_tracks = Load::Loading;
                self.fetch(
                    |t| t.favorite_tracks(),
                    |app, r| {
                        app.fav_tracks = into_load(r);
                        app.apply_sorts();
                        Task::none()
                    },
                )
            }
            Page::MyAlbums if self.fav_albums.is_idle() && self.signed_in => {
                self.fav_albums = Load::Loading;
                self.fetch(
                    |t| t.favorite_albums(),
                    |app, r| {
                        app.fav_albums = into_load(r);
                        app.apply_sorts();
                        Task::none()
                    },
                )
            }
            Page::MyArtists if self.fav_artists.is_idle() && self.signed_in => {
                self.fav_artists = Load::Loading;
                self.fetch(
                    |t| t.favorite_artists(),
                    |app, r| {
                        app.fav_artists = into_load(r);
                        app.apply_sorts();
                        Task::none()
                    },
                )
            }
            Page::MyMixes if self.my_mixes.is_idle() => self.load_my_mixes(),
            Page::Discover => {
                let mut tasks = Vec::new();
                if self.my_mixes.is_idle() {
                    tasks.push(self.load_my_mixes());
                }
                let uploads = !self.settings.lock().hide_uploads;
                for tab in [1usize, 2] {
                    if (tab == 1 || uploads) && !self.home_extra.contains_key(&tab) {
                        tasks.push(self.load_feed_tab(tab));
                    }
                }
                Task::batch(tasks)
            }
            Page::MyPlaylists if self.my_playlists.is_idle() => self.load_library(),
            Page::Settings => {
                self.devices = crate::audio::output::device_names();
                Task::none()
            }
            Page::Replay => self.replay_artist_pics(),
            _ => Task::none(),
        }
    }

    pub(super) fn retry_page(&mut self) -> Task<Message> {
        match &self.page {
            Page::MyTracks if matches!(self.fav_tracks, Load::Failed(_)) => self.fav_tracks = Load::Idle,
            Page::MyAlbums if matches!(self.fav_albums, Load::Failed(_)) => self.fav_albums = Load::Idle,
            Page::MyArtists if matches!(self.fav_artists, Load::Failed(_)) => self.fav_artists = Load::Idle,
            Page::MyPlaylists if matches!(self.my_playlists, Load::Failed(_)) => self.my_playlists = Load::Idle,
            Page::MyMixes | Page::Discover => {
                if matches!(self.my_mixes, Load::Failed(_)) {
                    self.my_mixes = Load::Idle;
                }
                self.home_extra.retain(|_, state| !matches!(state, Load::Failed(_)));
            }
            Page::Home if self.home_tab == 0 && matches!(self.home, Load::Failed(_)) => self.home = Load::Idle,
            Page::Home => remove_failure(&mut self.home_extra, &self.home_tab),
            Page::Explore if matches!(self.explore, Load::Failed(_)) => self.explore = Load::Idle,
            Page::Browse(key, _) => remove_failure(&mut self.browse, key),
            Page::Album(id) => remove_failure(&mut self.albums, id),
            Page::Artist(id) => remove_failure(&mut self.artists, id),
            Page::Playlist(id) => remove_failure(&mut self.playlists, id),
            Page::Mix(id) => remove_failure(&mut self.mixes, id),
            Page::Search(query) => remove_failure(&mut self.search, query),
            _ => {}
        }
        self.load_page_data()
    }

    pub(super) fn retry_inline_album(&mut self, album: &Album) -> Task<Message> {
        if self.expanded_album == Some(album.id) && matches!(self.albums.get(&album.id), Some(Load::Failed(_))) {
            self.albums.remove(&album.id);
            self.expanded_album = None;
            return self.toggle_album_inline(album);
        }
        Task::none()
    }

    fn load_my_mixes(&mut self) -> Task<Message> {
        self.my_mixes = Load::Loading;
        self.fetch(
            |t| t.my_mixes(),
            |app, r| {
                app.my_mixes = into_load(r);
                app.apply_sorts();
                Task::none()
            },
        )
    }

    fn load_feed_tab(&mut self, tab: usize) -> Task<Message> {
        self.home_extra.insert(tab, Load::Loading);
        let vibe = if tab == 1 { "editorial" } else { "uploads" };
        self.fetch(
            move |t| t.home_feed(vibe),
            move |app, r| {
                app.home_extra.insert(tab, into_load(r));
                Task::none()
            },
        )
    }

    pub fn open_item(&mut self, item: &Item) -> Task<Message> {
        match item {
            Item::Link(l) if l.path.starts_with('@') => match &l.path[1..] {
                "tracks" => self.set_library_cat("tracks"),
                "albums" => self.set_library_cat("albums"),
                "artists" => self.set_library_cat("artists"),
                "playlists" => self.navigate(Page::MyPlaylists),
                "mixes" => self.navigate(Page::MyMixes),
                _ => Task::none(),
            },
            Item::Link(l) => self.navigate(Page::Browse(l.path.clone(), l.title.clone())),
            Item::Promo(p) => match p.kind.as_str() {
                "PLAYLIST" => self.navigate(Page::Playlist(p.target.clone())),
                "ALBUM" => p.target.parse().map(|id| self.navigate(Page::Album(id))).unwrap_or_else(|_| Task::none()),
                "ARTIST" => p.target.parse().map(|id| self.navigate(Page::Artist(id))).unwrap_or_else(|_| Task::none()),
                _ => {
                    self.notify_toast(crate::i18n::text(
                        "this_content_is_only_available_on_the_website",
                        &[crate::i18n::t("tidal").to_string()],
                    ));
                    Task::none()
                }
            },
            // In the collection, albums unfold in place (MusicBee style).
            Item::Album(a) if self.page == Page::MyAlbums => self.toggle_album_inline(a),
            Item::Album(a) => self.navigate(Page::Album(a.id)),
            Item::Playlist(p) => self.navigate(Page::Playlist(p.uuid.clone())),
            Item::Artist(a) => self.navigate(Page::Artist(a.id)),
            Item::Mix(m) => self.navigate(Page::Mix(m.id.clone())),
            Item::Track(t) => self.play_tracks(vec![t.clone()], 0),
        }
    }

    /// Play a card's contents directly (the play button on cards).
    pub fn play_item(&mut self, item: &Item) -> Task<Message> {
        match item.clone() {
            Item::Link(_) | Item::Promo(_) => Task::none(),
            Item::Track(t) => self.play_tracks(vec![t], 0),
            Item::Album(a) => {
                if let Some((_, tracks)) = self.albums.get(&a.id).and_then(Load::ready) {
                    let t = tracks.clone();
                    self.play_tracks(t, 0)
                } else {
                    self.fetch(
                        move |t| t.album(a.id),
                        move |app, r| {
                            let Ok((al, tracks)) = r else { return Task::none() };
                            let task = app.play_tracks(tracks.clone(), 0);
                            app.albums.insert(al.id, Load::Ready((al, tracks)));
                            task
                        },
                    )
                }
            }
            Item::Playlist(p) => {
                let u = p.uuid.clone();
                self.fetch(
                    move |t| t.playlist(&u),
                    move |app, r| {
                        let Ok((pl, tracks)) = r else { return Task::none() };
                        let task = app.play_tracks(tracks.clone(), 0);
                        app.playlists.insert(pl.uuid.clone(), Load::Ready((pl, tracks)));
                        task
                    },
                )
            }
            Item::Mix(m) => {
                let id = m.id.clone();
                self.fetch(
                    move |t| t.mix(&id),
                    move |app, r| {
                        let Ok((mix, tracks)) = r else { return Task::none() };
                        let task = app.play_tracks(tracks.clone(), 0);
                        app.mixes.insert(mix.id.clone(), Load::Ready((mix, tracks)));
                        task
                    },
                )
            }
            Item::Artist(a) if crate::local::is_local_id(a.id) => {
                let tracks = self.artists.get(&a.id).and_then(Load::ready).map(|p| p.top_tracks.clone()).unwrap_or_default();
                self.play_tracks(tracks, 0)
            }
            Item::Artist(a) => self.fetch(
                move |t| t.artist(a.id),
                move |app, r| {
                    let Ok(page) = r else { return Task::none() };
                    let task = app.play_tracks(page.top_tracks.clone(), 0);
                    app.artists.insert(page.artist.id, Load::Ready(page));
                    task
                },
            ),
        }
    }

    /// Toggle the inline track list of an album in the collection grid.
    pub fn toggle_album_inline(&mut self, album: &Album) -> Task<Message> {
        if self.expanded_album == Some(album.id) {
            self.expanded_album = None;
            return Task::none();
        }
        self.expanded_album = Some(album.id);
        self.expanded_album_height = 0.0;
        let id = album.id;
        if let std::collections::hash_map::Entry::Vacant(e) = self.albums.entry(id) {
            e.insert(Load::Loading);
            return self.fetch(
                move |t| t.album(id),
                move |app, r| {
                    app.albums.insert(id, into_load(r));
                    Task::none()
                },
            );
        }
        Task::none()
    }

    /// The tracks of the list a page shows (to play from a row).
    pub fn page_tracks(&self) -> Vec<Track> {
        match &self.page {
            Page::Album(id) => self.albums.get(id).and_then(Load::ready).map(|(_, t)| t.clone()).unwrap_or_default(),
            Page::Playlist(u) => self.playlists.get(u).and_then(Load::ready).map(|(_, t)| t.clone()).unwrap_or_default(),
            Page::Mix(m) => self.mixes.get(m).and_then(Load::ready).map(|(_, t)| t.clone()).unwrap_or_default(),
            Page::Artist(id) => self.artists.get(id).and_then(Load::ready).map(|p| p.top_tracks.clone()).unwrap_or_default(),
            // The whole Library, also under a filter: it only hides rows (as in GPUI).
            Page::MyTracks => self.lib_tracks.clone(),
            Page::Search(q) => self.search.get(q).and_then(Load::ready).map(|r| r.tracks.clone()).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    pub fn track_matches(&self, t: &Track) -> bool {
        if self.filter.trim().is_empty() {
            return true;
        }
        self.track_matches_query(t, &crate::local::fold(self.filter.trim()))
    }

    fn track_matches_query(&self, t: &Track, q: &str) -> bool {
        match self.local_search_keys.get(&t.id) {
            Some(key) => key.contains(q),
            None => crate::local::search_key(t).contains(q),
        }
    }

    pub(super) fn refresh_lib_shown(&mut self) {
        let q = crate::local::fold(self.filter.trim());
        self.lib_shown = match self.page {
            Page::MyTracks => library_indices(&self.lib_tracks, &q, |t, q| self.track_matches_query(t, q)),
            Page::MyAlbums => library_indices(&self.lib_albums, &q, |a, q| {
                crate::local::fold(&a.title).contains(q) || crate::local::fold(&a.artist_names()).contains(q)
            }),
            Page::MyArtists => library_indices(&self.lib_artists, &q, |a, q| crate::local::fold(&a.name).contains(q)),
            _ => Vec::new(),
        };
    }

    pub fn text_matches(&self, fields: &[&str]) -> bool {
        if self.filter.trim().is_empty() {
            return true;
        }
        let q = crate::local::fold(self.filter.trim());
        fields.iter().any(|f| crate::local::fold(f).contains(&q))
    }

    // --------------------------------------------------------------- search

    /// Local tracks, albums and artists matching `query` in front of the TIDAL results.
    pub fn with_local_search(&self, query: &str, mut results: SearchResults) -> SearchResults {
        let query = crate::local::fold(query);
        let matches = |fields: &[&str]| fields.iter().any(|s| crate::local::fold(s).contains(&query));
        results.tracks.retain(|t| t.local_path.is_none());
        results.albums.retain(|a| !crate::local::is_local_id(a.id));
        results.artists.retain(|a| !crate::local::is_local_id(a.id));
        if results.top.as_ref().is_some_and(|item| match item {
            Item::Track(t) => t.local_path.is_some(),
            Item::Album(a) => crate::local::is_local_id(a.id),
            Item::Artist(a) => crate::local::is_local_id(a.id),
            _ => false,
        }) {
            results.top = None;
        }
        let mut tracks: Vec<_> = self
            .lib_tracks
            .iter()
            .filter(|t| {
                t.local_path.is_some()
                    && match self.local_search_keys.get(&t.id) {
                        Some(key) => key.contains(&query),
                        None => crate::local::search_key(t).contains(&query),
                    }
            })
            .cloned()
            .collect();
        tracks.extend(results.tracks);
        results.tracks = tracks;
        let mut albums: Vec<_> = self
            .lib_albums
            .iter()
            .filter(|a| crate::local::is_local_id(a.id) && matches(&[&a.title, &a.artist_names()]))
            .cloned()
            .collect();
        albums.extend(results.albums);
        results.albums = albums;
        let mut artists: Vec<_> =
            self.lib_artists.iter().filter(|a| crate::local::is_local_id(a.id) && matches(&[&a.name])).cloned().collect();
        artists.extend(results.artists);
        results.artists = artists;
        if results.top.is_none() {
            results.top = results
                .tracks
                .first()
                .map(|t| Item::Track(t.clone()))
                .or_else(|| results.albums.first().map(|a| Item::Album(a.clone())))
                .or_else(|| results.artists.first().map(|a| Item::Artist(a.clone())));
        }
        results
    }

    pub fn load_search(&mut self, query: String) -> Task<Message> {
        let local = self.with_local_search(&query, SearchResults::default());
        let has_local = local.top.is_some();
        self.search.insert(query.clone(), if has_local || !self.signed_in { Load::Ready(local) } else { Load::Loading });
        if !self.signed_in {
            return Task::none();
        }
        let q = query.clone();
        self.fetch(
            move |t| t.search(&q),
            move |app, result| {
                let result = match result {
                    Ok(remote) => Load::Ready(app.with_local_search(&query, remote)),
                    Err(error) => {
                        let local = app.with_local_search(&query, SearchResults::default());
                        if local.top.is_some() {
                            Load::Ready(local)
                        } else {
                            Load::Failed(format!("{error:#}"))
                        }
                    }
                };
                app.search.insert(query, result);
                Task::none()
            },
        )
    }

    /// The text in the search box changed: show the menu and search after a short pause.
    pub fn search_typed(&mut self, text: String) -> Task<Message> {
        self.search_query = text;
        self.search_open = true;
        self.search_gen += 1;
        let gen = self.search_gen;
        let q = self.search_query.trim().to_string();
        if q.is_empty() || self.search.contains_key(&q) {
            return Task::none();
        }
        Task::perform(blocking(|| std::thread::sleep(DEBOUNCE)), move |_| {
            Message::Done(Callback::new(move |app: &mut App| {
                if app.search_gen == gen && !app.search.contains_key(&q) {
                    return app.load_search(q);
                }
                Task::none()
            }))
        })
    }

    /// Open the full results page for the query (Enter or "see all").
    pub fn search_all(&mut self, q: String) -> Task<Message> {
        let q = q.trim().to_string();
        self.search_open = false;
        if q.is_empty() {
            return Task::none();
        }
        {
            let mut s = self.settings.lock();
            s.recent_searches.retain(|x| x != &q);
            s.recent_searches.insert(0, q.clone());
            s.recent_searches.truncate(12);
            s.save();
        }
        if let Page::Search(_) = self.page {
            self.page = Page::Search(q);
            self.page_changed()
        } else {
            self.navigate(Page::Search(q))
        }
    }

    // ------------------------------------------------------------- sorting

    pub fn sort_of(&self, section: &str) -> (SortKey, bool) {
        self.settings.lock().sorts.get(section).map(|(k, d)| (SortKey::from_code(k), *d)).unwrap_or((SortKey::Added, false))
    }

    pub fn set_sort(&mut self, section: &str, key: SortKey) {
        let (cur, desc) = self.sort_of(section);
        // Clicking the active option flips the direction.
        let desc = if cur == key { !desc } else { false };
        {
            let mut s = self.settings.lock();
            s.sorts.insert(section.to_string(), (key.code().to_string(), desc));
            s.save();
        }
        self.apply_sorts();
    }

    /// Sort every loaded collection list by its chosen key.
    pub fn apply_sorts(&mut self) {
        let (tk, td) = self.sort_of("tracks");
        let (ak, ad) = self.sort_of("albums");
        let (rk, rd) = self.sort_of("artists");
        let (pk, pd) = self.sort_of("playlists");
        let (mk, md) = self.sort_of("mixes");
        let ratings = self.ratings.clone();
        let dir = |o: std::cmp::Ordering, desc: bool| if desc { o.reverse() } else { o };
        if let Load::Ready(list) = &mut self.fav_tracks {
            let ranks = self.sort_ranks.entry("tracks").or_default();
            rank(ranks, list, |t| t.id.to_string());
            list.sort_by(|a, b| {
                let o = match tk {
                    SortKey::Name => text_key(&a.title).cmp(&text_key(&b.title)),
                    SortKey::Artist => text_key(&a.artist_names()).cmp(&text_key(&b.artist_names())),
                    SortKey::Album => {
                        let t = |x: &Track| x.album.as_ref().map(|a| text_key(&a.title)).unwrap_or_default();
                        t(a).cmp(&t(b)).then(a.track_number.cmp(&b.track_number))
                    }
                    SortKey::Duration => a.duration.cmp(&b.duration),
                    SortKey::Rating => ratings.get(b.id).cmp(&ratings.get(a.id)),
                    _ => ranks.get(&a.id.to_string()).cmp(&ranks.get(&b.id.to_string())),
                };
                dir(o, td)
            });
        }
        if let Load::Ready(list) = &mut self.fav_albums {
            let ranks = self.sort_ranks.entry("albums").or_default();
            rank(ranks, list, |a| a.id.to_string());
            list.sort_by(|a, b| {
                let o = match ak {
                    SortKey::Name => text_key(&a.title).cmp(&text_key(&b.title)),
                    SortKey::Artist => {
                        text_key(&a.artist_names()).cmp(&text_key(&b.artist_names())).then(a.release_date.cmp(&b.release_date))
                    }
                    SortKey::Year => b.release_date.cmp(&a.release_date),
                    SortKey::Rating => ratings.get_album(b.id).cmp(&ratings.get_album(a.id)),
                    _ => ranks.get(&a.id.to_string()).cmp(&ranks.get(&b.id.to_string())),
                };
                dir(o, ad)
            });
        }
        if let Load::Ready(list) = &mut self.fav_artists {
            let ranks = self.sort_ranks.entry("artists").or_default();
            rank(ranks, list, |a| a.id.to_string());
            list.sort_by(|a, b| {
                let o = match rk {
                    SortKey::Name => text_key(&a.name).cmp(&text_key(&b.name)),
                    _ => ranks.get(&a.id.to_string()).cmp(&ranks.get(&b.id.to_string())),
                };
                dir(o, rd)
            });
        }
        if let Load::Ready(list) = &mut self.my_playlists {
            let ranks = self.sort_ranks.entry("playlists").or_default();
            rank(ranks, list, |p| p.uuid.clone());
            list.sort_by(|a, b| {
                let o = match pk {
                    SortKey::Name => text_key(&a.title).cmp(&text_key(&b.title)),
                    SortKey::Count => b.number_of_tracks.cmp(&a.number_of_tracks),
                    _ => ranks.get(&a.uuid).cmp(&ranks.get(&b.uuid)),
                };
                dir(o, pd)
            });
        }
        if let Load::Ready(list) = &mut self.my_mixes {
            let ranks = self.sort_ranks.entry("mixes").or_default();
            rank(ranks, list, |m| m.id.clone());
            list.sort_by(|a, b| {
                let o = match mk {
                    SortKey::Name => text_key(&a.title).cmp(&text_key(&b.title)),
                    _ => ranks.get(&a.id).cmp(&ranks.get(&b.id)),
                };
                dir(o, md)
            });
        }
        self.rebuild_library();
    }

    /// The Library's lists: TIDAL favourites and local files together, in the chosen order.
    pub fn rebuild_library(&mut self) {
        let (tk, td) = self.sort_of("tracks");
        let (ak, ad) = self.sort_of("albums");
        let (rk, rd) = self.sort_of("artists");
        let ratings = self.ratings.clone();
        let dir = |o: std::cmp::Ordering, desc: bool| if desc { o.reverse() } else { o };

        let mut tracks: Vec<Track> = self.local_tracks.clone();
        tracks.extend(self.fav_tracks.ready().cloned().unwrap_or_default());
        let ranks = self.sort_ranks.entry("lib-tracks").or_default();
        rank(ranks, &tracks, |t| t.id.to_string());
        tracks.sort_by(|a, b| {
            let o = match tk {
                SortKey::Name => text_key(&a.title).cmp(&text_key(&b.title)),
                SortKey::Artist => text_key(&a.artist_names()).cmp(&text_key(&b.artist_names())),
                SortKey::Album => {
                    let t = |x: &Track| x.album.as_ref().map(|a| text_key(&a.title)).unwrap_or_default();
                    t(a).cmp(&t(b)).then(a.volume_number.cmp(&b.volume_number)).then(a.track_number.cmp(&b.track_number))
                }
                SortKey::Duration => a.duration.cmp(&b.duration),
                SortKey::Rating => ratings.get(b.id).cmp(&ratings.get(a.id)),
                _ => b.added.cmp(&a.added).then(ranks.get(&a.id.to_string()).cmp(&ranks.get(&b.id.to_string()))),
            };
            dir(o, td)
        });
        self.lib_tracks = tracks;

        let mut albums: Vec<Album> = self.local_albums.clone();
        albums.extend(self.fav_albums.ready().cloned().unwrap_or_default());
        let ranks = self.sort_ranks.entry("lib-albums").or_default();
        rank(ranks, &albums, |a| a.id.to_string());
        albums.sort_by(|a, b| {
            let o = match ak {
                SortKey::Name => text_key(&a.title).cmp(&text_key(&b.title)),
                SortKey::Artist => text_key(&a.artist_names()).cmp(&text_key(&b.artist_names())).then(a.release_date.cmp(&b.release_date)),
                SortKey::Year => b.release_date.cmp(&a.release_date),
                SortKey::Rating => ratings.get_album(b.id).cmp(&ratings.get_album(a.id)),
                _ => ranks.get(&a.id.to_string()).cmp(&ranks.get(&b.id.to_string())),
            };
            dir(o, ad)
        });
        self.lib_albums = albums;

        let mut artists = self.local_artists.clone();
        artists.extend(self.fav_artists.ready().cloned().unwrap_or_default());
        let ranks = self.sort_ranks.entry("lib-artists").or_default();
        rank(ranks, &artists, |a| a.id.to_string());
        artists.sort_by(|a, b| {
            let o = match rk {
                SortKey::Name => text_key(&a.name).cmp(&text_key(&b.name)),
                _ => ranks.get(&a.id.to_string()).cmp(&ranks.get(&b.id.to_string())),
            };
            dir(o, rd)
        });
        self.lib_artists = artists;
        self.refresh_lib_shown();
    }

    // ---------------------------------------------------------- local files

    pub fn choose_local_files(&mut self, folders: bool) -> Task<Message> {
        if self.local_busy {
            return Task::none();
        }
        let title = if folders { crate::i18n::t("add_music_folder") } else { crate::i18n::t("add_audio_files") };
        Task::perform(
            blocking(move || {
                let dialog = rfd::FileDialog::new().set_title(title);
                if folders {
                    dialog.pick_folders()
                } else {
                    dialog.add_filter("Audio", &["flac", "mp3", "m4a", "aac", "alac", "wav", "aif", "aiff", "ogg", "opus"]).pick_files()
                }
                .unwrap_or_default()
            }),
            Message::LocalAdded,
        )
    }

    /// Import files and folders (picker or drag and drop) straight into the Library.
    pub fn import_local_paths(&mut self, paths: Vec<std::path::PathBuf>) -> Task<Message> {
        if self.local_busy || paths.is_empty() {
            return Task::none();
        }
        self.local_busy = true;
        self.notify_toast(crate::i18n::t("adding_to_your_library"));
        let known = self.local_tracks.iter().filter_map(|t| t.local_path.clone()).collect();
        Task::perform(
            blocking(move || {
                let folders: Vec<_> = paths.iter().filter(|p| p.is_dir()).cloned().collect();
                crate::local::remember_folders(&folders);
                crate::local::import(paths, true, known)
            }),
            |imported| {
                Message::Done(Callback::new(move |app: &mut App| {
                    app.local_busy = false;
                    let count = app.merge_local(imported.tracks);
                    let mut parts = vec![crate::i18n::text(
                        if count == 1 { "local.track_imported" } else { "local.tracks_imported" },
                        &[count.to_string()],
                    )];
                    if imported.duplicates > 0 {
                        parts.push(crate::i18n::text("already_present", &[format!("{}", imported.duplicates)]));
                    }
                    if imported.rejected > 0 {
                        parts.push(crate::i18n::text("unsupported", &[format!("{}", imported.rejected)]));
                    }
                    app.notify_toast(parts.join(" · "));
                    if count > 0 && app.page != Page::Settings {
                        return app.set_library_cat("tracks");
                    }
                    Task::none()
                }))
            },
        )
    }

    /// Look for new files in the remembered folders (in the background).
    pub(super) fn rescan_local_folders(&mut self) -> Task<Message> {
        let folders = crate::local::folders();
        if folders.is_empty() {
            return Task::none();
        }
        let known = self.local_tracks.iter().filter_map(|t| t.local_path.clone()).collect();
        Task::perform(blocking(move || crate::local::import(folders, true, known)), |imported| {
            Message::Done(Callback::new(move |app: &mut App| {
                let count = app.merge_local(imported.tracks);
                if count > 0 {
                    app.notify_toast(crate::i18n::text("new_files_from_your_folders_added_to_the_library", &[format!("{count}")]));
                }
                Task::none()
            }))
        })
    }

    fn merge_local(&mut self, tracks: Vec<Track>) -> usize {
        let mut known: std::collections::HashSet<_> = self.local_tracks.iter().map(|t| t.id).collect();
        let tracks: Vec<_> = tracks.into_iter().filter(|t| known.insert(t.id)).collect();
        let count = tracks.len();
        if count == 0 {
            return 0;
        }
        let mut combined = self.local_tracks.clone();
        combined.extend(tracks);
        crate::local::save_async(combined.clone());
        self.local_tracks = combined;
        self.local_changed();
        count
    }

    /// Rebuild the local albums and artists (cached like TIDAL ones so their
    /// pages open) and the Library's lists.
    pub(super) fn local_changed(&mut self) {
        self.local_search_keys = self.local_tracks.iter().map(|t| (t.id, crate::local::search_key(t))).collect();
        for a in &self.local_albums {
            self.albums.remove(&a.id);
        }
        let albums = crate::local::albums(&self.local_tracks);
        self.local_albums = albums.iter().map(|(a, _)| a.clone()).collect();
        for (album, tracks) in albums {
            self.albums.insert(album.id, Load::Ready((album, tracks)));
        }
        for artist in &self.local_artists {
            self.artists.remove(&artist.id);
        }
        let artists = crate::local::artists(&self.local_tracks, &self.local_albums);
        self.local_artists = artists.iter().map(|page| page.artist.clone()).collect();
        for page in artists {
            self.artists.insert(page.artist.id, Load::Ready(page));
        }
        // Nothing left to edit once the track has left the library.
        if self.local_editor.as_ref().is_some_and(|e| !self.local_tracks.iter().any(|t| t.id == e.id)) {
            self.local_editor = None;
        }
        self.apply_sorts();
        let searches: Vec<_> = self.search.iter().filter_map(|(q, r)| r.ready().map(|r| (q.clone(), r.clone()))).collect();
        for (q, r) in searches {
            let result = self.with_local_search(&q, r);
            self.search.insert(q, Load::Ready(result));
        }
    }

    pub fn remove_local_album(&mut self, album: u64) -> Task<Message> {
        if self.local_busy {
            return Task::none();
        }
        let remaining: Vec<_> = self.local_tracks.iter().filter(|t| t.album.as_ref().map(|a| a.id) != Some(album)).cloned().collect();
        crate::local::save_async(remaining.clone());
        self.local_tracks = remaining;
        self.local_changed();
        self.albums.remove(&album);
        self.expanded_album = None;
        self.notify_toast(crate::i18n::t("album_removed_from_library_files_kept"));
        if self.page == Page::Album(album) {
            return self.set_library_cat("albums");
        }
        Task::none()
    }

    pub fn remove_local_track(&mut self, id: u64) {
        if self.local_busy {
            return;
        }
        let remaining: Vec<_> = self.local_tracks.iter().filter(|t| t.id != id).cloned().collect();
        crate::local::save_async(remaining.clone());
        self.local_tracks = remaining;
        self.local_changed();
        self.notify_toast(crate::i18n::t("removed_from_library_file_kept"));
    }

    // ------------------------------------------------------------ playlists

    pub fn user_id(&self) -> Option<u64> {
        self.settings.lock().user_id
    }

    pub fn own_playlists(&self) -> Vec<Playlist> {
        let uid = self.user_id();
        self.my_playlists.ready().map(|l| l.iter().filter(|p| playlist_is_editable(p, uid)).cloned().collect()).unwrap_or_default()
    }

    pub fn is_own_playlist(&self, uuid: &str) -> bool {
        let uid = self.user_id();
        let check = |p: &Playlist| p.uuid == uuid && playlist_is_editable(p, uid);
        self.my_playlists.ready().map(|l| l.iter().any(check)).unwrap_or(false)
            || self.playlists.get(uuid).and_then(Load::ready).map(|(p, _)| Some(p.creator_id) == uid).unwrap_or(false)
    }

    pub fn is_saved_playlist(&self, uuid: &str) -> bool {
        self.my_playlists.ready().map(|l| l.iter().any(|p| p.uuid == uuid)).unwrap_or(false)
    }

    pub fn open_dialog(&mut self, dialog: Dialog) -> Task<Message> {
        let (title, desc) = match &dialog {
            Dialog::EditPlaylist { uuid } => {
                self.playlists.get(uuid).and_then(Load::ready).map(|(p, _)| (p.title.clone(), p.description.clone())).unwrap_or_default()
            }
            _ => (String::new(), String::new()),
        };
        self.dlg_title = title;
        self.dlg_desc = desc;
        let has_fields = matches!(dialog, Dialog::NewPlaylist { .. } | Dialog::EditPlaylist { .. });
        self.dialog = Some(dialog);
        self.menu = None;
        if has_fields {
            return iced::widget::operation::focus(super::pages::DIALOG_TITLE_ID);
        }
        Task::none()
    }

    pub fn submit_dialog(&mut self) -> Task<Message> {
        let Some(dialog) = self.dialog.clone() else { return Task::none() };
        let title = self.dlg_title.trim().to_string();
        let desc = self.dlg_desc.trim().to_string();
        let task = match dialog {
            Dialog::NewPlaylist { add } => {
                if title.is_empty() {
                    self.notify_toast(crate::i18n::t("enter_a_playlist_name"));
                    return Task::none();
                }
                self.create_playlist(title, desc, add)
            }
            Dialog::EditPlaylist { uuid } => {
                if title.is_empty() {
                    self.notify_toast(crate::i18n::t("the_name_cannot_be_empty"));
                    return Task::none();
                }
                self.edit_playlist(uuid, title, desc)
            }
            Dialog::DeletePlaylist { uuid, .. } => self.delete_playlist(uuid),
            Dialog::SignOut => self.sign_out(),
            Dialog::ConfirmRemove(k) => match k {
                RemoveKind::PlaylistTrack { uuid, index, .. } => self.remove_playlist_track(uuid, index),
                RemoveKind::FavTrack(t) if self.fav_ids.contains(&t.id) => self.toggle_favorite(&t),
                RemoveKind::FavAlbum(a) if self.fav_album_ids.contains(&a.id) => self.toggle_favorite_album(&a),
                RemoveKind::Unfollow(id, name) if self.fav_artist_ids.contains(&id) => {
                    let artist = Artist { id, name, picture: None };
                    self.toggle_favorite_artist(&artist)
                }
                _ => Task::none(),
            },
        };
        self.dialog = None;
        task
    }

    fn create_playlist(&mut self, title: String, desc: String, add: Vec<u64>) -> Task<Message> {
        self.fetch(
            move |t| {
                let pl = t.create_playlist(&title, &desc)?;
                if !add.is_empty() {
                    t.add_to_playlist(&pl.uuid, &add)?;
                }
                Ok(pl)
            },
            |app, r| match r {
                Ok(pl) => {
                    if let Load::Ready(list) = &mut app.my_playlists {
                        list.insert(0, pl.clone());
                    }
                    app.notify_toast(crate::i18n::text("playlist_created", std::slice::from_ref(&pl.title)));
                    app.navigate(Page::Playlist(pl.uuid))
                }
                Err(e) => {
                    app.notify_toast(crate::i18n::text("could_not_create_the_playlist", &[format!("{e:#}")]));
                    Task::none()
                }
            },
        )
    }

    fn edit_playlist(&mut self, uuid: String, title: String, desc: String) -> Task<Message> {
        let (u, t2, d2) = (uuid.clone(), title.clone(), desc.clone());
        self.fetch(
            move |t| t.edit_playlist(&u, &t2, &d2),
            move |app, r| {
                match r {
                    Ok(()) => {
                        if let Some(Load::Ready((p, _))) = app.playlists.get_mut(&uuid) {
                            p.title = title.clone();
                            p.description = desc.clone();
                        }
                        if let Load::Ready(list) = &mut app.my_playlists {
                            if let Some(p) = list.iter_mut().find(|p| p.uuid == uuid) {
                                p.title = title.clone();
                                p.description = desc.clone();
                            }
                        }
                        app.notify_toast(crate::i18n::t("playlist_updated"));
                    }
                    Err(e) => app.notify_toast(crate::i18n::text("could_not_edit", &[format!("{e:#}")])),
                }
                Task::none()
            },
        )
    }

    fn delete_playlist(&mut self, uuid: String) -> Task<Message> {
        let u = uuid.clone();
        self.fetch(
            move |t| t.delete_playlist(&u),
            move |app, r| match r {
                Ok(()) => {
                    if let Load::Ready(list) = &mut app.my_playlists {
                        list.retain(|p| p.uuid != uuid);
                    }
                    app.playlists.remove(&uuid);
                    app.notify_toast(crate::i18n::t("playlist_deleted"));
                    if app.page == Page::Playlist(uuid.clone()) {
                        return app.navigate(Page::MyPlaylists);
                    }
                    Task::none()
                }
                Err(e) => {
                    app.notify_toast(crate::i18n::text("could_not_delete", &[format!("{e:#}")]));
                    Task::none()
                }
            },
        )
    }

    pub fn add_tracks_to_playlist(&mut self, uuid: String, tracks: Vec<Track>) -> Task<Message> {
        if tracks.iter().any(|track| track.local_path.is_some()) {
            self.notify_toast(crate::i18n::text("local_files_cannot_be_added_to_playlists", &[crate::i18n::t("tidal").to_string()]));
            return Task::none();
        }
        let ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
        let name = self
            .my_playlists
            .ready()
            .and_then(|l| l.iter().find(|p| p.uuid == uuid))
            .map(|p| p.title.clone())
            .unwrap_or_else(|| crate::i18n::t("the_playlist").into());
        let u = uuid.clone();
        self.menu = None;
        self.fetch(
            move |t| t.add_to_playlist(&u, &ids),
            move |app, r| {
                match r {
                    Ok(()) => {
                        if let Some(Load::Ready((p, list))) = app.playlists.get_mut(&uuid) {
                            list.extend(tracks.iter().cloned());
                            p.number_of_tracks = list.len() as u32;
                        }
                        if let Load::Ready(all) = &mut app.my_playlists {
                            if let Some(p) = all.iter_mut().find(|p| p.uuid == uuid) {
                                p.number_of_tracks += tracks.len() as u32;
                            }
                        }
                        let n = tracks.len();
                        app.notify_toast(if n == 1 {
                            crate::i18n::text("added_to", std::slice::from_ref(&name))
                        } else {
                            crate::i18n::text("tracks_added_to", &[format!("{n}"), name.to_string()])
                        });
                    }
                    Err(e) => app.notify_toast(crate::i18n::text("could_not_add", &[format!("{e:#}")])),
                }
                Task::none()
            },
        )
    }

    pub fn remove_playlist_track(&mut self, uuid: String, index: usize) -> Task<Message> {
        // Optimistic: update the list now, reload it if TIDAL refuses.
        if let Some(Load::Ready((p, list))) = self.playlists.get_mut(&uuid) {
            if index < list.len() {
                list.remove(index);
                p.number_of_tracks = list.len() as u32;
            }
        }
        let u = uuid.clone();
        self.fetch(
            move |t| t.remove_from_playlist(&u, index),
            move |app, r| match r {
                Ok(()) => {
                    app.notify_toast(crate::i18n::t("removed_from_playlist"));
                    Task::none()
                }
                Err(e) => {
                    app.playlists.remove(&uuid);
                    app.notify_toast(crate::i18n::text("could_not_remove", &[format!("{e:#}")]));
                    app.load_page_data()
                }
            },
        )
    }

    pub fn toggle_saved_playlist(&mut self, pl: Playlist) -> Task<Message> {
        let save = !self.is_saved_playlist(&pl.uuid);
        if let Load::Ready(list) = &mut self.my_playlists {
            if save {
                list.insert(0, pl.clone());
            } else {
                list.retain(|p| p.uuid != pl.uuid);
            }
        }
        let u = pl.uuid.clone();
        self.fetch(
            move |t| t.set_favorite_playlist(&u, save),
            move |app, r| {
                match r {
                    Ok(()) => app.notify_toast(if save {
                        crate::i18n::t("saved_to_my_collection")
                    } else {
                        crate::i18n::t("removed_from_my_collection")
                    }),
                    Err(e) => app.notify_toast(crate::i18n::text("error", &[format!("{e:#}")])),
                }
                Task::none()
            },
        )
    }

    // -------------------------------------------------------------- replay

    fn replay_artist_pics(&mut self) -> Task<Message> {
        let stats = self.replay_stats();
        // Followed artists are known already: no request for them.
        let known: Vec<(u64, Option<String>)> = self
            .fav_artists
            .ready()
            .into_iter()
            .flatten()
            .filter(|a| stats.top_artists.iter().any(|r| r.id == a.id))
            .map(|a| (a.id, a.picture(320)))
            .collect();
        for (id, picture) in known {
            self.artist_pics.entry(id).or_insert(Load::Ready(picture));
        }
        let ids: Vec<u64> =
            stats.top_artists.iter().take(10).map(|r| r.id).filter(|id| *id != 0 && !self.artist_pics.contains_key(id)).collect();
        if ids.is_empty() || !self.signed_in {
            return Task::none();
        }
        for id in &ids {
            self.artist_pics.insert(*id, Load::Loading);
        }
        self.fetch(
            move |t| Ok(ids.iter().map(|&id| (id, t.artist_basic(id).ok().and_then(|a| a.picture(320)))).collect::<Vec<_>>()),
            |app, r: anyhow::Result<Vec<(u64, Option<String>)>>| {
                if let Ok(list) = r {
                    for (id, pic) in list {
                        app.artist_pics.insert(id, Load::Ready(pic));
                    }
                }
                Task::none()
            },
        )
    }

    /// Diagnostics: `ABYSS_DEMO=<page>` opens a page at start (screenshots).
    pub(super) fn demo_from_env(&mut self) -> Task<Message> {
        let Ok(name) = std::env::var("ABYSS_DEMO").or_else(|_| std::env::var("TIDE_DEMO")) else { return Task::none() };
        match name.as_str() {
            "settings" => self.navigate(Page::Settings),
            "tracks" => self.set_library_cat("tracks"),
            "albums" => self.set_library_cat("albums"),
            "replay" => self.navigate(Page::Replay),
            "np" | "nowplaying" => {
                let tracks = self.lib_tracks.clone();
                let t = self.play_tracks(tracks, 0);
                Task::batch([t, self.toggle_now_playing()])
            }
            "lyrics" => {
                let tracks = self.lib_tracks.clone();
                let t = self.play_tracks(tracks, 0);
                self.show_lyrics = true;
                t
            }
            _ => Task::none(),
        }
    }
}

// --------------------------------------------------------------- lyrics

impl App {
    pub(super) fn reset_lyric_follow(&mut self) {
        self.lyrics_follow = true;
        self.lyrics_sync_at = None;
        super::lyrics::reset_scroll();
    }

    pub fn wants_lyrics(&self) -> bool {
        if self.now_playing_open {
            self.np_tab == super::NpTab::Lyrics || self.now.as_ref().is_some_and(|n| n.track.local_path.is_some())
        } else {
            self.show_lyrics
        }
    }

    pub fn load_lyrics(&mut self, id: u64) -> Task<Message> {
        if matches!(self.lyrics.get(&id), Some(Load::Ready(_) | Load::Loading)) {
            return Task::none();
        }
        let epoch = self.lyrics_epoch;
        self.lyrics.insert(id, Load::Loading);
        self.lyrics_follow = true;
        let track = self.now.as_ref().map(|n| n.track.clone()).filter(|t| t.id == id);
        self.fetch(
            move |t| {
                // Diagnostics: `ABYSS_TEST_LYRICS_DELAY_MS` slows the lookup (loading state).
                if let Some(ms) = std::env::var("ABYSS_TEST_LYRICS_DELAY_MS").ok().and_then(|v| v.parse().ok()) {
                    std::thread::sleep(Duration::from_millis(ms));
                }
                match track {
                    Some(track) => t.lyrics_any(&track),
                    None => t.lyrics(id),
                }
            },
            move |app, r| {
                if app.lyrics_epoch == epoch {
                    app.lyrics.insert(id, into_load(r));
                    app.lyrics_docs.remove(&id);
                }
                Task::none()
            },
        )
    }

    /// The lyrics of `id` as the GPU view lays them out (built once per load).
    pub fn lyrics_doc(&self, id: u64) -> Option<std::sync::Arc<super::lyrics::LyricsDoc>> {
        if let Some(doc) = self.lyrics_docs.get(&id) {
            return Some(doc.clone());
        }
        let l = self.lyrics.get(&id)?.ready()?;
        Some(std::sync::Arc::new(super::lyrics::LyricsDoc::new(id, l, self.settings.lock().lyrics_romanize)))
    }

    /// Cache the laid-out lyrics of the playing track (called from update).
    pub(super) fn cache_lyrics_doc(&mut self) {
        let Some(id) = self.now.as_ref().map(|n| n.track.id) else { return };
        if self.lyrics_docs.contains_key(&id) {
            return;
        }
        if let Some(doc) = self.lyrics_doc(id) {
            if self.lyrics_docs.len() > 8 {
                self.lyrics_docs.clear();
            }
            self.lyrics_docs.insert(id, doc);
        }
    }

    /// Fetch what the visible Now Playing tab needs for the playing track.
    pub fn load_np_data(&mut self) -> Task<Message> {
        let Some(id) = self.now.as_ref().map(|n| n.track.id) else { return Task::none() };
        if !self.now_playing_open {
            return Task::none();
        }
        if self.now.as_ref().is_some_and(|n| n.track.local_path.is_some()) {
            return self.load_lyrics(id);
        }
        match self.np_tab {
            super::NpTab::Lyrics => self.load_lyrics(id),
            super::NpTab::Queue => Task::none(),
            super::NpTab::Credits => self.load_credits(id),
            super::NpTab::Similar => {
                if let std::collections::hash_map::Entry::Vacant(e) = self.similar.entry(id) {
                    e.insert(Load::Loading);
                    return self.fetch(
                        move |t| t.track_radio(id),
                        move |app, r| {
                            app.similar.insert(id, into_load(r));
                            Task::none()
                        },
                    );
                }
                Task::none()
            }
        }
    }

    pub fn toggle_now_playing(&mut self) -> Task<Message> {
        self.now_playing_open = !self.now_playing_open;
        self.menu = None;
        let mode = self.set_fullscreen(self.now_playing_open);
        if self.now_playing_open {
            self.lyrics_follow = true;
            return Task::batch([mode, self.load_np_data()]);
        }
        // The page is built again on leaving full screen: put it back where it was.
        let back = iced::widget::operation::scroll_to(
            super::pages::PAGE_SCROLL,
            iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: self.page_scroll },
        );
        if self.show_lyrics {
            if let Some(id) = self.now.as_ref().map(|n| n.track.id) {
                return Task::batch([mode, back, self.load_lyrics(id)]);
            }
        }
        Task::batch([mode, back])
    }

    pub fn toggle_lyrics(&mut self) -> Task<Message> {
        if self.now_playing_open {
            self.np_tab = super::NpTab::Lyrics;
            return self.load_np_data();
        }
        self.show_lyrics = !self.show_lyrics;
        if self.show_lyrics {
            self.show_queue = false;
            self.lyrics_follow = true;
            if let Some(id) = self.now.as_ref().map(|n| n.track.id) {
                return self.load_lyrics(id);
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod index_tests {
    use super::library_indices;

    #[test]
    fn filtered_indices_keep_collection_order_and_original_positions() {
        let titles = ["Alpha", "Beta", "Amber", "Alpha"];
        assert_eq!(library_indices(&titles, "A", |title, q| title.starts_with(q)), [0, 2, 3]);
        assert_eq!(library_indices(&titles, "", |_, _| panic!("empty query should match all")), [0, 1, 2, 3]);
    }
}
