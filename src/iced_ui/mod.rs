//! The Iced interface: one window, Elm-style state / message / view, drawn by
//! Iced's wgpu renderer. Custom `wgpu` pipelines are used only for effects
//! the stock widgets cannot draw (see `gpu`): the blurred synchronised lyrics
//! and the turning, blurred cover behind Now Playing.

mod actions;
mod artwork;
mod chrome;
mod dark_draw;
mod ellipsis;
mod equalizer;
mod glide;
mod gpu;
mod home;
mod icons;
mod images;
mod info;
mod library;
mod liquid_glass;
mod local;
mod lyrics;
mod menus;
mod motion;
mod now_playing;
mod pages;
mod panel_motion;
mod perf;
mod playback;
mod player_bar;
mod queue;
mod replay;
mod rows;
mod settings;
mod sheen;
mod sidebar;
#[cfg(test)]
mod tests;
mod theme;
mod wash;
mod widgets;
mod winhit;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{column, container, row};
use iced::{event, keyboard, mouse, window, Element, Length, Point, Size, Subscription, Task};
use parking_lot::Mutex;

use crate::audio::{self, OutputPrefs, Player, Snapshot};
use crate::config::{Quality, Repeat, Settings};
use crate::shared::{Load, Page, QItem};
use crate::tidal::*;

pub use now_playing::NpTab;

// ------------------------------------------------------------------ wake-ups

static WAKER: Mutex<Option<iced::futures::channel::mpsc::Sender<()>>> = Mutex::new(None);

/// Ask the interface to look again (an image finished loading, a background
/// job stored something). Coalesced: at most one pending wake-up.
pub fn wake() {
    if let Some(tx) = WAKER.lock().as_mut() {
        let _ = tx.try_send(());
    }
}

fn wake_stream() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::StreamExt;
    let (tx, rx) = iced::futures::channel::mpsc::channel(1);
    *WAKER.lock() = Some(tx);
    rx.map(|_| Message::Wake)
}

// ---------------------------------------------------------- background jobs

type DoneFn = Box<dyn FnOnce(&mut App) -> Task<Message> + Send>;

/// The UI-thread half of a background job (see [`App::fetch`]).
#[derive(Clone)]
pub struct Callback(Arc<Mutex<Option<DoneFn>>>);

impl Callback {
    fn new(f: impl FnOnce(&mut App) -> Task<Message> + Send + 'static) -> Self {
        Self(Arc::new(Mutex::new(Some(Box::new(f)))))
    }
    fn run(self, app: &mut App) -> Task<Message> {
        match self.0.lock().take() {
            Some(f) => f(app),
            None => Task::none(),
        }
    }
}

impl std::fmt::Debug for Callback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Callback")
    }
}

/// Run `job` on its own thread and resolve with its result.
pub async fn blocking<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("abyss-job".into())
        .spawn(move || {
            let _ = tx.send(job());
        })
        .expect("spawn a worker thread");
    rx.await.expect("the worker thread finished")
}

// ------------------------------------------------------------------ state

pub struct NowPlaying {
    pub track: Track,
    pub info: Option<StreamInfo>,
    /// Engine key of this play.
    pub key: u64,
    pub index: usize,
}

/// What a right-click menu acts on.
#[derive(Clone, Debug)]
pub enum MenuTarget {
    Track(Track),
    QueueItem(u64),
    /// Sort options of a library section.
    Sort(&'static str),
    /// A track inside one of the user's own playlists (can be removed).
    PlaylistTrack {
        uuid: String,
        index: usize,
        track: Track,
    },
    /// Pick a playlist to add these tracks to.
    AddToPlaylist(Vec<Track>),
    /// Pick a playlist to add a whole album to (loaded when picked).
    AddAlbumToPlaylist(u64),
    Album(Album),
    Artist(Artist),
    /// The sound output devices.
    Output,
}

pub enum LoginState {
    Idle,
    Starting,
    Waiting(DeviceCode),
    Failed(String),
}

/// Which slider is being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drag {
    Progress,
    Volume,
}

/// A modal dialog (playlists, confirmations).
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)] // one at a time, never in a collection
pub enum Dialog {
    NewPlaylist { add: Vec<u64> },
    EditPlaylist { uuid: String },
    DeletePlaylist { uuid: String, title: String },
    SignOut,
    ConfirmRemove(RemoveKind),
}

/// What a removal confirmation removes.
#[derive(Clone, Debug)]
pub enum RemoveKind {
    PlaylistTrack { uuid: String, index: usize, title: String },
    FavTrack(Track),
    FavAlbum(Album),
    Unfollow(u64, String),
}

pub struct App {
    pub window: Option<window::Id>,
    pub tidal: Tidal,
    data_epoch: u64,
    pub player: Player,
    events: Receiver<audio::Event>,
    pub settings: Arc<Mutex<Settings>>,
    pub signed_in: bool,
    pub login: LoginState,
    pub login_busy: bool,
    pub welcome_step: Option<usize>,

    pub page: Page,
    back: Vec<Page>,
    forward: Vec<Page>,
    /// Direction of the last navigation and when it happened (page fade).
    pub page_at: Instant,
    /// Where the page transition comes from: 1 (forward) or -1 (back), as GPUI.
    pub nav_dir: f32,

    // Data.
    pub home: Load<Vec<Row>>,
    pub home_tab: usize,
    pub home_extra: HashMap<usize, Load<Vec<Row>>>,
    pub home_loaded: Option<Instant>,
    pub explore: Load<Vec<Row>>,
    pub browse: HashMap<String, Load<Vec<Row>>>,
    #[allow(clippy::type_complexity)]
    pub album_extra: HashMap<u64, Load<(Vec<Album>, Vec<Artist>)>>,
    pub albums: HashMap<u64, Load<(Album, Vec<Track>)>>,
    pub playlists: HashMap<String, Load<(Playlist, Vec<Track>)>>,
    pub artists: HashMap<u64, Load<ArtistPage>>,
    pub mixes: HashMap<String, Load<(Mix, Vec<Track>)>>,
    pub search: HashMap<String, Load<SearchResults>>,
    pub search_query: String,
    pub search_open: bool,
    pub search_gen: u64,
    pub fav_tracks: Load<Vec<Track>>,
    pub fav_albums: Load<Vec<Album>>,
    pub fav_artists: Load<Vec<Artist>>,
    pub my_playlists: Load<Vec<Playlist>>,
    pub my_mixes: Load<Vec<Mix>>,
    pub fav_ids: HashSet<u64>,
    pub fav_album_ids: HashSet<u64>,
    pub fav_artist_ids: HashSet<u64>,
    pub lyrics: HashMap<u64, Load<Lyrics>>,
    pub lyrics_docs: HashMap<u64, Arc<lyrics::LyricsDoc>>,
    pub lyrics_epoch: u64,
    #[allow(clippy::type_complexity)]
    pub credits: HashMap<u64, Load<Vec<(String, Vec<String>)>>>,
    pub similar: HashMap<u64, Load<Vec<Track>>>,
    pub tech_info: HashMap<u64, Load<(crate::tidal::probe::TechInfo, String)>>,
    pub artist_pics: HashMap<u64, Load<Option<String>>>,

    // Playback.
    pub queue: Vec<QItem>,
    unshuffled: Option<Vec<QItem>>,
    pub now: Option<NowPlaying>,
    queued_next: Option<(u64, usize)>,
    next_info: Option<(u64, StreamInfo)>,
    pub ended: bool,
    resume_at: Option<(usize, f64)>,
    queue_restoring: bool,
    play_failures: u32,
    queue_saved: (u64, usize, i64),
    queue_saved_at: Instant,
    pub snap: Snapshot,
    pub snap_at: Instant,
    next_key: u64,
    next_uid: u64,
    pub volume: f32,
    pub muted: bool,
    pub shuffle: bool,
    pub repeat: Repeat,
    autoplay_loading: bool,
    autoplay_jump: bool,
    pub drag: Option<Drag>,
    pub drag_value: f32,

