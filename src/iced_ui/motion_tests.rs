//! Motion measured on a booted [`App`], offscreen: the view is rebuilt,
//! laid out, sent its events and drawn as Iced's event loop does it, and what
//! the widgets ask for next (`Shell::redraw_request`) is followed in real
//! time. That is how the idle frame rate, reversals and stress sequences are
//! checked without a window (`motion::full_motion_for_tests`).
//!
//! `cargo test --bin Abyss -- --include-ignored motion_on_app` with
//! `ABYSS_CONFIG_DIR` / `ABYSS_CACHE_DIR` set to empty temp folders.

use super::super::*;
use iced::advanced::{clipboard, layout, renderer::Headless, renderer::Renderer as _, widget::Tree, Layout, Shell};
use iced::window::RedrawRequest;
use iced::{Pixels, Rectangle, Renderer};
use serde_json::json;

/// An offscreen window: the widget tree kept between frames, as Iced keeps it.
pub(super) struct Screen {
    renderer: Renderer,
    tree: Option<Tree>,
    pub size: Size,
    pub cursor: mouse::Cursor,
}

/// What one frame asked for.
pub(super) struct Frame {
    pub next: RedrawRequest,
    pub messages: Vec<Message>,
}

impl Screen {
    pub fn new(size: Size) -> Self {
        static FONTS_LOADED: std::sync::Once = std::sync::Once::new();
        FONTS_LOADED.call_once(|| {
            let mut fs = iced::advanced::graphics::text::font_system().write().expect("font system");
            for f in FONTS {
                fs.load_font(std::borrow::Cow::Borrowed(f));
            }
        });
        let renderer =
            iced::futures::executor::block_on(<Renderer as Headless>::new(INTER, Pixels(16.0), None)).expect("offscreen wgpu adapter");
        Self { renderer, tree: None, size, cursor: mouse::Cursor::Unavailable }
    }

    /// One pass of Iced's loop over `view`: diffed into the kept tree, laid
    /// out, sent `events` and then the redraw at `at`, and drawn.
    pub fn frame_of(&mut self, mut view: Element<'_, Message>, theme: &iced::Theme, events: &[iced::Event], at: Instant) -> Frame {
        let tree = match &mut self.tree {
            Some(tree) => {
                tree.diff(view.as_widget());
                tree
            }
            None => self.tree.insert(Tree::new(view.as_widget())),
        };
        let limits = layout::Limits::new(self.size, self.size);
        let mut node = view.as_widget_mut().layout(tree, &self.renderer, &limits);
        let bounds = Rectangle::with_size(self.size);
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        let redraw = iced::Event::Window(window::Event::RedrawRequested(at));
        for event in events.iter().chain(std::iter::once(&redraw)) {
            let mut inner = Vec::new();
            let mut s = Shell::new(&mut inner);
            view.as_widget_mut().update(
                tree,
                event,
                Layout::new(&node),
                self.cursor,
                &self.renderer,
                &mut clipboard::Null,
                &mut s,
                &bounds,
            );
            let relayout = s.is_layout_invalid();
            let next = s.redraw_request();
            drop(s);
            for m in inner {
                shell.publish(m);
            }
            match next {
                RedrawRequest::NextFrame => shell.request_redraw(),
                RedrawRequest::At(t) => shell.request_redraw_at(t),
                RedrawRequest::Wait => {}
            }
            if relayout {
                node = view.as_widget_mut().layout(tree, &self.renderer, &limits);
            }
        }
        let next = shell.redraw_request();
        drop(shell);
        self.renderer.reset(bounds);
        view.as_widget().draw(tree, &mut self.renderer, theme, &Default::default(), Layout::new(&node), self.cursor, &bounds);
        Frame { next, messages }
    }

    /// [`Self::frame_of`] the whole window, with the published messages
    /// sent back to the app (as Iced does before the next frame).
    pub fn frame(&mut self, app: &mut App, events: &[iced::Event]) -> RedrawRequest {
        let theme = app.iced_theme();
        let f = self.frame_of(app.view(), &theme, events, Instant::now());
        for m in f.messages {
            drop(app.update(m));
        }
        f.next
    }

    /// Frames until the artwork has arrived and every fade has settled.
    pub fn settle(&mut self, app: &mut App) {
        let mut quiet = 0;
        for _ in 0..120 {
            let _ = self.frame(app, &[]);
            std::thread::sleep(Duration::from_millis(30));
            if images::take_dirty() {
                quiet = 0;
            } else {
                quiet += 1;
                if quiet >= 15 {
                    break;
                }
            }
        }
    }

    /// Follow what the widgets ask for during `span` (as the window would,
    /// at most every `vsync`): the number of frames they made it draw.
    pub fn frames_asked(&mut self, app: &mut App, span: Duration, vsync: Duration) -> usize {
        let (a, b) = self.frames_split(app, span, vsync);
        a + b
    }

