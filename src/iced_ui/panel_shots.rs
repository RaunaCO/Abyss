//! Offscreen renders of the player-side panels in the states `page_shots`
//! does not reach: synced lyrics with long and romanised lines (side panel
//! and Now Playing), very long titles, an empty queue and a 200-track one,
//! Replay with a history, the graphic equaliser and Settings at the window's
//! minimum width. Every shot is drawn at the sizes that matter (or only at
//! `ABYSS_SHOT_SIZE`), in both themes:
//!
//! `cargo test --bin Abyss -- --include-ignored panel_shots --nocapture`
//! (with `ABYSS_CONFIG_DIR`, `ABYSS_CACHE_DIR`, `ABYSS_COVERS`, `ABYSS_SHOT_DIR`
//! and optionally `ABYSS_SHOTS` set as for `page_shots`).
//!
//! The frame helpers repeat `pages::shots`' (private to `pages`).

use super::super::*;
use iced::advanced::{
    layout,
    renderer::{Headless, Renderer as _},
    widget::Tree,
    Layout,
};
use iced::{Color, Pixels, Rectangle, Renderer};
use serde_json::json;

fn covers() -> Vec<String> {
    let dir = std::env::var_os("ABYSS_COVERS").map(std::path::PathBuf::from).unwrap_or_else(|| crate::config::cache_dir().join("images"));
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "img")).collect())
        .unwrap_or_default();
    files.sort();
    files.into_iter().step_by(37).take(16).map(|p| format!("local:{}", p.display())).collect()
}