    // Interface state.
    pub viewport: Size,
    pub dpi: f32,
    pub fullscreen: bool,
    pub now_playing_open: bool,
    pub np_tab: NpTab,
    pub show_queue: bool,
    pub show_lyrics: bool,
    pub side_reveal: panel_motion::Reveal,
    pub side_last_lyrics: bool,
    pub lyrics_follow: bool,
    /// "Sincronizar" was pressed: the lyrics glide back (the icon spins).
    pub lyrics_sync_at: Option<Instant>,
    pub menu: Option<(Point, MenuTarget)>,
    /// The menu a click elsewhere closed, and when: it fades out (`menus::ghost`).
    pub menu_exit: Option<(Point, MenuTarget, Instant)>,
    pub maximized: bool,
    hwnd: Option<isize>,
    /// Ctrl is down: the wheel zooms instead of scrolling.
    ctrl_held: bool,
    /// The last time something other than a timer happened (see `poll_ms`).
    active_at: Instant,
    /// Side panel resize: where the pointer started and the width then.
    pub side_resize: Option<(f32, f32)>,
    pub queue_scroll: f32,
    queue_centred: Option<queue::Centred>,
    /// The queue's last glide to the playing track: (count, px down).
    pub queue_nudge: (u64, f32),
    pub dialog: Option<Dialog>,
    pub dlg_title: String,
    pub dlg_desc: String,
    pub toast: Option<(String, Instant)>,
    pub filter: String,
    pub expanded_album: Option<u64>,
    pub expanded_album_height: f32,
    pub sort_ranks: HashMap<&'static str, HashMap<String, i64>>,
    pub lib_queries: HashMap<&'static str, String>,
    pub ratings: crate::ratings::Ratings,
    pub heart_pop: Option<(u64, Instant)>,
    /// The double-click heart over the Now Playing cover.
    pub heart_burst: Option<Instant>,
    /// Horizontal touchpad swipe in progress (px; positive = back) and when it last moved.
    pub swipe: f32,
    pub swipe_at: Option<Instant>,
    /// When the volume last changed: its percentage floats over the slider for a moment.
    pub volume_tip: Option<Instant>,
    pub ambient: theme_fade::Fade,
    pub palette: theme_fade::PaletteFade,
    pub bg_spin: now_playing::BgSpin,
    pub settings_tab: usize,
    /// The sound output list unfolded in its Settings card (GPUI).
    pub output_list_open: bool,
    pub devices: Vec<String>,
    pub info_panel: Option<info::InfoTarget>,
    pub cover_view: Option<String>,
    pub update_check: Option<Load<Option<String>>>,
    pub sidebar_reveal: panel_motion::Reveal,
    pub page_scroll: f32,
    pub page_view_h: f32,

    // Library.
    pub recents: Vec<crate::recents::RecentAlbum>,
    pub history: Vec<crate::history::Entry>,
    pub recorder: crate::history::Recorder,
    history_tick: Instant,
    pub local_tracks: Vec<Track>,
    pub local_busy: bool,
    pub local_editor: Option<local::Editor>,
    pub row_drag: Option<rows::RowDrag>,
    /// Scroll of the track list in Settings > Local files.
    pub local_scroll: f32,
    pub local_search_keys: HashMap<u64, String>,
    pub local_albums: Vec<Album>,
    pub local_artists: Vec<Artist>,
    pub lib_tracks: Vec<Track>,
    /// Indices into the active Library category, in display order.
    pub lib_shown: Vec<usize>,
    pub lib_bar_h: f32,
    /// Dropped files waiting to be imported: Windows reports one event per file.
    dropped: Vec<std::path::PathBuf>,
    pub lib_albums: Vec<Album>,
    pub lib_artists: Vec<Artist>,
    pub scrobbler: crate::scrobble::Scrobbler,
    pub scrobble_tracker: crate::scrobble::tracker::Tracker,
    lastfm_tick: Instant,
    pub lastfm_seen: u64,
    pub discord: crate::discord::Discord,
    media: Option<playback::Media>,
    pub replay_period: crate::shared::replay::Period,
    pub replay_all_tracks: bool,
    replay_cache: std::cell::RefCell<Option<(replay::CacheKey, std::rc::Rc<crate::shared::replay::Stats>)>>,
    pub eq_meter: crate::shared::eqmeter::Meter,
    pub eq_meter_at: Instant,
    pub eq_selected: Option<usize>,
    pub eq_band_selected: Option<usize>,
    /// What is being typed in an equaliser field.
    pub eq_draft: Option<(equalizer::Field, String)>,
    /// Vertical half-range of the response graph; None follows the curves.
    pub eq_scale: Option<f32>,

    /// Frames are being drawn continuously (animations): when the last one was.
    pub last_frame: Instant,
    /// Where each page of the history was scrolled when it was left (Back and
    /// Forward bring it back there), newest last.
    scroll_memory: Vec<(Page, f32)>,
    /// A seek the engine may not have done yet: (target s, when). The tick
    /// keeps showing the target instead of the engine's older position.
    seek_hold: Option<(f64, Instant)>,
}

/// Fades of the ambient colour and palette (track changes).
pub mod theme_fade {
    use iced::Color;
    use std::time::Instant;

    use super::motion::{ease_out_soft as ease_out, AMBIENT as FADE};

    pub struct Fade {
        pub from: Color,
        pub to: Color,
        pub at: Instant,
        pub seen: bool,
    }

    impl Fade {
        pub fn new(c: Color) -> Self {
            Self { from: c, to: c, at: Instant::now() - FADE, seen: false }
        }
        pub fn target(&mut self, c: Color) {
            if c != self.to {
                self.from = self.now();
                self.to = c;
                self.at = Instant::now();
            }
        }
        pub fn now(&self) -> Color {
            let t =
                if crate::shared::lyrics::reduced_motion() { 1.0 } else { ease_out(self.at.elapsed().as_secs_f32() / FADE.as_secs_f32()) };
            super::theme::lerp(self.from, self.to, t)
        }
        pub fn animating(&self) -> bool {
            !crate::shared::lyrics::reduced_motion() && self.at.elapsed() < FADE
        }
    }

    pub struct PaletteFade {
        pub from: Vec<Color>,
        pub to: Vec<Color>,
        pub at: Instant,
    }

    impl PaletteFade {
        pub fn target(&mut self, p: Vec<Color>) {
            if p != self.to {
                let now = self.now();
                self.from = if now.len() == p.len() { now } else { p.clone() };
                self.to = p;
                self.at = Instant::now();
            }
        }
        pub fn now(&self) -> Vec<Color> {
            let t =
                if crate::shared::lyrics::reduced_motion() { 1.0 } else { ease_out(self.at.elapsed().as_secs_f32() / FADE.as_secs_f32()) };
            self.from.iter().zip(&self.to).map(|(a, b)| super::theme::lerp(*a, *b, t)).collect()
        }
        pub fn animating(&self) -> bool {
            !crate::shared::lyrics::reduced_motion() && self.at.elapsed() < FADE
        }
    }
}

/// Tracks in a row that may fail before playback stops.
const MAX_PLAY_FAILURES: u32 = 4;

// ---------------------------------------------------------------- messages

#[derive(Clone, Debug)]
pub enum Message {
    Tick,
    Frame,
    Wake,
    Done(Callback),
    WindowOpened(window::Id, Size),
    WindowResized(Size),
    WindowRescaled(f32),
    CloseRequested,
    FilesDropped(std::path::PathBuf),
    Key(keyboard::Key, keyboard::key::Physical, keyboard::Modifiers),
    MouseBack,
    MouseForward,
    /// Horizontal touchpad / tilt-wheel movement (px; positive = fingers right = back).
    Swipe(f32),
    Chrome(chrome::ChromeMsg),
    QueueScrolled(f32),
    Modifiers(keyboard::Modifiers),
    /// The window lost the focus: keys and buttons held are let go.
    Unfocused,
    AccentHue(f32),
    AccentSave,
    PointerUp,
    SidebarHover(bool),
    MediaReady(Option<isize>),

