//! Queue and playback control: the same rules as the GPUI reference
//! (`src/ui/mod.rs`), driven by messages instead of a GPUI context. The audio
//! engine, its clock and WASAPI output are `crate::audio`, untouched.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use iced::Task;

use super::{App, Message, NowPlaying, MAX_PLAY_FAILURES};
use crate::audio::{self, Cmd, OutputPrefs};
use crate::config::{Quality, Repeat};
use crate::shared::queue::{playable_start, restore_queue_order, shuffle};
use crate::shared::{into_load, now_secs, Load, QItem};
use crate::tidal::Track;

impl App {
    pub(super) fn new_uid(&mut self) -> u64 {
        self.next_uid += 1;
        self.next_uid
    }

    pub(super) fn queue_fingerprint(&self) -> u64 {
        crate::queue_store::fingerprint(self.queue.iter().map(|q| q.track.id))
    }

    /// Keep `queue.json` up to date: at most every 20 s while things change, or
    /// right away with `now` (closing). Writes only when something moved.
    pub(super) fn save_queue(&mut self, now: bool) {
        if self.queue_restoring || (!now && self.queue_saved_at.elapsed() < Duration::from_secs(20)) {
            return;
        }
        self.queue_saved_at = Instant::now();
        let (index, position) = match (&self.now, self.resume_at) {
            (Some(n), _) if !self.ended => (n.index, self.snap.position),
            (_, Some(resume)) => resume,
            (Some(n), None) => (n.index, 0.0),
            (None, None) => (0, 0.0),
        };
        let state = (self.queue_fingerprint(), index, position as i64);
        if state == self.queue_saved {
            return;
        }
        let tracks: Vec<Track> = self.queue.iter().map(|q| q.track.clone()).collect();
        match crate::queue_store::save(&tracks, index, position) {
            Ok(()) => self.queue_saved = state,
            Err(error) => log::warn!("could not save the queue: {error}"),
        }
    }

    pub(super) fn flush_history(&mut self) {
        if let Err(error) = self.recorder.flush() {
            log::warn!("No se pudo guardar Replay al cerrar Abyss: {error:#}");
        }
    }

    /// Play `tracks`, starting with `tracks[start]` (as shown). Unplayable
    /// tracks are left out of the queue; the start is mapped to the chosen
    /// track's place in what remains, so the track that plays is the one picked.
    pub fn play_tracks(&mut self, tracks: Vec<Track>, start: usize) -> Task<Message> {
        if tracks.is_empty() {
            return Task::none();
        }
        self.play_failures = 0;
        let flags: Vec<bool> = tracks.iter().map(|t| t.streamable).collect();
        let Some(start) = playable_start(&flags, start) else {
            // Never play a different track instead of an unavailable one.
            self.notify_toast(crate::i18n::t("this_track_is_unavailable_for_playback"));
            return Task::none();
        };
        let tracks: Vec<Track> = tracks.into_iter().filter(|t| t.streamable).collect();
        log::info!("play_tracks: «{}» (queue position {start} of {})", tracks[start].title, tracks.len());
        self.autoplay_jump = false;
        self.queue = tracks.into_iter().map(|track| QItem { uid: 0, track }).collect();
        for i in 0..self.queue.len() {
            self.queue[i].uid = self.new_uid();
        }
        self.unshuffled = None;
        let mut index = start;
        if self.shuffle {
            self.unshuffled = Some(self.queue.clone());
            let first = self.queue.remove(start);
            shuffle(&mut self.queue);
            self.queue.insert(0, first);
            index = 0;
        }
        self.play_index(index, 0.0, false)
    }

    pub fn play_shuffled(&mut self, tracks: Vec<Track>) -> Task<Message> {
        let playable: Vec<usize> = (0..tracks.len()).filter(|&i| tracks[i].streamable).collect();
        if playable.is_empty() {
            return Task::none();
        }
        self.shuffle = true;
        let start = playable[(now_secs() as usize) % playable.len()];
        let task = self.play_tracks(tracks, start);
        self.save_settings();
        task
    }

