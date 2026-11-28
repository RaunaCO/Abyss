//! Offscreen renders of every page, in both themes, for judging the design by
//! eye: `cargo test --release --bin Abyss -- --include-ignored page_shots`
//! writes `%TEMP%\abyss-shots\<theme>-<page>.png`.

use super::super::*;
use iced::advanced::{
    layout,
    renderer::{Headless, Renderer as _},
    widget::Tree,
    Layout,
};
use iced::{Color, Pixels, Rectangle, Renderer};
use serde_json::json;

/// Real covers from a cache folder (`ABYSS_COVERS`, or the installed app's).
pub(super) fn covers() -> Vec<String> {
    let dir = std::env::var_os("ABYSS_COVERS").map(std::path::PathBuf::from).unwrap_or_else(|| crate::config::cache_dir().join("images"));
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "img")).collect())
        .unwrap_or_default();
    files.sort();
    files.into_iter().step_by(37).take(16).map(|p| format!("local:{}", p.display())).collect()
}

/// One frame: the view diffed into the tree kept between frames (so fades
/// and menus animate in as they do on screen), a redraw event, then drawn.
pub(super) fn draw(app: &App, renderer: &mut Renderer, tree: &mut Option<Tree>, size: Size) -> Vec<u8> {
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
    view.as_widget_mut().update(
        tree,
        &iced::Event::Window(window::Event::RedrawRequested(Instant::now())),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        renderer,
        &mut iced::advanced::clipboard::Null,
        &mut iced::advanced::Shell::new(&mut messages),
        &frame,
    );
    renderer.reset(frame);
    let theme = app.iced_theme();
    view.as_widget().draw(tree, renderer, &theme, &Default::default(), Layout::new(&node), mouse::Cursor::Unavailable, &frame);
    Headless::screenshot(renderer, Size::new(size.width as u32, size.height as u32), 1.0, Color::BLACK)
}

/// Draw until the artwork has arrived and every fade has settled.
pub(super) fn settle(app: &App, renderer: &mut Renderer, size: Size) -> Vec<u8> {
    let mut tree = None;
    let mut quiet = 0;
    for _ in 0..80 {
        let _ = draw(app, renderer, &mut tree, size);
        std::thread::sleep(Duration::from_millis(40));
        if images::take_dirty() {
            quiet = 0;
        } else {
            quiet += 1;
            if quiet >= 12 {
                break;
            }
        }
    }
    draw(app, renderer, &mut tree, size)
}

pub(super) fn renderer_new() -> Renderer {
    // The bundled Inter weights, as `run` gives them to the application.
    static FONTS_LOADED: std::sync::Once = std::sync::Once::new();
    FONTS_LOADED.call_once(|| {
        let mut fs = iced::advanced::graphics::text::font_system().write().expect("font system");
        for f in FONTS {
            fs.load_font(std::borrow::Cow::Borrowed(f));
        }
    });
    iced::futures::executor::block_on(<Renderer as Headless>::new(INTER, Pixels(16.0), None)).expect("offscreen wgpu adapter")
}

pub(super) fn save(name: &str, size: Size, pixels: &[u8]) {
    let dir = std::env::var_os("ABYSS_SHOT_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("abyss-shots"));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{name}.png"));
    image::save_buffer(&path, pixels, size.width as u32, size.height as u32, image::ColorType::Rgba8).expect("write the PNG");
    println!("{}", path.display());
}

/// The size every shot is drawn at, from `ABYSS_SHOT_SIZE=<w>x<h>` (design px).
pub(super) fn forced_size() -> Option<Size> {
    let v = std::env::var("ABYSS_SHOT_SIZE").ok()?;
    let (w, h) = v.split_once('x')?;
    Some(Size::new(w.trim().parse().ok()?, h.trim().parse().ok()?))
}