    // Navigation.
    Navigate(Page),
    Back,
    Forward,
    OpenItem(Item),
    PlayItem(Item),
    LibraryCat(&'static str),
    LibraryBarSized(f32),
    InlineAlbumSized(u64, f32),
    RetryPage,
    RetryInlineAlbum(Album),
    LibGrid(&'static str, bool),
    Filter(String),
    SearchInput(String),
    SearchSubmit,
    SearchClose,
    SearchRecent(String),
    PageScrolled(f32, f32),
    ToggleAlbumInline(Album),

    // Playback.
    TogglePlay,
    Next,
    Prev,
    SeekBy(f64),
    ProgressDrag(f32),
    ProgressRelease,
    VolumeDrag(f32),
    VolumeRelease,
    VolumeStep(f32),
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    PlayTracks(Arc<Vec<Track>>, usize),
    /// A row of a list: play that list from it.
    PlayFrom(rows::Source, usize),
    /// A list's Play button: from the start, in order.
    PlayAll(rows::Source),
    ShuffleAll(rows::Source),
    PlayShuffled(Arc<Vec<Track>>),
    Row(rows::RowMsg),
    PlayQueued(u64),
    RemoveQueued(u64),
    AddToQueue(Track, bool),
    StartRadio(Track),
    ClearQueue,

    // Now Playing and side panels.
    ToggleNowPlaying,
    NpTab(NpTab),
    ToggleLyrics,
    ToggleQueue,
    CloseSidePanel,
    WideSidePanel,
    LyricsManual,
    LyricsSync,
    LyricLine(f64),
    RetryLyrics,
    CoverDoubleClick,
    OpenCover(String),
    CloseCover,
    OpenInfo(info::InfoTarget),
    AlbumToPlaylist(u64, String),
    QueueAlbum(u64, bool),
    CloseInfo,

    // Collection.
    ToggleFavorite(Track),
    ToggleFavoriteAlbum(Album),
    ToggleFavoriteArtist(Artist),
    SetRating(u64, u8),
    SetAlbumRating(u64, u8),
    AddToPlaylist(String, Vec<Track>),
    ToggleSavedPlaylist(Playlist),
    OpenDialog(Dialog),
    DialogTitle(String),
    DialogDesc(String),
    DialogSubmit,
    DialogClose,

    // Menus and notices.
    OpenMenu(MenuTarget),
    OpenSortMenu(&'static str, iced::Rectangle),
    CloseMenu,
    CopyLink(String),

    // Settings.
    SettingsTab(usize),
    SetQuality(Quality),
    ToggleSetting(&'static str),
    SetLight(bool),
    SetAccent(u32),
    StepZoom(i32),
    SetOutput(Option<String>, bool, bool),
    ToggleOutputList,
    SetLanguage(Option<String>),
    SetSort(&'static str, String),
    ChooseLocal(bool),
    LocalAdded(Vec<std::path::PathBuf>),
    RemoveLocalTrack(u64),
    RemoveLocalAlbum(u64),
    RescanLocal,
    CheckUpdates,
    ExportData,
    Replay(replay::ReplayMsg),
    LastfmConnect,
    LastfmDisconnect,
    LastfmCancel,
    ChooseService(&'static str),
    WelcomeBack,
    LastfmToggle,
    LastfmSync,
    DiscordId(String),
    Eq(equalizer::EqMsg),
    Local(local::LocalMsg),
    OpenUrl(String),
    WelcomeRepeat,

    // Session.
    StartLogin,
    CancelLogin,
    WelcomeNext,
    WelcomeSkip,
    UseLocalOnly,

    // Window chrome.
    WindowMinimize,
    WindowMaximize,
    WindowClose,
}

// ------------------------------------------------------------------ run

/// Bundled Inter (SIL Open Font License, assets/fonts/Inter-LICENSE.txt).
const FONTS: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/Inter-Bold.ttf"),
];

pub const INTER: iced::Font = iced::Font::with_name("Inter");

pub fn font(weight: iced::font::Weight) -> iced::Font {
    iced::Font { weight, ..INTER }
}

pub fn run() -> iced::Result {
    let icon = window::icon::from_file_data(include_bytes!("../../assets/logo.png"), Some(image::ImageFormat::Png)).ok();
    let mut app = iced::application(App::boot, App::update, App::view)
        .title(|_: &App| crate::i18n::t("app.name").to_string())
        .subscription(App::subscription)
        .theme(App::iced_theme)
        .scale_factor(|_: &App| crate::shared::zoom::zoom())
        .default_font(INTER)
        .antialiasing(true)
        .window(window::Settings {
            // Closing saves the queue and the history first (see `CloseRequested`).
            exit_on_close_request: false,
            size: Size::new(1440.0, 900.0),
            min_size: Some(Size::new(960.0, 600.0)),
            position: window::Position::Centered,
            // The top bar is the title bar, as in GPUI (see `chrome`).
            decorations: false,
            #[cfg(windows)]
            platform_specific: window::settings::PlatformSpecific { undecorated_shadow: true, ..Default::default() },
            icon,
            ..Default::default()
        });
    for f in FONTS {
        app = app.font(f);
    }
    app.run()
}

impl App {
    fn boot() -> (Self, Task<Message>) {
        let settings = Arc::new(Mutex::new(Settings::load()));
        theme::set_light(settings.lock().light_theme);
        theme::set_accent(settings.lock().accent);
        crate::shared::zoom::set_zoom(
            std::env::var("TIDE_ZOOM").ok().and_then(|z| z.parse().ok()).unwrap_or_else(|| settings.lock().zoom).clamp(0.5, 2.0),
        );
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
        images::init(tidal.http().clone());
        let discord = {
            let id = std::env::var("ABYSS_DISCORD_ID")
                .or_else(|_| std::env::var("TIDE_DISCORD_ID"))
                .ok()
                .or_else(|| Some(s.discord_client_id.clone()).filter(|s| !s.is_empty()))
                .unwrap_or_else(|| crate::config::DISCORD_APP_ID.to_string());
            crate::discord::Discord::new(id, s.discord)
        };
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
        let signed_in = s.signed_in() && s.preferred_service != "local";
        let mut app = App {
            window: None,
            tidal,
            data_epoch: 0,
            player,
            events,
            settings,
            signed_in,
            login: LoginState::Idle,
            login_busy: false,
            welcome_step: (!s.welcome_complete).then_some(0),
            page: Page::Home,
            back: Vec::new(),
            forward: Vec::new(),
            page_at: Instant::now(),
            nav_dir: 1.0,
            home: Load::Idle,
            home_tab: 0,
            home_extra: HashMap::new(),
            home_loaded: None,
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
            fav_tracks: Load::Idle,
            fav_albums: Load::Idle,
            fav_artists: Load::Idle,
            my_playlists: Load::Idle,
            my_mixes: Load::Idle,
            fav_ids: HashSet::new(),
            fav_album_ids: HashSet::new(),
            fav_artist_ids: HashSet::new(),
            lyrics: HashMap::new(),
            lyrics_docs: HashMap::new(),
            lyrics_epoch: 0,
            credits: HashMap::new(),
            similar: HashMap::new(),
            tech_info: HashMap::new(),
            artist_pics: HashMap::new(),
            queue: Vec::new(),
            unshuffled: None,
            now: None,
            queued_next: None,
            next_info: None,
            ended: false,
            resume_at: None,
            queue_restoring: true,
            play_failures: 0,
            queue_saved: (crate::queue_store::fingerprint(std::iter::empty()), 0, 0),
            queue_saved_at: Instant::now(),
            snap: Snapshot::default(),
            snap_at: Instant::now(),
            next_key: 1,
            next_uid: 1,
            volume: s.volume,
            muted: false,
            shuffle: s.shuffle,
            repeat: s.repeat,
            autoplay_loading: false,
            autoplay_jump: false,
            drag: None,
            drag_value: 0.0,
            viewport: Size::new(1440.0, 900.0),
            dpi: 1.0,
            fullscreen: false,
            now_playing_open: false,
            np_tab: NpTab::Lyrics,
            show_queue: false,
            show_lyrics: false,
            side_reveal: panel_motion::Reveal::closed(panel_motion::PANEL),
            side_last_lyrics: false,
            lyrics_follow: true,
            lyrics_sync_at: None,
            menu: None,
            menu_exit: None,
            maximized: false,
            hwnd: None,
            ctrl_held: false,
            active_at: Instant::now(),
            side_resize: None,
            queue_scroll: 0.0,
            queue_centred: None,
            queue_nudge: (0, 0.0),
            dialog: None,
            dlg_title: String::new(),
            dlg_desc: String::new(),
            toast: None,
            filter: String::new(),
            expanded_album: None,
            expanded_album_height: 0.0,
            sort_ranks: HashMap::new(),
            lib_queries: HashMap::new(),
            ratings,
            heart_pop: None,
            heart_burst: None,
            swipe: 0.0,
            swipe_at: None,
            volume_tip: None,
            ambient: theme_fade::Fade::new(theme::accent()),
            palette: theme_fade::PaletteFade { from: Vec::new(), to: Vec::new(), at: Instant::now() },
            bg_spin: now_playing::BgSpin::Stopped(0.0),
            settings_tab: 0,
            output_list_open: false,
            devices: audio::output::device_names(),
            info_panel: None,
            cover_view: None,
            update_check: None,
            sidebar_reveal: panel_motion::Reveal::closed(panel_motion::CONTROL),
            page_scroll: 0.0,
            page_view_h: 600.0,
            recents: crate::recents::load(),
            history,
            recorder: Default::default(),
            history_tick: Instant::now(),
            local_tracks: crate::local::load().unwrap_or_else(|error| {
                log::warn!("No se pudo cargar la biblioteca local: {error:#}");
                Vec::new()
            }),
            local_busy: false,
            local_editor: None,
            row_drag: None,
            local_scroll: 0.0,
            local_search_keys: HashMap::new(),
            local_albums: Vec::new(),
            local_artists: Vec::new(),
            lib_tracks: Vec::new(),
            lib_shown: Vec::new(),
            lib_bar_h: pages::INITIAL_LIBRARY_BAR_H,
            dropped: Vec::new(),
            lib_albums: Vec::new(),
            lib_artists: Vec::new(),
            scrobbler: crate::scrobble::Scrobbler::spawn(crate::config::config_dir()),
            scrobble_tracker: Default::default(),
            lastfm_tick: Instant::now(),
            lastfm_seen: 0,
            discord,
            media: None,
            replay_period: Default::default(),
            replay_all_tracks: false,
            replay_cache: Default::default(),
            eq_meter: Default::default(),
            eq_meter_at: Instant::now(),
            eq_selected: None,
            eq_band_selected: None,
            eq_draft: None,
            eq_scale: None,
            last_frame: Instant::now(),
            scroll_memory: Vec::new(),
            seek_hold: None,
        };
        let mut tasks = vec![window::oldest().and_then(|id| window::size(id).map(move |size| Message::WindowOpened(id, size)))];
        if storage_unavailable {
            app.notify_toast(crate::i18n::t("storage.partial_load_warning"));
        }
        // Read the saved queue off the UI thread: checking that local files still
        // exist can stall for seconds on an unplugged drive or a sleeping share.
        let signed_in = app.signed_in;
        tasks.push(Task::perform(blocking(move || crate::queue_store::load(signed_in)), |saved| {
            Message::Done(Callback::new(move |app: &mut App| {
                app.queue_restoring = false;
                let Some(saved) = saved else { return Task::none() };
                // Something already started playing while the file was read: keep it.
                if !app.queue.is_empty() || app.now.is_some() {
                    return Task::none();
                }
                let count = saved.tracks.len();
                app.queue = saved.tracks.into_iter().map(|track| QItem { uid: 0, track }).collect();
                for i in 0..app.queue.len() {
                    app.queue[i].uid = app.new_uid();
                }
                app.resume_at = Some((saved.index, saved.position));
                app.queue_saved = (app.queue_fingerprint(), saved.index, saved.position as i64);
                app.notify_toast(crate::i18n::text("queue.restored", &[count.to_string()]));
                Task::none()
            }))
        }));
        if let Some(name) = &s.settings_recovered {
            app.notify_toast_for(crate::i18n::text("settings.recovered", std::slice::from_ref(name)), 12);
        }
        if s.privacy_notice {
            app.notify_toast_for(crate::i18n::t("privacy.defaults_notice"), 12);
            let mut st = app.settings.lock();
            st.privacy_notice = false;
            st.save();
        }
        if app.signed_in {
            tasks.push(app.after_login());
        }
        if s.preferred_service == "local" {
            app.page = Page::MyTracks;
        }
        // The welcome guide shows once: it counts as seen as soon as it appears.
        if app.welcome_step.is_some() {
            let mut s = app.settings.lock();
            s.welcome_complete = true;
            s.save();
        }
        app.local_changed();
        app.lastfm_apply();
        app.eq_start();
        tasks.push(app.rescan_local_folders());
        tasks.push(app.load_page_data());
        tasks.push(app.demo_from_env());
        (app, Task::batch(tasks))
    }

    fn iced_theme(&self) -> iced::Theme {
        let palette = iced::theme::Palette {
            background: theme::bg(),
            text: theme::text(),
            primary: theme::accent(),
            success: theme::accent(),
            warning: theme::gold(),
            danger: theme::danger(),
        };
        iced::Theme::custom("Abyss".to_string(), palette)
    }

    // ------------------------------------------------------------ helpers

    /// Run `job` on a worker thread with a snapshot of the TIDAL client, then
    /// `done` on the UI thread (dropped if the session data was reset since).
    pub fn fetch<T: Send + 'static>(
        &mut self,
        job: impl FnOnce(Tidal) -> anyhow::Result<T> + Send + 'static,
        done: impl FnOnce(&mut App, anyhow::Result<T>) -> Task<Message> + Send + 'static,
    ) -> Task<Message> {
        let tidal = self.tidal.request_snapshot();
        let epoch = self.data_epoch;
        Task::perform(blocking(move || job(tidal)), move |r| {
            Message::Done(Callback::new(move |app: &mut App| {
                if app.data_epoch != epoch {
                    return Task::none();
                }
                done(app, r)
            }))
        })
    }

    pub fn notify_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    /// A toast that stays `secs` seconds instead of the usual 4.
    pub fn notify_toast_for(&mut self, msg: impl Into<String>, secs: u64) {
        let at = Instant::now() + Duration::from_secs(secs.saturating_sub(4));
        self.toast = Some((msg.into(), at));
    }

    fn save_settings(&self) {
        let mut s = self.settings.lock();
        s.volume = self.volume;
        s.shuffle = self.shuffle;
        s.repeat = self.repeat;
        s.save();
    }

    /// Position shown in the UI: interpolated between engine snapshots so the
    /// progress moves smoothly instead of in 200 ms steps.
    pub fn display_position(&self) -> f64 {
        if !self.is_playing() || self.drag == Some(Drag::Progress) {
            return self.snap.position;
        }
        let dur = self.duration();
        let p = self.snap.position + self.snap_at.elapsed().as_secs_f64();
        if dur > 0.0 {
            p.min(dur)
        } else {
            p
        }
    }

    pub fn is_playing(&self) -> bool {
        self.now.is_some() && !self.snap.paused && !self.ended && !self.snap.buffering
    }

    pub fn duration(&self) -> f64 {
        if self.snap.duration > 0.0 && self.snap.key == self.now.as_ref().map(|n| n.key) {
            self.snap.duration
        } else {
            self.now.as_ref().map(|n| n.track.duration as f64).unwrap_or(0.0)
        }
    }

    /// Something on screen moves by itself: the view is rebuilt for frames.
    #[cfg(test)]
    fn animating(&self) -> bool {
        self.animating_now() || self.animating_ambient()
    }

    /// Motion the eye follows (the swipe's disc, the sync glyph's turn):
    /// a new view on every vsync.
    fn animating_now(&self) -> bool {
        if self.welcome_step.is_some() {
            return false;
        }
        // The lyrics and the backdrop are shaders that ask for their own redraws:
        // a frame message here would rebuild the whole view for each of theirs.
        self.swipe != 0.0 || self.lyrics_sync_at.is_some()
    }

    /// Colour and opacity only (the ambient tint and palette after a track
    /// change, covers fading in as they arrive): GPUI paces these "ambient"
    /// changes at 30 frames a second, and so does this, instead of a whole
    /// view per vsync (144 a second on a fast screen) for something that does
    /// not move.
    fn animating_ambient(&self) -> bool {
        self.welcome_step.is_none() && (self.ambient.animating() || self.palette.animating() || images::fading())
    }

    // ------------------------------------------------------------ update

    fn update(&mut self, message: Message) -> Task<Message> {
        perf::note_message(|| message_kind(&message));
        let quiet = matches!(message, Message::Tick | Message::Frame | Message::Wake);
        let before = (!quiet).then(|| (self.page.clone(), self.page_scroll, self.back.len(), self.forward.len()));
        let mut task = self.dispatch(message);
        if let Some(before) = before {
            task = Task::batch([task, self.restore_scroll(before)]);
        }
        // A dismissed menu fades out unless another one took its place.
        if self.menu.is_some() || self.menu_exit.as_ref().is_some_and(|(_, _, at)| at.elapsed() > motion::EXIT) {
            self.menu_exit = None;
        }
        if self.show_queue || self.show_lyrics {
            self.side_last_lyrics = self.show_lyrics;
        }
        self.side_reveal.set(self.show_queue || self.show_lyrics);
        if !self.signed_in || self.now_playing_open {
            self.sidebar_reveal.set(false);
        }
        // Whatever the message changed (track, panel, size), the queue follows.
        Task::batch([task, self.centre_queue()])
    }

    fn dispatch(&mut self, message: Message) -> Task<Message> {
        if !matches!(message, Message::Tick | Message::Frame | Message::Wake | Message::Done(_)) {
            log::trace!("message {message:?}");
        }
        if !matches!(message, Message::Tick | Message::Frame) {
            self.active_at = Instant::now();
        }
        match message {
            Message::Tick => {
                let mut tasks = vec![self.tick()];
                if !self.local_busy && !self.dropped.is_empty() {
                    let paths = std::mem::take(&mut self.dropped);
                    tasks.push(self.import_local_paths(paths));
                }
                self.cache_lyrics_doc();
                Task::batch(tasks)
            }
            Message::Frame => {
                let now = Instant::now();
                let dt = now.saturating_duration_since(self.last_frame).as_secs_f32().min(0.05);
                self.last_frame = now;
                self.step_frame(dt);
                Task::none()
            }
            Message::Wake => {
                let _ = images::take_dirty();
                // Media keys wake the window: act on them now, not at the next slow tick.
                self.tick()
            }
            Message::Done(cb) => {
                let task = cb.run(self);
                self.cache_lyrics_doc();
                task
            }
            Message::WindowOpened(id, size) => {
                self.window = Some(id);
                self.viewport = size;
                let media = window::run(id, |w| {
                    use raw_window_handle::RawWindowHandle;
                    let hwnd = match w.window_handle().ok().map(|h| h.as_raw()) {
                        Some(RawWindowHandle::Win32(h)) => Some(h.hwnd.get()),
                        _ => None,
                    };
                    // On the window's own thread: the system's hit test now
                    // knows the caption and the resize borders (see `winhit`).
                    if let Some(hwnd) = hwnd {
                        winhit::install(hwnd);
                    }
                    hwnd
                })
                .map(Message::MediaReady);
                let scale = window::scale_factor(id).map(Message::WindowRescaled);
                Task::batch([media, scale])
            }
            Message::MediaReady(hwnd) => {
                self.hwnd = hwnd;
                self.paint_border();
                self.media = playback::Media::new(hwnd);
                Task::none()
            }
            Message::WindowResized(size) => {
                self.viewport = size;
                self.refit();
                match self.window {
                    Some(id) => window::is_maximized(id).map(|on| Message::Chrome(chrome::ChromeMsg::Maximized(on))),
                    None => Task::none(),
                }
            }
            Message::WindowRescaled(dpi) => {
                self.dpi = dpi;
                self.refit();
                Task::none()
            }
            Message::CloseRequested | Message::WindowClose => {
                log::info!("closing: saving the queue and the history");
                self.flush_history();
                self.save_queue(true);
                crate::local::flush_index(Duration::from_secs(5));
                iced::exit()
            }
            Message::FilesDropped(path) => {
                self.dropped.push(path);
                Task::none()
            }
            Message::Key(key, physical, mods) => self.on_key(key, physical, mods),
            Message::MouseBack => self.go_back(),
            Message::MouseForward => self.go_forward(),
            Message::Swipe(dx) => self.swipe_by(dx),
            Message::Chrome(msg) => self.chrome_update(msg),
            Message::Modifiers(mods) => {
                self.ctrl_held = mods.control();
                Task::none()
            }
            // Alt+Tab in the middle of a drag: Windows takes the mouse capture
            // and the release never arrives. Let go of everything held, as a
            // release would (Ctrl, a slider, the side panel's edge, a row).
            Message::Unfocused => {
                self.ctrl_held = false;
                let slider = match self.drag {
                    Some(Drag::Progress) => self.update_more(Message::ProgressRelease),
                    Some(Drag::Volume) => self.update_more(Message::VolumeRelease),
                    None => Task::none(),
                };
                let side = self.chrome_update(chrome::ChromeMsg::PointerUp);
                Task::batch([slider, side, self.row_update(rows::RowMsg::Release)])
            }
            Message::QueueScrolled(y) => {
                self.queue_scroll = y;
                Task::none()
            }
            Message::PointerUp => {
                let side = self.chrome_update(chrome::ChromeMsg::PointerUp);
                Task::batch([side, self.row_update(rows::RowMsg::Release)])
            }
            Message::WindowMinimize => self.window.map_or_else(Task::none, |id| window::minimize(id, true)),
            Message::WindowMaximize => self.window.map_or_else(Task::none, window::toggle_maximize),
            other => self.update_more(other),
        }
    }

    /// The window's size or scale changed: refit the automatic zoom.
    fn refit(&mut self) {
        let z = crate::shared::zoom::zoom();
        // `viewport` is in design px (already divided by the zoom).
        let logical = Size::new(self.viewport.width * z, self.viewport.height * z);
        if crate::shared::zoom::fit_viewport_size(logical.width, logical.height) {
            // Iced applies the new scale without another `Resized`: follow it here.
            let now = crate::shared::zoom::zoom();
            self.viewport = Size::new(logical.width / now, logical.height / now);
        }
        images::set_scale(self.dpi * crate::shared::zoom::zoom());
    }

    pub fn set_fullscreen(&mut self, on: bool) -> Task<Message> {
        self.fullscreen = on;
        let Some(id) = self.window else { return Task::none() };
        let mode = window::set_mode(id, if on { window::Mode::Fullscreen } else { window::Mode::Windowed });
        match self.hwnd.filter(|_| on) {
            Some(hwnd) => mode.chain(window::run(id, move |_| chrome::cover_screen(hwnd)).discard()),
            None => mode,
        }
    }

    pub(super) fn paint_border(&self) {
        if let Some(hwnd) = self.hwnd {
            chrome::set_border_color(hwnd, theme::frame_rgb());
        }
    }

    /// A shortcut's answer is there at once (no page slide, no fade-in of
    /// what it opens): shortcuts are pressed hundreds of times a day, and an
    /// entrance would only make them feel slow. A mouse or a swipe still
    /// gets the motion that shows where things came from.
    fn on_key(&mut self, key: keyboard::Key, physical: keyboard::key::Physical, mods: keyboard::Modifiers) -> Task<Message> {
        motion::keyed();
        let page_at = self.page_at;
        let task = self.on_key_inner(key, physical, mods);
        if self.page_at != page_at {
            self.page_at = self.page_at.checked_sub(Duration::from_secs(1)).unwrap_or(self.page_at);
        }
        task
    }

    fn on_key_inner(&mut self, key: keyboard::Key, physical: keyboard::key::Physical, mods: keyboard::Modifiers) -> Task<Message> {
        use keyboard::key::{Code, Named, Physical};
        use keyboard::Key;
        let ctrl = mods.command();
        // A key that types "+", "-" or "0" is matched below by what it types;
        // the physical keys only decide when the layout gives no character.
        if ctrl && !matches!(key, Key::Character(_)) {
            match physical {
                Physical::Code(Code::Equal | Code::NumpadAdd) => return self.step_zoom(1),
                Physical::Code(Code::Minus | Code::NumpadSubtract) => return self.step_zoom(-1),
                Physical::Code(Code::Digit0 | Code::Numpad0) => return self.set_zoom_level(1.0),
                _ => {}
            }
        }
        // A selected track row takes the arrows, Home / End, Enter and Escape.
        if let Some(task) = self.row_key(&key, mods) {
            return task;
        }
        match key.as_ref() {
            Key::Named(Named::Space) if !ctrl => self.toggle_play(),
            Key::Named(Named::ArrowLeft) if mods.alt() => self.go_back(),
            Key::Named(Named::ArrowRight) if mods.alt() => self.go_forward(),
            Key::Named(Named::ArrowRight) if ctrl && !mods.shift() => self.next(),
            Key::Named(Named::ArrowLeft) if ctrl && !mods.shift() => self.prev(),
            Key::Named(Named::ArrowRight) => {
                let p = (self.snap.position + 10.0).min(self.duration());
                self.seek(p)
            }
            Key::Named(Named::ArrowLeft) => {
                let p = (self.snap.position - 10.0).max(0.0);
                self.seek(p)
            }
            Key::Named(Named::ArrowUp) if ctrl => {
                self.set_volume(self.volume + 0.05);
                self.save_settings();
                Task::none()
            }
            Key::Named(Named::ArrowDown) if ctrl => {
                self.set_volume(self.volume - 0.05);
                self.save_settings();
                Task::none()
            }
            Key::Named(Named::Backspace) if mods.shift() => self.go_forward(),
            Key::Named(Named::Backspace) => self.go_back(),
            Key::Named(Named::Escape) => {
                if self.dialog.is_some() {
                    self.dialog = None;
                } else if self.menu.is_some() {
                    self.menu = None;
                } else if self.search_open {
                    self.search_open = false;
                } else if self.info_panel.is_some() {
                    self.info_panel = None;
                } else if self.cover_view.is_some() {
                    self.cover_view = None;
                } else if self.now_playing_open {
                    return self.toggle_now_playing();
                }
                Task::none()
            }
            Key::Character(c) if ctrl => match c {
                "f" | "l" => {
                    self.search_open = self.signed_in;
                    iced::widget::operation::focus(widgets::SEARCH_ID)
                }
                "p" => self.navigate(Page::Settings),
                "h" => self.navigate(Page::Home),
                "s" => self.toggle_shuffle(),
                "=" | "+" => self.step_zoom(1),
                "-" => self.step_zoom(-1),
                "0" => self.set_zoom_level(1.0),
                _ => Task::none(),
            },
            Key::Character("m") if !ctrl => {
                self.toggle_mute();
                Task::none()
            }
            _ => Task::none(),
        }
    }

    pub fn step_zoom(&mut self, dir: i32) -> Task<Message> {
        let z = crate::shared::zoom::step_from(crate::shared::zoom::user_zoom(), dir);
        self.set_zoom_level(z)
    }

    pub fn set_zoom_level(&mut self, z: f32) -> Task<Message> {
        log::info!("zoom -> {z}");
        // Keep the window's size in DPI px while the design px change.
        let before = crate::shared::zoom::zoom();
        crate::shared::zoom::set_zoom(z);
        let after = crate::shared::zoom::zoom();
        self.viewport = Size::new(self.viewport.width * before / after, self.viewport.height * before / after);
        self.refit();
        {
            let mut s = self.settings.lock();
            s.zoom = z;
            s.save();
        }
        self.notify_toast(crate::i18n::text("zoom", &[format!("{}", (z * 100.0).round() as i32)]));
        Task::none()
    }

    /// Advance what animates on its own between frames.
    /// GPUI's touchpad gesture: 150 px of horizontal swipe goes back or forward.
    fn swipe_by(&mut self, dx: f32) -> Task<Message> {
        const TRIGGER: f32 = 150.0;
        if self.dialog.is_some() || self.menu.is_some() || self.cover_view.is_some() {
            return Task::none();
        }
        if self.swipe != 0.0 && self.swipe.signum() != dx.signum() {
            self.swipe = 0.0;
        }
        self.swipe += dx;
        self.swipe_at = Some(Instant::now());
        if self.swipe >= TRIGGER {
            self.swipe = 0.0;
            if self.can_go_back() {
                return self.go_back();
            }
        } else if self.swipe <= -TRIGGER {
            self.swipe = 0.0;
            if self.can_go_forward() {
                return self.go_forward();
            }
        }
        Task::none()
    }

    /// Advance what animates on its own, `dt` seconds after the last frame.
    fn step_frame(&mut self, dt: f32) {
        if self.lyrics_sync_at.is_some_and(|at| at.elapsed() > Duration::from_millis(900)) {
            self.lyrics_sync_at = None;
        }
        if self.heart_pop.is_some_and(|(_, at)| at.elapsed() > Duration::from_millis(400)) {
            self.heart_pop = None;
        }
        // An unfinished swipe slides back once the fingers stop (GPUI `haptics`).
        if self.volume_tip.is_some_and(|at| at.elapsed() > player_bar::VOLUME_TIP) && self.drag != Some(Drag::Volume) {
            self.volume_tip = None;
        }
        if self.swipe != 0.0 && self.swipe_at.is_some_and(|at| at.elapsed() > Duration::from_millis(160)) {
            self.swipe *= swipe_decay(dt);
            if self.swipe.abs() < 2.0 || crate::shared::lyrics::reduced_motion() {
                self.swipe = 0.0;
            }
        }
        self.track_colors();
    }

    /// Follow the playing cover's colours (cross-faded on track changes).
    pub fn track_colors(&mut self) {
        let url = self.now.as_ref().and_then(|n| n.track.cover(640));
        let target = match url.as_deref().and_then(images::color) {
            Some(c) => c,
            None if self.now.is_some() && self.ambient.seen => self.ambient.to,
            None => theme::accent(),
        };
        self.ambient.seen |= self.now.is_some();
        self.ambient.target(target);
        let pal = url.as_deref().map(images::palette).unwrap_or_default();
        let target = if pal.len() >= 3 {
            pal
        } else if self.palette.to.len() >= 3 {
            self.palette.to.clone()
        } else {
            let a = self.ambient.now();
            vec![a, a, a]
        };
        self.palette.target(target);
    }

    // ------------------------------------------------------------ view

    fn view(&self) -> Element<'_, Message> {
        let t0 = Instant::now();
        images::evict();
        // What the glass surfaces refract in this view.
        liquid_glass::update_scene(self);
        let root = perf::root(chrome::hit_map(self, self.view_root()));
        perf::note_view(t0.elapsed(), || self.perf_context());
        root
    }

    fn view_root(&self) -> Element<'_, Message> {
        if self.welcome_step.is_some() {
            let screen = chrome::caption_strip(self, perf::probe("welcome", pages::welcome(self)));
            return self.with_overlays(screen);
        }
        let offline_ok = match &self.page {
            Page::MyTracks | Page::MyAlbums | Page::MyArtists | Page::Settings | Page::Search(_) => true,
            Page::Album(id) | Page::Artist(id) => crate::local::is_local_id(*id),
            _ => false,
        };
        if !self.signed_in && !offline_ok {
            let screen = chrome::caption_strip(self, perf::probe("login", pages::login_screen(self)));
            return self.with_overlays(screen);
        }
        if self.now_playing_open {
            // Built and drawn in the dark palette (styles are evaluated at draw time).
            let np = perf::probe("now_playing", dark_draw::dark(theme::dark_scope(|| now_playing::view(self))));
            return self.with_overlays(np);
        }
        let gap = theme::FRAME_GAP;
        let content = row![perf::probe("content", pages::content(self))]
            .push(pages::side_panel(self).map(|p| perf::probe("side_panel", p)))
            .height(Length::Fill);
        let panel = container(column![perf::probe("top_bar", pages::top_bar(self)), content])
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .style(|_| container::Style {
                background: Some(theme::bg().into()),
                border: iced::Border { color: theme::panel_border(), width: 1.0, radius: theme::PANEL_R.into() },
                ..Default::default()
            });
        let body = row![]
            .push(self.signed_in.then(|| perf::probe("sidebar", sidebar::view(self, false))))
            .push(container(panel).padding(iced::Padding {
                top: gap,
                right: gap,
                bottom: gap,
                left: if self.signed_in { 0.0 } else { gap },
            }))
            .height(Length::Fill);
        let bar = container(container(perf::probe("player_bar", player_bar::view(self))).clip(true).style(|_| container::Style {
            border: iced::Border { color: theme::divider(), width: 1.0, radius: theme::PANEL_R.into() },
            background: Some(theme::bg().into()),
            ..Default::default()
        }))
        .padding(iced::Padding { top: 0.0, right: gap, bottom: gap, left: gap });
        let main = container(column![body, bar]).width(Length::Fill).height(Length::Fill).style(|_| container::Style {
            background: Some(theme::frame().into()),
            text_color: Some(theme::text()),
            ..Default::default()
        });
        self.with_overlays(main.into())
    }

    /// Menus, dialogs, toasts and full-window panels over `base`.
    fn with_overlays<'a>(&'a self, base: Element<'a, Message>) -> Element<'a, Message> {
        // Always the same slots in the same order (see `widgets::layered`).
        let sidebar = (self.sidebar_reveal.visible() && self.signed_in && !self.now_playing_open).then(|| {
            container(sidebar::view(self, true))
                .height(Length::Fill)
                .padding(iced::Padding { bottom: theme::PLAYER_H + theme::FRAME_GAP, ..Default::default() })
                .into()
        });
        let named = |name: &'static str, layer: Option<Element<'a, Message>>| layer.map(|l| perf::probe(name, l));
        let over = vec![
            named("sidebar_overlay", sidebar),
            named("search_overlay", home::search_overlay(self)),
            named("cover_view", self.cover_view.as_ref().map(|cover| pages::cover_overlay(self, cover))),
            named("info_panel", self.info_panel.as_ref().map(|target| info::overlay(self, target))),
            named("menu", self.menu.as_ref().map(|(at, target)| menus::overlay(self, *at, target))),
            named("menu_exit", self.menu_exit.as_ref().map(|(at, target, closed)| menus::ghost(self, *at, target, *closed))),
            named("dialog", self.dialog.as_ref().map(|dialog| pages::dialog_overlay(self, dialog))),
            // A toast rises 8 px into place as it fades in, once per message
            // (a shortcut's toast, such as the zoom's, is there at once).
            named("toast", self.toast.as_ref().map(|(text, at)| wash::rise(widgets::toast(text), motion::PANEL, toast_key(*at), 8.0))),
            named("swipe", widgets::swipe_indicator(self.swipe)),
            named("resize_edges", chrome::resize_edges(self)),
            // Over everything while Ctrl is held, so the wheel reaches nothing else.
            self.ctrl_held.then(|| {
                iced::widget::mouse_area(iced::widget::Space::new().width(Length::Fill).height(Length::Fill))
                    .on_scroll(|delta| {
                        let y = match delta {
                            mouse::ScrollDelta::Lines { y, .. } | mouse::ScrollDelta::Pixels { y, .. } => y,
                        };
                        Message::StepZoom(if y > 0.0 {
                            1
                        } else if y < 0.0 {
                            -1
                        } else {
                            0
                        })
                    })
                    .into()
            }),
        ];
        widgets::layered(base, over)
    }

    // ------------------------------------------------------------ subscriptions

    /// Every tick rebuilds and redraws the window. While playing (or just
    /// after an input) it follows the engine 5 times a second; while
    /// something waits on it (a toast to hide, an import, a retry, Last.fm
    /// connecting) once a second; otherwise every 5 s: media keys, finished
    /// jobs and the audio engine's events wake the window by themselves
    /// (`wake`), so an idle Abyss has nothing to poll for.
    fn poll_ms(&self) -> u64 {
        let playing = self.now.is_some() && !self.ended && !self.snap.paused;
        if playing || self.active_at.elapsed() < Duration::from_secs(3) {
            POLL_ACTIVE_MS
        } else if self.waiting_on_tick() {
            POLL_WAITING_MS
        } else {
            POLL_IDLE_MS
        }
    }

    /// Something the tick finishes is under way.
    fn waiting_on_tick(&self) -> bool {
        self.toast.is_some()
            || self.local_busy
            || !self.dropped.is_empty()
            || self.queue_restoring
            || self.autoplay_loading
            || self.ratings.has_pending()
            || self.now.as_ref().is_some_and(|_| self.snap.buffering)
            || !matches!(self.scrobbler.status.lock().phase, crate::scrobble::worker::Phase::Idle)
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            every(self.poll_ms()).map(|_| Message::Tick),
            Subscription::run(wake_stream),
            event::listen_with(|event, status, _window| {
                // A person is there: ambient motion runs (see `shared::activity`).
                let woke = match &event {
                    iced::Event::Keyboard(_) | iced::Event::Touch(_) => crate::shared::activity::input(),
                    iced::Event::Mouse(
                        mouse::Event::CursorMoved { .. } | mouse::Event::ButtonPressed(_) | mouse::Event::WheelScrolled { .. },
                    ) => crate::shared::activity::input(),
                    _ => false,
                };
                let message = Self::on_event(event, status);
                // The first input after a rest: a new view so the motion starts again.
                if message.is_none() && woke {
                    return Some(Message::Wake);
                }
                message
            }),
        ];
        if self.eq_visible() {
            // The meters, about 30 times a second while the equaliser shows.
            subs.push(every(33).map(|_| Message::Eq(equalizer::EqMsg::Meter)));
        }
        if self.animating_now() {
            subs.push(window::frames().map(|_| Message::Frame));
        } else if self.animating_ambient() {
            subs.push(every(AMBIENT_FRAME_MS).map(|_| Message::Frame));
        }
        // While playing, the 200 ms tick already rebuilds the view (the
        // progress and the time move with it): a separate 250 ms frame timer
        // made it 9 views a second instead of 5, out of step with each other.
        // The volume's percentage stays still until it goes: one wake-up when
        // it is due (it was a whole view per vsync for 1.1 s on every change).
        if let Some(at) = self.volume_tip.filter(|_| self.drag != Some(Drag::Volume)) {
            subs.push(after(at + player_bar::VOLUME_TIP + Duration::from_millis(5)).map(|_| Message::Frame));
        }
        Subscription::batch(subs)
    }

    /// The window events the interface listens to, as messages.
    fn on_event(event: iced::Event, status: event::Status) -> Option<Message> {
        match event {
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modified_key, physical_key, modifiers, .. })
                if status == event::Status::Ignored =>
            {
                // Characters as typed (Shift and the layout applied), named keys as they are.
                Some(Message::Key(
                    if matches!(modified_key, keyboard::Key::Character(_)) { modified_key } else { key },
                    physical_key,
                    modifiers,
                ))
            }
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(mods)) => Some(Message::Modifiers(mods)),
            iced::Event::Window(window::Event::Unfocused) => {
                // Ambient motion slows down behind other windows (see `motion::ambient_interval`).
                motion::set_focused(false);
                let _ = crate::shared::activity::set_focused(false);
                Some(Message::Unfocused)
            }
            iced::Event::Window(window::Event::Focused) => {
                motion::set_focused(true);
                crate::shared::activity::set_focused(true).then_some(Message::Wake)
            }
            iced::Event::Window(window::Event::Resized(size)) => Some(Message::WindowResized(size)),
            iced::Event::Window(window::Event::Rescaled(f)) => Some(Message::WindowRescaled(f)),
            iced::Event::Window(window::Event::CloseRequested) => Some(Message::CloseRequested),
            iced::Event::Window(window::Event::FileDropped(path)) => Some(Message::FilesDropped(path)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Some(Message::PointerUp),
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)) => Some(Message::MouseBack),
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)) => Some(Message::MouseForward),
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => chrome::pointer_moved(position),
            // Horizontal swipes nothing else scrolled with: back / forward (GPUI's wheel catcher).
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) if status == event::Status::Ignored => {
                let (dx, dy) = match delta {
                    mouse::ScrollDelta::Pixels { x, y } => (x, y),
                    mouse::ScrollDelta::Lines { x, y } => (x * 60.0, y * 60.0),
                };
                (dx != 0.0 && dx.abs() > dy.abs() * 1.5).then_some(Message::Swipe(dx))
            }
            _ => None,
        }
    }

    /// The state behind the numbers of the `ABYSS_PERF` line: what the
    /// subscriptions tick at, and how big the caches are.
    fn perf_context(&self) -> String {
        let (dec, enc) = images::usage();
        let frames = if self.animating_now() {
            "vsync"
        } else if self.animating_ambient() {
            "ambient-33ms"
        } else if self.is_playing() {
            "playing-250ms"
        } else {
            "none"
        };
        format!(
            "tick {} ms, frame sub {frames}{}, playing {} | images {:.1}+{:.1} MB ({} entries) | maps albums {} artists {} playlists {} mixes {} search {} lyrics {}/{} credits {} similar {} tech {} pics {} history {}+{}",
            self.poll_ms(),
            if self.eq_visible() { " + eq-33ms" } else { "" },
            self.is_playing(),
            dec as f64 / 1048576.0,
            enc as f64 / 1048576.0,
            images::entries(),
            self.albums.len(),
            self.artists.len(),
            self.playlists.len(),
            self.mixes.len(),
            self.search.len(),
            self.lyrics.len(),
            self.lyrics_docs.len(),
            self.credits.len(),
            self.similar.len(),
            self.tech_info.len(),
            self.artist_pics.len(),
            self.back.len(),
            self.forward.len(),
        )
    }
}