    pub fn play_index(&mut self, index: usize, start: f64, paused: bool) -> Task<Message> {
        let Some(item) = self.queue.get(index).cloned() else { return Task::none() };
        self.resume_at = None;
        self.next_key += 1;
        let key = self.next_key;
        self.queued_next = None;
        self.ended = false;
        self.player.send(Cmd::ClearQueue);
        self.now = Some(NowPlaying { track: item.track.clone(), info: None, key, index });
        self.reset_lyric_follow();
        self.next_info = None;
        self.snap.position = start;
        self.snap_at = Instant::now();
        let mut tasks = Vec::new();
        if let Some(path) = item.track.local_path.clone() {
            self.snap.paused = paused;
            self.snap.buffering = true;
            self.player.send(Cmd::Play { key, source: audio::Source::Local(path), start, paused });
            if self.wants_lyrics() {
                tasks.push(self.load_lyrics(item.track.id));
            }
            return Task::batch(tasks);
        }
        crate::recents::record(&mut self.recents, &item.track);
        let quality = self.settings.lock().quality;
        let id = item.track.id;
        tasks.push(self.fetch(
            move |t| t.stream(id, quality),
            move |app, r| {
                let Some(now) = app.now.as_mut().filter(|n| n.key == key) else { return Task::none() };
                match r {
                    Ok(info) => {
                        now.info = Some(info.clone());
                        app.player.send(Cmd::Play { key, source: audio::Source::Stream(info), start, paused });
                        Task::none()
                    }
                    Err(e) => {
                        app.notify_toast(crate::i18n::text("could_not_play", &[item.track.title.to_string(), format!("{e:#}")]));
                        app.skip_after_error(key)
                    }
                }
            },
        ));
        if self.wants_lyrics() {
            tasks.push(self.load_lyrics(id));
        }
        Task::batch(tasks)
    }

    fn skip_after_error(&mut self, key: u64) -> Task<Message> {
        let Some(now) = &self.now else { return Task::none() };
        if now.key != key {
            return Task::none();
        }
        let idx = now.index;
        // Without a limit, an unplugged drive walks the whole queue.
        self.play_failures += 1;
        if self.play_failures >= MAX_PLAY_FAILURES {
            self.play_failures = 0;
            self.ended = true;
            self.player.send(Cmd::Stop);
            self.notify_toast_for(crate::i18n::text("playback.stopped_after_errors", &[MAX_PLAY_FAILURES.to_string()]), 10);
            return Task::none();
        }
        if idx + 1 < self.queue.len() {
            self.play_index(idx + 1, 0.0, false)
        } else {
            self.ended = true;
            self.player.send(Cmd::Stop);
            Task::none()
        }
    }

    fn next_index(&self, from: usize) -> Option<usize> {
        match self.repeat {
            Repeat::One => Some(from),
            _ if from + 1 < self.queue.len() => Some(from + 1),
            Repeat::All if !self.queue.is_empty() => Some(0),
            _ => None,
        }
    }

    /// Resolve and hand the next track to the engine for gapless playback.
    fn queue_next(&mut self) -> Task<Message> {
        let Some(now) = &self.now else { return Task::none() };
        let Some(idx) = self.next_index(now.index) else {
            return self.autoplay_more();
        };
        let Some(item) = self.queue.get(idx).cloned() else { return Task::none() };
        let mut index = now.index;
        for _ in 0..2 {
            let Some(next) = self.next_index(index).filter(|&next| next != index) else { break };
            let Some(track) = self.queue.get(next).map(|item| &item.track) else { break };
            if let Some(cover) = track.cover(160) {
                super::images::sized(&cover, 42.0, 42.0);
            }
            if let Some(cover) = track.cover(640) {
                super::images::backdrop(&cover);
            }
            index = next;
        }
        self.next_key += 1;
        let key = self.next_key;
        self.queued_next = Some((key, idx));
        self.next_info = None;
        if let Some(path) = item.track.local_path {
            self.player.send(Cmd::Queue { key, source: audio::Source::Local(path) });
            return Task::none();
        }
        let quality = self.settings.lock().quality;
        self.fetch(
            move |t| t.stream(item.track.id, quality),
            move |app, r| {
                if app.queued_next.map(|q| q.0) != Some(key) {
                    return Task::none();
                }
                match r {
                    Ok(info) => {
                        app.next_info = Some((key, info.clone()));
                        app.player.send(Cmd::Queue { key, source: audio::Source::Stream(info) });
                    }
                    Err(_) => app.queued_next = None,
                }
                Task::none()
            },
        )
    }

    /// The queue changed around the current track: re-queue the next one.
    fn requeue(&mut self) {
        if self.queued_next.take().is_some() {
            self.player.send(Cmd::ClearQueue);
        }
    }

