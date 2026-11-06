//! TIDE: an unofficial native TIDAL client in Rust + GPUI.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod tidal;
mod ui;

use gpui::{prelude::*, px, size, App, Application, Bounds, SharedString, TitlebarOptions, WindowBounds, WindowOptions};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("tide=info")).init();

    // Diagnostics: `TIDE --probe <track id>` prints what TIDAL offers.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--probe") {
        let settings = std::sync::Arc::new(parking_lot::Mutex::new(config::Settings::load()));
        let t = tidal::Tidal::new(settings);
        let by_search = args.get(2).filter(|a| a.parse::<u64>().is_err()).and_then(|q| {
            let r = t.search(q).ok()?;
            let pick = r.tracks.first()?;
            println!("track {} «{}» tags={:?} quality={}", pick.id, pick.title, pick.tags, pick.audio_quality);
            Some(pick.id)
        });
        let id: u64 = by_search.or_else(|| args.get(2).and_then(|a| a.parse().ok())).unwrap_or_else(|| {
            let favs = t.favorite_tracks().unwrap_or_default();
            let pick = favs.iter().find(|x| x.is_hires()).or(favs.first()).expect("no favourite tracks");
            println!("track {} «{}» tags={:?} quality={}", pick.id, pick.title, pick.tags, pick.audio_quality);
            pick.id
        });
        if let Some(track) = t.favorite_tracks().ok().and_then(|f| f.into_iter().find(|x| x.id == id)) {
            match t.lyrics(id) {
                Ok(l) => println!("lyrics TIDAL: {} synced lines, {} chars", l.synced.len(), l.text.len()),
                Err(e) => println!("lyrics TIDAL: none ({e:#})"),
            }
            match t.lyrics_any(&track) {
                Ok(l) => println!("lyrics best: provider={} synced={} chars={}", l.provider, l.synced.len(), l.text.len()),
                Err(e) => println!("lyrics best: none ({e:#})"),
            }
        }
        match t.credits(id) {
            Ok(c) => println!("credits: {} roles {:?}", c.len(), c.iter().take(4).map(|(r, n)| (r.as_str(), n.len())).collect::<Vec<_>>()),
            Err(e) => println!("credits ERROR: {e:#}"),
        }
        match t.stream(id, config::Quality::Max) {
            Ok(i) => println!("replay gain: {:?} dB, peak {:?}", i.replay_gain, i.peak),
            Err(e) => println!("stream ERROR: {e:#}"),
        }
        match t.my_playlists() {
            Ok(p) => println!("my_playlists: {} ({:?})", p.len(), p.iter().take(3).map(|x| &x.title).collect::<Vec<_>>()),
            Err(e) => println!("my_playlists ERROR: {e:#}"),
        }
        for name in ["explore", "genre_page"] {
            println!("--- page {name}");
            for line in t.page_modules(name) {
                println!("{line}");
            }
        }
        match t.page("home") {
            Ok(r) => println!("home rows: {} {:?}", r.len(), r.iter().take(4).map(|x| (&x.title, x.items.len())).collect::<Vec<_>>()),
            Err(e) => println!("home ERROR: {e:#}"),
        }
        for line in t.probe(id) {
            println!("{line}");
        }
        // Decode test: resolve in Max, download, decode 20 s, then seek.
        match t.stream(id, config::Quality::Max) {
            Ok(info) => {
                println!("stream: {} {} {:?}", info.tier(), info.detail(), match &info.source {
                    tidal::StreamSource::Single(_) => "single file".to_string(),
                    tidal::StreamSource::Segments(u) => format!("{} DASH segments", u.len()),
                });
                let mp4 = matches!(info.source, tidal::StreamSource::Segments(_)) || !info.codec.eq_ignore_ascii_case("flac");
                let start = std::time::Instant::now();
                let buf = audio::stream::start(t.http().clone(), info.source.clone());
                match audio::decode::Decoder::open(buf.clone(), mp4) {
                    Ok(mut d) => {
                        println!("decoder: {} Hz, {:?} bit, {} ch, duration {:?}, opened in {:?}", d.sample_rate, d.bits_per_sample, d.channels, d.duration, start.elapsed());
                        let mut frames = 0usize;
                        let mut peak = 0f32;
                        while frames < d.sample_rate as usize * 20 {
                            match d.next_chunk() {
                                Ok(Some(c)) => {
                                    peak = c.iter().fold(peak, |m, s| m.max(s.abs()));
                                    frames += c.len() / 2;
                                }
                                Ok(None) => break,
                                Err(e) => {
                                    println!("decode error: {e:#}");
                                    break;
                                }
                            }
                        }
                        println!("decoded {:.1} s, peak {peak:.3}, first audio after {:?}", frames as f64 / d.sample_rate as f64, start.elapsed());
                        match d.seek(120.0) {
                            Ok(at) => println!("seek to 120 s -> {at:.2} s"),
                            Err(e) => println!("seek failed (fallback will be used): {e:#}"),
                        }
                        let mut r = audio::decode::Resampler2::new(d.sample_rate, 48_000).unwrap();
                        let mut out = 0usize;
                        for _ in 0..50 {
                            if let Ok(Some(c)) = d.next_chunk() {
                                out += r.process(&c).len() / 2;
                            }
                        }
                        println!("resampler to 48 kHz produced {out} frames");
                    }
                    Err(e) => println!("decoder open FAILED: {e:#}"),
                }
                buf.cancel();
            }
            Err(e) => println!("stream FAILED: {e:#}"),
        }
        return;
    }

    Application::new().with_assets(ui::icons::Assets).run(|cx: &mut App| {
        // Bundled Inter (SIL Open Font License, assets/fonts/Inter-LICENSE.txt).
        let fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Regular.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Medium.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Bold.ttf")),
        ];
        if let Err(e) = cx.text_system().add_fonts(fonts) {
            log::warn!("could not load Inter: {e:#}");
        }
        cx.bind_keys(ui::input::bindings());
        cx.bind_keys(ui::bindings());
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("TIDE")),
                    appears_transparent: true,
                    ..Default::default()
                }),
                window_min_size: Some(size(px(960.), px(600.))),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| ui::AppView::new(window, cx)),
        )
        .expect("open window");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