/// The name of a message's variant for the `ABYSS_PERF` log (the payload of
/// most is not formatted: a `PlayTracks` would print whole track lists).
fn message_kind(message: &Message) -> String {
    match message {
        Message::Tick => "Tick".into(),
        Message::Frame => "Frame".into(),
        Message::Wake => "Wake".into(),
        Message::Done(_) => "Done".into(),
        Message::Modifiers(_) => "Modifiers".into(),
        Message::Chrome(m) => format!("Chrome::{}", perf::variant(&format!("{m:?}"))),
        Message::PageScrolled(..) => "PageScrolled".into(),
        Message::QueueScrolled(_) => "QueueScrolled".into(),
        Message::Eq(m) => format!("Eq::{}", perf::variant(&format!("{m:?}"))),
        Message::Row(_) => "Row".into(),
        Message::Key(..) => "Key".into(),
        Message::Swipe(_) => "Swipe".into(),
        Message::PlayTracks(..) => "PlayTracks".into(),
        Message::PlayShuffled(_) => "PlayShuffled".into(),
        Message::AddToPlaylist(..) => "AddToPlaylist".into(),
        Message::OpenMenu(_) => "OpenMenu".into(),
        Message::OpenItem(_) => "OpenItem".into(),
        Message::PlayItem(_) => "PlayItem".into(),
        other => perf::variant(&format!("{other:?}")),
    }
}

