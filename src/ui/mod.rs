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

/// One line of text that ends in "…" when it does not fit.
///
/// GPUI's `truncate()` keeps the text unwrapped and caches its first
/// measurement, which inside a flex row is taken before the width is known, so
/// the text was clipped without an ellipsis. Wrapping clamped to one line is
/// measured again once the width is known, and gets the ellipsis.
pub trait OneLine: gpui::Styled + Sized {
    fn one_line(self) -> Self {
        self.overflow_hidden().text_ellipsis().line_clamp(1)
    }
}
impl<T: gpui::Styled> OneLine for T {}

/// Zoom levels, as in a browser.
pub const ZOOM_STEPS: [f32; 11] = [0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];
mod album_inline;
mod appmenu;
mod cover_view;
mod ctxmenu;
mod debug;
pub mod fx;
mod glass;
mod haptics;
pub mod images;
mod info;
pub mod input;
mod interact;
mod library;
mod local;
mod media;
mod motion;
pub mod now_playing;
mod onboarding;
mod pages;
mod player_bar;
pub mod playlists;
mod replay;
mod scrollbar;
mod search_menu;
mod settings;
mod sidebar;
mod smooth;
mod sort;
pub mod theme;
mod tilt;
mod titlebar;
mod widgets;
mod winctl;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use gpui::{
    actions, div, prelude::*, px, App, Bounds, Context, Entity, FocusHandle, Focusable, KeyBinding, ListAlignment, ListState, MouseButton,
    MouseMoveEvent, Pixels, Point, ScrollHandle, Window,
};
use parking_lot::Mutex;

use crate::audio::{self, Cmd, OutputPrefs, Player, Snapshot};
use crate::config::{Quality, Repeat, Settings};
use crate::tidal::*;
use input::{InputEvent, TextInput};

actions!(
    app,
    [
        OpenSettings,
        GoHome,
        ToggleShuffle,
        PlayPause,
        NextTrack,
        PrevTrack,
        FocusSearch,
        SeekForward,
        SeekBack,
        VolumeUp,
        VolumeDown,
        ToggleMute,
        GoBack,
        GoForward,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        CloseOverlay
    ]
);

pub fn bindings() -> Vec<KeyBinding> {
    // Single-key shortcuts must not fire while typing in a text field.
    let ctx = Some("App && !TextInput");
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
        KeyBinding::new("escape", CloseOverlay, ctx),
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
    /// Albums played recently in Abyss.
    Recent,
    /// Personal listening statistics.
    Replay,
    /// "Mixes & novedades": the user's mixes, Staff Picks and Uploads.
    Discover,
    Settings,
}

#[derive(Default)]
pub enum Load<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
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
    QueueItem(u64),
    /// Sort options of a collection section.
    Sort(&'static str),
    /// A track inside one of the user's own playlists (can be removed).
    PlaylistTrack {
        uuid: String,
        index: usize,
        track: Track,
    },
    /// Pick a playlist to add these tracks to.
    AddToPlaylist(Vec<Track>),
    Album(Album),
    Artist(Artist),
}

pub enum LoginState {
    Idle,
    Waiting(DeviceCode),
    Web(PkceLogin),
    Failed(String),
}

pub struct AppView {
    pub welcome_step: Option<usize>,
    pub tidal: Tidal,
    data_epoch: u64,
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
    /// Quick-search menu under the header box.
    pub search_query: String,
    pub search_open: bool,
    pub search_gen: u64,
    pub search_scroll: ScrollHandle,
    pub search_sb_drag: bool,
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
    pub lyrics_epoch: u64,

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
    /// Which panel was open last (drawn while the side panel closes).
    pub side_last_lyrics: bool,
    pub side_panel_w: f32,
    queue_open_view: Option<&'static str>,
    queue_center_pending: bool,
    queue_current: Option<(u64, usize)>,
    queue_heights: HashMap<&'static str, f32>,
    side_resize: Option<(f32, f32)>,
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
    pub ambient_seen: bool,
    pub palette_from: Vec<gpui::Rgba>,
    pub palette_to: Vec<gpui::Rgba>,
    pub palette_at: std::time::Instant,
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
    /// Now Playing cover tilt (follows the mouse) and where the cover is.
    pub np_tilt: tilt::Tilt,
    pub np_cover_bounds: Bounds<Pixels>,
    /// Where the custom-accent hue strip is (Ajustes), and whether it is being dragged.
    pub hue_bounds: Bounds<Pixels>,
    pub hue_drag: bool,
    /// Library: search text of each category (kept for the session) and
    /// where the sort selector is (its options open under it).
    pub lib_queries: HashMap<&'static str, String>,
    pub lib_sort_bounds: Bounds<Pixels>,
    /// Window width in logical px at the last render (narrow-window layout).
    pub viewport_w: f32,
    /// State of the rotating Now Playing / player-bar background.
    pub bg_spin: fx::BgSpin,
    /// Its angle for the current frame (computed once per render).
    pub bg_angle_now: f32,
    /// Albums played here (newest first) and when the home feed was last fetched.
    pub recents: Vec<crate::recents::RecentAlbum>,
    /// Listening history recorded by Abyss (Replay) and its recorder.
    pub history: Vec<crate::history::Entry>,
    pub local_tracks: Vec<Track>,
    pub local_busy: bool,
    pub local_editor: Option<local::LocalEditor>,
    /// Albums made from the local tracks (also cached in `albums`).
    pub local_albums: Vec<Album>,
    /// The Library's lists: TIDAL favourites and local files together.
    pub lib_tracks: Vec<Track>,
    pub lib_albums: Vec<Album>,
    pub local_artists: Vec<Artist>,
    pub lib_artists: Vec<Artist>,
    pub recorder: crate::history::Recorder,
    pub history_tick: std::time::Instant,
    /// Replay: chosen period and the statistics computed for it.
    pub replay_period: replay::Period,
    pub replay_all_tracks: bool,
    #[allow(clippy::type_complexity)]
    pub replay_cache: Option<((usize, u64, replay::Period, u64), std::rc::Rc<replay::Stats>)>,
    /// Where the Replay dashboard is on screen (Share Image).
    pub replay_bounds: Bounds<Pixels>,
    pub replay_head_bounds: Bounds<Pixels>,
    /// Pictures of artists shown in Replay (artist id -> picture URL).
    pub artist_pics: HashMap<u64, Load<Option<String>>>,
    pub home_loaded: Option<std::time::Instant>,
    /// Slow running mean of each band, so `vis` follows the music's dynamics rather than its loudness.
    vis_mean: [f32; 3],
    /// Lyrics follow the song (off after a manual scroll).
    pub lyrics_follow: bool,
    /// Per-track lyrics timing correction in seconds.
    pub lyrics_scroll: ScrollHandle,
    /// Synced line last scrolled to (auto-follow).
    pub lyrics_line: Option<usize>,
    pub lyrics_scroll_at: std::time::Instant,
    /// "Sincronizar" was pressed and the lyrics are gliding back (the icon spins).
    pub lyrics_sync_at: Option<std::time::Instant>,
    /// Start of the icon's spin; kept until the button has faded out.
    pub lyrics_spin_at: Option<std::time::Instant>,
    /// Album cover shown full screen (see cover_view.rs).
    pub cover_view: Option<cover_view::CoverView>,
    /// Song / Album / Artist Info panel, and the stream facts read for it.
    pub info_panel: Option<info::InfoTarget>,
    #[allow(clippy::type_complexity)]
    pub tech_info: HashMap<u64, Load<(crate::tidal::probe::TechInfo, String)>>,
    /// The context menu's "Add to playlist" list is unfolded.
    pub ctx_pl_open: bool,
    /// Previous active lyric line and a counter, to fade lines in and out.
    pub lyrics_prev: Option<usize>,
    pub lyrics_changes: usize,
    /// When `snap` was taken (to interpolate the progress bar).
    pub snap_at: std::time::Instant,
    /// Sidebar width animation: (width at start, start time).
    pub sidebar_anim: Option<(f32, std::time::Instant)>,
    /// Sidebar hover: pointer inside its area, and when it left (close delay).
    pub sb_hover: bool,
    /// Sidebar unfolding (0..1) and width (logical px) for this frame.
    pub sb_t: f32,
    pub sb_w: f32,
    pub sb_left_at: Option<std::time::Instant>,
    /// When the current frame's render started, and whether a paced frame is
    /// already scheduled (see `pace_frames`).
    /// Track-change cross-fades (cover, background) for Now Playing and the bar.
    pub np_cover_fade: motion::Crossfade<gpui::ImageSource>,
    /// Whether the page showed the loading indicator last frame, and when its
    /// content replaced it (for the content fade-in).
    pub page_was_loading: bool,
    /// Height of the page list's visible area at the last frame.
    pub list_view_h: f32,
    pub content_in_at: Option<std::time::Instant>,
    pub bar_cover_fade: motion::Crossfade<gpui::ImageSource>,
    pub np_frames_fade: motion::Crossfade<std::sync::Arc<Vec<std::sync::Arc<gpui::RenderImage>>>>,
    pub frame_started: std::time::Instant,
    pub frame_timer: bool,
    /// When the last paced (ambient) frame was asked for.
    pub last_paced: std::time::Instant,
    /// The requested native full-screen state, applied on the window's next render.
    pub np_fullscreen_request: Option<bool>,
    /// Bumped on every page change to restart the page transition.
    pub page_anim: usize,
    /// A drag on the progress/volume bar started: ignore the bar's click.
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
    pub current_blocks: Vec<pages::Block>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = Arc::new(Mutex::new(Settings::load()));
        theme::set_light(settings.lock().light_theme);
        theme::set_accent(settings.lock().accent);
        set_zoom(std::env::var("TIDE_ZOOM").ok().and_then(|z| z.parse().ok()).unwrap_or_else(|| settings.lock().zoom).clamp(0.5, 2.0));
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