    /// [`Self::frames_asked`], split into frames asked for the next vsync
    /// (something gliding) and frames asked for a later time (the drift).
    pub fn frames_split(&mut self, app: &mut App, span: Duration, vsync: Duration) -> (usize, usize) {
        let end = Instant::now() + span;
        let mut next = self.frame(app, &[]);
        let (mut on_vsync, mut timed) = (0, 0);
        loop {
            let at = match next {
                RedrawRequest::Wait => return (on_vsync, timed),
                RedrawRequest::NextFrame => {
                    on_vsync += 1;
                    Instant::now() + vsync
                }
                RedrawRequest::At(t) => {
                    timed += 1;
                    t.max(Instant::now() + vsync)
                }
            };
            if at >= end {
                // The last request was not drawn within the span.
                match next {
                    RedrawRequest::NextFrame => on_vsync -= 1,
                    _ => timed -= 1,
                }
                return (on_vsync, timed);
            }
            std::thread::sleep(at.saturating_duration_since(Instant::now()));
            next = self.frame(app, &[]);
        }
    }
}

/// The 200 Hz monitor's frame interval.
pub(super) const VSYNC_200: Duration = Duration::from_millis(5);

/// A booted app with a signed-in library and a Home feed of real covers,
/// nothing playing, on Home (the 1.7.1 idle measurement's state).
pub(super) fn booted_app() -> App {
    assert!(
        std::env::var_os("ABYSS_CONFIG_DIR").is_some() && std::env::var_os("ABYSS_CACHE_DIR").is_some(),
        "set ABYSS_CONFIG_DIR and ABYSS_CACHE_DIR to temp folders"
    );
    motion::full_motion_for_tests();
    let (mut app, boot) = App::boot();
    drop(boot);
    app.welcome_step = None;
    app.signed_in = true;
    app.toast = None;
    let art = shots_covers();
    let cover = |i: usize| art.get(i % art.len().max(1)).cloned();
    let albums: Vec<Album> = (0..8)
        .map(|i| {
            let mut a = Album::parse(&json!({
                "id": 200 + i, "title": format!("Album {i}"), "artist": {"id": 10 + i, "name": format!("Artist {i}")},
                "releaseDate": "2020-03-14", "numberOfTracks": 9, "duration": 2400, "type": "ALBUM"
            }))
            .unwrap();
            a.cover = cover(i);
            a
        })
        .collect();
    let tracks: Vec<Track> = (0..12)
        .map(|i| {
            let album = &albums[i % 3];
            Track::parse(&json!({
                "id": 1000 + i, "title": format!("Track {i}"), "duration": 150 + i * 23, "allowStreaming": true,
                "artist": {"id": 10, "name": "Artist 0"},
                "album": {"id": album.id, "title": album.title, "cover": album.cover},
                "trackNumber": i + 1, "audioQuality": "LOSSLESS"
            }))
            .unwrap()
        })
        .collect();
    let rows = vec![
        Row { title: "Recently played".into(), items: albums.iter().cloned().map(Item::Album).collect(), kind: RowKind::Cards },
        Row { title: "More".into(), items: albums.iter().rev().cloned().map(Item::Album).collect(), kind: RowKind::Cards },
    ];
    app.home = Load::Ready(rows);
    app.my_playlists = Load::Ready(Vec::new());
    app.my_mixes = Load::Ready(Vec::new());
    app.fav_tracks = Load::Ready(tracks.clone());
    app.fav_albums = Load::Ready(albums.clone());
    for album in &albums {
        app.albums.insert(album.id, Load::Ready((album.clone(), tracks.clone())));
    }
    app.apply_sorts();
    app.page = Page::Home;
    app.page_at = Instant::now() - Duration::from_secs(2);
    app.viewport = Size::new(1366.0, 768.0);
    app
}

/// Real covers from `ABYSS_COVERS` (or the installed app's cache).
fn shots_covers() -> Vec<String> {
    let dir = std::env::var_os("ABYSS_COVERS").map(std::path::PathBuf::from).unwrap_or_else(|| crate::config::cache_dir().join("images"));
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "img")).collect())
        .unwrap_or_default();
    files.sort();
    files.into_iter().step_by(37).take(16).map(|p| format!("local:{}", p.display())).collect()
}

/// Play the first album's tracks (the engine has no output in tests: the
/// track stays "loading", which is what the bar shows on a skip too).
pub(super) fn play_something(app: &mut App) {
    drop(app.update(Message::Navigate(Page::Album(200))));
    drop(app.update(Message::PlayFrom(rows::Source::Page, 1)));
    drop(app.update(Message::Navigate(Page::Home)));
    app.page_at = Instant::now() - Duration::from_secs(2);
    app.toast = None;
}