impl App {
    /// The page changed: remember where the old one was scrolled, and put a
    /// page reached with Back or Forward where it was left (a new visit
    /// starts at the top).
    fn restore_scroll(&mut self, (page, scroll, back, forward): (Page, f32, usize, usize)) -> Task<Message> {
        if self.page == page {
            return Task::none();
        }
        // A menu belongs to the page it was opened on (Back with the mouse's
        // button or Alt+Left left it floating over the next one, acting on a
        // row no longer shown).
        self.menu = None;
        self.scroll_memory.retain(|(p, _)| *p != page);
        self.scroll_memory.push((page, scroll));
        if self.scroll_memory.len() > HISTORY_CAP {
            self.scroll_memory.remove(0);
        }
        // Back pops the back stack; Forward pops the forward one (and pushes one back).
        let returned = self.back.len() < back || (self.forward.len() + 1 == forward && self.back.len() == back + 1);
        if !returned {
            return Task::none();
        }
        let Some(y) = self.scroll_memory.iter().rev().find(|(p, _)| *p == self.page).map(|(_, y)| *y).filter(|y| *y > 0.0) else {
            return Task::none();
        };
        self.page_scroll = y;
        iced::widget::operation::scroll_to(pages::PAGE_SCROLL, iced::widget::scrollable::AbsoluteOffset { x: 0.0, y })
    }

