use super::*;
use iced::advanced::{
    layout,
    renderer::{Headless, Renderer as _},
    widget::{Id, Operation, Tree},
    Layout,
};
use iced::{Color, Pixels, Rectangle, Renderer, Theme};
use serde_json::json;

fn layout_of(element: &mut Element<'_, Message>, renderer: &Renderer, size: Size) -> (Tree, layout::Node) {
    let mut tree = Tree::new(element.as_widget());
    let node = element.as_widget_mut().layout(&mut tree, renderer, &layout::Limits::new(size, size));
    (tree, node)
}

#[derive(Default)]
struct SearchField(Option<Rectangle>);

#[test]
fn sort_menu_opens_below_the_button_or_above_when_needed() {
    let bounds = Rectangle { x: 180.0, y: 110.0, width: 84.0, height: 32.0 };
    assert_eq!(menus::sort_menu_origin(bounds, 600.0, 6), Point::new(180.0, 146.0));
    let near_bottom = Rectangle { y: 510.0, ..bounds };
    let origin = menus::sort_menu_origin(near_bottom, 600.0, 6);
    assert_eq!(origin, Point::new(180.0, 286.0));
}

impl Operation for SearchField {
    fn traverse(&mut self, children: &mut dyn FnMut(&mut dyn Operation)) {
        children(self);
    }

    fn text_input(&mut self, id: Option<&Id>, bounds: Rectangle, _: &mut dyn iced::advanced::widget::operation::TextInput) {
        if id == Some(&Id::new(widgets::SEARCH_ID)) {
            self.0 = Some(bounds);
        }
    }
}