    /// End of the queue: append the radio of the last track (autoplay).
    fn autoplay_more(&mut self) -> Task<Message> {
        if self.autoplay_loading || !self.settings.lock().autoplay || self.repeat != Repeat::Off {
            return Task::none();
        }
        let Some(seed) = self.queue.last().filter(|q| q.track.local_path.is_none()) else { return Task::none() };
        let seed_uid = seed.uid;
        let id = seed.track.id;
        self.autoplay_loading = true;
        self.fetch(
            move |t| t.track_radio(id),
            move |app, r| {
                app.autoplay_loading = false;
                if app.queue.last().map(|q| q.uid) != Some(seed_uid) {
                    return Task::none();
                }
                let Ok(tracks) = r else { return Task::none() };
                let known: HashSet<u64> = app.queue.iter().map(|q| q.track.id).collect();
                let fresh: Vec<Track> = tracks.into_iter().filter(|t| t.streamable && !known.contains(&t.id)).take(25).collect();
                if fresh.is_empty() {
                    return Task::none();
                }
                for track in fresh {
                    let uid = app.new_uid();
                    app.queue.push(QItem { uid, track });
                }
                if app.ended || std::mem::take(&mut app.autoplay_jump) {
                    // Playback stopped, or the user pressed next: continue right away.
                    if let Some(i) = app.now.as_ref().map(|n| n.index + 1) {
                        return app.play_index(i, 0.0, false);
                    }
                    Task::none()
                } else {
                    app.queue_next()
                }
            },
        )
    }

    pub fn next(&mut self) -> Task<Message> {
        let Some(now) = &self.now else { return Task::none() };
        let idx = now.index;
        let next = if idx + 1 < self.queue.len() {
            Some(idx + 1)
        } else if self.repeat == Repeat::All {
            Some(0)
        } else {
            None
        };
        match next {
            Some(n) => self.play_index(n, 0.0, false),
            None if now.track.local_path.is_none() => {
                // The user asked for the next track: jump as soon as autoplay has one.
                self.autoplay_jump = true;
                self.autoplay_more()
            }
            None => Task::none(),
        }
    }

    pub fn prev(&mut self) -> Task<Message> {
        let Some(now) = &self.now else { return Task::none() };
        if self.snap.position > 3.0 || now.index == 0 {
            self.seek(0.0)
        } else {
            let i = now.index - 1;
            self.play_index(i, 0.0, false)
        }
    }

    pub fn toggle_play(&mut self) -> Task<Message> {
        self.play_failures = 0;
        if self.now.is_none() || self.ended {
            let i = self.now.as_ref().map(|n| n.index).unwrap_or(0);
            if !self.queue.is_empty() {
                // A queue restored at startup continues where it was left.
                let (index, start) = match self.resume_at {
                    Some((r, pos)) if self.now.is_none() && r < self.queue.len() => (r, pos),
                    _ => (if self.ended { i } else { 0 }, 0.0),
                };
                return self.play_index(index, start, false);
            }
            return Task::none();
        }
        if self.snap.paused {
            self.player.send(Cmd::Resume);
        } else {
            // Freeze the shown position where the music stops.
            self.snap.position = self.display_position();
            self.player.send(Cmd::Pause);
        }
        self.snap.paused = !self.snap.paused;
        self.snap_at = Instant::now();
        Task::none()
    }

    pub fn seek(&mut self, secs: f64) -> Task<Message> {
        self.player.send(Cmd::Seek(secs));
        self.snap.position = secs;
        self.snap_at = Instant::now();
        self.seek_hold = Some((secs, Instant::now()));
        Task::none()
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        self.muted = false;
        self.player.send(Cmd::Volume(self.volume));
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        self.player.send(Cmd::Volume(if self.muted { 0.0 } else { self.volume }));
    }

    pub fn toggle_shuffle(&mut self) -> Task<Message> {
        self.shuffle = !self.shuffle;
        let current = self.now.as_ref().map(|n| n.index).filter(|&i| i < self.queue.len());
        let current_uid = current.map(|i| self.queue[i].uid);
        let resume = self.resume_at.and_then(|(i, position)| self.queue.get(i).map(|q| (q.uid, position)));
        if self.shuffle {
            self.unshuffled = Some(self.queue.clone());
            shuffle(&mut self.queue[current.map_or(0, |i| i + 1)..]);
        } else if let Some(original) = self.unshuffled.take() {
            self.queue = restore_queue_order(original, std::mem::take(&mut self.queue));
            if let (Some(uid), Some(now)) = (current_uid, self.now.as_mut()) {
                if let Some(index) = self.queue.iter().position(|q| q.uid == uid) {
                    now.index = index;
                }
            }
        }
        self.resume_at = resume.and_then(|(uid, position)| self.queue.iter().position(|q| q.uid == uid).map(|i| (i, position)));
        self.requeue();
        self.save_settings();
        Task::none()
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
        self.requeue();
        self.save_settings();
    }