#[test]
#[ignore = "requires an isolated profile and a wgpu adapter; writes PNGs for review"]
fn page_shots() {
    assert!(std::env::var_os("ABYSS_CONFIG_DIR").is_some() && std::env::var_os("ABYSS_CACHE_DIR").is_some());
    let _theme = theme::THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let only = std::env::var("ABYSS_SHOTS").ok();
    let wanted = |name: &str| only.as_deref().is_none_or(|o| o.split(',').any(|w| name.contains(w)));
    let mut renderer = renderer_new();
    let (mut app, boot) = App::boot();
    drop(boot);
    app.welcome_step = None;
    app.signed_in = true;
    app.shuffle = false;
    let art = covers();
    let cover = |i: usize| art.get(i % art.len().max(1)).cloned();

    let names = ["Aurora Lindqvist", "The Velvet Static", "Marea Baja", "Kōji Sato", "Nadia Okafor", "Hollow Pines"];
    let artists: Vec<Artist> = names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let mut a = Artist::parse(&json!({"id": 10 + i, "name": n})).unwrap();
            a.picture = cover(i + 7);
            a
        })
        .collect();
    let titles = [
        "Midnight Cartography",
        "Salt & Static",
        "Las horas lentas",
        "Paper Moons (Deluxe Edition)",
        "Echo Chamber",
        "Northern Lights",
        "Glass Garden",
        "Undertow",
    ];
    let albums: Vec<Album> = titles
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let a = &artists[i % artists.len()];
            let mut album = Album::parse(&json!({
                "id": 200 + i, "title": t, "artist": {"id": a.id, "name": a.name},
                "releaseDate": format!("20{:02}-03-14", 12 + i), "numberOfTracks": 9 + i, "duration": 2400 + i * 180,
                "type": if i % 3 == 2 { "EP" } else { "ALBUM" }, "explicit": i % 4 == 1,
                "copyright": "℗ 2024 Abyss Records",
                "mediaMetadata": {"tags": if i % 2 == 0 { json!(["LOSSLESS", "HIRES_LOSSLESS"]) } else { json!(["LOSSLESS"]) }}
            }))
            .unwrap();
            album.cover = cover(i);
            album
        })
        .collect();
    let track_titles = [
        "Opening Theme",
        "A Very Long Song Title That Keeps Going Past The Edge Of The Column",
        "Interlude",
        "Lighthouse",
        "Tidal Pull",
        "Quiet Machines",
        "Last Train Home",
        "Coda",
        "Bonus: Live at the Palace",
        "Outro",
        "Static Bloom",
        "Paper Boats",
    ];
    let tracks: Vec<Track> = track_titles
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let album = &albums[i % 3];
            let a = &album.artists[0];
            Track::parse(&json!({
                "id": 1000 + i, "title": t, "duration": 150 + i * 23, "allowStreaming": true, "explicit": i % 5 == 1,
                "artist": {"id": a.id, "name": a.name},
                "album": {"id": album.id, "title": album.title, "cover": album.cover},
                "trackNumber": i + 1, "volumeNumber": if i >= 8 { 2 } else { 1 }, "audioQuality": "LOSSLESS",
                "mediaMetadata": {"tags": if i % 3 == 0 { json!(["LOSSLESS", "HIRES_LOSSLESS"]) } else { json!(["LOSSLESS"]) }}
            }))
            .unwrap()
        })
        .collect();
    let playlists: Vec<Playlist> = ["Late night drive", "Focus", "Domingo", "Gym 2025"]
        .iter()
        .enumerate()
        .map(|(i, t)| {
            Playlist::parse(&json!({
                "uuid": format!("pl-{i}"), "title": t, "type": "USER", "numberOfTracks": 12 + i * 7, "duration": 3600 + i * 600,
                "description": "Canciones para no dormirse al volante.", "creator": {"id": 1, "name": "Rauna"}
            }))
            .unwrap()
        })
        .collect();
    let mixes: Vec<Mix> = ["My Daily Discovery", "My New Arrivals", "Mix 1", "Mix 2"]
        .iter()
        .enumerate()
        .map(|(i, t)| Mix::parse(&json!({"id": format!("mix-{i}"), "title": t, "subTitle": "Aurora Lindqvist, Marea Baja"})).unwrap())
        .collect();

    let mut cards: Vec<Item> = albums.iter().cloned().map(Item::Album).collect();
    cards.extend(playlists.iter().cloned().map(Item::Playlist));
    let rows = vec![
        Row { title: "Recently played".into(), items: albums.iter().cloned().map(Item::Album).collect(), kind: RowKind::Cards },
        Row { title: "Your mixes".into(), items: mixes.iter().cloned().map(Item::Mix).collect(), kind: RowKind::Cards },
        Row { title: "Suggested artists".into(), items: artists.iter().cloned().map(Item::Artist).collect(), kind: RowKind::Cards },
        Row { title: "Popular playlists".into(), items: cards.clone(), kind: RowKind::Cards },
        Row {
            title: "Genres".into(),
            items: ["Pop", "Hip-Hop", "Electronic", "Jazz", "Classical", "Latin"]
                .iter()
                .map(|t| Item::Link(Link { title: (*t).into(), path: t.to_lowercase(), icon: String::new() }))
                .collect(),
            kind: RowKind::Chips,
        },
    ];
    app.home = Load::Ready(rows.clone());
    app.explore = Load::Ready(rows);
    app.my_playlists = Load::Ready(playlists.clone());
    app.my_mixes = Load::Ready(mixes.clone());
    app.fav_tracks = Load::Ready(tracks.clone());
    app.fav_albums = Load::Ready(albums.clone());
    app.fav_artists = Load::Ready(artists.clone());
    for album in &albums {
        app.albums.insert(album.id, Load::Ready((album.clone(), tracks.clone())));
    }
    app.album_extra.insert(albums[0].id, Load::Ready((albums[1..6].to_vec(), artists[1..5].to_vec())));
    app.artists.insert(
        artists[0].id,
        Load::Ready(ArtistPage {
            artist: artists[0].clone(),
            top_tracks: tracks[..6].to_vec(),
            albums: albums.clone(),
            singles: albums[2..5].to_vec(),
            appears_on: albums[4..].to_vec(),
            similar: artists[1..].to_vec(),
            bio: Some("Aurora Lindqvist is a Swedish songwriter whose records blend glacial synths with folk storytelling.".into()),
        }),
    );
    for p in &playlists {
        app.playlists.insert(p.uuid.clone(), Load::Ready((p.clone(), tracks.clone())));
    }
    for m in &mixes {
        app.mixes.insert(m.id.clone(), Load::Ready((m.clone(), tracks.clone())));
    }
    app.search.insert(
        "aurora".into(),
        Load::Ready(SearchResults {
            top: Some(Item::Artist(artists[0].clone())),
            tracks: tracks[..6].to_vec(),
            albums: albums.clone(),
            artists: artists.clone(),
            playlists: playlists.clone(),
        }),
    );
    app.apply_sorts();
    // Something playing, so the bar, the rows and the queue show it.
    drop(app.update(Message::Navigate(Page::Album(albums[0].id))));
    drop(app.update(Message::PlayFrom(rows::Source::Page, 1)));
    app.toast = None;

    let mut size = Size::new(1440.0, 900.0);
    app.viewport = size;
    let mut shoot = |app: &mut App, name: &str, prep: &dyn Fn(&mut App)| {
        // `ABYSS_SHOT_SIZE=1366x768` draws every shot at that size (design px).
        size = match forced_size() {
            Some(forced) => forced,
            None if name.starts_with("narrow") => Size::new(1000.0, 700.0),
            None => Size::new(1440.0, 900.0),
        };
        app.viewport = size;
        for light in [false, true] {
            let full = format!("{}-{name}", if light { "light" } else { "dark" });
            if !wanted(&full) {
                continue;
            }
            theme::set_light(light);
            prep(app);
            app.page_at = Instant::now() - Duration::from_secs(2);
            let pixels = settle(app, &mut renderer, size);
            save(&full, size, &pixels);
        }
        theme::set_light(false);
    };
    let nav = |page: Page| {
        move |app: &mut App| {
            app.menu = None;
            app.dialog = None;
            app.info_panel = None;
            app.search_open = false;
            app.search_query.clear();
            if app.page != page {
                drop(app.update(Message::Navigate(page.clone())));
            }
        }
    };
    shoot(&mut app, "home", &nav(Page::Home));
    shoot(&mut app, "explore", &nav(Page::Explore));
    shoot(&mut app, "album", &nav(Page::Album(albums[0].id)));
    shoot(&mut app, "artist", &nav(Page::Artist(artists[0].id)));
    shoot(&mut app, "playlist", &nav(Page::Playlist(playlists[0].uuid.clone())));
    shoot(&mut app, "mix", &nav(Page::Mix(mixes[0].id.clone())));
    shoot(&mut app, "search", &nav(Page::Search("aurora".into())));
    shoot(&mut app, "tracks", &nav(Page::MyTracks));
    shoot(&mut app, "artists", &nav(Page::MyArtists));
    shoot(&mut app, "playlists", &nav(Page::MyPlaylists));
    shoot(&mut app, "mixes", &nav(Page::MyMixes));
    shoot(&mut app, "recent", &nav(Page::Recent));
    shoot(&mut app, "replay", &nav(Page::Replay));
    shoot(&mut app, "discover", &nav(Page::Discover));
    let grid_albums = |app: &mut App| {
        nav(Page::MyAlbums)(app);
        app.settings.lock().lib_grid.insert("albums".into(), true);
        app.expanded_album = None;
    };
    shoot(&mut app, "albums-grid", &grid_albums);
    shoot(&mut app, "albums-expanded", &|app| {
        grid_albums(app);
        app.expanded_album = Some(albums[1].id);
    });
    shoot(&mut app, "albums-list", &|app| {
        nav(Page::MyAlbums)(app);
        app.settings.lock().lib_grid.insert("albums".into(), false);
        app.expanded_album = None;
    });
    for tab in 0..=settings::EQ_TAB {
        shoot(&mut app, &format!("settings-{tab}"), &|app| {
            nav(Page::Settings)(app);
            app.settings_tab = tab;
        });
    }
    let track = tracks[3].clone();
    shoot(&mut app, "menu-track", &|app| {
        nav(Page::Album(albums[0].id))(app);
        app.menu = Some((Point::new(700.0, 380.0), MenuTarget::Track(track.clone())));
    });
    shoot(&mut app, "menu-sort", &|app| {
        nav(Page::MyTracks)(app);
        app.menu = Some((Point::new(1100.0, 140.0), MenuTarget::Sort("tracks")));
    });
    shoot(&mut app, "dialog-new", &|app| {
        nav(Page::MyPlaylists)(app);
        app.dialog = Some(Dialog::NewPlaylist { add: Vec::new() });
    });
    shoot(&mut app, "dialog-remove", &|app| {
        nav(Page::MyTracks)(app);
        app.dialog = Some(Dialog::ConfirmRemove(RemoveKind::FavTrack(track.clone())));
    });
    shoot(&mut app, "info-album", &|app| {
        nav(Page::Album(albums[0].id))(app);
        app.info_panel = Some(info::InfoTarget::Album(albums[0].clone()));
    });
    shoot(&mut app, "search-overlay", &|app| {
        nav(Page::Home)(app);
        app.search_open = true;
        app.search_query = "aurora".into();
    });
    shoot(&mut app, "queue", &|app| {
        nav(Page::Album(albums[0].id))(app);
        app.search_open = false;
        if !app.show_queue {
            drop(app.update(Message::ToggleQueue));
            std::thread::sleep(panel_motion::PANEL);
        }
    });
    if app.show_queue {
        drop(app.update(Message::CloseSidePanel));
        std::thread::sleep(panel_motion::PANEL);
    }
    let playing = app.now.as_ref().map_or(0, |n| n.track.id);
    for (name, lyrics) in [
        (
            "lyrics",
            Load::Ready(crate::tidal::Lyrics {
                text: String::new(),
                synced: (0..24).map(|i| (f64::from(i) * 4.0, format!("Line {i} of a song about the tide coming in"))).collect(),
                provider: "LRCLIB".into(),
                roman: Vec::new(),
            }),
        ),
        ("lyrics-none", Load::Failed("no lyrics".into())),
    ] {
        app.lyrics.insert(playing, lyrics);
        app.lyrics_docs.clear();
        shoot(&mut app, name, &|app| {
            nav(Page::Album(albums[0].id))(app);
            if !app.show_lyrics {
                drop(app.update(Message::ToggleLyrics));
                std::thread::sleep(panel_motion::PANEL);
            }
        });
    }
    if app.show_lyrics {
        drop(app.update(Message::CloseSidePanel));
        std::thread::sleep(panel_motion::PANEL);
    }
    // The full-screen player over its flowing background: queue (glass pane),
    // lyrics, and lyrics missing.
    for (name, tab) in [("np-queue", NpTab::Queue), ("np-lyrics-none", NpTab::Lyrics)] {
        shoot(&mut app, name, &|app| {
            app.now_playing_open = true;
            app.np_tab = tab;
        });
    }
    app.now_playing_open = false;
    shoot(&mut app, "narrow-tracks", &nav(Page::MyTracks));
    shoot(&mut app, "narrow-album", &nav(Page::Album(albums[0].id)));
    shoot(&mut app, "narrow-home", &nav(Page::Home));
    shoot(&mut app, "narrow-artist", &nav(Page::Artist(artists[0].id)));
    // Empty, loading and failed states.
    shoot(&mut app, "empty-tracks", &|app| {
        app.fav_tracks = Load::Ready(Vec::new());
        app.apply_sorts();
        nav(Page::MyTracks)(app);
    });
    shoot(&mut app, "loading-playlists", &|app| {
        app.my_playlists = Load::Loading;
        nav(Page::MyPlaylists)(app);
    });
    shoot(&mut app, "failed-album", &|app| {
        app.albums.insert(999, Load::Failed("HTTP 503: the service is unavailable".into()));
        nav(Page::Album(999))(app);
    });
    shoot(&mut app, "loading-home", &|app| {
        app.home = Load::Loading;
        nav(Page::Home)(app);
    });
}