#[test]
#[ignore = "requires an isolated profile and a wgpu adapter; runs offscreen"]
fn signed_in_catalogue_layouts_and_filtered_queue() {
    assert!(std::env::var_os("ABYSS_CONFIG_DIR").is_some() && std::env::var_os("ABYSS_CACHE_DIR").is_some());
    let mut renderer =
        iced::futures::executor::block_on(<Renderer as Headless>::new(INTER, Pixels(16.0), None)).expect("offscreen wgpu adapter");
    let (mut app, boot) = App::boot();
    drop(boot);
    app.welcome_step = None;
    app.signed_in = true;
    app.shuffle = false;
    app.page_at = Instant::now() - Duration::from_secs(1);
    let artist = Artist::parse(&json!({"id":1, "name":"Artista de prueba"})).unwrap();
    let album = Album::parse(&json!({"id":2, "title":"Álbum de prueba", "artist":{"id":1,"name":artist.name}})).unwrap();
    let mut card = widgets::grid_card(&app, &Item::Album(album.clone()), 112.0);
    let mut card_tree = Tree::new(card.as_widget());
    let card_node = card.as_widget_mut().layout(&mut card_tree, &renderer, &layout::Limits::new(Size::ZERO, Size::new(112.0, 200.0)));
    let card_layout = Layout::new(&card_node);
    let viewport = card_layout.bounds();
    let draw_corner = |renderer: &mut Renderer, cursor| {
        renderer.reset(viewport);
        card.as_widget().draw(&card_tree, renderer, &Theme::Dark, &Default::default(), card_layout, cursor, &viewport);
        let size = Size::new(viewport.width.ceil() as u32, viewport.height.ceil() as u32);
        let pixels = Headless::screenshot(renderer, size, 1.0, Color::BLACK);
        pixels[((87 * size.width + 75) * 4) as usize]
    };
    let idle = draw_corner(&mut renderer, mouse::Cursor::Unavailable);
    let shown = draw_corner(&mut renderer, mouse::Cursor::Available(Point::new(35.0, 40.0)));
    let left = draw_corner(&mut renderer, mouse::Cursor::Available(Point::new(120.0, 40.0)));
    assert!(shown > idle.saturating_add(80), "the play button should appear over the cover");
    assert_eq!(idle, left, "the play button must not be drawn outside the card");
    for (position, play) in [(Point::new(12.0, 12.0), false), (Point::new(88.0, 88.0), true)] {
        let mut messages = Vec::new();
        let mut clipboard = iced::advanced::clipboard::Null;
        for (event, pressed) in
            [(mouse::Event::ButtonPressed(mouse::Button::Left), true), (mouse::Event::ButtonReleased(mouse::Button::Left), false)]
        {
            card.as_widget_mut().update(
                &mut card_tree,
                &iced::Event::Mouse(event),
                card_layout,
                mouse::Cursor::Available(position),
                &renderer,
                &mut clipboard,
                &mut iced::advanced::Shell::new(&mut messages),
                &card_layout.bounds(),
            );
            if pressed {
                assert!(messages.is_empty(), "hold the cover before navigating");
            }
        }
        if play {
            assert!(matches!(messages.as_slice(), [Message::PlayItem(Item::Album(_))]));
        } else {
            assert!(matches!(messages.as_slice(), [Message::OpenItem(Item::Album(_))]));
        }
    }
    let mut messages = Vec::new();
    let mut clipboard = iced::advanced::clipboard::Null;
    for (event, cursor) in [
        (mouse::Event::ButtonPressed(mouse::Button::Left), mouse::Cursor::Available(Point::new(88.0, 88.0))),
        (mouse::Event::ButtonReleased(mouse::Button::Left), mouse::Cursor::Unavailable),
    ] {
        card.as_widget_mut().update(
            &mut card_tree,
            &iced::Event::Mouse(event),
            card_layout,
            cursor,
            &renderer,
            &mut clipboard,
            &mut iced::advanced::Shell::new(&mut messages),
            &viewport,
        );
    }
    assert!(messages.is_empty(), "leaving the card cancels a click on Play");
    let tracks: Vec<_> = ["Primera pista", "Pista filtrada", "Última pista"]
        .iter()
        .enumerate()
        .map(|(i, title)| {
            Track::parse(&json!({
                "id":100+i,"title":title,"duration":240,"allowStreaming":true,
                "artist":{"id":1,"name":artist.name},"album":{"id":2,"title":album.title},"trackNumber":i+1
            }))
            .unwrap()
        })
        .collect();
    let playlist = Playlist::parse(&json!({"uuid":"test-list", "title":"Lista de prueba", "type":"USER", "numberOfTracks":3})).unwrap();
    let mix = Mix::parse(&json!({"id":"test-mix", "title":"Mix de prueba"})).unwrap();
    let rows = vec![Row {
        title: "Catálogo".into(),
        items: vec![Item::Album(album.clone()), Item::Playlist(playlist.clone()), Item::Mix(mix.clone())],
        kind: RowKind::Cards,
    }];
    app.home = Load::Ready(rows.clone());
    app.explore = Load::Ready(rows);
    app.my_playlists = Load::Ready(vec![playlist.clone()]);
    app.my_mixes = Load::Ready(vec![mix.clone()]);
    app.fav_tracks = Load::Ready(tracks.clone());
    app.fav_albums = Load::Ready(vec![album.clone()]);
    app.fav_artists = Load::Ready(vec![artist.clone()]);
    app.albums.insert(album.id, Load::Ready((album.clone(), tracks.clone())));
    app.artists.insert(
        artist.id,
        Load::Ready(ArtistPage {
            artist: artist.clone(),
            top_tracks: tracks.clone(),
            albums: vec![album.clone()],
            singles: Vec::new(),
            appears_on: Vec::new(),
            similar: Vec::new(),
            bio: None,
        }),
    );
    app.playlists.insert(playlist.uuid.clone(), Load::Ready((playlist.clone(), tracks.clone())));
    app.mixes.insert(mix.id.clone(), Load::Ready((mix.clone(), tracks.clone())));
    app.search.insert(
        "pista".into(),
        Load::Ready(SearchResults {
            top: Some(Item::Track(tracks[0].clone())),
            tracks: tracks.clone(),
            albums: vec![album.clone()],
            artists: vec![artist.clone()],
            playlists: vec![playlist.clone()],
        }),
    );
    app.apply_sorts();
    drop(app.update(Message::Navigate(Page::MyTracks)));
    drop(app.update(Message::Filter("filtrada".into())));
    assert_eq!(app.lib_shown, [1]);
    drop(app.update(Message::PlayFrom(rows::Source::Page, app.lib_shown[0])));
    assert_eq!(app.queue.iter().map(|entry| entry.track.id).collect::<Vec<_>>(), [100, 101, 102]);
    assert_eq!(app.now.as_ref().map(|now| now.track.id), Some(101));
    drop(app.update(Message::ToggleQueue));
    let panel_width = |app: &App| {
        let mut panel = pages::side_panel(app).expect("queue panel");
        let mut tree = Tree::new(panel.as_widget());
        panel.as_widget_mut().layout(&mut tree, &renderer, &layout::Limits::new(Size::ZERO, Size::new(800.0, 600.0))).size().width
    };
    let immediately = panel_width(&app);
    std::thread::sleep(panel_motion::PANEL);
    let open = panel_width(&app);
    if !crate::shared::lyrics::reduced_motion() {
        assert!(immediately < open, "queue width: {immediately} → {open}");
    }
    assert!(open >= 240.0);

    let pages = [
        Page::Home,
        Page::Explore,
        Page::MyTracks,
        Page::MyAlbums,
        Page::MyArtists,
        Page::MyPlaylists,
        Page::MyMixes,
        Page::Search("pista".into()),
        Page::Album(album.id),
        Page::Artist(artist.id),
        Page::Playlist(playlist.uuid),
        Page::Mix(mix.id),
        Page::Replay,
        Page::Recent,
        Page::Settings,
    ];
    for size in [Size::new(480.0, 340.0), Size::new(768.0, 960.0), Size::new(1440.0, 810.0), Size::new(1920.0, 820.0)] {
        app.viewport = size;
        let frame = Rectangle::with_size(size);
        for page in &pages {
            app.search_open = false;
            drop(app.update(Message::Navigate(page.clone())));
            app.page_at = Instant::now() - Duration::from_secs(1);
            let mut view = app.view();
            let (mut tree, node) = layout_of(&mut view, &renderer, size);
            assert_eq!(node.size(), size);
            let mut search = SearchField::default();
            view.as_widget_mut().operate(&mut tree, Layout::new(&node), &renderer, &mut search);
            let (x, width) = pages::search_anchor(size.width);
            let bounds = search.0.expect("search is available on every signed-in page");
            assert!((x - bounds.x).abs() < 1.0 && (width - bounds.width).abs() < 1.0, "{page:?} at {size:?}: {bounds:?}");
            renderer.reset(frame);
            view.as_widget().draw(
                &tree,
                &mut renderer,
                &Theme::Dark,
                &Default::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &frame,
            );
            if size.width == 1440.0 {
                let pixels = Headless::screenshot(&mut renderer, Size::new(size.width as u32, size.height as u32), 1.0, Color::BLACK);
                assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[0] > 80), "{page:?}");
            }
        }
        app.search_open = true;
        app.search_query = "pista".into();
        let mut view = app.view();
        let (tree, node) = layout_of(&mut view, &renderer, size);
        renderer.reset(frame);
        view.as_widget().draw(
            &tree,
            &mut renderer,
            &Theme::Dark,
            &Default::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &frame,
        );
        let pixels = Headless::screenshot(&mut renderer, Size::new(size.width as u32, size.height as u32), 1.0, Color::BLACK);
        assert_eq!(pixels.len(), (size.width * size.height * 4.0) as usize);
        assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[0] > 80));
    }

    drop(app.update(Message::CloseSidePanel));
    std::thread::sleep(panel_motion::PANEL);
    assert!(pages::side_panel(&app).is_none());

    app.viewport = Size::new(1440.0, 810.0);
    app.search_open = false;
    app.page = Page::Settings;
    let draw_panel = |app: &App, renderer: &mut Renderer| {
        let viewport = Rectangle::with_size(app.viewport);
        let mut view = app.view();
        let (tree, node) = layout_of(&mut view, renderer, app.viewport);
        renderer.reset(viewport);
        view.as_widget().draw(
            &tree,
            renderer,
            &Theme::Dark,
            &Default::default(),
            Layout::new(&node),
            mouse::Cursor::Available(Point::new(400.0, 300.0)),
            &viewport,
        );
    };
    for tab in 0..=settings::EQ_TAB {
        app.settings_tab = tab;
        draw_panel(&app, &mut renderer);
    }
    app.now_playing_open = true;
    app.similar.insert(101, Load::Ready(tracks.clone()));
    for tab in [NpTab::Queue, NpTab::Similar, NpTab::Credits, NpTab::Lyrics] {
        app.np_tab = tab;
        draw_panel(&app, &mut renderer);
    }
    app.now_playing_open = false;

    let mut viewed = album.clone();
    viewed.cover = Some("local:viewed.jpg".into());
    app.now.as_mut().unwrap().track.album.as_mut().unwrap().cover = Some("local:playing.jpg".into());
    app.albums.insert(viewed.id, Load::Ready((viewed.clone(), tracks)));
    app.page = Page::Album(viewed.id);
    assert_eq!(liquid_glass::scene_url(&app), viewed.cover(320));
    app.page = Page::Album(0);
    assert_eq!(liquid_glass::scene_url(&app), None);
    app.page = Page::MyAlbums;
    app.settings.lock().lib_grid.insert("albums".into(), true);
    app.lib_albums = vec![viewed.clone()];
    app.lib_shown = vec![0];
    app.expanded_album = Some(viewed.id);
    assert_eq!(liquid_glass::scene_url(&app), viewed.cover(320));
    app.lib_shown.clear();
    assert_eq!(liquid_glass::scene_url(&app), None);
    app.now_playing_open = true;
    assert_eq!(liquid_glass::scene_url(&app).as_deref(), Some("local:playing.jpg"));
    app.now_playing_open = false;

    app.page = Page::MyTracks;
    app.fav_tracks = Load::Failed("fixture request failed".into());
    drop(app.update(Message::RetryPage));
    assert!(matches!(app.fav_tracks, Load::Loading));
    app.reset_data();
    assert!(app.lib_tracks.is_empty() && app.lib_shown.is_empty());
    assert!(!app.search_open && app.search_query.is_empty());
}