    // ------------------------------------------------------------ caches

    /// Keep the per-page caches from growing for the whole session: past
    /// [`CACHE_CAP`] entries, a map keeps only what the pages in the history,
    /// the playing track and the open panels use (anything dropped loads
    /// again when visited). The history itself keeps its last [`HISTORY_CAP`] pages.
    pub(super) fn trim_caches(&mut self) {
        fn trim<K: std::hash::Hash + Eq, V>(map: &mut HashMap<K, V>, keep: impl Fn(&K) -> bool) {
            if map.len() > CACHE_CAP {
                map.retain(|k, _| keep(k));
            }
        }
        for stack in [&mut self.back, &mut self.forward] {
            if stack.len() > HISTORY_CAP {
                stack.drain(..stack.len() - HISTORY_CAP);
            }
        }
        let big = [
            self.albums.len(),
            self.album_extra.len(),
            self.artists.len(),
            self.playlists.len(),
            self.mixes.len(),
            self.browse.len(),
            self.search.len(),
            self.lyrics.len(),
            self.credits.len(),
            self.similar.len(),
            self.tech_info.len(),
        ]
        .into_iter()
        .any(|n| n > CACHE_CAP);
        if !big && self.artist_pics.len() <= 4 * CACHE_CAP {
            return;
        }
        let pages: Vec<&Page> = std::iter::once(&self.page).chain(&self.back).chain(&self.forward).collect();
        let mut album_ids: HashSet<u64> = pages.iter().filter_map(|p| if let Page::Album(id) = p { Some(*id) } else { None }).collect();
        album_ids.extend(self.expanded_album);
        album_ids.extend(self.now.as_ref().and_then(|n| n.track.album.as_ref()).map(|a| a.id));
        if let Some(info::InfoTarget::Album(a)) = &self.info_panel {
            album_ids.insert(a.id);
        }
        let artist_ids: HashSet<u64> = pages.iter().filter_map(|p| if let Page::Artist(id) = p { Some(*id) } else { None }).collect();
        let playlist_ids: HashSet<&str> =
            pages.iter().filter_map(|p| if let Page::Playlist(u) = p { Some(u.as_str()) } else { None }).collect();
        let mix_ids: HashSet<&str> = pages.iter().filter_map(|p| if let Page::Mix(m) = p { Some(m.as_str()) } else { None }).collect();
        let browse_ids: HashSet<&str> =
            pages.iter().filter_map(|p| if let Page::Browse(b, _) = p { Some(b.as_str()) } else { None }).collect();
        let mut queries: HashSet<&str> =
            pages.iter().filter_map(|p| if let Page::Search(q) = p { Some(q.as_str()) } else { None }).collect();
        queries.insert(self.search_query.trim());
        // The playing track, the next few (gapless, lyrics ready) and the one an info panel shows.
        let mut track_ids: HashSet<u64> = HashSet::new();
        if let Some(now) = &self.now {
            track_ids.insert(now.track.id);
            track_ids.extend(self.queue.iter().skip(now.index).take(4).map(|q| q.track.id));
        }
        if let Some(info::InfoTarget::Song(t)) = &self.info_panel {
            track_ids.insert(t.id);
        }
        let (albums, artists) = (&album_ids, &artist_ids);
        trim(&mut self.albums, |id| albums.contains(id));
        trim(&mut self.album_extra, |id| albums.contains(id));
        trim(&mut self.artists, |id| artists.contains(id));
        trim(&mut self.playlists, |u| playlist_ids.contains(u.as_str()));
        trim(&mut self.mixes, |m| mix_ids.contains(m.as_str()));
        trim(&mut self.browse, |b| browse_ids.contains(b.as_str()));
        trim(&mut self.search, |q| queries.contains(q.as_str()));
        let tracks = &track_ids;
        trim(&mut self.lyrics, |id| tracks.contains(id));
        self.lyrics_docs.retain(|id, _| tracks.contains(id) || self.lyrics.contains_key(id));
        trim(&mut self.credits, |id| tracks.contains(id));
        trim(&mut self.similar, |id| tracks.contains(id));
        trim(&mut self.tech_info, |id| tracks.contains(id));
        if self.artist_pics.len() > 4 * CACHE_CAP {
            self.artist_pics.clear();
        }
    }
}