    pub fn add_to_queue(&mut self, track: Track, next: bool) -> Task<Message> {
        let item = QItem { uid: self.new_uid(), track };
        if let Some(original) = &mut self.unshuffled {
            let anchor = self.now.as_ref().and_then(|n| self.queue.get(n.index));
            let at = if next {
                anchor.and_then(|q| original.iter().position(|item| item.uid == q.uid)).map_or(original.len(), |i| i + 1)
            } else {
                original.len()
            };
            original.insert(at, item.clone());
        }
        match (&self.now, next) {
            (Some(now), true) => {
                let at = (now.index + 1).min(self.queue.len());
                self.queue.insert(at, item);
            }
            _ => self.queue.push(item),
        }
        let task = if self.now.is_none() { self.play_index(0, 0.0, false) } else { Task::none() };
        self.requeue();
        self.notify_toast(if next { crate::i18n::t("playing_next") } else { crate::i18n::t("added_to_queue") });
        task
    }

    pub fn remove_from_queue(&mut self, index: usize) {
        let Some(now) = &mut self.now else {
            if index < self.queue.len() {
                self.queue.remove(index);
                self.resume_at = self.resume_at.and_then(|(i, position)| {
                    if index == i {
                        (!self.queue.is_empty()).then(|| (i.min(self.queue.len() - 1), 0.0))
                    } else {
                        Some((i - usize::from(index < i), position))
                    }
                });
            }
            return;
        };
        if index == now.index || index >= self.queue.len() {
            return;
        }
        self.queue.remove(index);
        if index < now.index {
            now.index -= 1;
        }
        self.requeue();
    }

    /// Everything after the playing track leaves the queue.
    pub fn clear_queue(&mut self) {
        match self.now.as_ref().map(|n| n.index) {
            Some(i) if i < self.queue.len() => {
                self.queue.truncate(i + 1);
                if let Some(orig) = &mut self.unshuffled {
                    let keep: HashSet<u64> = self.queue.iter().map(|q| q.uid).collect();
                    orig.retain(|q| keep.contains(&q.uid));
                }
            }
            _ => {
                self.queue.clear();
                self.unshuffled = None;
                self.resume_at = None;
            }
        }
        self.requeue();
    }

    pub fn start_radio(&mut self, track: &Track) -> Task<Message> {
        let first = track.clone();
        let id = track.id;
        self.fetch(
            move |t| t.track_radio(id),
            move |app, r| match r {
                Ok(mut tracks) => {
                    tracks.retain(|t| t.id != first.id);
                    tracks.insert(0, first);
                    app.play_tracks(tracks, 0)
                }
                Err(e) => {
                    app.notify_toast(crate::i18n::text("radio_unavailable", &[format!("{e:#}")]));
                    Task::none()
                }
            },
        )
    }

    pub fn set_quality(&mut self, q: Quality) -> Task<Message> {
        {
            let mut s = self.settings.lock();
            s.quality = q;
            s.save();
        }
        self.requeue();
        // Restart the current track at the same position in the new quality.
        if let Some(now) = self.now.as_ref().filter(|n| n.track.local_path.is_none()) {
            let (i, pos, paused, key) = (now.index, self.snap.position, self.snap.paused, now.key);
            let task = self.play_index(i, pos, paused);
            if let Some(now) = &self.now {
                self.recorder.rekey(key, now.key);
                self.scrobble_tracker.rekey(key, now.key);
            }
            return task;
        }
        Task::none()
    }

    pub fn set_output(&mut self, device: Option<String>, exclusive: bool, force_volume: bool) {
        {
            let mut s = self.settings.lock();
            s.output_device = device.clone();
            s.exclusive_mode = exclusive;
            s.force_volume = force_volume;
            s.save();
        }
        let normalize = self.settings.lock().normalize;
        self.player.send(Cmd::Output(OutputPrefs { device, exclusive, force_volume, normalize }));
        self.player.send(Cmd::Volume(if self.muted { 0.0 } else { self.volume }));
    }