// ------------------------------------------------------------ interaction harness

/// One batch of events through the whole window (the view built anew and its
/// tree diffed, as Iced does after every message): the messages the widgets
/// published and the redraw they asked for.
pub(super) fn feed(
    app: &App,
    renderer: &Renderer,
    tree: &mut Option<Tree>,
    events: &[(iced::Event, mouse::Cursor)],
) -> (Vec<Message>, iced::window::RedrawRequest) {
    let size = app.viewport;
    let mut view = app.view();
    let tree = match tree {
        Some(tree) => {
            tree.diff(view.as_widget());
            tree
        }
        None => tree.insert(Tree::new(view.as_widget())),
    };
    let node = view.as_widget_mut().layout(tree, renderer, &layout::Limits::new(size, size));
    let frame = Rectangle::with_size(size);
    let mut messages = Vec::new();
    let mut redraw = iced::window::RedrawRequest::Wait;
    for (event, cursor) in events {
        let mut shell = iced::advanced::Shell::new(&mut messages);
        view.as_widget_mut().update(
            tree,
            event,
            Layout::new(&node),
            *cursor,
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &frame,
        );
        redraw = redraw.min(shell.redraw_request());
    }
    (messages, redraw)
}

/// A left click at `at`: pointer moved there, pressed, released.
pub(super) fn click(app: &App, renderer: &Renderer, at: Point) -> Vec<Message> {
    let mut tree = None;
    let cursor = mouse::Cursor::Available(at);
    let redraw = iced::Event::Window(iced::window::Event::RedrawRequested(Instant::now()));
    let _ = feed(app, renderer, &mut tree, &[(redraw.clone(), cursor)]);
    let (messages, _) = feed(
        app,
        renderer,
        &mut tree,
        &[
            (iced::Event::Mouse(mouse::Event::CursorMoved { position: at }), cursor),
            (redraw, cursor),
            (iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), cursor),
            (iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)), cursor),
        ],
    );
    messages
}

