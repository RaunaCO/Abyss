//! The application window: state, navigation, queue and playback control.
#![allow(unused_imports)]

pub mod icons;

use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

static ZOOM_BITS: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0

/// Current UI zoom (1.0 = 100 %).
pub fn zoom() -> f32 {
    f32::from_bits(ZOOM_BITS.load(AtomicOrdering::Relaxed))
}

pub fn set_zoom(z: f32) {
    ZOOM_BITS.store(z.to_bits(), AtomicOrdering::Relaxed);
}

/// A design pixel at the current zoom. All layout sizes use this instead of `px`.
pub fn sc(v: f32) -> gpui::Pixels {
    gpui::px(v * zoom())
}

/// Zoom levels, as in a browser.
pub const ZOOM_STEPS: [f32; 11] = [0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];
pub mod images;
pub mod input;
mod pages;
mod player_bar;
pub mod playlists;
pub mod now_playing;
mod debug;
mod media;
mod smooth;
mod motion;
mod titlebar;
mod winctl;
mod album_inline;
mod sort;
pub mod fx;
mod settings;
mod appmenu;
mod haptics;
mod scrollbar;
mod sidebar;
pub mod theme;
mod widgets;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use gpui::{
    actions, div, prelude::*, px, App, Bounds, Context, Entity, FocusHandle, Focusable, KeyBinding, ListAlignment,
    ListState, MouseButton, MouseMoveEvent, Pixels, Point, ScrollHandle, Window,
};
use parking_lot::Mutex;

use crate::audio::{self, Cmd, OutputPrefs, Player, Snapshot};
use crate::config::{Quality, Repeat, Settings};
use crate::tidal::*;
use input::{InputEvent, TextInput};

actions!(app, [OpenSettings, GoHome, ToggleShuffle, PlayPause, NextTrack, PrevTrack, FocusSearch, SeekForward, SeekBack, VolumeUp, VolumeDown, ToggleMute, GoBack, GoForward, ZoomIn, ZoomOut, ZoomReset]);

pub fn bindings() -> Vec<KeyBinding> {
    let ctx = Some("App");
    vec![
        KeyBinding::new("space", PlayPause, ctx),
        KeyBinding::new("ctrl-right", NextTrack, ctx),
        KeyBinding::new("ctrl-left", PrevTrack, ctx),
        KeyBinding::new("ctrl-f", FocusSearch, None),
        KeyBinding::new("ctrl-l", FocusSearch, None),
        KeyBinding::new("right", SeekForward, ctx),
        KeyBinding::new("left", SeekBack, ctx),
        KeyBinding::new("ctrl-up", VolumeUp, None),
        KeyBinding::new("ctrl-down", VolumeDown, None),
        KeyBinding::new("m", ToggleMute, ctx),
        KeyBinding::new("alt-left", GoBack, None),
        KeyBinding::new("alt-right", GoForward, None),
        KeyBinding::new("ctrl-=", ZoomIn, None),
        KeyBinding::new("ctrl-+", ZoomIn, None),
        KeyBinding::new("ctrl-shift-=", ZoomIn, None),
        KeyBinding::new("ctrl--", ZoomOut, None),
        KeyBinding::new("ctrl-0", ZoomReset, None),
        KeyBinding::new("backspace", GoBack, ctx),
        KeyBinding::new("shift-backspace", GoForward, ctx),
        KeyBinding::new("ctrl-p", OpenSettings, None),
        KeyBinding::new("ctrl-h", GoHome, None),
        KeyBinding::new("ctrl-s", ToggleShuffle, None),
        KeyBinding::new("ctrl-shift-right", SeekForward, None),
        KeyBinding::new("ctrl-shift-left", SeekBack, None),
    ]
}

#[derive(Debug, Clone, PartialEq)]
pub enum Page {
    Home,
    Explore,
    /// An editorial page (genre, mood, decade...): (page name, title).
    Browse(String, String),
    Search(String),
    Album(u64),
    Artist(u64),
    Playlist(String),
    Mix(String),
    MyPlaylists,
    MyAlbums,
    MyTracks,
    MyArtists,
    MyMixes,
    Settings,
}

pub enum Load<T> {
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Default for Load<T> {
    fn default() -> Self {
        Load::Idle
    }
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(t) => Some(t),
            _ => None,
        }
    }
    fn is_idle(&self) -> bool {
        matches!(self, Load::Idle)
    }
}

#[derive(Clone)]
pub struct QItem {
    pub uid: u64,
    pub track: Track,
}

pub struct NowPlaying {
    pub track: Track,
    pub info: Option<StreamInfo>,
    /// Engine key of this play.
    pub key: u64,
    pub index: usize,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Drag {
    Progress,
    Volume,
}

/// What a right-click menu acts on.
#[derive(Clone)]
pub enum MenuTarget {
    Track(Track),
    QueueItem(usize),
    /// Sort options of a collection section.
    Sort(&'static str),
    /// A track inside one of the user's own playlists (can be removed).
    PlaylistTrack { uuid: String, index: usize, track: Track },
    /// Pick a playlist to add these tracks to.
    AddToPlaylist(Vec<Track>),
}

pub enum LoginState {
    Idle,
    Waiting(DeviceCode),
    Web(PkceLogin),
    Failed(String),
}

pub struct AppView {
    pub tidal: Tidal,
    pub player: Player,
    events: Receiver<audio::Event>,
    pub settings: Arc<Mutex<Settings>>,
    pub focus: FocusHandle,
    pub search_input: Entity<TextInput>,
    pub login_input: Entity<TextInput>,
    pub login_busy: bool,

    pub page: Page,
    back: Vec<Page>,
    forward: Vec<Page>,
    pub list: ListState,
    pub list_len: usize,
    pub row_scroll: HashMap<usize, ScrollHandle>,

    // Data.
    pub home: Load<Vec<Row>>,
    /// Selected home tab (0 For you, 1 Staff Picks, 2 Uploads) and the feeds of tabs 1 and 2.
    pub home_tab: usize,
    /// Settings page: selected tab and whether the sound-output list is unfolded.
    pub settings_tab: usize,
    /// Open hamburger menu: the path of hovered entries (empty = only the groups).
    pub app_menu: Option<Vec<usize>>,
    pub settings_dev_open: bool,
    pub home_extra: HashMap<usize, Load<Vec<Row>>>,
    pub explore: Load<Vec<Row>>,
    pub browse: HashMap<String, Load<Vec<Row>>>,
    /// More albums by the artist + related artists, per album page.
    pub album_extra: HashMap<u64, Load<(Vec<Album>, Vec<Artist>)>>,
    pub albums: HashMap<u64, Load<(Album, Vec<Track>)>>,
    pub playlists: HashMap<String, Load<(Playlist, Vec<Track>)>>,
    pub artists: HashMap<u64, Load<ArtistPage>>,
    pub mixes: HashMap<String, Load<(Mix, Vec<Track>)>>,
    pub search: HashMap<String, Load<SearchResults>>,
    pub fav_tracks: Load<Vec<Track>>,
    pub fav_albums: Load<Vec<Album>>,
    pub fav_artists: Load<Vec<Artist>>,
    pub my_playlists: Load<Vec<Playlist>>,
    pub my_mixes: Load<Vec<Mix>>,
    pub fav_ids: HashSet<u64>,
    pub fav_album_ids: HashSet<u64>,
    pub fav_artist_ids: HashSet<u64>,
    pub dialog: Option<playlists::Dialog>,
    pub dlg_title: Entity<TextInput>,
    pub dlg_desc: Entity<TextInput>,
    pub lyrics: HashMap<u64, Load<Lyrics>>,

    // Playback.
    pub queue: Vec<QItem>,
    /// Original order, kept while shuffle is on.
    unshuffled: Option<Vec<QItem>>,
    pub now: Option<NowPlaying>,
    /// Next item handed to the engine: (key, index).
    queued_next: Option<(u64, usize)>,
    next_info: Option<(u64, StreamInfo)>,
    /// Playback reached the end of the queue.
    pub ended: bool,
    pub snap: Snapshot,
    next_key: u64,
    next_uid: u64,
    pub volume: f32,
    pub muted: bool,
    pub shuffle: bool,
    pub repeat: Repeat,