    pub fn toggle_setting(&mut self, which: &str) -> Task<Message> {
        {
            let mut s = self.settings.lock();
            match which {
                "normalize" => s.normalize = !s.normalize,
                "autoplay" => s.autoplay = !s.autoplay,
                "discord" => s.discord = !s.discord,
                "audio_metadata" => s.audio_metadata = !s.audio_metadata,
                "show_ratings" => s.show_ratings = !s.show_ratings,
                "lyrics_romanize" => s.lyrics_romanize = !s.lyrics_romanize,
                "external_lyrics" => s.external_lyrics = !s.external_lyrics,
                "np_blur" => s.np_blur = !s.np_blur,
                "exclusive_mode" => s.exclusive_mode = !s.exclusive_mode,
                "force_volume" => s.force_volume = !s.force_volume,
                "hide_uploads" => {
                    s.hide_uploads = !s.hide_uploads;
                    if s.hide_uploads && self.home_tab == 2 {
                        self.home_tab = 0;
                    }
                }
                _ => {}
            }
            s.save();
        }
        let mut task = Task::none();
        match which {
            "external_lyrics" => {
                self.lyrics_epoch = self.lyrics_epoch.wrapping_add(1);
                self.lyrics.clear();
                self.lyrics_docs.clear();
                if self.wants_lyrics() {
                    if let Some(id) = self.now.as_ref().map(|n| n.track.id) {
                        task = self.load_lyrics(id);
                    }
                }
            }
            "lyrics_romanize" => self.lyrics_docs.clear(),
            "discord" => {
                let on = self.settings.lock().discord;
                self.discord.set_enabled(on);
            }
            "normalize" | "exclusive_mode" | "force_volume" => {
                let s = self.settings.lock().clone();
                self.player.send(Cmd::Output(OutputPrefs {
                    device: s.output_device,
                    exclusive: s.exclusive_mode,
                    force_volume: s.force_volume,
                    normalize: s.normalize,
                }));
                self.player.send(Cmd::Volume(if self.muted { 0.0 } else { self.volume }));
            }
            _ => {}
        }
        task
    }

    // --------------------------------------------------------------- tick

