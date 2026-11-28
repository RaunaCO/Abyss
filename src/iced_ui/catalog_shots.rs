//! Offscreen renders of catalogue and library panel states `page_shots` does not reach: hovered
//! tooltips (drawn through their overlay), selected / playing / hovered
//! track rows, menus at the window's edges, long texts and narrow widths.
//! `cargo test --bin Abyss -- --include-ignored catalog_shots` (same
//! environment as `page_shots`; PNGs go to `ABYSS_SHOT_DIR`).

use super::super::*;
use super::shots::{renderer_new, save};
use iced::advanced::{
    layout,
    renderer::{Headless, Renderer as _},
    widget::Tree,
    Layout,
};
use iced::widget::container;
use iced::{Color, Rectangle, Renderer, Vector};

/// Draw `el` at `size` with the pointer at `cursor`, its overlays included
/// (the tooltip's bubble lives in one), after `wait` with the pointer resting.
fn draw_with_overlay(renderer: &mut Renderer, mut el: Element<'_, Message>, size: Size, cursor: Point, wait: Duration) -> Vec<u8> {
    let mut tree = Tree::new(el.as_widget());
    let frame = Rectangle::with_size(size);
    let cur = mouse::Cursor::Available(cursor);
    let mut messages = Vec::new();
    let mut node = el.as_widget_mut().layout(&mut tree, renderer, &layout::Limits::new(size, size));
    let mut send = |el: &mut Element<'_, Message>, tree: &mut Tree, node: &layout::Node, event: iced::Event| {
        el.as_widget_mut().update(
            tree,
            &event,
            Layout::new(node),
            cur,
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut iced::advanced::Shell::new(&mut messages),
            &frame,
        );
    };
    send(&mut el, &mut tree, &node, iced::Event::Mouse(mouse::Event::CursorMoved { position: cursor }));
    let start = Instant::now();
    while start.elapsed() < wait {
        std::thread::sleep(Duration::from_millis(40));
        send(&mut el, &mut tree, &node, iced::Event::Window(window::Event::RedrawRequested(Instant::now())));
    }
    node = el.as_widget_mut().layout(&mut tree, renderer, &layout::Limits::new(size, size));
    send(&mut el, &mut tree, &node, iced::Event::Window(window::Event::RedrawRequested(Instant::now())));
    renderer.reset(frame);
    let theme = iced::Theme::Dark;
    let style = iced::advanced::renderer::Style { text_color: theme::text() };
    el.as_widget().draw(&tree, renderer, &theme, &style, Layout::new(&node), cur, &frame);
    if let Some(mut overlay) = el.as_widget_mut().overlay(&mut tree, Layout::new(&node), renderer, &frame, Vector::ZERO) {
        let over = overlay.as_overlay_mut().layout(renderer, size);
        overlay.as_overlay().draw(renderer, &theme, &style, Layout::new(&over), cur);
    }
    Headless::screenshot(renderer, Size::new(size.width as u32, size.height as u32), 1.0, Color::BLACK)
}

/// Pixels of `px` (RGBA, `w` wide) inside `r` that differ clearly from the
/// colour at its top-left corner: something (text) is drawn there.
fn ink_in(px: &[u8], w: u32, r: Rectangle) -> usize {
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2]]
    };
    let base = at(r.x as u32, r.y as u32);
    let mut n = 0;
    for y in r.y as u32..(r.y + r.height) as u32 {
        for x in r.x as u32..(r.x + r.width) as u32 {
            let c = at(x, y);
            if c.iter().zip(base).any(|(a, b)| a.abs_diff(b) > 60) {
                n += 1;
            }
        }
    }
    n
}