/// Entries a per-page cache holds before it is trimmed (see `App::trim_caches`).
const CACHE_CAP: usize = 48;
/// Pages kept for Back and for Forward.
const HISTORY_CAP: usize = 100;

/// A key for the toast shown since `at`: a new message enters anew.
fn toast_key(at: Instant) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    at.hash(&mut h);
    h.finish()
}

/// Tick intervals (see `App::poll_ms`).
const POLL_ACTIVE_MS: u64 = 200;
const POLL_WAITING_MS: u64 = 1000;
const POLL_IDLE_MS: u64 = 5000;

/// GPUI's `AMBIENT_FPS`: 30 views a second for colour-only changes.
const AMBIENT_FRAME_MS: u64 = 33;

/// How much of a released swipe is left after `dt` seconds: GPUI's 18 % per
/// frame at 60 Hz, the same speed at any refresh rate.
fn swipe_decay(dt: f32) -> f32 {
    0.82_f32.powf(dt.max(0.0) * 60.0)
}

/// One tick at `at` (nothing before it), from a sleeping thread.
fn after(at: Instant) -> Subscription<()> {
    Subscription::run_with(at, |at| {
        let at = *at;
        let (mut tx, rx) = iced::futures::channel::mpsc::channel(1);
        std::thread::Builder::new()
            .name("wake-once".into())
            .spawn(move || {
                std::thread::sleep(at.saturating_duration_since(Instant::now()));
                let _ = tx.try_send(());
            })
            .expect("spawn the wake-up");
        rx
    })
}