    pub(super) fn tick(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        self.save_queue(false);
        // Ratings the database refused (busy, unavailable) wait in a journal,
        // and a start that could only read the mirror reads the database again:
        // every 10 s, never on every tick (a busy database blocks up to 750 ms).
        if self.ratings.needs_retry() {
            static LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
            let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
            if last.is_none_or(|at| at.elapsed() >= Duration::from_secs(10)) {
                *last = Some(Instant::now());
                self.ratings.retry_pending();
            }
        }
        // Media keys / system media widget.
        let actions = self.media.as_mut().map(|m| m.poll()).unwrap_or_default();
        for a in actions {
            tasks.push(match a {
                MediaAction::Toggle => self.toggle_play(),
                MediaAction::Play if self.snap.paused || self.ended => self.toggle_play(),
                MediaAction::Pause | MediaAction::Stop if !self.snap.paused => self.toggle_play(),
                MediaAction::Next => self.next(),
                MediaAction::Prev => self.prev(),
                MediaAction::SeekTo(s) => self.seek(s),
                MediaAction::SeekBy(d) => {
                    let p = (self.snap.position + d).clamp(0.0, self.duration());
                    self.seek(p)
                }
                _ => Task::none(),
            });
        }
        let mut finished = None;
        while let Ok(ev) = self.events.try_recv() {
            match ev {
                audio::Event::Finished { key } => {
                    if self.now.as_ref().is_some_and(|now| now.key == key) {
                        finished = Some(key);
                    }
                }
                audio::Event::Error { key, message } => {
                    self.notify_toast(message);
                    if let Some(k) = key {
                        tasks.push(self.skip_after_error(k));
                    }
                }
            }
        }

        let snap = self.player.snapshot();
        // A track is really playing: the failure streak is over.
        if self.play_failures > 0 && !snap.paused && !snap.buffering && snap.position > 1.0 && snap.key == self.now.as_ref().map(|n| n.key)
        {
            self.play_failures = 0;
        }
        if let Some(error) = crate::local::take_save_error() {
            self.notify_toast(crate::i18n::text("could_not_save_the_local_library", &[error]));
        }
        // The engine moved on to the queued track (gapless transition).
        if let (Some(k), Some((qk, qi))) = (snap.key, self.queued_next) {
            if k == qk {
                if let Some(item) = self.queue.get(qi).cloned() {
                    let info = self.next_info.take().filter(|(key, _)| *key == qk).map(|(_, i)| i);
                    log::info!(
                        "gapless transition to track {} ({}) at output position {:.2} s",
                        item.track.id,
                        info.as_ref().map(|i| i.detail()).unwrap_or_default(),
                        snap.position
                    );
                    self.now = Some(NowPlaying { track: item.track, info, key: qk, index: qi });
                    self.reset_lyric_follow();
                    self.queued_next = None;
                    if self.wants_lyrics() {
                        let id = self.now.as_ref().map(|n| n.track.id).unwrap_or_default();
                        tasks.push(self.load_lyrics(id));
                    }
                    tasks.push(self.load_np_data());
                }
            }
        }
        if let Some(now) = self.now.as_ref().filter(|now| Some(now.key) == finished) {
            // The engine drained before the next track was ready.
            if let Some(index) = self.next_index(now.index) {
                tasks.push(self.play_index(index, 0.0, false));
            } else {
                self.ended = true;
            }
        }
        // Queue the next track once the current one is downloaded or near its end.
        if let Some(now) = &self.now {
            if !self.ended && snap.key == Some(now.key) && self.queued_next.is_none() {
                let dur = if snap.duration > 0.0 { snap.duration } else { now.track.duration as f64 };
                if snap.downloaded >= 0.999 || dur - snap.position < 25.0 {
                    tasks.push(self.queue_next());
                }
            }
        }
        // A seek just sent: the engine may still report where it was (it can
        // sit up to 200 ms flushing the output), and the thumb would jump back
        // for a tick. Keep the target until the engine is there (or 1.5 s).
        let held = self.seek_hold.is_some_and(|(target, at)| {
            let since = at.elapsed().as_secs_f64();
            since < 1.5 && snap.key == self.snap.key && (snap.position - target).abs() > 1.0 + since
        });
        if !held {
            self.seek_hold = None;
        }
        if held {
            let (position, at) = (self.snap.position, self.snap_at);
            self.snap = snap;
            self.snap.position = position;
            self.snap_at = at;
        } else if self.drag != Some(super::Drag::Progress) {
            // The engine's playback clock is the reference: the shown position
            // is extrapolated from its latest reading until the next one.
            self.snap = snap;
            self.snap_at = Instant::now();
        } else {
            let pos = self.snap.position;
            self.snap = snap;
            self.snap.position = pos;
        }
        // Listening history: only seconds actually heard count.
        {
            let dt = self.history_tick.elapsed().as_secs_f32();
            self.history_tick = Instant::now();
            let track = self.now.as_ref().filter(|n| !self.ended && n.track.local_path.is_none());
            let audible = track.is_some_and(|n| Some(n.key) == self.snap.key) && !self.snap.paused && !self.snap.buffering;
            if let Some(batch) = self.recorder.tick(&mut self.history, track.map(|n| (n.key, &n.track)), audible, dt) {
                tasks.push(Task::perform(
                    super::blocking(move || crate::history::save_batch(&batch).map_err(|e| format!("{e:#}"))),
                    |result| {
                        Message::Done(super::Callback::new(move |app: &mut App| {
                            if app.recorder.saved(result.is_ok()) {
                                if let Err(error) = result {
                                    log::warn!("No se pudo guardar Replay: {error}");
                                    app.notify_toast(crate::i18n::t("replay.save_retry"));
                                }
                            }
                            Task::none()
                        }))
                    },
                ));
            }
        }
        self.lastfm_tick_run();
        if let Some((_, at)) = &self.toast {
            if at.elapsed() > Duration::from_secs(4) {
                self.toast = None;
            }
        }
        {
            let playing = self.is_playing();
            let dur = self.duration();
            let pos = self.snap.position;
            self.discord.update(self.now.as_ref().filter(|n| n.track.local_path.is_none()).map(|n| (&n.track, dur)), playing, pos);
        }
        if let Some(m) = self.media.as_mut() {
            let playing = self.now.is_some() && !self.snap.paused && !self.ended;
            m.update(self.now.as_ref().map(|n| (n.key, &n.track)), playing, self.snap.position);
        }
        self.step_spin();
        self.track_colors();
        self.trim_caches();
        // Ambient motion never rests while music plays (see `shared::activity`).
        crate::shared::activity::set_playing(self.now.is_some() && !self.snap.paused && !self.ended);
        Task::batch(tasks)
    }

    // --------------------------------------------------------------- Last.fm

    pub fn lastfm_apply(&self) {
        let s = self.settings.lock();
        self.scrobbler.configure(s.lastfm_session.clone(), s.lastfm_enabled);
    }

