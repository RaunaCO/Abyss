//! Abyss: an unofficial native TIDAL client in Rust, with an Iced interface on wgpu.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Engine parts the retired GPUI interface used and Iced does not (yet) are kept.
#[allow(dead_code)]
mod art;
#[allow(dead_code)]
mod audio;
#[allow(dead_code)]
mod config;
mod discord;
mod export;
mod history;
mod i18n;
#[cfg(feature = "ui-iced")]
mod iced_ui;
mod local;
mod queue_store;
mod ratings;
mod recents;
mod romanize;
mod scrobble;
mod secure;
#[allow(dead_code)]
mod shared;
mod storage;
#[allow(dead_code)]
mod tidal;
mod updates;

#[cfg(not(feature = "ui-iced"))]
compile_error!("Abyss needs its interface: build with the default `ui-iced` feature");

/// Crash reports must not carry signed links or tokens: query strings of URLs
/// and anything after "Bearer " are replaced.
fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut line = line.to_string();
        if let Some(i) = line.find("Bearer ") {
            line.truncate(i + 7);
            line.push_str("[redacted]");
        }
        let mut cleaned = String::new();
        let mut rest = line.as_str();
        while let Some(q) = rest.find("http").and_then(|h| rest[h..].find('?').map(|q| h + q)) {
            cleaned.push_str(&rest[..q]);
            cleaned.push_str("?[redacted]");
            let tail = &rest[q + 1..];
            let end = tail.find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ')').unwrap_or(tail.len());
            rest = &tail[end..];
        }
        cleaned.push_str(rest);
        out.push_str(&cleaned);
        out.push('\n');
    }
    out
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("Abyss=info,tide=info")).init();
    i18n::set_language(None);
    if let Err(error) = config::prepare_config() {
        log::warn!("{}", i18n::text("migration.failed", &[error.to_string()]));
    }
    // A panic inside a window callback aborts the process without a trace; keep the message.
    std::panic::set_hook(Box::new(|info| {
        let text = format!(
            "{info}
{}
",
            std::backtrace::Backtrace::force_capture()
        );
        let text = redact(&text);
        let dir = config::config_dir();
        let _ = std::fs::create_dir_all(&dir);
        // Keep the previous report as crash.old.log (two reports at most).
        let _ = std::fs::rename(dir.join("crash.log"), dir.join("crash.old.log"));
        let _ = std::fs::write(dir.join("crash.log"), &text);
        eprintln!("{text}");
    }));

    // Diagnostics: `Abyss --probe <track id>` prints what TIDAL offers.
    let args: Vec<String> = std::env::args().collect();
    // Diagnostics: `Abyss --seekprobe [track id]` plays through the real engine at
    // volume 0 and seeks forward, then back, printing positions and engine errors.
    if args.get(1).map(String::as_str) == Some("--seekprobe") {
        let settings = std::sync::Arc::new(parking_lot::Mutex::new(config::Settings::load()));
        let t = tidal::Tidal::new(settings.clone());
        let id: u64 = args
            .get(2)
            .and_then(|a| a.parse().ok())
            .unwrap_or_else(|| t.favorite_tracks().ok().and_then(|f| f.first().map(|x| x.id)).unwrap_or(0));
        let s = settings.lock().clone();
        let (player, events) = audio::Player::new(
            t.http().clone(),
            0.0,
            audio::OutputPrefs {
                device: s.output_device.clone(),
                exclusive: s.exclusive_mode,
                force_volume: s.force_volume,
                normalize: s.normalize,
            },
        );
        let info = match t.stream(id, s.quality) {
            Ok(i) => i,
            Err(e) => return println!("stream FAILED: {e:#}"),
        };
        println!("track {id}: {} {}", info.tier(), info.detail());
        player.send(audio::Cmd::Play { key: 1, source: audio::Source::Stream(info), start: 0.0, paused: false });
        let started = std::time::Instant::now();
        let mut plan: Vec<(f64, f64)> = vec![(12.0, 120.0), (18.0, 30.0), (24.0, 5.0), (30.0, 60.0), (34.0, 40.0)];
        while started.elapsed().as_secs_f64() < 40.0 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let at = started.elapsed().as_secs_f64();
            if plan.first().is_some_and(|(when, _)| at >= *when) {
                let (_, to) = plan.remove(0);
                println!("{at:5.1}s  SEEK -> {to}");
                player.send(audio::Cmd::Seek(to));
            }
            let snap = player.snapshot();
            println!(
                "{at:5.1}s  key={:?} pos={:.2} dur={:.1} buffering={} out={}",
                snap.key, snap.position, snap.duration, snap.buffering, snap.output
            );
            while let Ok(ev) = events.try_recv() {
                match ev {
                    audio::Event::Error { key, message } => println!("      EVENT error key={key:?}: {message}"),
                    audio::Event::Finished { key } => println!("      EVENT finished key={key}"),
                }
            }
        }
        player.send(audio::Cmd::Stop);
        return;
    }
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
                println!(
                    "stream: {} {} {:?}",
                    info.tier(),
                    info.detail(),
                    match &info.source {
                        tidal::StreamSource::Single(_) => "single file".to_string(),
                        tidal::StreamSource::Segments(u) => format!("{} DASH segments", u.len()),
                    }
                );
                let mp4 = matches!(info.source, tidal::StreamSource::Segments(_)) || !info.codec.eq_ignore_ascii_case("flac");
                let start = std::time::Instant::now();
                // TIDE_PROBE_EXPIRE=1: break every link's signature first, so the
                // CDN refuses them and the stream must renew its links itself.
                let source = match (&info.source, std::env::var_os("TIDE_PROBE_EXPIRE")) {
                    (tidal::StreamSource::Segments(urls), Some(_)) => {
                        println!("probe: links deliberately invalidated");
                        tidal::StreamSource::Segments(urls.iter().map(|u| u.replace("Signature=", "Signature=X")).collect())
                    }
                    (s, _) => s.clone(),
                };
                let buf = audio::stream::start(t.http().clone(), source, info.seg_secs.clone(), info.renew.clone().map(|r| r.0));
                match audio::decode::Decoder::open(buf.clone(), mp4) {
                    Ok(mut d) => {
                        println!(
                            "decoder: {} Hz, {:?} bit, {} ch, duration {:?}, opened in {:?}",
                            d.sample_rate,
                            d.bits_per_sample,
                            d.channels,
                            d.duration,
                            start.elapsed()
                        );
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
                        println!(
                            "decoded {:.1} s, peak {peak:.3}, first audio after {:?}",
                            frames as f64 / d.sample_rate as f64,
                            start.elapsed()
                        );
                        let st = std::time::Instant::now();
                        match audio::do_seek(&mut d, &buf, mp4, 150.0) {
                            Ok(at) => println!("seek to 150 s -> {at:.2} s in {:?}", st.elapsed()),
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
                        // Going back, as dragging the progress bar to the left does.
                        for back in [30.0, 5.0] {
                            let st = std::time::Instant::now();
                            match audio::do_seek(&mut d, &buf, mp4, back) {
                                Ok(at) => {
                                    let mut got = 0usize;
                                    let mut err = None;
                                    for _ in 0..50 {
                                        match d.next_chunk() {
                                            Ok(Some(c)) => got += c.len() / 2,
                                            Ok(None) => break,
                                            Err(e) => {
                                                err = Some(format!("{e:#}"));
                                                break;
                                            }
                                        }
                                    }
                                    println!("seek back to {back} s -> {at:.2} s in {:?}, then {got} frames, error {err:?}", st.elapsed())
                                }
                                Err(e) => println!("seek back to {back} s FAILED: {e:#}"),
                            }
                        }
                    }
                    Err(e) => println!("decoder open FAILED: {e:#}"),
                }
                buf.cancel();
            }
            Err(e) => println!("stream FAILED: {e:#}"),
        }
        return;
    }

    i18n::set_language(config::Settings::load().language.as_deref());

    #[cfg(feature = "ui-iced")]
    {
        // DirectX 12 unless the user chose: on an RX 6800 XT, Vulkan (wgpu's first choice)
        // took about 29 s of CPU in the driver at each start and five times the video
        // memory (docs/MIGRATION-ICED.md). Iced only takes the backend from this variable.
        #[cfg(windows)]
        if std::env::var_os("WGPU_BACKEND").is_none() {
            std::env::set_var("WGPU_BACKEND", "dx12");
        }
        if let Err(error) = iced_ui::run() {
            log::error!("the interface could not start: {error}");
            let report = redact(&format!("Abyss could not start its window: {error}\n"));
            let dir = config::config_dir();
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("startup.log"), &report);
            eprintln!("{report}");
            // The Windows build has no console, so without this the failure is invisible.
            let _ = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("Abyss")
                .set_description(report.trim())
                .set_buttons(rfd::MessageButtons::Ok)
                .show();
            std::process::exit(1);
        }
    }
}
