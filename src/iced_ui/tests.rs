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