    pub fn lastfm_connected(&self) -> bool {
        self.settings.lock().lastfm_session.is_some()
    }

    fn lastfm_tick_run(&mut self) {
        use crate::scrobble::tracker::Event;
        let dt = self.lastfm_tick.elapsed().as_secs_f32();
        self.lastfm_tick = Instant::now();
        let active = {
            let s = self.settings.lock();
            s.lastfm_enabled && s.lastfm_session.is_some()
        };
        if active {
            let track = self.now.as_ref().filter(|_| !self.ended);
            let audible = track.is_some_and(|n| Some(n.key) == self.snap.key) && !self.snap.paused && !self.snap.buffering;
            let events = self.scrobble_tracker.tick(track.map(|n| (n.key, &n.track)), audible, dt, crate::history::unix_now());
            for event in events {
                match event {
                    Event::NowPlaying(meta) => self.scrobbler.now_playing(meta),
                    Event::Scrobble(item) => self.scrobbler.scrobble(item),
                }
            }
        }
        let (version, new_session, invalid, synced) = {
            let mut status = self.scrobbler.status.lock();
            if status.version == self.lastfm_seen {
                return;
            }
            (status.version, status.new_session.take(), std::mem::take(&mut status.session_invalid), std::mem::take(&mut status.synced))
        };
        self.lastfm_seen = version;
        if let Some((user, key)) = new_session {
            {
                let mut s = self.settings.lock();
                s.lastfm_user = Some(user);
                s.lastfm_session = Some(key);
                s.lastfm_enabled = true;
                s.save();
            }
            self.lastfm_apply();
        }
        if invalid {
            {
                let mut s = self.settings.lock();
                s.lastfm_session = None;
                s.lastfm_enabled = false;
                s.save();
            }
            self.lastfm_apply();
            self.notify_toast(crate::i18n::t("lastfm.session_expired"));
        }
        if synced {
            self.notify_toast(crate::i18n::t("lastfm.sync_done"));
        }
    }

    pub fn lastfm_status_text(&self) -> String {
        use crate::scrobble::worker::Phase;
        let status = self.scrobbler.status.lock().clone();
        let user = self.settings.lock().lastfm_user.clone();
        match status.phase {
            Phase::Connecting => crate::i18n::t("lastfm.status_connecting").to_string(),
            Phase::Syncing => {
                crate::i18n::text("lastfm.status_syncing", &[status.done.min(status.total).to_string(), status.total.to_string()])
            }
            Phase::Idle => match (&status.error, user) {
                (Some(error), _) => crate::i18n::text("lastfm.status_error", std::slice::from_ref(error)),
                (None, Some(user)) => crate::i18n::text("lastfm.status_on", &[user]),
                (None, None) => crate::i18n::t("lastfm.status_off").to_string(),
            },
        }
    }

    pub fn lastfm_connect(&mut self) {
        if !crate::scrobble::configured() {
            self.notify_toast(crate::i18n::t("lastfm.not_available"));
            return;
        }
        self.scrobbler.connect();
    }

    pub fn lastfm_disconnect(&mut self) {
        {
            let mut s = self.settings.lock();
            s.lastfm_session = None;
            s.lastfm_user = None;
            s.lastfm_enabled = false;
            s.save();
        }
        self.lastfm_apply();
    }

    pub fn lastfm_toggle(&mut self) {
        {
            let mut s = self.settings.lock();
            if s.lastfm_session.is_none() {
                return;
            }
            s.lastfm_enabled = !s.lastfm_enabled;
            s.save();
        }
        self.lastfm_apply();
    }

    pub fn lastfm_sync(&mut self) {
        if self.lastfm_connected() && !self.settings.lock().lastfm_enabled {
            self.lastfm_toggle();
        }
        self.scrobbler.sync_history();
    }

    // ---------------------------------------------------------- favourites

    pub fn toggle_favorite(&mut self, track: &Track) -> Task<Message> {
        if track.local_path.is_some() {
            return Task::none();
        }
        let id = track.id;
        let fav = !self.fav_ids.contains(&id);
        if fav {
            self.heart_pop = Some((id, Instant::now()));
            self.fav_ids.insert(id);
            if let Load::Ready(list) = &mut self.fav_tracks {
                let mut t = track.clone();
                t.added = Some(crate::local::now_stamp());
                list.insert(0, t);
            }
        } else {
            self.fav_ids.remove(&id);
            if let Load::Ready(list) = &mut self.fav_tracks {
                list.retain(|t| t.id != id);
            }
        }
        self.rebuild_library();
        self.fetch(
            move |t| t.set_favorite_track(id, fav),
            move |app, r| {
                match r {
                    Err(e) => app.notify_toast(crate::i18n::text("error", &[format!("{e:#}")])),
                    Ok(()) => app.notify_toast(if fav {
                        crate::i18n::t("added_to_my_collection")
                    } else {
                        crate::i18n::t("removed_from_my_collection")
                    }),
                }
                Task::none()
            },
        )
    }