fn draw(app: &App, renderer: &mut Renderer, tree: &mut Option<Tree>, size: Size) -> Vec<u8> {
    let mut view = app.view();
    let tree = match tree {
        Some(tree) => {
            tree.diff(view.as_widget());
            tree
        }
        None => tree.insert(Tree::new(view.as_widget())),
    };
    let node = view.as_widget_mut().layout(tree, renderer, &layout::Limits::new(size, size));
    // The queue's centring arrives as a scroll operation (a `Task` on screen).
    let mut scroll = iced::advanced::widget::operation::scrollable::scroll_to::<()>(
        iced::advanced::widget::Id::new(queue::SCROLL_ID),
        iced::advanced::widget::operation::scrollable::AbsoluteOffset { x: None, y: Some(app.queue_scroll) },
    );
    view.as_widget_mut().operate(tree, Layout::new(&node), renderer, &mut scroll);
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

fn settle(app: &App, renderer: &mut Renderer, size: Size) -> Vec<u8> {
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

fn renderer_new() -> Renderer {
    static FONTS_LOADED: std::sync::Once = std::sync::Once::new();
    FONTS_LOADED.call_once(|| {
        let mut fs = iced::advanced::graphics::text::font_system().write().expect("font system");
        for f in FONTS {
            fs.load_font(std::borrow::Cow::Borrowed(f));
        }
    });
    iced::futures::executor::block_on(<Renderer as Headless>::new(INTER, Pixels(16.0), None)).expect("offscreen wgpu adapter")
}

fn save(name: &str, size: Size, pixels: &[u8]) {
    let dir = std::env::var_os("ABYSS_SHOT_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("abyss-shots"));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{name}.png"));
    image::save_buffer(&path, pixels, size.width as u32, size.height as u32, image::ColorType::Rgba8).expect("write the PNG");
    println!("{}", path.display());
}

/// The sizes to draw: `ABYSS_SHOT_SIZE=<w>x<h>`, or the five that matter.
fn sizes() -> Vec<Size> {
    let forced = std::env::var("ABYSS_SHOT_SIZE").ok().and_then(|v| {
        let (w, h) = v.split_once('x')?;
        Some(Size::new(w.trim().parse().ok()?, h.trim().parse().ok()?))
    });
    match forced {
        Some(size) => vec![size],
        None => [(960.0, 600.0), (1366.0, 768.0), (1536.0, 864.0), (1707.0, 960.0), (2048.0, 1152.0)]
            .into_iter()
            .map(|(w, h)| Size::new(w, h))
            .collect(),
    }
}

fn track(i: usize, title: &str, artist: &str, cover: Option<String>) -> Track {
    Track::parse(&json!({
        "id": 5000 + i, "title": title, "duration": 150 + (i % 9) * 23, "allowStreaming": true,
        "artist": {"id": 70 + i % 5, "name": artist},
        "album": {"id": 300 + i % 4, "title": "Glass Garden", "cover": cover},
        "trackNumber": i + 1, "volumeNumber": 1, "audioQuality": "LOSSLESS",
        "mediaMetadata": {"tags": ["LOSSLESS"]}
    }))
    .unwrap()
}

fn open_side(app: &mut App, lyrics: bool) {
    let shown = if lyrics { app.show_lyrics } else { app.show_queue };
    if !shown {
        drop(app.update(if lyrics { Message::ToggleLyrics } else { Message::ToggleQueue }));
        std::thread::sleep(panel_motion::PANEL);
    }
}

fn close_side(app: &mut App) {
    if app.show_queue || app.show_lyrics {
        drop(app.update(Message::CloseSidePanel));
        std::thread::sleep(panel_motion::PANEL);
    }
}

#[test]
#[ignore = "requires an isolated profile and a wgpu adapter; writes PNGs for review"]
fn panel_shots() {
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

    let long_title = "Everything I Never Said To You On The Last Night Of Summer (Extended Orchestral Version) [feat. The Velvet Static]";
    let long_artist = "Aurora Lindqvist, Marea Baja, The Velvet Static, Kōji Sato & Nadia Okafor";
    let names = ["Lighthouse", "Tidal Pull", "Quiet Machines", "Last Train Home", "Coda", "Paper Boats", "Static Bloom"];
    let short: Vec<Track> = (0..12)
        .map(|i| {
            track(i, if i == 1 { long_title } else { names[i % names.len()] }, if i == 1 { long_artist } else { "Marea Baja" }, cover(i))
        })
        .collect();
    let many: Vec<Track> =
        (0..200).map(|i| track(100 + i, &format!("{} {}", names[i % names.len()], i + 1), "Hollow Pines", cover(i))).collect();

    // Synced lyrics: long lines that wrap, an instrumental gap, and a
    // Japanese song with its romanised reading.
    let mut synced: Vec<(f64, String)> = Vec::new();
    let mut roman: Vec<Option<String>> = Vec::new();
    let latin = [
        "I kept the lights on in the hallway for the version of you that never came home",
        "Short line",
        "",
        "And every wave that broke against the harbour wall was counting down the hours we had left before the morning",
        "Hold on",
    ];
    let japanese = [
        ("夜明け前の海に君の声が溶けていく", "yoake mae no umi ni kimi no koe ga tokete iku"),
        ("忘れないで", "wasurenaide"),
        ("この街の灯りがすべて消えても僕はここで待っている", "kono machi no akari ga subete kiete mo boku wa koko de matte iru"),
    ];
    for i in 0..30 {
        let at = f64::from(i as u32) * 4.5 + if i > 12 { 8.0 } else { 0.0 };
        if i % 3 == 2 {
            let (line, reading) = japanese[(i / 3) % japanese.len()];
            synced.push((at, line.to_string()));
            roman.push(Some(reading.to_string()));
        } else {
            synced.push((at, latin[i % latin.len()].to_string()));
            roman.push(None);
        }
    }
    let lyrics = crate::tidal::Lyrics { text: String::new(), synced, provider: "LRCLIB".into(), roman };
    app.settings.lock().lyrics_romanize = true;

    let mut shoot_at = |app: &mut App, name: &str, sizes: &[Size], prep: &dyn Fn(&mut App)| {
        for &size in sizes {
            for light in [false, true] {
                let full = format!("{}-{name}-{}x{}", if light { "light" } else { "dark" }, size.width, size.height);
                if !wanted(&full) {
                    continue;
                }
                app.viewport = size;
                theme::set_light(light);
                prep(app);
                app.page_at = Instant::now() - Duration::from_secs(2);
                let pixels = settle(app, &mut renderer, size);
                save(&full, size, &pixels);
            }
        }
        theme::set_light(false);
    };
    let all = sizes();
    let page = |app: &mut App, page: Page| {
        app.menu = None;
        app.dialog = None;
        app.info_panel = None;
        app.search_open = false;
        app.now_playing_open = false;
        if app.page != page {
            drop(app.update(Message::Navigate(page)));
        }
    };

    // A long title playing, with lyrics.
    drop(app.play_tracks(short.clone(), 1));
    app.toast = None;
    let playing = app.now.as_ref().map_or(0, |n| n.track.id);
    app.lyrics.insert(playing, Load::Ready(lyrics.clone()));
    app.lyrics_docs.clear();
    let at_line = |app: &mut App| {
        // Between two lines of the second verse, so lines are past, current and next.
        app.snap.position = 41.0;
        app.snap_at = Instant::now();
        app.lyrics_follow = true;
    };
    shoot_at(&mut app, "pb-side-lyrics", &all, &|app| {
        page(app, Page::Home);
        at_line(app);
        open_side(app, true);
    });
    close_side(&mut app);
    shoot_at(&mut app, "pb-side-queue-long", &all, &|app| {
        page(app, Page::Home);
        open_side(app, false);
        drop(app.centre_queue());
    });
    close_side(&mut app);
    for (name, tab) in [
        ("pb-np-lyrics", NpTab::Lyrics),
        ("pb-np-queue-long", NpTab::Queue),
        ("pb-np-credits-none", NpTab::Credits),
        ("pb-np-similar-none", NpTab::Similar),
    ] {
        shoot_at(&mut app, name, &all, &|app| {
            at_line(app);
            app.now_playing_open = true;
            app.np_tab = tab;
            drop(app.centre_queue());
        });
    }
    // Credits and similar tracks loaded.
    app.credits.insert(
        playing,
        Load::Ready(vec![
            ("Main Artist".into(), vec!["Aurora Lindqvist".into()]),
            ("Featured Artist".into(), vec!["The Velvet Static".into(), "Kōji Sato".into()]),
            ("Producer".into(), vec!["Nadia Okafor".into(), "Hollow Pines".into()]),
            ("Composer".into(), vec!["Aurora Lindqvist".into(), "Marea Baja".into()]),
            ("Mastering Engineer".into(), vec!["Kōji Sato".into()]),
            ("Record Label".into(), vec!["Abyss Records".into()]),
        ]),
    );
    app.similar.insert(playing, Load::Ready(short[2..].to_vec()));
    for (name, tab) in [("pb-np-credits", NpTab::Credits), ("pb-np-similar", NpTab::Similar)] {
        shoot_at(&mut app, name, &all, &|app| {
            app.now_playing_open = true;
            app.np_tab = tab;
        });
    }
    // Stacked (a zoomed window: fewer design px than the minimum).
    for tab in [NpTab::Lyrics, NpTab::Queue] {
        shoot_at(&mut app, &format!("pb-np-stacked-{tab:?}"), &[Size::new(760.0, 900.0), Size::new(780.0, 560.0)], &|app| {
            at_line(app);
            app.now_playing_open = true;
            app.np_tab = tab;
        });
    }
    app.now_playing_open = false;

    // 200 tracks queued, the 120th playing.
    drop(app.play_tracks(many.clone(), 120));
    app.toast = None;
    shoot_at(&mut app, "pb-side-queue-200", &all, &|app| {
        page(app, Page::Home);
        open_side(app, false);
        drop(app.centre_queue());
    });
    close_side(&mut app);
    shoot_at(&mut app, "pb-np-queue-200", &all, &|app| {
        app.now_playing_open = true;
        app.np_tab = NpTab::Queue;
        drop(app.centre_queue());
    });
    app.now_playing_open = false;

    // Nothing queued.
    app.queue.clear();
    shoot_at(&mut app, "pb-side-queue-empty", &all, &|app| {
        page(app, Page::Home);
        open_side(app, false);
        drop(app.centre_queue());
    });
    close_side(&mut app);
    shoot_at(&mut app, "pb-np-queue-empty", &all, &|app| {
        app.now_playing_open = true;
        app.np_tab = NpTab::Queue;
    });
    app.now_playing_open = false;
    drop(app.play_tracks(short.clone(), 1));
    app.toast = None;

    // Replay with three weeks of listening.
    let now = crate::history::unix_now();
    app.history = (0..240u64)
        .map(|k| {
            let t = &short[(k as usize * 7 + k as usize / 5) % short.len()];
            crate::history::Entry {
                ts: now - (k * 7919) % (21 * 86400),
                secs: 180,
                id: t.id,
                title: t.title.clone(),
                artist: t.artist_names(),
                artist_id: t.artists.first().map_or(0, |a| a.id),
                album: "Glass Garden".into(),
                album_id: 300 + k % 4,
                cover: t.cover(320),
                dur: 200,
            }
        })
        .collect();
    app.history.sort_by_key(|e| e.ts);
    shoot_at(&mut app, "pb-replay", &all, &|app| page(app, Page::Replay));
    // Tall, to see the whole dashboard.
    shoot_at(&mut app, "pb-replay-tall", &[Size::new(1366.0, 1900.0), Size::new(960.0, 2600.0)], &|app| page(app, Page::Replay));

    // Song information, the stream's data loaded.
    let song = short[1].clone();
    app.tech_info.insert(
        song.id,
        Load::Ready((
            crate::tidal::probe::TechInfo {
                codec: "FLAC".into(),
                channels: Some(2),
                sample_rate: Some(96_000),
                bits: Some(24),
                total_samples: Some(96_000 * 173),
                file_size: Some(61_400_000),
                segmented: false,
            },
            "HI_RES_LOSSLESS".into(),
        )),
    );
    shoot_at(&mut app, "pb-info-song", &all, &|app| {
        page(app, Page::Home);
        app.info_panel = Some(info::InfoTarget::Song(song.clone()));
    });

    // The graphic equaliser, a bar selected.
    shoot_at(&mut app, "pb-eq-graphic", &[Size::new(1366.0, 1500.0), Size::new(960.0, 1700.0)], &|app| {
        page(app, Page::Settings);
        app.settings_tab = settings::EQ_TAB;
        app.settings.lock().eq_mode = crate::audio::graphic::EqMode::Graphic;
        app.eq_band_selected = Some(4);
    });
    app.settings.lock().eq_mode = Default::default();
    app.eq_band_selected = None;

    // Settings at the window's minimum size, every tab.
    for tab in 0..=settings::EQ_TAB {
        shoot_at(
            &mut app,
            &format!("pb-settings-min-{tab}"),
            &[Size::new(960.0, 600.0), Size::new(960.0, 1800.0), Size::new(1536.0, 1800.0)],
            &|app| {
                page(app, Page::Settings);
                app.settings_tab = tab;
            },
        );
    }
}
