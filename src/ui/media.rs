//! System media controls: Windows SMTC (media keys, the volume flyout and the
//! lock screen), MPRIS on Linux. Events arrive on another thread and are
//! picked up by the UI tick.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection};

pub enum MediaAction {
    Toggle,
    Play,
    Pause,
    Next,
    Prev,
    Stop,
    SeekTo(f64),
    SeekBy(f64),
}

pub struct Media {
    controls: MediaControls,
    rx: Receiver<MediaControlEvent>,
    shown: Option<u64>,
    playing: Option<bool>,
    last_pos: Instant,
}

impl Media {
    pub fn new(window: &gpui::Window) -> Option<Self> {
        #[cfg(windows)]
        let hwnd = {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let handle = HasWindowHandle::window_handle(window).ok()?;
            match handle.as_raw() {
                RawWindowHandle::Win32(h) => Some(h.hwnd.get() as *mut std::ffi::c_void),
                _ => None,
            }
        };
        #[cfg(not(windows))]
        let hwnd = {
            let _ = window;
            None
        };
        let config = PlatformConfig { dbus_name: "abyss", display_name: crate::i18n::t("app.name"), hwnd };
        let mut controls = match MediaControls::new(config) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("media controls unavailable: {e:?}");
                return None;
            }
        };
        let (tx, rx) = mpsc::channel();
        if let Err(e) = controls.attach(move |ev| {
            let _ = tx.send(ev);
        }) {
            log::warn!("media controls attach failed: {e:?}");
            return None;
        }
        Some(Self { controls, rx, shown: None, playing: None, last_pos: Instant::now() })
    }

    pub fn poll(&mut self) -> Vec<MediaAction> {
        let mut out = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            out.push(match ev {
                MediaControlEvent::Toggle => MediaAction::Toggle,
                MediaControlEvent::Play => MediaAction::Play,
                MediaControlEvent::Pause => MediaAction::Pause,
                MediaControlEvent::Next => MediaAction::Next,
                MediaControlEvent::Previous => MediaAction::Prev,
                MediaControlEvent::Stop => MediaAction::Stop,
                MediaControlEvent::SetPosition(MediaPosition(d)) => MediaAction::SeekTo(d.as_secs_f64()),
                MediaControlEvent::Seek(SeekDirection::Forward) => MediaAction::SeekBy(10.0),
                MediaControlEvent::Seek(SeekDirection::Backward) => MediaAction::SeekBy(-10.0),
                MediaControlEvent::SeekBy(SeekDirection::Forward, d) => MediaAction::SeekBy(d.as_secs_f64()),
                MediaControlEvent::SeekBy(SeekDirection::Backward, d) => MediaAction::SeekBy(-d.as_secs_f64()),
                _ => continue,
            });
        }
        out
    }

    /// Keep the system widget in sync with what is playing.
    pub fn update(&mut self, now: Option<(u64, &crate::tidal::Track)>, playing: bool, position: f64) {
        let Some((key, track)) = now else {
            if self.shown.take().is_some() {
                let _ = self.controls.set_playback(MediaPlayback::Stopped);
                self.playing = None;
            }
            return;
        };
        if self.shown != Some(key) {
            self.shown = Some(key);
            let cover = track.cover(640);
            let artist = track.artist_names();
            let title = track.display_title();
            let album = track.album.as_ref().map(|a| a.title.clone());
            let _ = self.controls.set_metadata(MediaMetadata {
                title: Some(&title),
                artist: Some(&artist),
                album: album.as_deref(),
                cover_url: cover.as_deref(),
                duration: Some(Duration::from_secs(track.duration as u64)),
            });
            self.playing = None;
        }
        let refresh = self.last_pos.elapsed() > Duration::from_secs(5);
        if self.playing != Some(playing) || refresh {
            self.playing = Some(playing);
            self.last_pos = Instant::now();
            let progress = Some(MediaPosition(Duration::from_secs_f64(position.max(0.0))));
            let _ =
                self.controls.set_playback(if playing { MediaPlayback::Playing { progress } } else { MediaPlayback::Paused { progress } });
        }
    }
}