/// A tick every `ms` milliseconds, from a sleeping thread (no async runtime needed).
fn every(ms: u64) -> Subscription<()> {
    Subscription::run_with(ms, |ms| {
        let ms = *ms;
        let (mut tx, rx) = iced::futures::channel::mpsc::channel(1);
        std::thread::Builder::new()
            .name(format!("tick-{ms}"))
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(ms));
                match tx.try_send(()) {
                    Ok(()) => {}
                    Err(e) if e.is_full() => {}
                    Err(_) => break,
                }
            })
            .expect("spawn the ticker");
        rx
    })
}

#[cfg(test)]
mod frame_tests {
    use super::swipe_decay;

    #[test]
    fn a_released_swipe_slides_back_as_fast_at_any_refresh_rate() {
        for hz in [60.0_f32, 144.0, 240.0] {
            let mut swipe = 100.0_f32;
            for _ in 0..(hz as usize / 4) {
                swipe *= swipe_decay(1.0 / hz);
            }
            // A quarter of a second later: 0.82^15 of it is left, whatever the screen.
            assert!((swipe - 100.0 * 0.82_f32.powi(15)).abs() < 0.05, "{hz}: {swipe}");
        }
        assert_eq!(swipe_decay(0.0), 1.0);
    }

    /// What asks for whole-view frames, what a shortcut animates and how a
    /// menu leaves, on a booted app.
    #[test]
    #[ignore = "requires an isolated profile (ABYSS_CONFIG_DIR, ABYSS_CACHE_DIR)"]
    fn frames_shortcuts_and_menu_exits_on_a_booted_app() {
        use super::{motion, App, MenuTarget, Message};
        use crate::shared::Page;
        use iced::keyboard::{self, key};
        use std::time::Instant;
        assert!(std::env::var_os("ABYSS_CONFIG_DIR").is_some() && std::env::var_os("ABYSS_CACHE_DIR").is_some());
        let (mut app, boot) = App::boot();
        drop(boot);
        app.welcome_step = None;

        // The volume's percentage is still: no frames while it shows.
        app.volume_tip = Some(Instant::now());
        assert!(!app.animating_now());
        app.volume_tip = None;
        app.swipe = 40.0;
        assert!(app.animating_now());
        app.swipe = 0.0;

        // Back with the mouse's button: the page slides in. With Backspace: it is just there.
        drop(app.update(Message::Navigate(Page::Settings)));
        drop(app.update(Message::Navigate(Page::MyTracks)));
        assert_eq!(app.page, Page::MyTracks);
        let before = Instant::now();
        drop(app.update(Message::MouseBack));
        assert_eq!(app.page, Page::Settings);
        assert!(app.page_at >= before, "a mouse's back slides the page in");
        drop(app.update(Message::MouseForward));
        let unknown = key::Physical::Unidentified(key::NativeCode::Unidentified);
        let before = Instant::now();
        drop(app.update(Message::Key(keyboard::Key::Named(key::Named::Backspace), unknown, keyboard::Modifiers::empty())));
        assert_eq!(app.page, Page::Settings);
        assert!(app.page_at + motion::PANEL <= before, "Backspace's page is there at once");

        // A click elsewhere: the menu fades out, until another one opens.
        app.menu = Some((iced::Point::new(100.0, 100.0), MenuTarget::Output));
        drop(app.update(Message::CloseMenu));
        assert!(app.menu.is_none() && app.menu_exit.is_some());
        drop(app.update(Message::OpenMenu(MenuTarget::Output)));
        assert!(app.menu.is_some() && app.menu_exit.is_none());
        // Esc closes it at once.
        let esc = keyboard::Key::Named(key::Named::Escape);
        drop(app.update(Message::Key(esc, unknown, keyboard::Modifiers::empty())));
        assert!(app.menu.is_none() && app.menu_exit.is_none());
    }
}