/// A hovered tooltip shows its label (it used to draw an empty capsule: the
/// tooltip overlay draws its bubble with an infinite viewport), centred
/// under its button, and stays inside the window at the right edge.
#[test]
#[ignore = "requires a wgpu adapter; writes PNGs for review"]
fn catalog_shots_tooltips() {
    let _theme = theme::THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut renderer = renderer_new();
    let size = Size::new(360.0, 120.0);
    for light in [false, true] {
        theme::set_light(light);
        for (name, align) in [("middle", iced::alignment::Horizontal::Center), ("right-edge", iced::alignment::Horizontal::Right)] {
            let button = widgets::icon_button(icons::Icon::Settings, 16.0, theme::text(), Message::Navigate(crate::shared::Page::Settings));
            let el: Element<'_, Message> = container(widgets::tip(button, "Settings and preferences"))
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(8)
                .align_x(align)
                .style(|_| container::Style { background: Some(theme::frame().into()), ..Default::default() })
                .into();
            // The 28 px button: centred, or against the right edge.
            let bx = if name == "middle" { (size.width - 28.0) / 2.0 } else { size.width - 8.0 - 28.0 };
            let px = draw_with_overlay(&mut renderer, el, size, Point::new(bx + 14.0, 22.0), Duration::from_millis(700));
            let file = format!("{}-tooltip-{name}", if light { "light" } else { "dark" });
            save(&file, size, &px);
            // The bubble's label sits under the button (36..70 px down): its glyphs must show.
            let band = Rectangle { x: 8.0, y: 44.0, width: size.width - 16.0, height: 14.0 };
            assert!(ink_in(&px, size.width as u32, band) > 40, "{file}: the tooltip shows no label");
        }
    }
    theme::set_light(false);
}

// ------------------------------------------------------------ app fixtures

/// An app with a Library of real covers, long titles and names, a track
/// playing (row 1 of the Library), signed in, on the Library's tracks.
fn fixture() -> App {
    use serde_json::json;
    let (mut app, boot) = App::boot();
    drop(boot);
    app.welcome_step = None;
    app.signed_in = true;
    app.shuffle = false;
    let art = super::shots::covers();
    let cover = |i: usize| art.get(i % art.len().max(1)).cloned();
    let names = ["Aurora Lindqvist", "The Velvet Static & The Extraordinarily Long Named Orchestra Of Gothenburg", "Marea Baja"];
    let album_titles =
        ["Midnight Cartography", "Salt & Static (Original Motion Picture Soundtrack, Deluxe 20th Anniversary Edition)", "Las horas lentas"];
    let albums: Vec<crate::tidal::Album> = album_titles
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut a = crate::tidal::Album::parse(&json!({
                "id": 200 + i, "title": t, "artist": {"id": 10 + i, "name": names[i]},
                "releaseDate": "2019-03-14", "numberOfTracks": 12, "duration": 3000, "type": "ALBUM",
                "mediaMetadata": {"tags": ["LOSSLESS"]}
            }))
            .unwrap();
            a.cover = cover(i);
            a
        })
        .collect();
    let titles = [
        "Opening Theme",
        "A Very Long Song Title That Keeps Going Past The Edge Of The Column (feat. Somebody Else)",
        "Interlude",
        "Lighthouse",
        "Tidal Pull",
        "Quiet Machines",
        "Last Train Home",
        "Coda",
        "Paper Boats",
        "Static Bloom",
    ];
    let tracks: Vec<crate::tidal::Track> = titles
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let album = &albums[i % 3];
            crate::tidal::Track::parse(&json!({
                "id": 1000 + i, "title": t, "duration": 150 + i * 23, "allowStreaming": true, "explicit": i % 4 == 1,
                "artist": {"id": 10 + i % 3, "name": names[i % 3]},
                "album": {"id": album.id, "title": album.title, "cover": album.cover},
                "trackNumber": i + 1, "volumeNumber": 1, "audioQuality": "LOSSLESS"
            }))
            .unwrap()
        })
        .collect();
    app.fav_tracks = Load::Ready(tracks.clone());
    app.fav_albums = Load::Ready(albums.clone());
    for a in &albums {
        app.albums.insert(a.id, Load::Ready((a.clone(), tracks.clone())));
    }
    // Shown in the order given (no sort by date added: none is set).
    app.apply_sorts();
    drop(app.update(Message::Navigate(crate::shared::Page::MyTracks)));
    let ix = app.lib_tracks.iter().position(|t| t.id == 1001).unwrap_or(1);
    drop(app.update(Message::PlayFrom(rows::Source::Page, ix)));
    app.toast = None;
    app
}