    // UI state.
    pub show_queue: bool,
    pub show_lyrics: bool,
    pub drag: Option<Drag>,
    pub drag_value: f32,
    pub progress_bounds: Bounds<Pixels>,
    pub volume_bounds: Bounds<Pixels>,
    pub menu: Option<(Point<Pixels>, MenuTarget)>,
    pub last_menu_pos: Option<Point<Pixels>>,
    pub now_playing_open: bool,
    pub np_tab: now_playing::NpTab,
    pub similar: HashMap<u64, Load<Vec<Track>>>,
    autoplay_loading: bool,
    pub scroll_pending: f32,
    pub volume_wheel_px: f32,
    pub filter_input: Entity<TextInput>,
    /// Lower-cased text of the filter box (collection pages, playlists).
    pub filter: String,
    /// Colour of the page's cover, shown as a wash behind the header.
    pub page_tint: Option<gpui::Rgba>,
    /// Album unfolded in the collection grid.
    pub expanded_album: Option<u64>,
    pub inline_anim: usize,
    pub sort_ranks: HashMap<&'static str, HashMap<String, i64>>,
    pub ambient_from: gpui::Rgba,
    pub ambient_to: gpui::Rgba,
    pub ambient_at: std::time::Instant,
    /// Heart that was just liked: (track id, when).
    pub heart_pop: Option<(u64, std::time::Instant)>,
    /// Local star ratings and the half-star value being hovered, if any.
    pub ratings: crate::ratings::Ratings,
    /// (is album, id, half stars)
    pub rating_hover: Option<(bool, u64, u8)>,
    /// Smoothed, compressed band levels (0..1) that gently modulate the backgrounds.
    pub vis: [f32; 3],
    /// Press pops, ripples, heart burst and swipe state.
    pub haptics: haptics::Haptics,
    /// Direction of the last navigation (1 forward / new page, -1 back), for the page slide.
    pub nav_dir: f32,
    /// The page scrollbar is being dragged.
    pub scrollbar_drag: bool,
    /// State of the rotating Now Playing / player-bar background.
    pub bg_spin: fx::BgSpin,
    /// Its angle for the current frame (computed once per render).
    pub bg_angle_now: f32,
    /// Albums played here (newest first) and when the home feed was last fetched.
    pub recents: Vec<crate::recents::RecentAlbum>,
    pub home_loaded: Option<std::time::Instant>,
    /// Slow running mean of each band, so `vis` follows the music's dynamics rather than its loudness.
    vis_mean: [f32; 3],
    /// Play queue docked beside the lyrics / credits / similar tabs in Now Playing.
    pub np_queue_side: bool,
    /// Lyrics follow the song (off after a manual scroll).
    pub lyrics_follow: bool,
    /// Per-track lyrics timing correction in seconds.
    pub lyrics_offset: HashMap<u64, f64>,
    pub lyrics_scroll: ScrollHandle,
    /// Synced line last scrolled to (auto-follow).
    pub lyrics_line: Option<usize>,
    pub lyrics_scroll_at: std::time::Instant,
    /// Previous active lyric line and a counter, to fade lines in and out.
    pub lyrics_prev: Option<usize>,
    pub lyrics_changes: usize,
    /// When `snap` was taken (to interpolate the progress bar).
    pub snap_at: std::time::Instant,
    /// Sidebar width animation: (width at start, start time).
    pub sidebar_anim: Option<(f32, std::time::Instant)>,
    /// Now Playing slide: (opening?, start time).
    pub np_anim: Option<(bool, std::time::Instant)>,
    /// Bumped on every page change to restart the page transition.
    pub page_anim: usize,
    /// A drag on the progress/volume bar started: ignore the bar's click.
    pub bar_click_block: bool,
    /// Title bar press waiting to turn into a window drag.
    pub title_press: Option<Point<Pixels>>,
    /// Window being moved by its title bar.
    pub win_drag: Option<winctl::WinDrag>,
    /// Second press of a title bar double-click (acted on at release).
    pub title_double: bool,
    pub volume_hint_until: Option<std::time::Instant>,
    pub scroll_animating: bool,
    pub scroll_last: std::time::Instant,
    autoplay_jump: bool,
    media: Option<media::Media>,
    pub discord: crate::discord::Discord,
    pub discord_input: Entity<TextInput>,
    pub credits: HashMap<u64, Load<Vec<(String, Vec<String>)>>>,
    pub toast: Option<(String, std::time::Instant)>,
    pub login: LoginState,
    pub signed_in: bool,
    pub devices: Vec<String>,
    pub show_quality_info: bool,
    pub current_blocks: Vec<pages::Block>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = Arc::new(Mutex::new(Settings::load()));
        set_zoom(
            std::env::var("TIDE_ZOOM").ok().and_then(|z| z.parse().ok()).unwrap_or_else(|| settings.lock().zoom).clamp(0.5, 2.0),
        );
        if std::env::var("TIDE_RAIL").is_ok() {
            settings.lock().sidebar_collapsed = true;
        }
        let tidal = Tidal::new(settings.clone());
        let s = settings.lock().clone();
        let (player, events) = Player::new(
            tidal.http().clone(),
            s.volume,
            OutputPrefs {
                device: s.output_device.clone(),
                exclusive: s.exclusive_mode,
                force_volume: s.force_volume,
                normalize: s.normalize,
            },
        );
        images::Images::init(tidal.http().clone(), cx);

        let search_input = cx.new(|cx| TextInput::new("Buscar", cx).compact());
        cx.subscribe_in(&search_input, window, Self::on_search_event).detach();
        let login_input = cx.new(|cx| TextInput::new("Pega aquí la dirección (https://tidal.com/android/login/auth?code=…)", cx));
        cx.subscribe_in(&login_input, window, |this, _, ev: &InputEvent, _, cx| {
            if let InputEvent::Submit(text) = ev {
                this.finish_web_login(text.clone(), cx);
            }
        })
        .detach();