pub(super) fn headless() -> Renderer {
    iced::futures::executor::block_on(<Renderer as Headless>::new(INTER, Pixels(16.0), None)).expect("offscreen wgpu adapter")
}

/// A booted, signed-in app playing the first of three tracks.
pub(super) fn playing_app() -> (App, Vec<Track>, Album) {
    assert!(std::env::var_os("ABYSS_CONFIG_DIR").is_some() && std::env::var_os("ABYSS_CACHE_DIR").is_some());
    let (mut app, boot) = App::boot();
    drop(boot);
    app.welcome_step = None;
    app.signed_in = true;
    app.shuffle = false;
    let album = Album::parse(&json!({"id":2, "title":"Álbum de prueba", "artist":{"id":1,"name":"Artista"}})).unwrap();
    let tracks: Vec<Track> = (0..3)
        .map(|i| {
            Track::parse(&json!({
                "id":100+i,"title":format!("Pista {i}"),"duration":240,"allowStreaming":true,
                "artist":{"id":1,"name":"Artista"},"album":{"id":2,"title":album.title},"trackNumber":i+1
            }))
            .unwrap()
        })
        .collect();
    app.fav_albums = Load::Ready(vec![album.clone()]);
    app.albums.insert(album.id, Load::Ready((album.clone(), tracks.clone())));
    app.apply_sorts();
    drop(app.update(Message::PlayTracks(Arc::new(tracks.clone()), 0)));
    app.toast = None;
    (app, tracks, album)
}