/// Draw the app until it settles with the pointer at `cursor`: the view's
/// own messages (sizes measured by sensors, scrolls) are fed back, the
/// pointer's hover fades in, artwork arrives.
fn settle_with(app: &mut App, renderer: &mut Renderer, size: Size, cursor: Option<Point>) -> Vec<u8> {
    let cur = cursor.map_or(mouse::Cursor::Unavailable, mouse::Cursor::Available);
    let frame = Rectangle::with_size(size);
    let mut tree: Option<Tree> = None;
    let mut quiet = 0;
    for step in 0..90 {
        let mut messages = Vec::new();
        {
            let mut view = app.view();
            let t = match &mut tree {
                Some(t) => {
                    t.diff(view.as_widget());
                    t
                }
                None => tree.insert(Tree::new(view.as_widget())),
            };
            let node = view.as_widget_mut().layout(t, renderer, &layout::Limits::new(size, size));
            let mut events = vec![iced::Event::Window(window::Event::RedrawRequested(Instant::now()))];
            if let (Some(p), true) = (cursor, step == 2) {
                events.push(iced::Event::Mouse(mouse::Event::CursorMoved { position: p }));
            }
            for e in events {
                view.as_widget_mut().update(
                    t,
                    &e,
                    Layout::new(&node),
                    cur,
                    renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut iced::advanced::Shell::new(&mut messages),
                    &frame,
                );
            }
        }
        let fed = !messages.is_empty();
        for m in messages {
            drop(app.update(m));
        }
        std::thread::sleep(Duration::from_millis(30));
        if images::take_dirty() || fed || step < 12 {
            quiet = 0;
        } else {
            quiet += 1;
            if quiet >= 10 {
                break;
            }
        }
    }
    let mut view = app.view();
    let t = tree.get_or_insert_with(|| Tree::new(view.as_widget()));
    t.diff(view.as_widget());
    let node = view.as_widget_mut().layout(t, renderer, &layout::Limits::new(size, size));
    renderer.reset(frame);
    let theme = app.iced_theme();
    view.as_widget().draw(t, renderer, &theme, &Default::default(), Layout::new(&node), cur, &frame);
    Headless::screenshot(renderer, Size::new(size.width as u32, size.height as u32), 1.0, Color::BLACK)
}

fn sizes() -> Vec<Size> {
    match super::shots::forced_size() {
        Some(s) => vec![s],
        None => vec![Size::new(1366.0, 768.0), Size::new(960.0, 600.0)],
    }
}

/// Centre of row `pos` (display order) of the Library's track list.
fn library_row_y(app: &App, pos: usize) -> f32 {
    super::page_area(app).y
        + super::PAGE_TOP
        + app.lib_bar_h
        + super::LIB_ACTIONS_H
        + rows::HEADER_H
        + pos as f32 * rows::ROW_PITCH
        + rows::ROW_PITCH / 2.0
        - app.page_scroll
}

/// Selected, playing and hovered track rows, apart and together, in both
/// themes: the selection (neutral ring) must never read as the playing row
/// (accent) nor as the hover (a wash without a ring).
#[test]
#[ignore = "requires an isolated profile and a wgpu adapter; writes PNGs for review"]
fn catalog_shots_row_states() {
    assert!(std::env::var_os("ABYSS_CONFIG_DIR").is_some() && std::env::var_os("ABYSS_CACHE_DIR").is_some());
    let _theme = theme::THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut renderer = renderer_new();
    let mut app = fixture();
    let id_at = |app: &App, pos: usize| app.lib_tracks[app.lib_shown[pos]].id;
    let ix_at = |app: &App, pos: usize| app.lib_shown[pos];
    for size in sizes() {
        app.viewport = size;
        for light in [false, true] {
            theme::set_light(light);
            let tag = format!("{}-{}x{}", if light { "light" } else { "dark" }, size.width, size.height);
            // Measure the Library bar first (its height places the rows).
            app.page_at = Instant::now() - Duration::from_secs(2);
            let _ = settle_with(&mut app, &mut renderer, size, None);
            // Playing = track 1001; selected = the row at 3; pointer over the row at 5.
            drop(app.update(Message::Row(rows::RowMsg::Select(ix_at(&app, 3), id_at(&app, 3)))));
            let hover = Point::new(size.width * 0.45, library_row_y(&app, 5));
            save(&format!("{tag}-rows-apart"), size, &settle_with(&mut app, &mut renderer, size, Some(hover)));
            // Selected + playing on the same row, the pointer over it.
            let playing = app.lib_shown.iter().position(|&i| app.lib_tracks[i].id == 1001).unwrap_or(0);
            drop(app.update(Message::Row(rows::RowMsg::Select(ix_at(&app, playing), 1001))));
            let hover = Point::new(size.width * 0.45, library_row_y(&app, playing));
            save(&format!("{tag}-rows-together"), size, &settle_with(&mut app, &mut renderer, size, Some(hover)));
            // Keyboard: Down twice moves the selection two rows on.
            for _ in 0..2 {
                drop(app.row_key(&iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown), iced::keyboard::Modifiers::empty()));
            }
            save(&format!("{tag}-rows-keyboard"), size, &settle_with(&mut app, &mut renderer, size, None));
        }
    }
    theme::set_light(false);
}

