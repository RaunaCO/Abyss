//! Screenshot helper: `TIDE_PAGE=<name>` opens the app straight on a
//! page, so layouts can be compared without sending clicks to any window.
//! Playback is always started paused (silent).

use std::time::Duration;

use gpui::Context;

use super::{AppView, Page, QItem};

impl AppView {
    pub fn demo_from_env(&mut self, cx: &mut Context<Self>) {
        let Ok(name) = std::env::var("TIDE_PAGE") else { return };
        cx.spawn(async move |this, cx| {
            // Let the library finish loading first.
            cx.background_executor().timer(Duration::from_secs(if name == "albums-open" { 9 } else { 4 })).await;
            let _ = this.update(cx, |this, cx| this.demo_go(&name, cx));
        })
        .detach();
    }

    fn demo_go(&mut self, name: &str, cx: &mut Context<Self>) {
        match name {
            "explore" => self.navigate(Page::Explore, cx),
            "genre" => self.navigate(Page::Browse("genre_hip_hop".into(), "Hip-Hop".into()), cx),
            "tracks" => self.navigate(Page::MyTracks, cx),
            "albums" => self.navigate(Page::MyAlbums, cx),
            "albums-open" => {
                self.navigate(Page::MyAlbums, cx);
                if let Some(a) = self.fav_albums.ready().and_then(|l| l.iter().take(12).max_by_key(|a| a.number_of_tracks)).cloned() {
                    self.toggle_album_inline(&a, cx);
                }
            }
            "artists" => self.navigate(Page::MyArtists, cx),
            "playlists" => self.navigate(Page::MyPlaylists, cx),
            "mixes" => self.navigate(Page::MyMixes, cx),
            "settings" => self.navigate(Page::Settings, cx),
            "menu" => self.app_menu = Some(vec![2, 7]),
            "playlist" => {
                if let Some(p) = self.my_playlists.ready().and_then(|l| l.first()).cloned() {
                    self.navigate(Page::Playlist(p.uuid), cx);
                }
            }
            "album" => {
                if let Some(a) = self.fav_albums.ready().and_then(|l| l.first()).cloned() {
                    self.navigate(Page::Album(a.id), cx);
                }
            }
            "nowplaying" | "nowplaying-lyrics" | "nowplaying-credits" | "nowplaying-queue" => {
                let tab = name.to_string();
                self.fetch(cx, |t| t.favorite_tracks(), move |this, r, cx| {
                    let Ok(tracks) = r else { return };
                    let tracks: Vec<_> = tracks.into_iter().filter(|t| t.streamable).skip(std::env::var("TIDE_DEMO_TRACK").ok().and_then(|v| v.parse().ok()).unwrap_or(0)).take(30).collect();
                    if tracks.is_empty() {
                        return;
                    }
                    this.queue = tracks
                        .into_iter()
                        .map(|track| {
                            let uid = this.new_uid();
                            QItem { uid, track }
                        })
                        .collect();
                    this.play_index(0, 0.0, true, cx);
                    if std::env::var("TIDE_DEMO_PLAY").is_ok() {
                        // Silent playback (volume 0) to watch the visual levels.
                        cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(std::time::Duration::from_secs(5)).await;
                            let _ = this.update(cx, |this, _| {
                                this.player.send(crate::audio::Cmd::Volume(0.0));
                                this.player.send(crate::audio::Cmd::Resume);
                            });
                            loop {
                            cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
                            let _ = this.update(cx, |this, _| {
                                log::info!("levels raw={:?} vis={:?} paused={}", this.player.levels(), this.vis, this.snap.paused);
                            });
                            }
                        })
                        .detach();
                    }
                    this.toggle_now_playing(cx);
                    if let Some((x, y)) = std::env::var("TIDE_TILT").ok().and_then(|v| {
                        let (a, b) = v.split_once(',')?;
                        Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?))
                    }) {
                        this.np_tilt.point(x, y);
                    }
                    this.np_tab = match tab.as_str() {
                        "nowplaying-credits" => super::now_playing::NpTab::Credits,
                        "nowplaying-queue" => {
                            this.np_queue_side = true;
                            super::now_playing::NpTab::Lyrics
                        }
                        _ => super::now_playing::NpTab::Lyrics,
                    };
                    if let Some(id) = this.now.as_ref().map(|n| n.track.id) {
                        this.load_lyrics(id, cx);
                    }
                });
            }
            _ => {}
        }
    }
}