/// The 1.7.1 idle Home (nothing playing) drew ~5.7 frames a second: the
/// featured pane, lit by its artwork (`Glass::album_light`), asked for a
/// frame every 50 ms forever. Now: while the window is in use the drift runs
/// at its slow pace (never at vsync); idle with nothing playing it comes to
/// rest within ~2 s and no widget asks for another frame; input wakes it.
#[test]
#[ignore = "requires an isolated profile and a wgpu adapter"]
fn motion_on_app_idle_home_asks_for_no_frames() {
    let _theme = theme::THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = booted_app();
    let mut screen = Screen::new(app.viewport);
    screen.settle(&mut app);
    // Nothing moves by itself: no frame subscription.
    assert!(!app.animating(), "animating at rest: now {} ambient {}", app.animating_now(), app.animating_ambient());
    // In use: the featured pane's lights drift at 20 fps, not at vsync.
    let (vsync, timed) = screen.frames_split(&mut app, Duration::from_millis(1000), VSYNC_200);
    assert_eq!(vsync, 0, "nothing should ask for vsync frames at rest");
    assert!(timed <= 25, "the drift at its slow pace: {timed} frames in 1 s");
    // Idle with nothing playing: it comes to rest, then no frames at all.
    motion::idle_for_tests();
    let settling = screen.frames_asked(&mut app, Duration::from_millis(3000), VSYNC_200);
    assert!(settling <= 60, "came to rest in {settling} frames");
    let frames = screen.frames_asked(&mut app, Duration::from_millis(1500), VSYNC_200);
    assert_eq!(frames, 0, "widgets kept asking for frames on the idle Home");
    // A pointer move wakes it: the hover glides at vsync, then only the drift.
    screen.cursor = mouse::Cursor::Available(Point::new(700.0, 400.0));
    let moved = iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::new(700.0, 400.0) });
    let _ = screen.frame(&mut app, &[moved]);
    let _ = screen.frames_asked(&mut app, Duration::from_millis(400), VSYNC_200);
    let (vsync, timed) = screen.frames_split(&mut app, Duration::from_millis(1000), VSYNC_200);
    assert_eq!(vsync, 0, "a settled hover asks for no vsync frames");
    assert!(timed > 0, "input wakes the drift again");
}

#[test]
#[ignore = "diagnostic: prints what each part of the window asks for at rest"]
fn motion_on_app_parts_at_rest() {
    let _theme = theme::THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = booted_app();
    let mut screen = Screen::new(app.viewport);
    screen.settle(&mut app);
    let theme = app.iced_theme();
    type Part = fn(&App) -> Element<'_, Message>;
    let parts: [(&str, Part); 5] = [
        ("sidebar", |a| sidebar::view(a, false)),
        ("top_bar", |a| pages::top_bar(a)),
        ("content", |a| pages::content(a)),
        ("side_panel", |a| pages::side_panel(a).map_or_else(|| iced::widget::Space::new().into(), Into::into)),
        ("player_bar", |a| player_bar::view(a)),
    ];
    for (name, part) in parts {
        let mut s = Screen::new(app.viewport);
        let mut kinds = Vec::new();
        for _ in 0..40 {
            let f = s.frame_of(part(&app), &theme, &[], Instant::now());
            kinds.push(match f.next {
                RedrawRequest::Wait => "w".to_string(),
                RedrawRequest::NextFrame => "N".to_string(),
                RedrawRequest::At(t) => format!("{}", t.saturating_duration_since(Instant::now()).as_millis()),
            });
            std::thread::sleep(Duration::from_millis(20));
        }
        println!("{name}: {}", kinds[20..].join(" "));
    }
}

/// Opening and closing the queue fast, through `update` as clicks do: every
/// turn starts from the width the panel has (no jump, no restart), the last
/// click wins within one `PANEL`, and nothing keeps drawing afterwards.
#[test]
#[ignore = "requires an isolated profile and a wgpu adapter"]
fn motion_on_app_queue_toggled_fast_reverses_and_settles() {
    let _theme = theme::THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = booted_app();
    let mut screen = Screen::new(app.viewport);
    screen.settle(&mut app);
    motion::idle_for_tests();
    let _ = screen.frames_asked(&mut app, Duration::from_millis(2500), VSYNC_200);
    let mut seen = Vec::new();
    for i in 0..12 {
        // 20..65 ms apart: faster than the panel's 195 ms.
        let gap = Duration::from_millis(20 + (i * 17 % 46));
        let until = Instant::now() + gap;
        while Instant::now() < until {
            let _ = screen.frame(&mut app, &[]);
            std::thread::sleep(VSYNC_200);
        }
        let t = Instant::now();
        let before = app.side_reveal.fraction_at(t);
        drop(app.update(Message::ToggleQueue));
        let after = app.side_reveal.fraction_at(t);
        assert!((after - before).abs() < 0.03, "toggle {i}: the panel jumped {before:.3} -> {after:.3}");
        seen.push(after);
        let _ = screen.frame(&mut app, &[]);
    }
    assert!(seen.iter().any(|f| *f > 0.05 && *f < 0.95), "the toggles caught it mid-way: {seen:?}");
    // 12 toggles: closed again. It is closed one PANEL later, and stays still.
    assert!(!app.show_queue);
    let _ = screen.frames_asked(&mut app, panel_motion::PANEL + Duration::from_millis(30), VSYNC_200);
    assert_eq!(app.side_reveal.fraction(), 0.0);
    motion::idle_for_tests();
    let _ = screen.frames_asked(&mut app, Duration::from_millis(2500), VSYNC_200);
    let (vsync, _) = screen.frames_split(&mut app, Duration::from_millis(600), VSYNC_200);
    assert_eq!(vsync, 0, "something kept gliding after the toggles");
}