        let search_input = cx.new(|cx| TextInput::new(crate::i18n::t("search"), cx).compact());
        cx.subscribe_in(&search_input, window, Self::on_search_event).detach();
        let login_input = cx.new(|cx| TextInput::new(crate::i18n::t("type_or_paste_the_tidal_login_address"), cx));
        cx.subscribe_in(&login_input, window, |this, _, ev: &InputEvent, _, cx| {
            if let InputEvent::Submit(text) = ev {
                this.finish_web_login(text.clone(), cx);
            }
        })
        .detach();

        let filter_input = cx.new(|cx| TextInput::new(crate::i18n::t("filter"), cx).compact());
        cx.subscribe_in(&filter_input, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Changed(text) => {
                if let Some(sec) = library::section(&this.page) {
                    this.lib_queries.insert(sec, text.clone());
                }
                this.filter = text.trim().to_lowercase();
                this.content_changed_pub();
                cx.notify();
            }
            InputEvent::Escape | InputEvent::Submit(_) => window.focus(&this.focus),
        })
        .detach();
        let discord_input = cx.new(|cx| TextInput::new(crate::i18n::t("discord_application_id"), cx));
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
            let id = std::env::var("ABYSS_DISCORD_ID")
                .or_else(|_| std::env::var("TIDE_DISCORD_ID"))
                .ok()
                .or_else(|| Some(st.discord_client_id.clone()).filter(|s| !s.is_empty()))
                .unwrap_or_else(|| crate::config::DISCORD_APP_ID.to_string());
            crate::discord::Discord::new(id, st.discord)
        };
        let dlg_title = cx.new(|cx| TextInput::new(crate::i18n::t("my_playlist"), cx));
        let dlg_desc = cx.new(|cx| TextInput::new("", cx));
        for input in [&dlg_title, &dlg_desc] {
            cx.subscribe_in(input, window, |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Submit(_) => this.submit_dialog(window, cx),
                InputEvent::Escape => this.close_dialog(window, cx),
                _ => {}
            })
            .detach();
        }

        let history = crate::history::load();
        let ratings = crate::ratings::Ratings::load();
        let storage_unavailable = history.is_err() || ratings.is_err();
        let history = history.unwrap_or_else(|error| {
            log::warn!("No se pudo cargar Replay: {error:#}");
            Vec::new()
        });
        let ratings = ratings.unwrap_or_else(|error| {
            log::warn!("No se pudieron cargar las valoraciones: {error:#}");
            Default::default()
        });
        let mut this = Self {
            tidal,
            data_epoch: 0,
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
            search_query: String::new(),
            search_open: false,
            search_gen: 0,
            search_scroll: ScrollHandle::new(),
            search_sb_drag: false,
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
            lyrics_epoch: 0,
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
            side_last_lyrics: false,
            side_panel_w: 0.0,
            queue_open_view: None,
            queue_center_pending: false,
            queue_current: None,
            queue_heights: HashMap::new(),
            side_resize: None,
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
            ambient_seen: false,
            palette_from: Vec::new(),
            palette_to: Vec::new(),
            palette_at: std::time::Instant::now(),
            heart_pop: None,
            ratings,
            rating_hover: None,
            vis: [0.0; 3],
            haptics: Default::default(),
            nav_dir: 1.0,
            scrollbar_drag: false,
            np_tilt: Default::default(),
            np_cover_bounds: Bounds::default(),
            hue_bounds: Bounds::default(),
            hue_drag: false,
            lib_queries: HashMap::new(),
            lib_sort_bounds: Bounds::default(),
            viewport_w: 0.0,
            bg_spin: fx::BgSpin::Stopped(0.0),
            bg_angle_now: 0.0,
            recents: crate::recents::load(),
            history,
            local_tracks: crate::local::load().unwrap_or_else(|error| {
                log::warn!("No se pudo cargar la biblioteca local: {error:#}");
                Vec::new()
            }),
            local_busy: false,
            local_editor: None,
            local_albums: Vec::new(),
            local_artists: Vec::new(),
            lib_artists: Vec::new(),
            lib_tracks: Vec::new(),
            lib_albums: Vec::new(),
            recorder: Default::default(),
            history_tick: std::time::Instant::now(),
            replay_period: Default::default(),
            replay_all_tracks: false,
            replay_cache: None,
            replay_bounds: Bounds::default(),
            replay_head_bounds: Bounds::default(),
            artist_pics: HashMap::new(),
            home_loaded: None,
            vis_mean: [0.0; 3],
            lyrics_follow: true,
            lyrics_scroll: ScrollHandle::new(),
            lyrics_line: None,
            lyrics_scroll_at: std::time::Instant::now(),
            lyrics_sync_at: None,
            lyrics_spin_at: None,
            cover_view: None,
            info_panel: None,
            tech_info: HashMap::new(),
            ctx_pl_open: false,
            lyrics_prev: None,
            lyrics_changes: 0,
            snap_at: std::time::Instant::now(),
            sidebar_anim: None,
            sb_hover: false,
            sb_t: 0.0,
            sb_w: sidebar::RAIL_W,
            sb_left_at: None,
            np_cover_fade: Default::default(),
            page_was_loading: false,
            list_view_h: 600.0,
            content_in_at: None,
            bar_cover_fade: Default::default(),
            np_frames_fade: Default::default(),
            frame_started: std::time::Instant::now(),
            frame_timer: false,
            last_paced: std::time::Instant::now(),
            np_fullscreen_request: None,
            welcome_step: (!s.welcome_complete).then_some(0),
            page_anim: 0,
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
            signed_in: s.signed_in() && s.preferred_service != "local",
            devices: audio::output::device_names(),
            current_blocks: Vec::new(),
        };
        if storage_unavailable {
            this.notify_toast(crate::i18n::t("storage.partial_load_warning"), cx);
        }
        let view = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            let _ = view.update(cx, |this, _| this.flush_history());
            true
        });
        cx.on_app_quit(|this, _| {
            this.flush_history();
            std::future::ready(())
        })
        .detach();
        if this.signed_in {
            this.after_login(cx);
        }
        if s.preferred_service == "local" {
            this.page = Page::MyTracks;
        }
        // The welcome guide shows once: it counts as seen as soon as it appears, so
        // closing the window before the last step does not bring it back.
        if this.welcome_step.is_some() {
            let mut s = this.settings.lock();
            s.welcome_complete = true;
            s.save();
        }
        this.local_changed();
        this.rescan_local_folders(cx);
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

    fn flush_history(&mut self) {
        if let Err(error) = self.recorder.flush() {
            log::warn!("No se pudo guardar Replay al cerrar Abyss: {error:#}");
        }
    }

    // ------------------------------------------------------------ helpers

    /// Run `job` on a background thread, then `done` on the UI thread.
    pub fn fetch<T: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        job: impl FnOnce(Tidal) -> Result<T> + Send + 'static,
        done: impl FnOnce(&mut Self, Result<T>, &mut Context<Self>) + 'static,
    ) {
        let tidal = self.tidal.request_snapshot();
        let epoch = self.data_epoch;
        cx.spawn(async move |this, cx| {
            let r = cx.background_executor().spawn(async move { job(tidal) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.data_epoch != epoch {
                    return;
                }
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
        self.fetch(
            cx,
            move |t| t.finish_pkce_login(&login, &pasted),
            |this, r, cx| {
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
            },
        );
    }

    fn reset_data(&mut self) {
        self.data_epoch = self.data_epoch.wrapping_add(1);
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
        self.local_changed();
    }

    /// Sign out and go straight to the web login (from Settings).
    pub fn relogin_full_quality(&mut self, cx: &mut Context<Self>) {
        self.sign_out(cx);
        self.start_web_login(cx);
    }

    pub fn start_login(&mut self, cx: &mut Context<Self>) {
        self.login = LoginState::Idle;
        self.fetch(
            cx,
            |t| t.start_device_login(),
            |this, r, cx| match r {
                Ok(code) => {
                    let _ = open::that(&code.verification_uri);
                    this.login = LoginState::Waiting(code.clone());
                    this.poll_login(code, cx);
                }
                Err(e) => this.login = LoginState::Failed(format!("{e:#}")),
            },
        );
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
                                this.login = LoginState::Failed(crate::i18n::t("the_code_expired_try_again").into());
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
        self.search.clear();
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
        self.reset_data();
        cx.notify();
    }

    pub fn load_library(&mut self, cx: &mut Context<Self>) {
        self.my_playlists = Load::Loading;
        self.fetch(cx, |t| t.my_playlists(), |this, r, _| this.my_playlists = into_load(r));
        self.fetch(
            cx,
            |t| t.favorite_ids(),
            |this, r, _| {
                if let Ok((ids, artists)) = r {
                    this.fav_ids = ids.into_iter().collect();
                    this.fav_artist_ids = artists.into_iter().collect();
                }
            },
        );
        self.fetch(
            cx,
            |t| t.favorite_albums(),
            |this, r, _| {
                if let Ok(a) = &r {
                    this.fav_album_ids = a.iter().map(|a| a.id).collect();
                }
                this.fav_albums = into_load(r);
            },
        );
    }

    // -------------------------------------------------------- navigation

    pub fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        if !self.signed_in {
            match page {
                Page::Settings | Page::MyTracks | Page::MyAlbums | Page::MyArtists | Page::Search(_) => {}
                Page::Album(id) | Page::Artist(id) if crate::local::is_local_id(id) => {}
                Page::Home => {
                    self.player.send(Cmd::Pause);
                    self.snap.paused = true;
                }
                _ => return,
            }
        }
        log::info!("navigate -> {page:?}");
        self.nav_dir = 1.0;
        if page == self.page {
            return;
        }
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
        self.notify_toast(crate::i18n::text("zoom", &[format!("{}", (z * 100.0).round() as i32)]), cx);
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
        if self.now_playing_open {
            self.np_fullscreen_request = Some(false);
        }
        self.stop_smooth_scroll();
        self.page_tint = None;
        if !self.filter.is_empty() {
            self.filter.clear();
            self.filter_input.update(cx, |i, cx| i.set_text("", cx));
        }
        let ph = match self.page {
            Page::MyTracks => crate::i18n::t("filter_tracks"),
            Page::MyAlbums => crate::i18n::t("filter_albums"),
            Page::MyArtists => crate::i18n::t("filter_artists"),
            Page::MyPlaylists => crate::i18n::t("filter_playlists"),
            Page::MyMixes => crate::i18n::t("filter_mixes"),
            _ => crate::i18n::t("filter_by_title_artist_or_album"),
        };
        self.filter_input.update(cx, |i, cx| i.set_placeholder(ph, cx));
        if library::section(&self.page).is_some() {
            // Library: restores the category's own search and remembers it.
            self.after_library_switch(cx);
        }
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
                self.fetch(
                    cx,
                    |t| t.home_feed("static"),
                    |this, r, _| {
                        if let Ok(rows) = r {
                            this.home = Load::Ready(rows);
                            this.content_changed_pub();
                        }
                    },
                );
            }
            Page::Home if self.home_tab > 0 && !self.home_extra.contains_key(&self.home_tab) => {
                let tab = self.home_tab;
                self.home_extra.insert(tab, Load::Loading);
                let vibe = if tab == 1 { "editorial" } else { "uploads" };
                self.fetch(
                    cx,
                    move |t| t.home_feed(vibe),
                    move |this, r, _| {
                        this.home_extra.insert(tab, into_load(r));
                    },
                );
            }
            Page::Explore if self.explore.is_idle() => {
                self.explore = Load::Loading;
                self.fetch(cx, |t| t.page("explore"), |this, r, _| this.explore = into_load(r));
            }
            Page::Album(id) if !self.albums.contains_key(&id) && !crate::local::is_local_id(id) => {
                self.albums.insert(id, Load::Loading);
                self.fetch(
                    cx,
                    move |t| t.album(id),
                    move |this, r, cx| {
                        if let Ok((album, _)) = &r {
                            if let Some(artist) = album.artists.first().map(|a| a.id) {
                                this.album_extra.insert(id, Load::Loading);
                                this.fetch(
                                    cx,
                                    move |t| t.album_extras(artist, id),
                                    move |this, r, _| {
                                        this.album_extra.insert(id, into_load(r));
                                    },
                                );
                            }
                        }
                        this.albums.insert(id, into_load(r));
                    },
                );
            }
            Page::Browse(path, _) if !self.browse.contains_key(&path) => {
                self.browse.insert(path.clone(), Load::Loading);
                let p = path.clone();
                self.fetch(
                    cx,
                    move |t| t.page(&p),
                    move |this, r, _| {
                        this.browse.insert(path, into_load(r));
                    },
                );
            }
            Page::Playlist(uuid) if !self.playlists.contains_key(&uuid) => {
                self.playlists.insert(uuid.clone(), Load::Loading);
                let u = uuid.clone();
                self.fetch(
                    cx,
                    move |t| t.playlist(&u),
                    move |this, r, _| {
                        this.playlists.insert(uuid, into_load(r));
                    },
                );
            }
            Page::Artist(id) if !self.artists.contains_key(&id) && !crate::local::is_local_id(id) => {
                self.artists.insert(id, Load::Loading);
                self.fetch(
                    cx,
                    move |t| t.artist(id),
                    move |this, r, _| {
                        this.artists.insert(id, into_load(r));
                    },
                );
            }
            Page::Mix(id) if !self.mixes.contains_key(&id) => {
                self.mixes.insert(id.clone(), Load::Loading);
                let i = id.clone();
                self.fetch(
                    cx,
                    move |t| t.mix(&i),
                    move |this, r, _| {
                        this.mixes.insert(id, into_load(r));
                    },
                );
            }
            Page::Search(q) if !q.trim().is_empty() && !self.search.contains_key(&q) => self.load_search(q, cx),
            Page::MyTracks if self.fav_tracks.is_idle() && self.signed_in => {
                self.fav_tracks = Load::Loading;
                self.fetch(cx, |t| t.favorite_tracks(), |this, r, _| this.fav_tracks = into_load(r));
            }
            Page::MyAlbums if self.fav_albums.is_idle() && self.signed_in => {
                self.fav_albums = Load::Loading;
                self.fetch(cx, |t| t.favorite_albums(), |this, r, _| this.fav_albums = into_load(r));
            }
            Page::MyArtists if self.fav_artists.is_idle() && self.signed_in => {
                self.fav_artists = Load::Loading;
                self.fetch(cx, |t| t.favorite_artists(), |this, r, _| this.fav_artists = into_load(r));
            }
            Page::MyMixes if self.my_mixes.is_idle() => {
                self.my_mixes = Load::Loading;
                self.fetch(cx, |t| t.my_mixes(), |this, r, _| this.my_mixes = into_load(r));
            }
            Page::Discover => {
                // The three existing sources, each loaded once.
                if self.my_mixes.is_idle() {
                    self.my_mixes = Load::Loading;
                    self.fetch(cx, |t| t.my_mixes(), |this, r, _| this.my_mixes = into_load(r));
                }
                let uploads = !self.settings.lock().hide_uploads;
                for tab in [1usize, 2] {
                    if (tab == 1 || uploads) && !self.home_extra.contains_key(&tab) {
                        self.home_extra.insert(tab, Load::Loading);
                        let vibe = if tab == 1 { "editorial" } else { "uploads" };
                        self.fetch(
                            cx,
                            move |t| t.home_feed(vibe),
                            move |this, r, _| {
                                this.home_extra.insert(tab, into_load(r));
                            },
                        );
                    }
                }
            }
            Page::MyPlaylists if self.my_playlists.is_idle() => {
                self.load_library(cx);
            }
            Page::Settings => self.devices = audio::output::device_names(),
            _ => {}
        }
    }

    pub fn open_item(&mut self, item: &Item, cx: &mut Context<Self>) {
        match item {
            // Shortcut to a collection page ("My Tracks"...).
            Item::Link(l) if l.path.starts_with('@') => match &l.path[1..] {
                "tracks" => self.set_library_cat("tracks", cx),
                "albums" => self.set_library_cat("albums", cx),
                "artists" => self.set_library_cat("artists", cx),
                "playlists" => self.navigate(Page::MyPlaylists, cx),
                "mixes" => self.navigate(Page::MyMixes, cx),
                _ => {}
            },
            Item::Link(l) => self.navigate(Page::Browse(l.path.clone(), l.title.clone()), cx),
            Item::Promo(p) => match p.kind.as_str() {
                "PLAYLIST" => self.navigate(Page::Playlist(p.target.clone()), cx),
                "ALBUM" => {
                    if let Ok(id) = p.target.parse() {
                        self.navigate(Page::Album(id), cx)
                    }
                }
                "ARTIST" => {
                    if let Ok(id) = p.target.parse() {
                        self.navigate(Page::Artist(id), cx)
                    }
                }
                _ => self.notify_toast(
                    crate::i18n::text("this_content_is_only_available_on_the_website", &[crate::i18n::t("tidal").to_string()]),
                    cx,
                ),
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
                    self.fetch(
                        cx,
                        move |t| t.album(a.id),
                        move |this, r, cx| {
                            if let Ok((al, tracks)) = r {
                                this.play_tracks(tracks.clone(), 0, cx);
                                this.albums.insert(al.id, Load::Ready((al, tracks)));
                            }
                        },
                    );
                }
            }
            Item::Playlist(p) => {
                let u = p.uuid.clone();
                self.fetch(
                    cx,
                    move |t| t.playlist(&u),
                    move |this, r, cx| {
                        if let Ok((pl, tracks)) = r {
                            this.play_tracks(tracks.clone(), 0, cx);
                            this.playlists.insert(pl.uuid.clone(), Load::Ready((pl, tracks)));
                        }
                    },
                );
            }
            Item::Mix(m) => {
                let id = m.id.clone();
                self.fetch(
                    cx,
                    move |t| t.mix(&id),
                    move |this, r, cx| {
                        if let Ok((mix, tracks)) = r {
                            this.play_tracks(tracks.clone(), 0, cx);
                            this.mixes.insert(mix.id.clone(), Load::Ready((mix, tracks)));
                        }
                    },
                );
            }
            Item::Artist(a) if crate::local::is_local_id(a.id) => {
                if let Some(page) = self.artists.get(&a.id).and_then(Load::ready) {
                    self.play_tracks(page.top_tracks.clone(), 0, cx);
                }
            }
            Item::Artist(a) => {
                self.fetch(
                    cx,
                    move |t| t.artist(a.id),
                    move |this, r, cx| {
                        if let Ok(page) = r {
                            this.play_tracks(page.top_tracks.clone(), 0, cx);
                            this.artists.insert(page.artist.id, Load::Ready(page));
                        }
                    },
                );
            }
        }
    }

    fn on_search_event(&mut self, _: &Entity<TextInput>, ev: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            InputEvent::Submit(q) => self.search_all(q.clone(), window, cx),
            InputEvent::Escape => {
                self.search_open = false;
                window.focus(&self.focus);
                cx.notify();
            }
            InputEvent::Changed(text) => self.search_typed(text, cx),
        }
    }

    // ------------------------------------------------------------- queue

    fn new_uid(&mut self) -> u64 {
        self.next_uid += 1;
        self.next_uid
    }

    /// Play `tracks`, starting with `tracks[start]` (`start` indexes the list
    /// as given, i.e. as shown). Unplayable tracks are left out of the queue;
    /// the start is mapped to the chosen track's place in what remains, so the
    /// track that plays is always the one that was picked.
    pub fn play_tracks(&mut self, tracks: Vec<Track>, start: usize, cx: &mut Context<Self>) {
        if tracks.is_empty() {
            return;
        }
        let flags: Vec<bool> = tracks.iter().map(|t| t.streamable).collect();
        let Some(start) = playable_start(&flags, start) else {
            // Never play a different track instead of an unavailable one.
            self.notify_toast(crate::i18n::t("this_track_is_unavailable_for_playback"), cx);
            return;
        };
        let tracks: Vec<Track> = tracks.into_iter().filter(|t| t.streamable).collect();
        log::info!("play_tracks: «{}» (queue position {start} of {})", tracks[start].title, tracks.len());
        self.autoplay_jump = false;
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
        // Start on a random playable track.
        let playable: Vec<usize> = (0..tracks.len()).filter(|&i| tracks[i].streamable).collect();
        if playable.is_empty() {
            return;
        }
        let start = playable[(now_secs() as usize) % playable.len()];
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
        self.next_info = None;
        self.snap.position = start;
        if let Some(path) = item.track.local_path.clone() {
            self.snap.paused = paused;
            self.snap.buffering = true;
            self.player.send(Cmd::Play { key, source: audio::Source::Local(path), start, paused });
            if self.wants_lyrics() {
                self.load_lyrics(item.track.id, cx);
            }
            cx.notify();
            return;
        }
        if crate::recents::record(&mut self.recents, &item.track) {
            // The home shortcuts show it as soon as the user is back there.
            self.content_changed_pub();
        }
        let quality = self.settings.lock().quality;
        let id = item.track.id;
        self.fetch(
            cx,
            move |t| t.stream(id, quality),
            move |this, r, cx| {
                let Some(now) = this.now.as_mut().filter(|n| n.key == key) else { return };
                match r {
                    Ok(info) => {
                        now.info = Some(info.clone());
                        this.player.send(Cmd::Play { key, source: audio::Source::Stream(info), start, paused });
                    }
                    Err(e) => {
                        this.notify_toast(crate::i18n::text("could_not_play", &[item.track.title.to_string(), format!("{:#}", e)]), cx);
                        this.skip_after_error(key, cx);
                    }
                }
            },
        );
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
        } else {
            self.ended = true;
            self.player.send(Cmd::Stop);
            cx.notify();
        }
    }

    /// The queue / lyrics panel, sliding in from the right and fading in when
    /// opened, and the reverse when closed. Its content keeps its full width
    /// (nothing reflows inside it); only the space it takes grows or shrinks.
    fn side_resize_limits(&self) -> (f32, f32) {
        let body = (self.viewport_w - self.sidebar_width() - theme::FRAME_GAP).max(0.0);
        let max = body * 0.64;
        (240.0_f32.min(body * 0.44), max)
    }

    fn finish_side_resize(&mut self) {
        if self.side_resize.take().is_some() {
            self.save_settings();
        }
    }

    fn animated_side_panel(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let open = self.show_queue || self.show_lyrics;
        if open {
            self.side_last_lyrics = self.show_lyrics;
        }
        let t = interact::follow("side-panel", if open { 1.0 } else { 0.0 }, motion::seconds(motion::PANEL));
        let body_w = (self.viewport_w - self.sidebar_width() - theme::FRAME_GAP).max(0.0);
        let width = {
            let settings = self.settings.lock();
            let (min, max) = self.side_resize_limits();
            settings.side_panel_width.map_or_else(
                || {
                    if settings.wide_side_panel {
                        (theme::QUEUE_W * 1.55).min(0.58 * body_w)
                    } else {
                        theme::QUEUE_W.min(0.44 * body_w)
                    }
                },
                |width| width.clamp(min, max),
            )
        };
        self.side_panel_w = if t < 0.002 { 0.0 } else { width * t };
        if self.side_panel_w == 0.0 {
            return None;
        }
        Some(
            div()
                .w(sc(self.side_panel_w))
                .flex_none()
                .h_full()
                .relative()
                .overflow_hidden()
                // Anchored to the right edge, so the panel slides in and out
                // instead of being uncovered like a wipe.
                .child(
                    div().absolute().top(sc(0.)).right(sc(0.)).w(sc(width)).h_full().opacity(t).child(pages::side_panel(self, width, cx)),
                )
                .when(open, |d| {
                    d.child(
                        div()
                            .id("side-resize")
                            .absolute()
                            .left(sc(0.))
                            .top(sc(0.))
                            .w(sc(7.))
                            .h_full()
                            .cursor(gpui::CursorStyle::ResizeLeftRight)
                            .hover(|s| s.bg(theme::divider()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.side_resize = Some((f32::from(ev.position.x), width));
                                }),
                            ),
                    )
                })
                .into_any_element(),
        )
    }

    /// Opacity of the page content: 1, or easing in right after loading ended.
    pub fn content_alpha(&mut self) -> f32 {
        let Some(at) = self.content_in_at else { return 1.0 };
        let t = motion::progress(at, motion::PANEL);
        if t >= 1.0 {
            self.content_in_at = None;
            return 1.0;
        }
        motion::want_frame("content-in");
        motion::ease_out(t)
    }

    /// Covers of the next tracks in the queue, decoded ahead of time so a skip
    /// finds them ready and the cover changes together with the title.
    pub fn prefetch_next_covers(&mut self, np_size: Option<f32>, cx: &mut Context<Self>) {
        let Some(now) = &self.now else { return };
        let mut ix = now.index;
        for _ in 0..2 {
            let Some(next) = self.next_index(ix).filter(|n| *n != ix) else { break };
            ix = next;
            let Some(t) = self.queue.get(next).map(|q| q.track.clone()) else { break };
            if let Some(u) = t.cover(160) {
                images::sized(&u, 42., 42., cx);
            }
            if let (Some(size), Some(u)) = (np_size, t.cover(1280)) {
                images::sized(&u, size, size, cx);
            }
            if let Some(u) = t.cover(640) {
                images::get(&u, cx);
            }
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
        self.next_info = None;
        if let Some(path) = item.track.local_path {
            self.player.send(Cmd::Queue { key, source: audio::Source::Local(path) });
            return;
        }
        let quality = self.settings.lock().quality;
        self.fetch(
            cx,
            move |t| t.stream(item.track.id, quality),
            move |this, r, _| {
                if this.queued_next.map(|q| q.0) != Some(key) {
                    return;
                }
                match r {
                    Ok(info) => {
                        this.next_info = Some((key, info.clone()));
                        this.player.send(Cmd::Queue { key, source: audio::Source::Stream(info) });
                    }
                    Err(_) => this.queued_next = None,
                }
            },
        );
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
        let Some(seed) = self.queue.last().filter(|q| q.track.local_path.is_none()) else { return };
        let seed_uid = seed.uid;
        let id = seed.track.id;
        self.autoplay_loading = true;
        self.fetch(
            cx,
            move |t| t.track_radio(id),
            move |this, r, cx| {
                this.autoplay_loading = false;
                if this.queue.last().map(|q| q.uid) != Some(seed_uid) {
                    return;
                }
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
            },
        );
    }

    pub fn next(&mut self, cx: &mut Context<Self>) {
        let Some(now) = &self.now else { return };
        let idx = now.index;
        let next = if idx + 1 < self.queue.len() {
            Some(idx + 1)
        } else if self.repeat == Repeat::All {
            Some(0)
        } else {
            None
        };
        match next {
            Some(n) => self.play_index(n, 0.0, false, cx),
            None if now.track.local_path.is_none() => {
                // The user asked for the next track: jump as soon as autoplay has one.
                self.autoplay_jump = true;
                self.autoplay_more(cx);
            }
            None => {}
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
                self.play_index(if self.ended { i } else { 0 }, 0.0, false, cx);
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
        self.notify_toast(if next { crate::i18n::t("playing_next") } else { crate::i18n::t("added_to_queue") }, cx);
    }

    pub fn remove_from_queue(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(now) = &mut self.now else {
            if index < self.queue.len() {
                self.queue.remove(index);
                cx.notify();
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
        if let Err(error) = self.ratings.set(track, halves) {
            log::warn!("No se pudo guardar la valoración de pista: {error:#}");
            self.notify_toast(crate::i18n::t("could_not_save_the_track_rating"), cx);
            return;
        }
        if sort::SortKey::Rating == self.sort_of("tracks").0 {
            self.apply_sorts();
            self.content_changed_pub();
        }
        cx.notify();
    }

    pub fn set_album_rating(&mut self, album: u64, halves: u8, cx: &mut Context<Self>) {
        if let Err(error) = self.ratings.set_album(album, halves) {
            log::warn!("No se pudo guardar la valoración de álbum: {error:#}");
            self.notify_toast(crate::i18n::t("could_not_save_the_album_rating"), cx);
            return;
        }
        if sort::SortKey::Rating == self.sort_of("albums").0 {
            self.apply_sorts();
            self.content_changed_pub();
        }
        cx.notify();
    }

    pub fn toggle_favorite(&mut self, track: &Track, cx: &mut Context<Self>) {
        if track.local_path.is_some() {
            return;
        }
        let id = track.id;
        let fav = !self.fav_ids.contains(&id);
        if fav {
            self.heart_pop = Some((id, std::time::Instant::now()));
            self.fav_ids.insert(id);
            if let Load::Ready(list) = &mut self.fav_tracks {
                // Newest in the Library's "date added" order too.
                let mut t = track.clone();
                t.added = Some(crate::local::now_stamp());
                list.insert(0, t);
            }
        } else {
            self.fav_ids.remove(&id);
            if let Load::Ready(list) = &mut self.fav_tracks {
                list.retain(|t| t.id != id);
            }
        }
        self.rebuild_library();
        self.content_changed();
        self.fetch(
            cx,
            move |t| t.set_favorite_track(id, fav),
            move |this, r, cx| {
                if let Err(e) = r {
                    this.notify_toast(crate::i18n::text("error", &[format!("{:#}", e)]), cx);
                } else {
                    this.notify_toast(
                        if fav { crate::i18n::t("added_to_my_collection") } else { crate::i18n::t("removed_from_my_collection") },
                        cx,
                    );
                }
            },
        );
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
        self.fetch(
            cx,
            move |t| t.set_favorite_album(id, fav),
            move |this, r, cx| {
                if let Err(e) = r {
                    this.notify_toast(crate::i18n::text("error", &[format!("{:#}", e)]), cx);
                }
            },
        );
        cx.notify();
    }

    pub fn start_radio(&mut self, track: &Track, cx: &mut Context<Self>) {
        let first = track.clone();
        let id = track.id;
        self.fetch(
            cx,
            move |t| t.track_radio(id),
            move |this, r, cx| match r {
                Ok(mut tracks) => {
                    tracks.retain(|t| t.id != first.id);
                    tracks.insert(0, first);
                    this.play_tracks(tracks, 0, cx);
                }
                Err(e) => this.notify_toast(crate::i18n::text("radio_unavailable", &[format!("{:#}", e)]), cx),
            },
        );
    }

    pub fn load_lyrics(&mut self, id: u64, cx: &mut Context<Self>) {
        if matches!(self.lyrics.get(&id), Some(Load::Ready(_) | Load::Loading)) {
            return;
        }
        let epoch = self.lyrics_epoch;
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
                if this.lyrics_epoch != epoch {
                    return;
                }
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
        if let Some(now) = self.now.as_ref().filter(|n| n.track.local_path.is_none()) {
            let (i, pos, paused, key) = (now.index, self.snap.position, self.snap.paused, now.key);
            self.play_index(i, pos, paused, cx);
            if let Some(now) = &self.now {
                self.recorder.rekey(key, now.key);
            }
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
                "external_lyrics" => s.external_lyrics = !s.external_lyrics,
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
        if which == "external_lyrics" {
            self.lyrics_epoch = self.lyrics_epoch.wrapping_add(1);
            self.lyrics.clear();
            if self.wants_lyrics() {
                if let Some(id) = self.now.as_ref().map(|n| n.track.id) {
                    self.load_lyrics(id, cx);
                }
            }
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
        let mut finished = None;
        while let Ok(ev) = self.events.try_recv() {
            changed = true;
            match ev {
                audio::Event::Finished { key } => {
                    if self.now.as_ref().is_some_and(|now| now.key == key) {
                        finished = Some(key);
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
                    log::info!(
                        "gapless transition to track {} ({}) at output position {:.2} s",
                        item.track.id,
                        info.as_ref().map(|i| i.detail()).unwrap_or_default(),
                        snap.position
                    );
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
        if let Some(now) = self.now.as_ref().filter(|now| Some(now.key) == finished) {
            // The engine drained before the next track was ready.
            if let Some(index) = self.next_index(now.index) {
                self.play_index(index, 0.0, false, cx);
            } else {
                self.ended = true;
            }
        }
        // Queue the next track once the current one is downloaded or near its end.
        if let Some(now) = &self.now {
            if !self.ended && snap.key == Some(now.key) && self.queued_next.is_none() {
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
        // Listening history: only seconds actually heard count.
        {
            let dt = self.history_tick.elapsed().as_secs_f32();
            self.history_tick = std::time::Instant::now();
            let track = self.now.as_ref().filter(|n| !self.ended && n.track.local_path.is_none());
            let audible = track.is_some_and(|n| Some(n.key) == self.snap.key) && !self.snap.paused && !self.snap.buffering;
            if let Some(batch) = self.recorder.tick(&mut self.history, track.map(|n| (n.key, &n.track)), audible, dt) {
                cx.spawn(async move |this, cx| {
                    let result = cx.background_executor().spawn(async move { crate::history::save_batch(&batch) }).await;
                    let _ = this.update(cx, |this, cx| {
                        if this.recorder.saved(result.is_ok()) {
                            if let Err(error) = result {
                                log::warn!("No se pudo guardar Replay: {error:#}");
                                this.notify_toast(crate::i18n::t("replay.save_retry"), cx);
                            }
                        }
                    });
                })
                .detach();
            }
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
            self.discord.update(self.now.as_ref().filter(|n| n.track.local_path.is_none()).map(|n| (&n.track, dur)), playing, pos);
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
        images::set_scale(window.scale_factor());
        images::evict(window, cx);
        if self.welcome_step.is_some() {
            if window.is_fullscreen() {
                window.toggle_fullscreen();
            }
            self.now_playing_open = false;
            self.np_fullscreen_request = None;
            window.focus(&self.focus);
            let welcome = onboarding::render(self, window, cx);
            self.pace_frames(window, cx);
            return welcome;
        }
        // Without a TIDAL session, local music (Library tracks and albums,
        // local album pages) still works.
        let offline_ok = match &self.page {
            Page::MyTracks | Page::MyAlbums | Page::MyArtists | Page::Settings | Page::Search(_) => true,
            Page::Album(id) | Page::Artist(id) => crate::local::is_local_id(*id),
            _ => false,
        };
        if !self.signed_in && !offline_ok {
            if window.is_fullscreen() {
                window.toggle_fullscreen();
            }
            self.now_playing_open = false;
            self.np_fullscreen_request = None;
            return pages::login_screen(self, window, cx).into_any_element();
        }
        let perf_t0 = std::time::Instant::now();
        self.frame_started = perf_t0;
        self.step_smooth_scroll();
        self.drive_animations(window);
        self.viewport_w = f32::from(window.viewport_size().width) / zoom();
        self.update_sidebar();
        winctl::set_border_color(window, theme::frame_rgb());
        let fullscreen = window.is_fullscreen();
        let requested = self.np_fullscreen_request.take();
        let target = requested.unwrap_or(fullscreen);
        if target != fullscreen {
            window.toggle_fullscreen();
            window.request_animation_frame();
        }
        let was_open = self.now_playing_open;
        self.now_playing_open = fullscreen;
        if self.now_playing_open && !was_open {
            window.focus(&self.focus);
            self.load_np_data(cx);
        } else if was_open && !self.now_playing_open && self.show_lyrics {
            if let Some(id) = self.now.as_ref().map(|n| n.track.id) {
                self.load_lyrics(id, cx);
            }
        }
        if self.now_playing_open {
            self.bg_angle_now = self.bg_angle(window);
        }
        let queue_view = if self.now_playing_open {
            (self.np_tab == now_playing::NpTab::Queue).then_some("np-queue")
        } else {
            self.show_queue.then_some("queue")
        };
        let queue_current = self.now.as_ref().and_then(|n| self.queue.get(n.index).map(|q| (q.uid, n.index)));
        if queue_view != self.queue_open_view || queue_current != self.queue_current {
            self.queue_open_view = queue_view;
            self.queue_current = queue_current;
            self.queue_center_pending = queue_view.is_some();
        }
        // Now Playing sits on the darkened artwork: dark palette in both themes.
        let np_overlay = if self.now_playing_open { Some(theme::dark_scope(|| now_playing::render(self, window, cx))) } else { None };
        self.list_view_h = f32::from(self.list.viewport_bounds().size.height).max(1.0);
        let side_panel = if self.now_playing_open {
            self.side_panel_w = 0.0;
            None
        } else {
            self.animated_side_panel(cx)
        };
        if !self.now_playing_open {
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
            let loading = blocks.iter().any(|b| matches!(b, pages::Block::Loading));
            if self.page_was_loading && !loading {
                self.content_in_at = Some(std::time::Instant::now());
            }
            self.page_was_loading = loading;
            self.list_len = blocks.len();
            self.current_blocks = blocks;
        }
        let root = div()
            .id("root")
            .relative()
            .key_context("App")
            // Audio files or folders dropped anywhere join the Library.
            .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| this.import_local_paths(paths.paths().to_vec(), cx)))
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
            .on_action(cx.listener(|this, _: &CloseOverlay, window, cx| {
                if this.dialog.is_some() {
                    this.close_dialog(window, cx);
                } else if this.menu.is_some() {
                    this.menu = None;
                    cx.notify();
                } else if !this.close_info(cx) && !this.close_cover_view(cx) && this.now_playing_open {
                    this.toggle_now_playing(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &GoForward, _, cx| this.go_forward(cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.step_zoom(1, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.step_zoom(-1, cx)))
            .on_action(cx.listener(|this, _: &ZoomReset, _, cx| this.set_zoom_level(1.0, cx)))
            // The mouse's back / forward buttons.
            .on_mouse_down(MouseButton::Navigate(gpui::NavigationDirection::Back), cx.listener(|this, _, _, cx| this.go_back(cx)))
            .on_mouse_down(MouseButton::Navigate(gpui::NavigationDirection::Forward), cx.listener(|this, _, _, cx| this.go_forward(cx)))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                if this.signed_in {
                    let fh = this.search_input.focus_handle(cx);
                    window.focus(&fh);
                }
            }))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, window, cx| {
                if let Some((start, width)) = this.side_resize {
                    if ev.pressed_button == Some(MouseButton::Left) {
                        let (min, max) = this.side_resize_limits();
                        let delta = (start - f32::from(ev.position.x)) / zoom();
                        this.settings.lock().side_panel_width = Some((width + delta).clamp(min, max));
                        cx.notify();
                        return;
                    }
                    this.finish_side_resize();
                }
                if this.search_sb_drag {
                    if ev.pressed_button == Some(MouseButton::Left) {
                        this.search_scrollbar_to(ev.position);
                        cx.notify();
                        return;
                    }
                    this.search_sb_drag = false;
                }
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
                    winctl::drag_end(window);
                }
                if this.drag.is_some() {
                    // Button released outside the window: finish the drag
                    // instead of letting every later move change the value.
                    if ev.pressed_button != Some(MouseButton::Left) {
                        this.drag = None;
                        cx.notify();
                        return;
                    }
                    this.drag_to(ev.position, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    interact::release_all();
                    this.search_sb_drag = false;
                    this.scrollbar_drag = false;
                    if this.win_drag.take().is_some() {
                        winctl::drag_end(window);
                    }
                    this.finish_side_resize();
                    this.end_drag(cx)
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    interact::release_all();
                    this.search_sb_drag = false;
                    this.scrollbar_drag = false;
                    if this.win_drag.take().is_some() {
                        winctl::drag_end(window);
                    }
                    this.finish_side_resize();
                    this.end_drag(cx)
                }),
            )
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::bg())
            .text_color(theme::text())
            .text_size(sc(theme::TYPE_BODY))
            .font_family(theme::FONT)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_h(sc(0.))
                    // The window frame: the sidebar rail continues as a thin
                    // margin around the page panel.
                    .bg(theme::frame())
                    .when(self.signed_in && !self.now_playing_open, |d| d.child(div().flex_none().w(sc(sidebar::RAIL_W))))
                    .when(!self.now_playing_open, |d| {
                        // The page panel: black, rounded, inset in the frame.
                        d.child(
                            div()
                                .flex_1()
                                .min_w(sc(0.))
                                .mt(sc(theme::FRAME_GAP))
                                .mr(sc(theme::FRAME_GAP))
                                .mb(sc(theme::FRAME_GAP))
                                .rounded(sc(theme::PANEL_R))
                                .border_1()
                                .border_color(theme::panel_border())
                                .bg(theme::bg())
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .child(titlebar::render(self, window, cx))
                                .child(
                                    div()
                                        .flex()
                                        .flex_1()
                                        .min_h(sc(0.))
                                        .child(
                                            div().relative().flex_1().min_w(sc(0.)).h_full().flex().child(pages::content(self, window, cx)),
                                        )
                                        .children(side_panel),
                                ),
                        )
                    })
                    .when(self.signed_in && !self.now_playing_open, |d| d.child(sidebar::render(self, cx))),
            )
            .child(smooth::zoom_catcher(cx.entity()))
            .children(np_overlay)
            .when(!self.now_playing_open, |d| {
                d.child(
                    div().flex_none().w_full().px(sc(theme::FRAME_GAP)).pb(sc(theme::FRAME_GAP)).bg(theme::frame()).child(
                        div()
                            .rounded(sc(theme::PANEL_R))
                            .border_1()
                            .border_color(theme::divider())
                            .overflow_hidden()
                            .child(player_bar::render(self, window, cx)),
                    ),
                )
            })
            .children(pages::overlays(self, window, cx))
            .children(appmenu::overlay(self, window, cx))
            .children(search_menu::overlay(self, window, cx))
            .children(cover_view::overlay(self, window, cx))
            .children(info::overlay(self, window, cx))
            .children(debug::icon_sheet())
            .child(smooth::drag_catcher(cx.entity()))
            // Last, so it sees the wheel after every panel had its say.
            .child(smooth::wheel_resolver(cx.entity()))
            .children(haptics::swipe_indicator(self))
            .children(playlists::dialog_overlay(self, cx))
            .into_any_element();
        perf_note(perf_t0.elapsed());
        // Last: everything built above (bar, panels, overlays) may have asked
        // for another frame.
        self.pace_frames(window, cx);
        root
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
            log::info!(
                "perf: {} frames/s, render avg {:.2} ms, max {:.2} ms, frames for: {}, pacing {:?} at {} Hz",
                a.1,
                a.2 / a.1 as f64,
                a.3,
                motion::take_frame_reasons(),
                motion::take_pace_counts(),
                motion::LAST_HZ.with(|h| h.get())
            );
            *a = (std::time::Instant::now(), 0, 0.0, 0.0);
        }
    });
}

/// Where `list[start]` ends up once unplayable tracks are removed (`playable`
/// flags each track), or `None` if that track is itself unplayable.
pub fn playable_start(playable: &[bool], start: usize) -> Option<usize> {
    if !*playable.get(start)? {
        return None;
    }
    Some(playable[..start].iter().filter(|p| **p).count())
}

#[cfg(test)]
mod play_start_tests {
    use super::playable_start;

    #[test]
    fn picked_track_survives_unplayable_ones_above() {
        // Two unavailable tracks above row 3: the queue must still start on row 3.
        let flags = [true, false, false, true, true];
        assert_eq!(playable_start(&flags, 3), Some(1));
        assert_eq!(playable_start(&flags, 4), Some(2));
        assert_eq!(playable_start(&flags, 0), Some(0));
    }

    #[test]
    fn unplayable_or_missing_track_plays_nothing() {
        assert_eq!(playable_start(&[true, false], 1), None);
        assert_eq!(playable_start(&[true], 5), None);
    }
}