/// Reported on 1.7.1: the side panel's close button did not close the lyrics.
#[test]
#[ignore = "requires an isolated profile and a wgpu adapter; runs offscreen"]
fn the_side_panel_close_button_closes_the_lyrics() {
    let renderer = headless();
    let (mut app, tracks, album) = playing_app();
    app.viewport = Size::new(1440.0, 900.0);
    app.lyrics.insert(
        tracks[0].id,
        Load::Ready(crate::tidal::Lyrics {
            text: String::new(),
            synced: (0..24).map(|i| (f64::from(i) * 4.0, format!("Line {i}"))).collect(),
            provider: "LRCLIB".into(),
            roman: Vec::new(),
        }),
    );
    for page in [Page::MyAlbums, Page::Album(album.id)] {
        drop(app.update(Message::Navigate(page.clone())));
        app.page_at = Instant::now() - Duration::from_secs(2);
        drop(app.update(Message::ToggleLyrics));
        assert!(app.show_lyrics);
        std::thread::sleep(panel_motion::PANEL + Duration::from_millis(50));
        // The header row of the panel: scan its right end for what a click does.
        let mut found = Vec::new();
        let right = app.viewport.width;
        let mut y = 40.0;
        while y < 160.0 {
            let mut x = right - 120.0;
            while x < right {
                let messages = click(&app, &renderer, Point::new(x, y));
                for m in messages {
                    found.push((x, y, format!("{m:?}")));
                }
                x += 4.0;
            }
            y += 4.0;
        }
        let closes: Vec<_> = found.iter().filter(|(_, _, m)| m.starts_with("CloseSidePanel")).collect();
        let others: std::collections::BTreeSet<_> = found.iter().map(|(_, _, m)| m.chars().take(40).collect::<String>()).collect();
        println!("{page:?}: {} close hits, messages seen: {others:?}", closes.len());
        assert!(!closes.is_empty(), "{page:?}: no click on the header reaches the close button; seen {others:?}");
        let (x, y, _) = closes[closes.len() / 2];
        for m in click(&app, &renderer, Point::new(*x, *y)) {
            drop(app.update(m));
        }
        assert!(!app.show_lyrics && !app.show_queue, "{page:?}: the panel is still open after the close click");
        std::thread::sleep(panel_motion::PANEL + Duration::from_millis(50));
    }
}

/// Real covers (`ABYSS_COVERS`, or the profile's cache) as `local:` URLs.
pub(super) fn some_covers(n: usize) -> Vec<String> {
    let dir = std::env::var_os("ABYSS_COVERS").map(std::path::PathBuf::from).unwrap_or_else(|| crate::config::cache_dir().join("images"));
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "img")).collect())
        .unwrap_or_default();
    files.sort();
    files.into_iter().step_by(23).take(n).map(|p| format!("local:{}", p.display())).collect()
}