        let filter_input = cx.new(|cx| TextInput::new("Filtrar", cx).compact());
        cx.subscribe_in(&filter_input, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Changed(text) => {
                this.filter = text.trim().to_lowercase();
                this.content_changed_pub();
                cx.notify();
            }
            InputEvent::Escape | InputEvent::Submit(_) => window.focus(&this.focus),
        })
        .detach();
        let discord_input = cx.new(|cx| TextInput::new("ID de aplicación de Discord", cx));
        discord_input.update(cx, |i, cx| i.set_text(&settings.lock().discord_client_id, cx));
        cx.subscribe_in(&discord_input, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Changed(text) => {
                let id: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
                {
                    let mut s = this.settings.lock();
                    s.discord_client_id = id.clone();
                    s.save();
                }
                this.discord.set_client_id(id);
                cx.notify();
            }
            InputEvent::Escape | InputEvent::Submit(_) => window.focus(&this.focus),
        })
        .detach();
        let discord = {
            let st = settings.lock();
            let id = std::env::var("TIDE_DISCORD_ID")
                .ok()
                .or_else(|| Some(st.discord_client_id.clone()).filter(|s| !s.is_empty()))
                .unwrap_or_else(|| crate::config::DISCORD_APP_ID.to_string());
            crate::discord::Discord::new(id, st.discord)
        };
        let dlg_title = cx.new(|cx| TextInput::new("Mi playlist", cx));
        let dlg_desc = cx.new(|cx| TextInput::new("", cx));
        for input in [&dlg_title, &dlg_desc] {
            cx.subscribe_in(input, window, |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Submit(_) => this.submit_dialog(window, cx),
                InputEvent::Escape => this.close_dialog(window, cx),
                _ => {}
            })
            .detach();
        }

        let mut this = Self {
            tidal,
            player,
            events,
            settings,
            focus: cx.focus_handle(),
            search_input,
            login_input,
            login_busy: false,
            page: Page::Home,
            back: Vec::new(),
            forward: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, sc(600.)),
            list_len: 0,
            row_scroll: HashMap::new(),
            home: Load::Idle,
            home_tab: 0,
            settings_tab: 0,
            app_menu: None,
            settings_dev_open: false,
            home_extra: HashMap::new(),
            explore: Load::Idle,
            browse: HashMap::new(),
            album_extra: HashMap::new(),
            albums: HashMap::new(),
            playlists: HashMap::new(),
            artists: HashMap::new(),
            mixes: HashMap::new(),
            search: HashMap::new(),
            fav_tracks: Load::Idle,
            fav_albums: Load::Idle,
            fav_artists: Load::Idle,
            my_playlists: Load::Idle,
            my_mixes: Load::Idle,
            fav_ids: HashSet::new(),
            fav_album_ids: HashSet::new(),
            fav_artist_ids: HashSet::new(),
            dialog: None,
            dlg_title,
            dlg_desc,
            lyrics: HashMap::new(),
            queue: Vec::new(),
            unshuffled: None,
            now: None,
            queued_next: None,
            next_info: None,
            ended: false,
            snap: Snapshot::default(),
            next_key: 1,
            next_uid: 1,
            volume: s.volume,
            muted: false,
            shuffle: s.shuffle,
            repeat: s.repeat,
            show_queue: false,
            show_lyrics: false,
            drag: None,
            drag_value: 0.0,
            progress_bounds: Bounds::default(),
            volume_bounds: Bounds::default(),
            menu: None,
            last_menu_pos: None,
            now_playing_open: false,
            np_tab: now_playing::NpTab::Lyrics,
            similar: HashMap::new(),
            autoplay_loading: false,
            scroll_pending: 0.0,
            volume_wheel_px: 0.0,
            filter_input,
            filter: String::new(),
            page_tint: None,
            expanded_album: None,
            inline_anim: 0,
            sort_ranks: HashMap::new(),
            ambient_from: theme::accent(),
            ambient_to: theme::accent(),
            ambient_at: std::time::Instant::now(),
            heart_pop: None,
            ratings: crate::ratings::Ratings::load(),
            rating_hover: None,
            vis: [0.0; 3],
            haptics: Default::default(),
            nav_dir: 1.0,
            scrollbar_drag: false,
            bg_spin: fx::BgSpin::Stopped(0.0),
            bg_angle_now: 0.0,
            recents: crate::recents::load(),
            home_loaded: None,
            vis_mean: [0.0; 3],
            np_queue_side: false,
            lyrics_follow: true,
            lyrics_offset: HashMap::new(),
            lyrics_scroll: ScrollHandle::new(),
            lyrics_line: None,
            lyrics_scroll_at: std::time::Instant::now(),
            lyrics_prev: None,
            lyrics_changes: 0,
            snap_at: std::time::Instant::now(),
            sidebar_anim: None,
            np_anim: None,
            page_anim: 0,
            bar_click_block: false,
            title_press: None,
            win_drag: None,
            title_double: false,
            volume_hint_until: None,
            scroll_animating: false,
            scroll_last: std::time::Instant::now(),
            autoplay_jump: false,
            media: media::Media::new(window),
            discord,
            discord_input,
            credits: HashMap::new(),
            toast: None,
            login: LoginState::Idle,
            signed_in: s.signed_in(),
            devices: audio::output::device_names(),
            show_quality_info: false,
            current_blocks: Vec::new(),
        };
        if this.signed_in {
            this.after_login(cx);
        }
        this.start_ticker(cx);
        this.demo_from_env(cx);
        window.focus(&this.focus);
        this
    }

    fn start_ticker(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(200)).await;
            if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                break;
            }
        })
        .detach();
    }

    // ------------------------------------------------------------ helpers

    /// Run `job` on a background thread, then `done` on the UI thread.
    pub fn fetch<T: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        job: impl FnOnce(Tidal) -> Result<T> + Send + 'static,
        done: impl FnOnce(&mut Self, Result<T>, &mut Context<Self>) + 'static,
    ) {
        let tidal = self.tidal.clone();
        cx.spawn(async move |this, cx| {
            let r = cx.background_executor().spawn(async move { job(tidal) }).await;
            let _ = this.update(cx, |this, cx| {
                done(this, r, cx);
                this.apply_sorts();
                this.content_changed();
                cx.notify();
            });
        })
        .detach();
    }

    pub fn notify_toast(&mut self, msg: impl Into<String>, cx: &mut Context<Self>) {
        self.toast = Some((msg.into(), std::time::Instant::now()));
        cx.notify();
    }

    fn save_settings(&self) {
        let mut s = self.settings.lock();
        s.volume = self.volume;
        s.shuffle = self.shuffle;
        s.repeat = self.repeat;
        s.save();
    }

    // ------------------------------------------------------------- login

    /// Web (PKCE) login: FLAC for every track.
    pub fn start_web_login(&mut self, cx: &mut Context<Self>) {
        let login = self.tidal.start_pkce_login();
        let _ = open::that(&login.url);
        self.login = LoginState::Web(login);
        self.login_input.update(cx, |i, cx| i.set_text("", cx));
        cx.notify();
    }

    pub fn finish_web_login(&mut self, pasted: String, cx: &mut Context<Self>) {
        let LoginState::Web(login) = &self.login else { return };
        if self.login_busy {
            return;
        }
        let login = login.clone();
        self.login_busy = true;
        self.fetch(cx, move |t| t.finish_pkce_login(&login, &pasted), |this, r, cx| {
            this.login_busy = false;
            match r {
                Ok(()) => {
                    this.login = LoginState::Idle;
                    this.signed_in = true;
                    this.reset_data();
                    this.after_login(cx);
                }
                Err(e) => this.notify_toast(format!("{e:#}"), cx),
            }
        });
    }

    fn reset_data(&mut self) {
        self.home = Load::Idle;
        self.home_extra.clear();
        self.explore = Load::Idle;
        self.albums.clear();
        self.playlists.clear();
        self.artists.clear();
        self.mixes.clear();
        self.search.clear();
        self.fav_tracks = Load::Idle;
        self.fav_albums = Load::Idle;
        self.fav_artists = Load::Idle;
        self.my_playlists = Load::Idle;
        self.my_mixes = Load::Idle;
    }

    /// Sign out and go straight to the web login (from Settings).
    pub fn relogin_full_quality(&mut self, cx: &mut Context<Self>) {
        self.sign_out(cx);
        self.start_web_login(cx);
    }

    pub fn start_login(&mut self, cx: &mut Context<Self>) {
        self.login = LoginState::Idle;
        self.fetch(cx, |t| t.start_device_login(), |this, r, cx| match r {
            Ok(code) => {
                let _ = open::that(&code.verification_uri);
                this.login = LoginState::Waiting(code.clone());
                this.poll_login(code, cx);
            }
            Err(e) => this.login = LoginState::Failed(format!("{e:#}")),
        });
    }

    fn poll_login(&mut self, code: DeviceCode, cx: &mut Context<Self>) {
        let tidal = self.tidal.clone();
        cx.spawn(async move |this, cx| {
            let deadline = std::time::Instant::now() + Duration::from_secs(code.expires_in);
            loop {
                cx.background_executor().timer(Duration::from_secs(code.interval)).await;
                let t = tidal.clone();
                let c = code.clone();
                let r = cx.background_executor().spawn(async move { t.poll_device_login(&c) }).await;
                let still = this
                    .update(cx, |this, cx| {
                        if !matches!(this.login, LoginState::Waiting(_)) {
                            return false;
                        }
                        match r {
                            Ok(PollResult::Done) => {
                                this.login = LoginState::Idle;
                                this.signed_in = true;
                                this.after_login(cx);
                                cx.notify();
                                false
                            }
                            Ok(PollResult::Pending) if std::time::Instant::now() < deadline => true,
                            Ok(PollResult::Pending) => {
                                this.login = LoginState::Failed("El código caducó. Inténtalo de nuevo.".into());
                                cx.notify();
                                false
                            }
                            Err(e) => {
                                this.login = LoginState::Failed(format!("{e:#}"));
                                cx.notify();
                                false
                            }
                        }
                    })
                    .unwrap_or(false);
                if !still {
                    break;
                }
            }
        })
        .detach();
    }

    fn after_login(&mut self, cx: &mut Context<Self>) {
        self.fetch(cx, |t| t.load_session(), |_, _, _| {});
        self.load_page_data(cx);
        self.load_library(cx);
    }

    pub fn sign_out(&mut self, cx: &mut Context<Self>) {
        self.player.send(Cmd::Stop);
        self.tidal.sign_out();
        self.signed_in = false;
        self.now = None;
        self.queue.clear();
        self.home = Load::Idle;
        self.my_playlists = Load::Idle;
        self.fav_tracks = Load::Idle;
        cx.notify();
    }

    pub fn load_library(&mut self, cx: &mut Context<Self>) {
        self.my_playlists = Load::Loading;
        self.fetch(cx, |t| t.my_playlists(), |this, r, _| this.my_playlists = into_load(r));
        self.fetch(cx, |t| t.favorite_ids(), |this, r, _| {
            if let Ok((ids, artists)) = r {
                this.fav_ids = ids.into_iter().collect();
                this.fav_artist_ids = artists.into_iter().collect();
            }
        });
        self.fetch(cx, |t| t.favorite_albums(), |this, r, _| {
            if let Ok(a) = &r {
                this.fav_album_ids = a.iter().map(|a| a.id).collect();
            }
            this.fav_albums = into_load(r);
        });
    }

    // -------------------------------------------------------- navigation

    pub fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        log::info!("navigate -> {page:?}");
        self.nav_dir = 1.0;
        if page == self.page {
            return;
        }
        self.now_playing_open = false;
        self.np_anim = None;
        let old = std::mem::replace(&mut self.page, page);
        self.back.push(old);
        self.forward.clear();
        self.menu = None;
        self.page_changed(cx);
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        log::info!("go_back (history: {})", self.back.len());
        self.nav_dir = -1.0;
        if let Some(p) = self.back.pop() {
            let old = std::mem::replace(&mut self.page, p);
            self.forward.push(old);
            self.page_changed(cx);
        }
    }

    /// Zoom one step in (+1) or out (-1).
    pub fn step_zoom(&mut self, dir: i32, cx: &mut Context<Self>) {
        let cur = zoom();
        let idx = ZOOM_STEPS.iter().position(|z| (z - cur).abs() < 0.01).unwrap_or_else(|| {
            // Between steps (settings edited by hand): snap to the nearest.
            ZOOM_STEPS.iter().enumerate().min_by(|a, b| (a.1 - cur).abs().total_cmp(&(b.1 - cur).abs())).map(|(i, _)| i).unwrap_or(5)
        });
        let next = (idx as i32 + dir).clamp(0, ZOOM_STEPS.len() as i32 - 1) as usize;
        self.set_zoom_level(ZOOM_STEPS[next], cx);
    }

    pub fn set_zoom_level(&mut self, z: f32, cx: &mut Context<Self>) {
        log::info!("zoom -> {z}");
        set_zoom(z);
        {
            let mut s = self.settings.lock();
            s.zoom = z;
            s.save();
        }
        // Everything is re-measured at the new scale.
        self.list_len = usize::MAX - 1;
        self.notify_toast(format!("Zoom {} %", (z * 100.0).round() as i32), cx);
        cx.notify();
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        log::info!("go_forward (history: {})", self.forward.len());
        self.nav_dir = 1.0;
        if let Some(p) = self.forward.pop() {
            let old = std::mem::replace(&mut self.page, p);
            self.back.push(old);
            self.page_changed(cx);
        }
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }
    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    fn page_changed(&mut self, cx: &mut Context<Self>) {
        self.stop_smooth_scroll();
        self.page_tint = None;
        if !self.filter.is_empty() {
            self.filter.clear();
            self.filter_input.update(cx, |i, cx| i.set_text("", cx));
        }
        let ph = match self.page {
            Page::MyTracks => "Filtrar pistas",
            Page::MyAlbums => "Filtrar álbumes",
            Page::MyArtists => "Filtrar artistas",
            Page::MyPlaylists => "Filtrar playlists",
            Page::MyMixes => "Filtrar mixes",
            _ => "Filtrar por título, artista o álbum",
        };
        self.filter_input.update(cx, |i, cx| i.set_placeholder(ph, cx));
        self.page_anim += 1;
        self.row_scroll.clear();
        self.list_len = usize::MAX; // force a reset on next render
        self.load_page_data(cx);
        cx.notify();
    }

    pub fn content_changed_pub(&mut self) {
        self.content_changed();
    }

    /// Data for the page changed: re-measure the list on next render.
    fn content_changed(&mut self) {
        if self.list_len != usize::MAX {
            self.list_len = usize::MAX - 1;
        }
    }

    pub fn load_page_data(&mut self, cx: &mut Context<Self>) {
        match self.page.clone() {
            Page::Home if self.home_tab == 0 && self.home.is_idle() => {
                self.home = Load::Loading;
                self.home_loaded = Some(std::time::Instant::now());
                self.fetch(cx, |t| t.home_feed("static"), |this, r, _| this.home = into_load(r));
            }
            // Coming back to Home after a while: refresh quietly, keeping what is shown.
            Page::Home if self.home_tab == 0 && self.home_loaded.is_some_and(|t| t.elapsed() > Duration::from_secs(90)) => {
                self.home_loaded = Some(std::time::Instant::now());
                self.fetch(cx, |t| t.home_feed("static"), |this, r, _| {
                    if let Ok(rows) = r {
                        this.home = Load::Ready(rows);
                        this.content_changed_pub();
                    }
                });
            }
            Page::Home if self.home_tab > 0 && !self.home_extra.contains_key(&self.home_tab) => {
                let tab = self.home_tab;
                self.home_extra.insert(tab, Load::Loading);
                let vibe = if tab == 1 { "editorial" } else { "uploads" };
                self.fetch(cx, move |t| t.home_feed(vibe), move |this, r, _| {
                    this.home_extra.insert(tab, into_load(r));
                });
            }
            Page::Explore if self.explore.is_idle() => {
                self.explore = Load::Loading;
                self.fetch(cx, |t| t.page("explore"), |this, r, _| this.explore = into_load(r));
            }
            Page::Album(id) if !self.albums.contains_key(&id) => {
                self.albums.insert(id, Load::Loading);
                self.fetch(cx, move |t| t.album(id), move |this, r, cx| {
                    if let Ok((album, _)) = &r {
                        if let Some(artist) = album.artists.first().map(|a| a.id) {
                            this.album_extra.insert(id, Load::Loading);
                            this.fetch(cx, move |t| t.album_extras(artist, id), move |this, r, _| {
                                this.album_extra.insert(id, into_load(r));
                            });
                        }
                    }
                    this.albums.insert(id, into_load(r));
                });
            }
            Page::Browse(path, _) if !self.browse.contains_key(&path) => {
                self.browse.insert(path.clone(), Load::Loading);
                let p = path.clone();
                self.fetch(cx, move |t| t.page(&p), move |this, r, _| {
                    this.browse.insert(path, into_load(r));
                });
            }
            Page::Playlist(uuid) if !self.playlists.contains_key(&uuid) => {
                self.playlists.insert(uuid.clone(), Load::Loading);
                let u = uuid.clone();
                self.fetch(cx, move |t| t.playlist(&u), move |this, r, _| {
                    this.playlists.insert(uuid, into_load(r));
                });
            }
            Page::Artist(id) if !self.artists.contains_key(&id) => {
                self.artists.insert(id, Load::Loading);
                self.fetch(cx, move |t| t.artist(id), move |this, r, _| {
                    this.artists.insert(id, into_load(r));
                });
            }
            Page::Mix(id) if !self.mixes.contains_key(&id) => {
                self.mixes.insert(id.clone(), Load::Loading);
                let i = id.clone();
                self.fetch(cx, move |t| t.mix(&i), move |this, r, _| {
                    this.mixes.insert(id, into_load(r));
                });
            }
            Page::Search(q) if !q.trim().is_empty() && !self.search.contains_key(&q) => {
                self.search.insert(q.clone(), Load::Loading);
                let qq = q.clone();
                self.fetch(cx, move |t| t.search(&qq), move |this, r, _| {
                    this.search.insert(q, into_load(r));
                });
            }
            Page::MyTracks if self.fav_tracks.is_idle() => {
                self.fav_tracks = Load::Loading;
                self.fetch(cx, |t| t.favorite_tracks(), |this, r, _| this.fav_tracks = into_load(r));
            }
            Page::MyAlbums if self.fav_albums.is_idle() => {
                self.fav_albums = Load::Loading;
                self.fetch(cx, |t| t.favorite_albums(), |this, r, _| this.fav_albums = into_load(r));
            }
            Page::MyArtists if self.fav_artists.is_idle() => {
                self.fav_artists = Load::Loading;
                self.fetch(cx, |t| t.favorite_artists(), |this, r, _| this.fav_artists = into_load(r));
            }
            Page::MyMixes if self.my_mixes.is_idle() => {
                self.my_mixes = Load::Loading;
                self.fetch(cx, |t| t.my_mixes(), |this, r, _| this.my_mixes = into_load(r));
            }
            Page::MyPlaylists if self.my_playlists.is_idle() => {
                self.load_library(cx);
            }
            Page::Settings => self.devices = audio::output::device_names(),
            _ => {}
        }
    }

    pub fn set_home_tab(&mut self, tab: usize, cx: &mut Context<Self>) {
        if self.home_tab != tab {
            self.home_tab = tab;
            self.list_len = usize::MAX - 1;
            self.page_anim += 1;
            self.load_page_data(cx);
            self.content_changed_pub();
            cx.notify();
        }
    }

    pub fn open_item(&mut self, item: &Item, cx: &mut Context<Self>) {
        match item {
            // Shortcut to a collection page ("My Tracks"...).
            Item::Link(l) if l.path.starts_with('@') => match &l.path[1..] {
                "tracks" => self.navigate(Page::MyTracks, cx),
                "albums" => self.navigate(Page::MyAlbums, cx),
                "artists" => self.navigate(Page::MyArtists, cx),
                "playlists" => self.navigate(Page::MyPlaylists, cx),
                "mixes" => self.navigate(Page::MyMixes, cx),
                _ => {}
            },
            Item::Link(l) => self.navigate(Page::Browse(l.path.clone(), l.title.clone()), cx),
            Item::Promo(p) => match p.kind.as_str() {
                "PLAYLIST" => self.navigate(Page::Playlist(p.target.clone()), cx),
                "ALBUM" => match p.target.parse() {
                    Ok(id) => self.navigate(Page::Album(id), cx),
                    Err(_) => {}
                },
                "ARTIST" => match p.target.parse() {
                    Ok(id) => self.navigate(Page::Artist(id), cx),
                    Err(_) => {}
                },
                _ => self.notify_toast("Este contenido solo está disponible en la web de TIDAL", cx),
            },
            // In the collection, albums unfold in place (MusicBee style).
            Item::Album(a) if self.page == Page::MyAlbums => self.toggle_album_inline(a, cx),
            Item::Album(a) => self.navigate(Page::Album(a.id), cx),
            Item::Playlist(p) => self.navigate(Page::Playlist(p.uuid.clone()), cx),
            Item::Artist(a) => self.navigate(Page::Artist(a.id), cx),
            Item::Mix(m) => self.navigate(Page::Mix(m.id.clone()), cx),
            Item::Track(t) => self.play_tracks(vec![t.clone()], 0, cx),
        }
    }

    /// Play a card's contents directly (the play button on cards).
    pub fn play_item(&mut self, item: &Item, cx: &mut Context<Self>) {
        match item.clone() {
            Item::Link(_) | Item::Promo(_) => {}
            Item::Track(t) => self.play_tracks(vec![t], 0, cx),
            Item::Album(a) => {
                if let Some((_, tracks)) = self.albums.get(&a.id).and_then(Load::ready) {
                    let t = tracks.clone();
                    self.play_tracks(t, 0, cx);
                } else {
                    self.fetch(cx, move |t| t.album(a.id), move |this, r, cx| {
                        if let Ok((al, tracks)) = r {
                            this.play_tracks(tracks.clone(), 0, cx);
                            this.albums.insert(al.id, Load::Ready((al, tracks)));
                        }
                    });
                }
            }
            Item::Playlist(p) => {
                let u = p.uuid.clone();
                self.fetch(cx, move |t| t.playlist(&u), move |this, r, cx| {
                    if let Ok((pl, tracks)) = r {
                        this.play_tracks(tracks.clone(), 0, cx);
                        this.playlists.insert(pl.uuid.clone(), Load::Ready((pl, tracks)));
                    }
                });
            }
            Item::Mix(m) => {
                let id = m.id.clone();
                self.fetch(cx, move |t| t.mix(&id), move |this, r, cx| {
                    if let Ok((mix, tracks)) = r {
                        this.play_tracks(tracks.clone(), 0, cx);
                        this.mixes.insert(mix.id.clone(), Load::Ready((mix, tracks)));
                    }
                });
            }
            Item::Artist(a) => {
                self.fetch(cx, move |t| t.artist(a.id), move |this, r, cx| {
                    if let Ok(page) = r {
                        this.play_tracks(page.top_tracks.clone(), 0, cx);
                        this.artists.insert(page.artist.id, Load::Ready(page));
                    }
                });
            }
        }
    }

    fn on_search_event(&mut self, _: &Entity<TextInput>, ev: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            InputEvent::Submit(q) => {
                let q = q.trim().to_string();
                if q.is_empty() {
                    return;
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
                    self.page_changed(cx);
                } else {
                    self.navigate(Page::Search(q), cx);
                }
                window.focus(&self.focus);
            }
            InputEvent::Escape => window.focus(&self.focus),
            InputEvent::Changed(_) => {}
        }
    }

    // ------------------------------------------------------------- queue

    fn new_uid(&mut self) -> u64 {
        self.next_uid += 1;
        self.next_uid
    }

    pub fn play_tracks(&mut self, tracks: Vec<Track>, start: usize, cx: &mut Context<Self>) {
        let tracks: Vec<Track> = tracks.into_iter().filter(|t| t.streamable).collect();
        if tracks.is_empty() {
            return;
        }
        let start = start.min(tracks.len() - 1);
        self.queue = tracks.into_iter().map(|track| QItem { uid: 0, track }).collect();
        for i in 0..self.queue.len() {
            self.queue[i].uid = self.new_uid();
        }
        self.unshuffled = None;
        let mut index = start;
        if self.shuffle {
            self.unshuffled = Some(self.queue.clone());
            let first = self.queue.remove(start);
            shuffle(&mut self.queue);
            self.queue.insert(0, first);
            index = 0;
        }
        self.play_index(index, 0.0, false, cx);
    }

    pub fn play_shuffled(&mut self, tracks: Vec<Track>, cx: &mut Context<Self>) {
        if tracks.is_empty() {
            return;
        }
        self.shuffle = true;
        let n = tracks.len();
        let start = (now_secs() as usize) % n;
        self.play_tracks(tracks, start, cx);
        self.save_settings();
    }

    pub fn play_index(&mut self, index: usize, start: f64, paused: bool, cx: &mut Context<Self>) {
        let Some(item) = self.queue.get(index).cloned() else { return };
        self.next_key += 1;
        let key = self.next_key;
        self.queued_next = None;
        self.ended = false;
        self.player.send(Cmd::ClearQueue);
        self.now = Some(NowPlaying { track: item.track.clone(), info: None, key, index });
        if crate::recents::record(&mut self.recents, &item.track) {
            // The home shortcuts show it as soon as the user is back there.
            self.content_changed_pub();
        }
        self.snap.position = 0.0;
        let quality = self.settings.lock().quality;
        let id = item.track.id;
        self.fetch(cx, move |t| t.stream(id, quality), move |this, r, cx| {
            let Some(now) = this.now.as_mut().filter(|n| n.key == key) else { return };
            match r {
                Ok(info) => {
                    now.info = Some(info.clone());
                    this.player.send(Cmd::Play { key, info, start, paused });
                }
                Err(e) => {
                    this.notify_toast(format!("No se pudo reproducir «{}»: {e:#}", item.track.title), cx);
                    this.skip_after_error(key, cx);
                }
            }
        });
        if let Some(t) = &self.now {
            if self.wants_lyrics() {
                self.load_lyrics(t.track.id, cx);
            }
        }
        cx.notify();
    }

    fn skip_after_error(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(now) = &self.now else { return };
        if now.key != key {
            return;
        }
        let idx = now.index;
        if idx + 1 < self.queue.len() {
            self.play_index(idx + 1, 0.0, false, cx);
        }
    }

    fn next_index(&self, from: usize) -> Option<usize> {
        match self.repeat {
            Repeat::One => Some(from),
            _ if from + 1 < self.queue.len() => Some(from + 1),
            Repeat::All if !self.queue.is_empty() => Some(0),
            _ => None,
        }
    }

    /// Resolve and hand the next track to the engine for gapless playback.
    fn queue_next(&mut self, cx: &mut Context<Self>) {
        let Some(now) = &self.now else { return };
        let Some(idx) = self.next_index(now.index) else {
            self.autoplay_more(cx);
            return;
        };
        let Some(item) = self.queue.get(idx).cloned() else { return };
        self.next_key += 1;
        let key = self.next_key;
        self.queued_next = Some((key, idx));
        let quality = self.settings.lock().quality;
        self.fetch(cx, move |t| t.stream(item.track.id, quality), move |this, r, _| {
            if this.queued_next.map(|q| q.0) != Some(key) {
                return;
            }
            match r {
                Ok(info) => {
                    this.next_info = Some((key, info.clone()));
                    this.player.send(Cmd::Queue { key, info });
                }
                Err(_) => this.queued_next = None,
            }
        });
    }

    /// The queue changed around the current track: re-queue the next one.
    fn requeue(&mut self, cx: &mut Context<Self>) {
        if self.queued_next.take().is_some() {
            self.player.send(Cmd::ClearQueue);
        }
        let _ = cx;
    }

    /// End of the queue: append the radio of the last track (autoplay).
    fn autoplay_more(&mut self, cx: &mut Context<Self>) {
        if self.autoplay_loading || !self.settings.lock().autoplay || self.repeat != Repeat::Off {
            return;
        }
        let Some(seed) = self.queue.last().map(|q| q.track.clone()) else { return };
        self.autoplay_loading = true;
        let id = seed.id;
        self.fetch(cx, move |t| t.track_radio(id), |this, r, cx| {
            this.autoplay_loading = false;
            let Ok(tracks) = r else { return };
            let known: HashSet<u64> = this.queue.iter().map(|q| q.track.id).collect();
            let fresh: Vec<Track> = tracks.into_iter().filter(|t| t.streamable && !known.contains(&t.id)).take(25).collect();
            if fresh.is_empty() {
                return;
            }
            for track in fresh {
                let uid = this.new_uid();
                this.queue.push(QItem { uid, track });
            }
            if this.ended || std::mem::take(&mut this.autoplay_jump) {
                // Playback stopped, or the user pressed next: continue right away.
                if let Some(i) = this.now.as_ref().map(|n| n.index + 1) {
                    this.play_index(i, 0.0, false, cx);
                }
            } else {
                this.queue_next(cx);
            }
        });
    }

    pub fn next(&mut self, cx: &mut Context<Self>) {
        let Some(now) = &self.now else { return };
        let idx = now.index;
        let next = if idx + 1 < self.queue.len() { Some(idx + 1) } else if self.repeat == Repeat::All { Some(0) } else { None };
        match next {
            Some(n) => self.play_index(n, 0.0, false, cx),
            None => {
                // The user asked for the next track: jump as soon as autoplay has one.
                self.autoplay_jump = true;
                self.autoplay_more(cx);
            }
        }
    }

    pub fn prev(&mut self, cx: &mut Context<Self>) {
        let Some(now) = &self.now else { return };
        if self.snap.position > 3.0 || now.index == 0 {
            self.seek(0.0, cx);
        } else {
            let i = now.index - 1;
            self.play_index(i, 0.0, false, cx);
        }
    }

    pub fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if self.now.is_none() || self.ended {
            let i = self.now.as_ref().map(|n| n.index).unwrap_or(0);
            if !self.queue.is_empty() {
                self.play_index(if self.ended { 0.max(i) } else { 0 }, 0.0, false, cx);
            }
            return;
        }
        if self.snap.paused {
            self.player.send(Cmd::Resume);
        } else {
            self.player.send(Cmd::Pause);
        }
        self.snap.paused = !self.snap.paused;
        cx.notify();
    }

    pub fn seek(&mut self, secs: f64, cx: &mut Context<Self>) {
        self.player.send(Cmd::Seek(secs));
        self.snap.position = secs;
        cx.notify();
    }

    pub fn set_volume(&mut self, v: f32, cx: &mut Context<Self>) {
        self.volume = v.clamp(0.0, 1.0);
        self.muted = false;
        self.player.send(Cmd::Volume(self.volume));
        cx.notify();
    }

    pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        self.muted = !self.muted;
        self.player.send(Cmd::Volume(if self.muted { 0.0 } else { self.volume }));
        cx.notify();
    }

    pub fn toggle_shuffle(&mut self, cx: &mut Context<Self>) {
        self.shuffle = !self.shuffle;
        let cur = self.now.as_ref().map(|n| n.index);
        if self.shuffle {
            self.unshuffled = Some(self.queue.clone());
            if let Some(i) = cur {
                let rest: Vec<QItem> = self.queue.drain(i + 1..).collect();
                let mut rest = rest;
                shuffle(&mut rest);
                self.queue.extend(rest);
            } else {
                shuffle(&mut self.queue);
            }
        } else if let Some(orig) = self.unshuffled.take() {
            let cur_uid = cur.and_then(|i| self.queue.get(i)).map(|q| q.uid);
            self.queue = orig;
            if let (Some(uid), Some(now)) = (cur_uid, self.now.as_mut()) {
                if let Some(i) = self.queue.iter().position(|q| q.uid == uid) {
                    now.index = i;
                }
            }
        }
        self.requeue(cx);
        self.save_settings();
        cx.notify();
    }

    pub fn cycle_repeat(&mut self, cx: &mut Context<Self>) {
        self.repeat = match self.repeat {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
        self.requeue(cx);
        self.save_settings();
        cx.notify();
    }

    pub fn add_to_queue(&mut self, track: Track, next: bool, cx: &mut Context<Self>) {
        let item = QItem { uid: self.new_uid(), track };
        match (&self.now, next) {
            (Some(now), true) => {
                let at = (now.index + 1).min(self.queue.len());
                self.queue.insert(at, item);
            }
            _ => self.queue.push(item),
        }
        if self.now.is_none() {
            self.play_index(0, 0.0, false, cx);
        }
        self.requeue(cx);
        self.notify_toast(if next { "Se reproducirá a continuación" } else { "Añadida a la cola" }, cx);
    }

    pub fn remove_from_queue(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(now) = &mut self.now else {
            if index < self.queue.len() {
                self.queue.remove(index);
            }
            return;
        };
        if index == now.index || index >= self.queue.len() {
            return;
        }
        self.queue.remove(index);
        if index < now.index {
            now.index -= 1;
        }
        self.requeue(cx);
        cx.notify();
    }

    /// Set a track's rating in half stars (0 clears it).
    pub fn set_rating(&mut self, track: u64, halves: u8, cx: &mut Context<Self>) {
        self.ratings.set(track, halves);
        if sort::SortKey::Rating == self.sort_of("tracks").0 {
            self.apply_sorts();
            self.content_changed_pub();
        }
        cx.notify();
    }

    /// Ease the visual levels towards the audio. Compressed and heavily smoothed:
    /// the backgrounds should breathe, not dance.
    fn update_vis(&mut self) {
        let raw = self.player.levels();
        let playing = self.now.is_some() && !self.snap.paused && !self.ended;
        for i in 0..3 {
            let target = if playing {
                // Compare with the running mean: louder than usual -> towards 1,
                // quieter -> towards 0, so it moves with the music's dynamics at any volume.
                let m = &mut self.vis_mean[i];
                *m += (raw[i] - *m) * 0.006;
                (0.5 + (raw[i] - *m) / (*m + 0.004) * 1.1).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let k = if target > self.vis[i] { 0.16 } else { 0.05 };
            self.vis[i] += (target - self.vis[i]) * k;
        }
    }

    pub fn set_album_rating(&mut self, album: u64, halves: u8, cx: &mut Context<Self>) {
        self.ratings.set_album(album, halves);
        if sort::SortKey::Rating == self.sort_of("albums").0 {
            self.apply_sorts();
            self.content_changed_pub();
        }
        cx.notify();
    }

    pub fn toggle_favorite(&mut self, track: &Track, cx: &mut Context<Self>) {
        let id = track.id;
        let fav = !self.fav_ids.contains(&id);
        if fav {
            self.heart_pop = Some((id, std::time::Instant::now()));
            self.fav_ids.insert(id);
            if let Load::Ready(list) = &mut self.fav_tracks {
                list.insert(0, track.clone());
            }
        } else {
            self.fav_ids.remove(&id);
            if let Load::Ready(list) = &mut self.fav_tracks {
                list.retain(|t| t.id != id);
            }
        }
        self.content_changed();
        self.fetch(cx, move |t| t.set_favorite_track(id, fav), move |this, r, cx| {
            if let Err(e) = r {
                this.notify_toast(format!("Error: {e:#}"), cx);
            } else {
                this.notify_toast(if fav { "Añadida a Mi colección" } else { "Eliminada de Mi colección" }, cx);
            }
        });
        cx.notify();
    }

    pub fn toggle_favorite_album(&mut self, album: &Album, cx: &mut Context<Self>) {
        let id = album.id;
        let fav = !self.fav_album_ids.contains(&id);
        if fav {
            self.fav_album_ids.insert(id);
        } else {
            self.fav_album_ids.remove(&id);
        }
        self.fav_albums = Load::Idle;
        self.fetch(cx, move |t| t.set_favorite_album(id, fav), move |this, r, cx| {
            if let Err(e) = r {
                this.notify_toast(format!("Error: {e:#}"), cx);
            }
        });
        cx.notify();
    }

    pub fn start_radio(&mut self, track: &Track, cx: &mut Context<Self>) {
        let first = track.clone();
        let id = track.id;
        self.fetch(cx, move |t| t.track_radio(id), move |this, r, cx| match r {
            Ok(mut tracks) => {
                tracks.retain(|t| t.id != first.id);
                tracks.insert(0, first);
                this.play_tracks(tracks, 0, cx);
            }
            Err(e) => this.notify_toast(format!("Radio no disponible: {e:#}"), cx),
        });
    }

    pub fn load_lyrics(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.lyrics.contains_key(&id) {
            return;
        }
        self.lyrics.insert(id, Load::Loading);
        self.lyrics_line = None;
        self.lyrics_follow = true;
        let track = self.now.as_ref().map(|n| n.track.clone()).filter(|t| t.id == id);
        self.fetch(
            cx,
            move |t| match track {
                Some(track) => t.lyrics_any(&track),
                None => t.lyrics(id),
            },
            move |this, r, _| {
                this.lyrics.insert(id, into_load(r));
            },
        );
    }

    // ------------------------------------------------------------ settings

    pub fn set_quality(&mut self, q: Quality, cx: &mut Context<Self>) {
        {
            let mut s = self.settings.lock();
            s.quality = q;
            s.save();
        }
        self.requeue(cx);
        // Restart the current track at the same position in the new quality.
        if let Some(now) = &self.now {
            let (i, pos, paused) = (now.index, self.snap.position, self.snap.paused);
            self.play_index(i, pos, paused, cx);
        }
        cx.notify();
    }

    pub fn toggle_setting(&mut self, which: &str, cx: &mut Context<Self>) {
        {
            let mut s = self.settings.lock();
            match which {
                "normalize" => s.normalize = !s.normalize,
                "autoplay" => s.autoplay = !s.autoplay,
                "discord" => s.discord = !s.discord,
                "audio_metadata" => s.audio_metadata = !s.audio_metadata,
                "show_ratings" => s.show_ratings = !s.show_ratings,
                "lyrics_romanize" => s.lyrics_romanize = !s.lyrics_romanize,
                "hide_uploads" => {
                    s.hide_uploads = !s.hide_uploads;
                    if s.hide_uploads && self.home_tab == 2 {
                        self.home_tab = 0;
                    }
                }
                _ => {}
            }
            s.save();
        }
        if which == "discord" {
            let on = self.settings.lock().discord;
            self.discord.set_enabled(on);
        }
        if which == "normalize" {
            // Applies from the next track; re-send prefs so the engine knows.
            let s = self.settings.lock().clone();
            self.player.send(Cmd::Output(OutputPrefs {
                device: s.output_device,
                exclusive: s.exclusive_mode,
                force_volume: s.force_volume,
                normalize: s.normalize,
            }));
        }
        cx.notify();
    }

    pub fn set_output(&mut self, device: Option<String>, exclusive: bool, force_volume: bool, cx: &mut Context<Self>) {
        {
            let mut s = self.settings.lock();
            s.output_device = device.clone();
            s.exclusive_mode = exclusive;
            s.force_volume = force_volume;
            s.save();
        }
        let normalize = self.settings.lock().normalize;
        self.player.send(Cmd::Output(OutputPrefs { device, exclusive, force_volume, normalize }));
        self.player.send(Cmd::Volume(if self.muted { 0.0 } else { self.volume }));
        cx.notify();
    }

    // ----------------------------------------------------------------- tick

    fn tick(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        // Media keys / system media widget.
        let actions = self.media.as_mut().map(|m| m.poll()).unwrap_or_default();
        for a in actions {
            changed = true;
            match a {
                media::MediaAction::Toggle => self.toggle_play(cx),
                media::MediaAction::Play if self.snap.paused || self.ended => self.toggle_play(cx),
                media::MediaAction::Pause | media::MediaAction::Stop if !self.snap.paused => self.toggle_play(cx),
                media::MediaAction::Next => self.next(cx),
                media::MediaAction::Prev => self.prev(cx),
                media::MediaAction::SeekTo(s) => self.seek(s, cx),
                media::MediaAction::SeekBy(d) => {
                    let p = (self.snap.position + d).clamp(0.0, self.duration());
                    self.seek(p, cx)
                }
                _ => {}
            }
        }
        while let Ok(ev) = self.events.try_recv() {
            changed = true;
            match ev {
                audio::Event::Finished { key } => {
                    if self.now.as_ref().map(|n| n.key) == Some(key) && self.queued_next.is_none() {
                        // Keep the track shown, ready to play again.
                        self.ended = true;
                    }
                }
                audio::Event::Error { key, message } => {
                    self.notify_toast(message, cx);
                    if let Some(k) = key {
                        self.skip_after_error(k, cx);
                    }
                }
            }
        }

        let snap = self.player.snapshot();
        // The engine moved on to the queued track (gapless transition).
        if let (Some(k), Some((qk, qi))) = (snap.key, self.queued_next) {
            if k == qk {
                if let Some(item) = self.queue.get(qi).cloned() {
                    let info = self.next_info.take().filter(|(key, _)| *key == qk).map(|(_, i)| i);
                    self.now = Some(NowPlaying { track: item.track, info, key: qk, index: qi });
                    self.queued_next = None;
                    if self.wants_lyrics() {
                        let id = self.now.as_ref().unwrap().track.id;
                        self.load_lyrics(id, cx);
                    }
                    changed = true;
                }
            }
        }
        // Queue the next track once the current one is downloaded or near its end.
        if let Some(now) = &self.now {
            if snap.key == Some(now.key) && self.queued_next.is_none() {
                let dur = if snap.duration > 0.0 { snap.duration } else { now.track.duration as f64 };
                if snap.downloaded >= 0.999 || dur - snap.position < 25.0 {
                    self.queue_next(cx);
                }
            }
        }
        let differs = (snap.position - self.snap.position).abs() >= 0.2
            || snap.key != self.snap.key
            || snap.paused != self.snap.paused
            || snap.buffering != self.snap.buffering
            || snap.output != self.snap.output;
        if self.drag != Some(Drag::Progress) {
            self.snap = snap;
            self.snap_at = std::time::Instant::now();
        } else {
            let pos = self.snap.position;
            self.snap = snap;
            self.snap.position = pos;
        }
        if self.volume_hint_until.is_some_and(|t| std::time::Instant::now() >= t) {
            self.volume_hint_until = None;
            changed = true;
        }
        if let Some((_, at)) = &self.toast {
            if at.elapsed() > Duration::from_secs(4) {
                self.toast = None;
                changed = true;
            }
        }
        {
            let playing = self.now.is_some() && !self.snap.paused && !self.ended && !self.snap.buffering;
            let dur = self.duration();
            let pos = self.snap.position;
            self.discord.update(self.now.as_ref().map(|n| (&n.track, dur)), playing, pos);
        }
        if let Some(m) = self.media.as_mut() {
            let playing = self.now.is_some() && !self.snap.paused && !self.ended;
            m.update(self.now.as_ref().map(|n| (n.key, &n.track)), playing, self.snap.position);
        }
        if changed || differs {
            cx.notify();
        }
    }

    // ------------------------------------------------------------- dragging

    pub fn drag_to(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(drag) = self.drag else { return };
        let b = if drag == Drag::Progress { self.progress_bounds } else { self.volume_bounds };
        let w = f32::from(b.size.width).max(1.0);
        let frac = ((f32::from(pos.x) - f32::from(b.origin.x)) / w).clamp(0.0, 1.0);
        self.drag_value = frac;
        match drag {
            Drag::Progress => {
                self.snap.position = frac as f64 * self.duration();
            }
            Drag::Volume => self.set_volume(frac, cx),
        }
        cx.notify();
    }

    pub fn end_drag(&mut self, cx: &mut Context<Self>) {
        match self.drag.take() {
            Some(Drag::Progress) => {
                let secs = self.drag_value as f64 * self.duration();
                self.seek(secs, cx);
            }
            Some(Drag::Volume) => self.save_settings(),
            None => {}
        }
    }

    pub fn duration(&self) -> f64 {
        if self.snap.duration > 0.0 && self.snap.key == self.now.as_ref().map(|n| n.key) {
            self.snap.duration
        } else {
            self.now.as_ref().map(|n| n.track.duration as f64).unwrap_or(0.0)
        }
    }
}

fn into_load<T>(r: Result<T>) -> Load<T> {
    match r {
        Ok(v) => Load::Ready(v),
        Err(e) => Load::Failed(format!("{e:#}")),
    }
}

fn shuffle<T>(v: &mut [T]) {
    let mut seed = now_secs() | 1;
    for i in (1..v.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
}

impl Focusable for AppView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.signed_in {
            return pages::login_screen(self, cx).into_any_element();
        }
        let perf_t0 = std::time::Instant::now();
        self.step_smooth_scroll();
        self.drive_animations(window);
        self.bg_angle_now = self.bg_angle(window);
        let np_overlay = if self.now_playing_open && self.now.is_some() { Some(now_playing::render(self, window, cx)) } else { None };
        let blocks = self.blocks(window);
        if self.list_len == usize::MAX {
            // New page: always start at the top.
            self.list.reset(blocks.len());
            self.list.scroll_to(gpui::ListOffset { item_ix: 0, offset_in_item: sc(0.) });
        } else if self.list_len != blocks.len() || self.list_len == usize::MAX - 1 {
            // Same page, new content: re-measure every block but keep the
            // reading position (a full splice would jump back to the top).
            let keep = self.list.logical_scroll_top();
            let old = self.list.item_count();
            self.list.splice(0..old, blocks.len());
            if keep.item_ix < blocks.len() {
                self.list.scroll_to(keep);
            }
        }
        self.list_len = blocks.len();
        self.current_blocks = blocks;
        perf_note(perf_t0.elapsed());

        div()
            .id("root")
            .relative()
            .key_context("App")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &PlayPause, _, cx| this.toggle_play(cx)))
            .on_action(cx.listener(|this, _: &NextTrack, _, cx| this.next(cx)))
            .on_action(cx.listener(|this, _: &PrevTrack, _, cx| this.prev(cx)))
            .on_action(cx.listener(|this, _: &SeekForward, _, cx| {
                let p = (this.snap.position + 10.0).min(this.duration());
                this.seek(p, cx)
            }))
            .on_action(cx.listener(|this, _: &SeekBack, _, cx| {
                let p = (this.snap.position - 10.0).max(0.0);
                this.seek(p, cx)
            }))
            .on_action(cx.listener(|this, _: &VolumeUp, _, cx| {
                let v = this.volume + 0.05;
                this.set_volume(v, cx);
                this.save_settings();
            }))
            .on_action(cx.listener(|this, _: &VolumeDown, _, cx| {
                let v = this.volume - 0.05;
                this.set_volume(v, cx);
                this.save_settings();
            }))
            .on_action(cx.listener(|this, _: &ToggleMute, _, cx| this.toggle_mute(cx)))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| this.navigate(Page::Settings, cx)))
            .on_action(cx.listener(|this, _: &GoHome, _, cx| this.navigate(Page::Home, cx)))
            .on_action(cx.listener(|this, _: &ToggleShuffle, _, cx| this.toggle_shuffle(cx)))
            .on_action(cx.listener(|this, _: &GoBack, _, cx| this.go_back(cx)))
            .on_action(cx.listener(|this, _: &GoForward, _, cx| this.go_forward(cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.step_zoom(1, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.step_zoom(-1, cx)))
            .on_action(cx.listener(|this, _: &ZoomReset, _, cx| this.set_zoom_level(1.0, cx)))
            // The mouse's back / forward buttons.
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Back),
                cx.listener(|this, _, _, cx| this.go_back(cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Forward),
                cx.listener(|this, _, _, cx| this.go_forward(cx)),
            )
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                let fh = this.search_input.focus_handle(cx);
                window.focus(&fh);
            }))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, window, cx| {
                if this.scrollbar_drag {
                    if ev.pressed_button == Some(MouseButton::Left) {
                        this.scrollbar_to(ev.position);
                        cx.notify();
                        return;
                    }
                    this.scrollbar_drag = false;
                    cx.notify();
                }
                if let Some(d) = this.win_drag {
                    if ev.pressed_button == Some(MouseButton::Left) {
                        winctl::drag_move(window, d);
                        return;
                    }
                    this.win_drag = None;
                    winctl::drag_end();
                }
                if this.drag.is_some() {
                    // Button released outside the window: finish the drag
                    // instead of letting every later move change the value.
                    if ev.pressed_button != Some(MouseButton::Left) {
                        this.drag = None;
                        this.bar_click_block = false;
                        cx.notify();
                        return;
                    }
                    this.drag_to(ev.position, cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.scrollbar_drag = false;
                if this.win_drag.take().is_some() {
                    winctl::drag_end();
                }
                this.end_drag(cx)
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.scrollbar_drag = false;
                if this.win_drag.take().is_some() {
                    winctl::drag_end();
                }
                this.end_drag(cx)
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::bg())
            .text_color(theme::text())
            .text_size(sc(14.))
            .font_family(theme::FONT)
            .child(titlebar::render(self, window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(sc(0.))
                    .child(sidebar::render(self, cx))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w(sc(0.))
                            .h_full()
                            .flex()
                            .child(pages::content(self, window, cx)),
                    )
                    .when(self.show_queue || self.show_lyrics, |d| d.child(pages::side_panel(self, cx))),
            )
            .child(smooth::zoom_catcher(cx.entity()))
            .children(np_overlay)
            .child(player_bar::render(self, window, cx))
            .children(pages::overlays(self, window, cx))
            .children(appmenu::overlay(self, window, cx))
            .children(haptics::swipe_indicator(self))
            .children(playlists::dialog_overlay(self, cx))
            .into_any_element()
    }
}

/// `TIDE_PERF=1`: log once a second how many frames were rendered and what the
/// view's own render work cost (layout and paint are not included).
fn perf_note(spent: std::time::Duration) {
    use std::cell::RefCell;
    thread_local!(static ACC: RefCell<(std::time::Instant, u32, f64, f64)> = RefCell::new((std::time::Instant::now(), 0, 0.0, 0.0)));
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var("TIDE_PERF").is_ok()) {
        return;
    }
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        let ms = spent.as_secs_f64() * 1000.0;
        a.1 += 1;
        a.2 += ms;
        a.3 = a.3.max(ms);
        if a.0.elapsed().as_secs_f64() >= 1.0 {
            log::info!("perf: {} frames/s, render avg {:.2} ms, max {:.2} ms", a.1, a.2 / a.1 as f64, a.3);
            *a = (std::time::Instant::now(), 0, 0.0, 0.0);
        }
    });
}