/// Context menus opened near every edge of the window stay inside it, above
/// the player bar, without scrolling when they fit.
#[test]
#[ignore = "requires an isolated profile and a wgpu adapter; writes PNGs for review"]
fn catalog_shots_menu_edges() {
    assert!(std::env::var_os("ABYSS_CONFIG_DIR").is_some() && std::env::var_os("ABYSS_CACHE_DIR").is_some());
    let _theme = theme::THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut renderer = renderer_new();
    let mut app = fixture();
    let track = app.lib_tracks[1].clone();
    for size in sizes() {
        app.viewport = size;
        for light in [false, true] {
            theme::set_light(light);
            let tag = format!("{}-{}x{}", if light { "light" } else { "dark" }, size.width, size.height);
            let (w, h) = (size.width, size.height);
            for (name, at, target) in [
                ("menu-mid", Point::new(w * 0.5, h * 0.5), MenuTarget::Track(track.clone())),
                ("menu-bottom-right", Point::new(w - 10.0, h - 90.0), MenuTarget::Track(track.clone())),
                ("menu-top-right", Point::new(w - 6.0, 70.0), MenuTarget::Track(track.clone())),
                ("menu-sort-right", Point::new(w - 30.0, 200.0), MenuTarget::Sort("tracks")),
                ("menu-output-bar", Point::new(w - 120.0, h - 30.0), MenuTarget::Output),
            ] {
                app.menu = Some((at, target));
                app.page_at = Instant::now() - Duration::from_secs(2);
                save(&format!("{tag}-{name}"), size, &settle_with(&mut app, &mut renderer, size, None));
            }
            app.menu = None;
        }
    }
    theme::set_light(false);
}

/// The login screen and the four welcome steps, plus a catalogue page that
/// failed to load (centred where its dots were), in both themes.
#[test]
#[ignore = "requires an isolated profile and a wgpu adapter; writes PNGs for review"]
fn catalog_shots_session_and_states() {
    assert!(std::env::var_os("ABYSS_CONFIG_DIR").is_some() && std::env::var_os("ABYSS_CACHE_DIR").is_some());
    let _theme = theme::THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut renderer = renderer_new();
    let mut app = fixture();
    for size in sizes() {
        app.viewport = size;
        for light in [false, true] {
            theme::set_light(light);
            let tag = format!("{}-{}x{}", if light { "light" } else { "dark" }, size.width, size.height);
            app.signed_in = false;
            app.page = crate::shared::Page::Home;
            app.welcome_step = None;
            save(&format!("{tag}-login"), size, &settle_with(&mut app, &mut renderer, size, None));
            for step in 0..4 {
                app.welcome_step = Some(step);
                save(&format!("{tag}-welcome-{step}"), size, &settle_with(&mut app, &mut renderer, size, None));
            }
            app.welcome_step = None;
            app.signed_in = true;
            app.albums.insert(999, Load::Failed("HTTP 503: the service is unavailable".into()));
            drop(app.update(Message::Navigate(crate::shared::Page::Album(999))));
            app.page_at = Instant::now() - Duration::from_secs(2);
            save(&format!("{tag}-failed-album"), size, &settle_with(&mut app, &mut renderer, size, None));
            drop(app.update(Message::Navigate(crate::shared::Page::MyTracks)));
            // Quick search while "aurorab" loads: "aurora"'s results stay, dimmed.
            app.search.insert(
                "aurora".into(),
                Load::Ready(crate::tidal::SearchResults {
                    top: None,
                    tracks: app.lib_tracks[..5].to_vec(),
                    albums: Vec::new(),
                    artists: Vec::new(),
                    playlists: Vec::new(),
                }),
            );
            app.search_open = true;
            app.search_query = "aurorab".into();
            save(&format!("{tag}-search-stale"), size, &settle_with(&mut app, &mut renderer, size, None));
            app.search_open = false;
            app.search_query.clear();
        }
    }
    theme::set_light(false);
}