/// A menu dismissed by a click elsewhere while it is still growing in fades
/// out from the opacity it had reached (1.7.1 flashed it to full first).
#[test]
#[ignore = "requires an isolated profile and a wgpu adapter"]
fn motion_on_app_menu_closed_mid_entrance_leaves_from_where_it_was() {
    let _theme = theme::THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = booted_app();
    let mut screen = Screen::new(app.viewport);
    screen.settle(&mut app);
    for attempt in 0..3 {
        app.menu = Some((Point::new(500.0, 300.0), MenuTarget::Output));
        let _ = screen.frame(&mut app, &[]);
        std::thread::sleep(Duration::from_millis(25));
        let _ = screen.frame(&mut app, &[]);
        let Some(entering) = wash::last_trace_alpha(false) else { continue };
        drop(app.update(Message::CloseMenu));
        assert!(app.menu_exit.is_some());
        let _ = screen.frame(&mut app, &[]);
        let leaving = wash::last_trace_alpha(true).expect("the ghost was drawn");
        assert!(entering < 0.98, "attempt {attempt}: the entrance was already over ({entering})");
        assert!(leaving <= entering + 0.02, "the ghost flashed: entrance at {entering:.2}, ghost at {leaving:.2}");
        // And it is gone within EXIT.
        let _ = screen.frames_asked(&mut app, motion::EXIT + Duration::from_millis(40), VSYNC_200);
        drop(app.update(Message::Tick));
        assert!(app.menu_exit.is_none());
        return;
    }
    panic!("the menu never drew its entrance");
}

impl Screen {
    /// The window's pixels as last drawn (after a [`Self::frame`]).
    pub fn pixels(&mut self) -> Vec<u8> {
        Headless::screenshot(&mut self.renderer, Size::new(self.size.width as u32, self.size.height as u32), 1.0, iced::Color::BLACK)
    }
}

/// Save the bottom `h` px of `pixels` (a window `size` wide) as `<name>.png`
/// in `ABYSS_SHOT_DIR` (or `%TEMP%\abyss-shots`).
pub(super) fn save_bottom(name: &str, size: Size, pixels: &[u8], h: u32) {
    let (w, full) = (size.width as u32, size.height as u32);
    let h = h.min(full);
    let start = ((full - h) * w * 4) as usize;
    let dir = std::env::var_os("ABYSS_SHOT_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("abyss-shots"));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{name}.png"));
    image::save_buffer(&path, &pixels[start..], w, h, image::ColorType::Rgba8).expect("write the PNG");
    println!("{}", path.display());
}

/// The player bar with a track, at the widths that matter (960 minimum,
/// 1366, 1536 = 1080p@125 %, 2048 = the 1440p@125 % main screen), in both
/// themes, at rest and with the pointer over the transport.
#[test]
#[ignore = "requires an isolated profile and a wgpu adapter; writes PNGs for review"]
fn motion_on_app_player_bar_shots() {
    let _theme = theme::THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = booted_app();
    play_something(&mut app);
    app.fav_ids.insert(1001);
    for (w, h) in [(960.0, 600.0), (1150.0, 700.0), (1366.0, 768.0), (1536.0, 864.0), (2048.0, 1152.0)] {
        let size = Size::new(w, h);
        app.viewport = size;
        for light in [false, true] {
            theme::set_light(light);
            let mut screen = Screen::new(size);
            screen.settle(&mut app);
            app.track_colors();
            app.palette.at = Instant::now() - Duration::from_secs(2);
            app.ambient.at = Instant::now() - Duration::from_secs(2);
            let _ = screen.frame(&mut app, &[]);
            let px = screen.pixels();
            let name = format!("bar-{}-{}", if light { "light" } else { "dark" }, w as u32);
            save_bottom(&name, size, &px, 96);
        }
    }
    theme::set_light(false);
}