/// Draw frames as the window would, for `for_ms`: a frame whenever the
/// earliest redraw asked for is due. Returns the frames drawn and what each
/// named part of the window asked for (`perf::take_asks`).
pub(super) fn run_frames(app: &App, renderer: &Renderer, tree: &mut Option<Tree>, for_ms: u64) -> (u32, Vec<(&'static str, u32, f32)>) {
    let _ = perf::take_asks();
    let end = Instant::now() + Duration::from_millis(for_ms);
    let mut frames = 0;
    let mut next = Some(Instant::now());
    while let Some(at) = next.filter(|at| *at < end) {
        std::thread::sleep(at.saturating_duration_since(Instant::now()));
        let now = Instant::now();
        let (_, redraw) =
            feed(app, renderer, tree, &[(iced::Event::Window(iced::window::Event::RedrawRequested(now)), mouse::Cursor::Unavailable)]);
        frames += 1;
        next = match redraw {
            iced::window::RedrawRequest::NextFrame => Some(now + Duration::from_millis(5)),
            iced::window::RedrawRequest::At(at) => Some(at.max(now + Duration::from_millis(5))),
            iced::window::RedrawRequest::Wait => None,
        };
    }
    (frames, perf::take_asks())
}

/// Who keeps the window drawing when nothing is going on: Home at rest
/// (covers loaded), nothing playing; then a paused track; then a closed menu
/// after its exit; then playing. Prints the frames per second and their askers.
#[test]
#[ignore = "requires an isolated profile and a wgpu adapter; runs offscreen"]
fn idle_frames_and_who_asks_for_them() {
    perf::force(true);
    let renderer = headless();
    // One tree for the whole run, as the window keeps it (a new one restarts every entrance).
    let mut tree = None;
    let (mut app, mut tracks, album) = playing_app();
    app.viewport = Size::new(1440.0, 900.0);
    let art = some_covers(8);
    let mut albums = Vec::new();
    for (i, url) in art.iter().enumerate() {
        let mut a = album.clone();
        a.id = 300 + i as u64;
        a.cover = Some(url.clone());
        albums.push(a);
    }
    if let Some(url) = art.first() {
        for t in &mut tracks {
            t.album.as_mut().unwrap().cover = Some(url.clone());
        }
    }
    app.home = Load::Ready(vec![Row {
        title: "Recently played".into(),
        items: albums.iter().cloned().map(Item::Album).collect(),
        kind: RowKind::Cards,
    }]);
    // Nothing playing: the queue is empty, no track.
    app.now = None;
    app.queue.clear();
    app.player.send(crate::audio::Cmd::Stop);
    drop(app.update(Message::Navigate(Page::Home)));
    app.page_at = Instant::now() - Duration::from_secs(2);
    // Let the covers and their colours arrive.
    let _ = run_frames(&app, &renderer, &mut tree, 1500);
    let report = |label: &str, (frames, asks): (u32, Vec<(&'static str, u32, f32)>), secs: f32| {
        println!("{label}: {:.1} frames/s; asked by {asks:?}", frames as f32 / secs);
        frames as f32 / secs
    };
    let home_idle = report("home, nothing playing", run_frames(&app, &renderer, &mut tree, 3000), 3.0);

    // A track paused.
    drop(app.update(Message::PlayTracks(Arc::new(tracks.clone()), 0)));
    drop(app.update(Message::TogglePlay));
    app.snap.paused = true;
    app.toast = None;
    let _ = run_frames(&app, &renderer, &mut tree, 1500);
    let paused = report("home, paused", run_frames(&app, &renderer, &mut tree, 3000), 3.0);

    // A menu dismissed by a click elsewhere, long gone.
    drop(app.update(Message::OpenMenu(MenuTarget::Output)));
    drop(app.update(Message::CloseMenu));
    std::thread::sleep(Duration::from_millis(400));
    let after_menu = report("home, paused, after a menu", run_frames(&app, &renderer, &mut tree, 2000), 2.0);

    // Album page, paused.
    app.albums.insert(albums[0].id, Load::Ready((albums[0].clone(), tracks.clone())));
    drop(app.update(Message::Navigate(Page::Album(albums[0].id))));
    app.page_at = Instant::now() - Duration::from_secs(2);
    let _ = run_frames(&app, &renderer, &mut tree, 1500);
    let album_paused = report("album, paused", run_frames(&app, &renderer, &mut tree, 3000), 3.0);

    // Settings, paused: nothing on it moves.
    drop(app.update(Message::Navigate(Page::Settings)));
    app.page_at = Instant::now() - Duration::from_secs(2);
    let _ = run_frames(&app, &renderer, &mut tree, 1000);
    let settings = report("settings, paused", run_frames(&app, &renderer, &mut tree, 2000), 2.0);
    // Nobody there: the ambient motion should hold still (once the ambient
    // widgets follow `shared::activity::at_rest`, see team\requests.md).
    drop(app.update(Message::Navigate(Page::Home)));
    app.page_at = Instant::now() - Duration::from_secs(2);
    let _ = run_frames(&app, &renderer, &mut tree, 1000);
    crate::shared::activity::input_ago(crate::shared::activity::REST_AFTER + Duration::from_secs(1));
    assert!(crate::shared::activity::at_rest());
    let resting = report("home, paused, nobody there", run_frames(&app, &renderer, &mut tree, 2000), 2.0);
    crate::shared::activity::input();
    println!(
        "summary: home {home_idle:.1}, paused {paused:.1}, menu {after_menu:.1}, album {album_paused:.1}, settings {settings:.1}, resting {resting:.1}"
    );
    // Nothing moves on Settings with a paused track: no frames after the first.
    assert!(settings < 1.0, "settings, paused: {settings} frames/s");
    perf::force(false);
}

/// The redraws one part of the window asks for, frame after frame (diagnostics).
pub(super) fn part_requests<'a>(
    build: &dyn Fn() -> Element<'a, Message>,
    renderer: &Renderer,
    size: Size,
    frames: usize,
) -> Vec<Option<f32>> {
    let mut tree: Option<Tree> = None;
    let mut out = Vec::new();
    for _ in 0..frames {
        let mut view = build();
        let tree = match &mut tree {
            Some(tree) => {
                tree.diff(view.as_widget());
                tree
            }
            None => tree.insert(Tree::new(view.as_widget())),
        };
        let node = view.as_widget_mut().layout(tree, renderer, &layout::Limits::new(Size::ZERO, size));
        let now = Instant::now();
        let mut messages = Vec::new();
        let mut shell = iced::advanced::Shell::new(&mut messages);
        view.as_widget_mut().update(
            tree,
            &iced::Event::Window(iced::window::Event::RedrawRequested(now)),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &Rectangle::with_size(size),
        );
        out.push(match shell.redraw_request() {
            iced::window::RedrawRequest::NextFrame => Some(0.0),
            iced::window::RedrawRequest::At(at) => Some(at.saturating_duration_since(now).as_secs_f32() * 1000.0),
            iced::window::RedrawRequest::Wait => None,
        });
        std::thread::sleep(Duration::from_millis(30));
    }
    out
}

#[test]
#[ignore = "requires an isolated profile and a wgpu adapter; runs offscreen"]
fn paused_player_bar_requests() {
    let renderer = headless();
    let (mut app, _, _) = playing_app();
    app.viewport = Size::new(1440.0, 900.0);
    drop(app.update(Message::TogglePlay));
    app.snap.paused = true;
    app.toast = None;
    std::thread::sleep(Duration::from_millis(500));
    let size = Size::new(1428.0, theme::PLAYER_H);
    println!("bar: {:?}", part_requests(&|| player_bar::view(&app), &renderer, size, 40));
    println!("controls: {:?}", part_requests(&|| player_bar::controls(&app, false, 900.0), &renderer, size, 20));
}

/// The tick an idle app runs at, and what keeps it faster.
#[test]
#[ignore = "requires an isolated profile (ABYSS_CONFIG_DIR, ABYSS_CACHE_DIR)"]
fn the_tick_slows_down_when_nothing_waits_on_it() {
    let (mut app, tracks, _) = playing_app();
    app.snap.paused = false;
    app.ended = false;
    assert_eq!(app.poll_ms(), POLL_ACTIVE_MS, "playing");
    app.now = None;
    app.queue.clear();
    app.queue_restoring = false;
    app.toast = None;
    app.active_at = Instant::now() - Duration::from_secs(10);
    assert_eq!(app.poll_ms(), POLL_IDLE_MS, "idle");
    app.notify_toast("hola");
    assert_eq!(app.poll_ms(), POLL_WAITING_MS, "a toast to hide");
    app.toast = None;
    app.local_busy = true;
    assert_eq!(app.poll_ms(), POLL_WAITING_MS, "an import");
    app.local_busy = false;
    // Paused: nothing moves, nothing to follow.
    drop(app.update(Message::PlayTracks(Arc::new(tracks), 0)));
    app.toast = None;
    app.snap.paused = true;
    app.snap.buffering = false;
    app.active_at = Instant::now() - Duration::from_secs(10);
    assert_eq!(app.poll_ms(), POLL_IDLE_MS, "paused");
    // An input: quick for a moment.
    drop(app.update(Message::SettingsTab(1)));
    assert_eq!(app.poll_ms(), POLL_ACTIVE_MS);
}

/// Hundreds of pages visited, searches typed and tracks played: the caches and
/// the history stay bounded, and Back still finds its pages with their data.
#[test]
#[ignore = "requires an isolated profile (ABYSS_CONFIG_DIR, ABYSS_CACHE_DIR)"]
fn caches_and_history_stay_bounded_after_much_browsing() {
    let (mut app, tracks, album) = playing_app();
    for round in 0..3u64 {
        for i in 0..200u64 {
            let id = 10_000 + round * 1000 + i;
            drop(app.update(Message::Navigate(Page::Album(id))));
            // What a finished load leaves behind.
            app.albums.insert(id, Load::Ready((album.clone(), tracks.clone())));
            app.album_extra.insert(id, Load::Ready((Vec::new(), Vec::new())));
            drop(app.update(Message::Navigate(Page::Artist(id))));
            drop(app.update(Message::Navigate(Page::Mix(format!("mix-{id}")))));
            drop(app.update(Message::SearchInput(format!("query {id}"))));
            app.search.insert(format!("query {id}"), Load::Ready(SearchResults::default()));
            app.lyrics.insert(id, Load::Failed("none".into()));
            app.credits.insert(id, Load::Ready(Vec::new()));
        }
        drop(app.update(Message::Tick));
    }
    let maps = [
        ("albums", app.albums.len()),
        ("album_extra", app.album_extra.len()),
        ("artists", app.artists.len()),
        ("mixes", app.mixes.len()),
        ("search", app.search.len()),
        ("lyrics", app.lyrics.len()),
        ("credits", app.credits.len()),
    ];
    println!("after 1800 pages: {maps:?}, back {}, scroll memory {}", app.back.len(), app.scroll_memory.len());
    // Kept: what the history can reach (HISTORY_CAP pages) plus the cap's slack.
    for (name, n) in maps {
        assert!(n <= HISTORY_CAP + CACHE_CAP, "{name}: {n} entries");
    }
    assert!(app.back.len() <= HISTORY_CAP && app.scroll_memory.len() <= HISTORY_CAP);
    // Back: the pages are there with their data.
    // (Each round visited an album, its artist, a mix: two steps back is the last album.)
    drop(app.update(Message::Back));
    drop(app.update(Message::Back));
    let Page::Album(id) = app.page.clone() else { panic!("two steps back is an album: {:?}", app.page) };
    assert!(matches!(app.albums.get(&id), Some(Load::Ready(_))), "the album a Back reaches keeps its data");
    // The playing track's lyrics are kept.
    let playing = app.now.as_ref().unwrap().track.id;
    app.lyrics.insert(playing, Load::Failed("none".into()));
    for i in 0..100 {
        app.lyrics.insert(50_000 + i, Load::Failed("none".into()));
    }
    app.trim_caches();
    assert!(app.lyrics.contains_key(&playing));
}

/// Back and Forward put a page where it was scrolled; a new visit starts at the top.
#[test]
#[ignore = "requires an isolated profile (ABYSS_CONFIG_DIR, ABYSS_CACHE_DIR)"]
fn back_and_forward_restore_where_the_page_was_scrolled() {
    let (mut app, _, album) = playing_app();
    drop(app.update(Message::Navigate(Page::Album(album.id))));
    drop(app.update(Message::PageScrolled(640.0, 700.0)));
    drop(app.update(Message::Navigate(Page::MyTracks)));
    assert_eq!(app.page_scroll, 0.0, "a new page starts at the top");
    drop(app.update(Message::PageScrolled(120.0, 700.0)));
    drop(app.update(Message::Back));
    assert_eq!(app.page, Page::Album(album.id));
    assert_eq!(app.page_scroll, 640.0, "Back returns to where the album was");
    drop(app.update(Message::Forward));
    assert_eq!(app.page, Page::MyTracks);
    assert_eq!(app.page_scroll, 120.0, "and Forward to where the list was");
    // Visiting the album anew (not through the history) starts at its top.
    drop(app.update(Message::Navigate(Page::Album(album.id))));
    assert_eq!(app.page_scroll, 0.0);
    // The mouse's back button goes the same way.
    drop(app.update(Message::MouseBack));
    assert_eq!((app.page.clone(), app.page_scroll), (Page::MyTracks, 120.0));
}

/// Alt+Tab while dragging: the release never arrives, so losing the focus lets go.
#[test]
#[ignore = "requires an isolated profile (ABYSS_CONFIG_DIR, ABYSS_CACHE_DIR)"]
fn losing_the_focus_lets_go_of_drags_and_ctrl() {
    let (mut app, _, _) = playing_app();
    app.snap.duration = 240.0;
    drop(app.update(Message::Modifiers(keyboard::Modifiers::CTRL)));
    assert!(app.ctrl_held);
    drop(app.update(Message::ProgressDrag(0.5)));
    assert_eq!(app.drag, Some(Drag::Progress));
    drop(app.update(Message::Unfocused));
    assert!(!app.ctrl_held, "Ctrl is not stuck (the wheel would zoom)");
    assert_eq!(app.drag, None, "the progress is not frozen");
    assert!((app.snap.position - 120.0).abs() < 0.5, "the drag ends where it was: {}", app.snap.position);
    drop(app.update(Message::VolumeDrag(0.3)));
    drop(app.update(Message::Unfocused));
    assert_eq!(app.drag, None);
    app.window = Some(iced::window::Id::unique());
    app.side_resize = Some((900.0, 320.0));
    drop(app.update(Message::Unfocused));
    assert!(app.side_resize.is_none(), "the side panel edge is let go");
    drop(app.update(Message::Tick));
}

/// Skipping through a queue fast: the last press wins, nothing stale plays.
#[test]
#[ignore = "requires an isolated profile (ABYSS_CONFIG_DIR, ABYSS_CACHE_DIR)"]
fn skipping_fast_through_the_queue_lands_on_the_last_press() {
    let (mut app, _, album) = playing_app();
    let many: Vec<Track> = (0..60u64)
        .map(|i| {
            Track::parse(&json!({"id": 700 + i, "title": format!("T{i}"), "duration": 200, "allowStreaming": true,
                "artist": {"id": 1, "name": "A"}, "album": {"id": album.id, "title": album.title}}))
            .unwrap()
        })
        .collect();
    drop(app.update(Message::PlayTracks(Arc::new(many), 0)));
    let mut keys = Vec::new();
    for _ in 0..40 {
        drop(app.update(Message::Next));
        keys.push(app.now.as_ref().unwrap().key);
    }
    let now = app.now.as_ref().unwrap();
    assert_eq!(now.index, 40);
    assert_eq!(now.track.id, 740);
    assert!(keys.windows(2).all(|w| w[1] > w[0]), "every skip is a new play (stale stream answers are dropped by key)");
    for _ in 0..5 {
        drop(app.update(Message::Prev));
    }
    assert_eq!(app.now.as_ref().unwrap().index, 35);
    // Removing a queued track before the playing one keeps it playing.
    let first = app.queue[0].uid;
    drop(app.update(Message::RemoveQueued(first)));
    assert_eq!(app.now.as_ref().unwrap().index, 34);
    assert_eq!(app.queue[34].track.id, app.now.as_ref().unwrap().track.id);
    // The playing track cannot be removed from under itself.
    let playing = app.queue[34].uid;
    drop(app.update(Message::RemoveQueued(playing)));
    assert_eq!(app.queue[34].uid, playing);
}