    pub fn toggle_favorite_album(&mut self, album: &crate::tidal::Album) -> Task<Message> {
        let id = album.id;
        let fav = !self.fav_album_ids.contains(&id);
        if fav {
            self.fav_album_ids.insert(id);
        } else {
            self.fav_album_ids.remove(&id);
        }
        self.fav_albums = Load::Idle;
        self.fetch(
            move |t| t.set_favorite_album(id, fav),
            move |app, r| {
                if let Err(e) = r {
                    app.notify_toast(crate::i18n::text("error", &[format!("{e:#}")]));
                }
                app.load_page_data()
            },
        )
    }

    pub fn toggle_favorite_artist(&mut self, artist: &crate::tidal::Artist) -> Task<Message> {
        let id = artist.id;
        let fav = !self.fav_artist_ids.contains(&id);
        if fav {
            self.fav_artist_ids.insert(id);
        } else {
            self.fav_artist_ids.remove(&id);
        }
        self.fav_artists = Load::Idle;
        self.fetch(
            move |t| t.set_favorite_artist(id, fav),
            move |app, r| {
                match r {
                    Ok(()) => app.notify_toast(if fav { crate::i18n::t("following_artist") } else { crate::i18n::t("unfollowed_artist") }),
                    Err(e) => app.notify_toast(crate::i18n::text("error", &[format!("{e:#}")])),
                }
                app.load_page_data()
            },
        )
    }

    pub fn set_rating(&mut self, track: u64, halves: u8) {
        // The stars show the new value whatever happens; only when neither the
        // database nor the journal could keep it is there something to say.
        if let Err(error) = self.ratings.set(track, halves) {
            log::warn!("No se pudo guardar la valoración de pista: {error:#}");
            self.notify_toast(crate::i18n::t("could_not_save_the_track_rating"));
        }
        self.apply_sorts();
    }

    pub fn set_album_rating(&mut self, album: u64, halves: u8) {
        if let Err(error) = self.ratings.set_album(album, halves) {
            log::warn!("No se pudo guardar la valoración de álbum: {error:#}");
            self.notify_toast(crate::i18n::t("could_not_save_the_album_rating"));
        }
        self.apply_sorts();
    }

    pub fn load_credits(&mut self, id: u64) -> Task<Message> {
        if self.credits.contains_key(&id) {
            return Task::none();
        }
        self.credits.insert(id, Load::Loading);
        self.fetch(
            move |t| t.credits(id),
            move |app, r| {
                app.credits.insert(id, into_load(r));
                Task::none()
            },
        )
    }
}

// ------------------------------------------------------------ media controls

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

/// System media controls: Windows SMTC (media keys, the volume flyout and the
/// lock screen), MPRIS on Linux.
pub struct Media {
    controls: souvlaki::MediaControls,
    rx: std::sync::mpsc::Receiver<souvlaki::MediaControlEvent>,
    shown: Option<u64>,
    playing: Option<bool>,
    last_pos: Instant,
}

impl Media {
    pub fn new(hwnd: Option<isize>) -> Option<Self> {
        use souvlaki::{MediaControls, PlatformConfig};
        #[cfg(windows)]
        let hwnd = Some(hwnd? as *mut std::ffi::c_void);
        #[cfg(not(windows))]
        let hwnd = {
            let _ = hwnd;
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
        let (tx, rx) = std::sync::mpsc::channel();
        if let Err(e) = controls.attach(move |ev| {
            let _ = tx.send(ev);
            super::wake();
        }) {
            log::warn!("media controls attach failed: {e:?}");
            return None;
        }
        Some(Self { controls, rx, shown: None, playing: None, last_pos: Instant::now() })
    }

    pub fn poll(&mut self) -> Vec<MediaAction> {
        use souvlaki::{MediaControlEvent, MediaPosition, SeekDirection};
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

    pub fn update(&mut self, now: Option<(u64, &Track)>, playing: bool, position: f64) {
        use souvlaki::{MediaMetadata, MediaPlayback, MediaPosition};
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
