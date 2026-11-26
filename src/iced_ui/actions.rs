//! What every interface message does (the rest of `App::update`).

use std::time::Instant;

use iced::Task;

use super::{App, Drag, Message, NpTab};
use crate::shared::{Load, Page};

impl App {
    pub(super) fn update_more(&mut self, message: Message) -> Task<Message> {
        match message {
            // ------------------------------------------------ navigation
            Message::Navigate(page) => self.navigate(page),
            Message::Back => self.go_back(),
            Message::Forward => self.go_forward(),
            Message::OpenItem(item) => {
                self.menu = None;
                self.search_open = false;
                self.open_item(&item)
            }
            Message::PlayItem(item) => {
                self.menu = None;
                self.search_open = false;
                self.play_item(&item)
            }
            Message::LibraryCat(section) => self.set_library_cat(section),
            Message::LibraryBarSized(height) => {
                self.lib_bar_h = height;
                Task::none()
            }
            Message::InlineAlbumSized(id, height) => {
                if self.expanded_album == Some(id) {
                    self.expanded_album_height = height;
                }
                Task::none()
            }
            Message::RetryPage => self.retry_page(),
            Message::RetryInlineAlbum(album) => self.retry_inline_album(&album),
            Message::LibGrid(section, grid) => {
                self.set_lib_grid(section, grid);
                Task::none()
            }
            Message::Filter(text) => {
                if let Some(sec) = super::library::lib_section(&self.page) {
                    self.lib_queries.insert(sec, text.clone());
                }
                self.filter = text;
                if super::library::lib_section(&self.page).is_some() {
                    self.refresh_lib_shown();
                }
                // The matches are a new list: show it from its start.
                self.page_scroll = 0.0;
                iced::widget::operation::scroll_to(super::pages::PAGE_SCROLL, iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: 0.0 })
            }
            Message::SearchInput(text) => self.search_typed(text),
            Message::SearchSubmit => {
                let q = self.search_query.clone();
                self.search_all(q)
            }
            Message::SearchClose => {
                self.search_open = false;
                Task::none()
            }
            Message::SearchRecent(q) => {
                self.search_query = q.clone();
                self.search_all(q)
            }
            Message::PageScrolled(y, h) => {
                self.page_scroll = y;
                self.page_view_h = h;
                Task::none()
            }
            Message::ToggleAlbumInline(album) => self.toggle_album_inline(&album),

            // ------------------------------------------------ playback
            Message::TogglePlay => self.toggle_play(),
            Message::Next => self.next(),
            Message::Prev => self.prev(),
            Message::SeekBy(d) => {
                if self.now.is_none() {
                    return Task::none();
                }
                let p = (self.snap.position + d).clamp(0.0, self.duration().max(0.0));
                self.seek(p)
            }
            Message::ProgressDrag(frac) => {
                if self.now.is_none() {
                    return Task::none();
                }
                if self.drag != Some(Drag::Progress) {
                    self.drag = Some(Drag::Progress);
                    self.step_spin();
                }
                self.drag_value = frac;
                self.snap.position = frac as f64 * self.duration();
                Task::none()
            }
            Message::ProgressRelease => {
                if self.drag != Some(Drag::Progress) {
                    return Task::none();
                }
                self.drag = None;
                let secs = self.drag_value as f64 * self.duration();
                let t = self.seek(secs);
                self.step_spin();
                t
            }
            Message::VolumeDrag(v) => {
                self.drag = Some(Drag::Volume);
                self.volume_tip = Some(std::time::Instant::now());
                self.set_volume(v);
                Task::none()
            }
            Message::VolumeRelease => {
                self.drag = None;
                self.volume_tip = Some(std::time::Instant::now());
                self.save_settings();
                Task::none()
            }
            Message::VolumeStep(d) => {
                let base = if self.muted { 0.0 } else { self.volume };
                let pct = ((base * 100.0).round() + (d * 100.0).round()).clamp(0.0, 100.0);
                self.set_volume(pct / 100.0);
                self.volume_tip = Some(std::time::Instant::now());
                self.save_settings();
                Task::none()
            }
            Message::ToggleMute => {
                self.toggle_mute();
                Task::none()
            }
            Message::ToggleShuffle => self.toggle_shuffle(),
            Message::CycleRepeat => {
                self.cycle_repeat();
                Task::none()
            }
            Message::PlayTracks(tracks, start) => {
                self.menu = None;
                self.search_open = false;
                self.play_tracks(tracks.as_ref().clone(), start)
            }
            Message::PlayFrom(source, start) => {
                let tracks = self.source_tracks(&source);
                self.play_tracks(tracks, start)
            }
            Message::PlayAll(source) => {
                let tracks = self.source_tracks(&source);
                // Play means in order, as in GPUI; Shuffle is the button beside it.
                self.shuffle = false;
                self.play_tracks(tracks, 0)
            }
            Message::ShuffleAll(source) => {
                let tracks = self.source_tracks(&source);
                self.play_shuffled(tracks)
            }
            Message::Row(msg) => self.row_update(msg),
            Message::PlayShuffled(tracks) => {
                self.menu = None;
                self.play_shuffled(tracks.as_ref().clone())
            }
            Message::PlayQueued(uid) => {
                self.menu = None;
                match self.queue.iter().position(|q| q.uid == uid) {
                    Some(i) if self.now.as_ref().is_none_or(|n| n.index != i) => self.play_index(i, 0.0, false),
                    _ => Task::none(),
                }
            }
            Message::RemoveQueued(uid) => {
                self.menu = None;
                if let Some(i) = self.queue.iter().position(|q| q.uid == uid) {
                    self.remove_from_queue(i);
                }
                Task::none()
            }
            Message::AddToQueue(track, next) => {
                self.menu = None;
                self.add_to_queue(track, next)
            }
            Message::StartRadio(track) => {
                self.menu = None;
                self.start_radio(&track)
            }
            Message::ClearQueue => {
                self.menu = None;
                self.clear_queue();
                Task::none()
            }

            // ------------------------------------------------ panels
            Message::ToggleNowPlaying => self.toggle_now_playing(),
            Message::NpTab(tab) => {
                self.np_tab = tab;
                if tab == NpTab::Lyrics {
                    self.lyrics_follow = true;
                }
                self.load_np_data()
            }
            Message::ToggleLyrics => self.toggle_lyrics(),
            Message::ToggleQueue => {
                if self.now_playing_open {
                    self.np_tab = NpTab::Queue;
                    return self.load_np_data();
                }
                self.show_queue = !self.show_queue;
                if self.show_queue {
                    self.show_lyrics = false;
                }
                Task::none()
            }
            Message::CloseSidePanel => {
                self.show_queue = false;
                self.show_lyrics = false;
                Task::none()
            }
            Message::WideSidePanel => {
                let mut s = self.settings.lock();
                s.side_panel_width = None;
                s.wide_side_panel = !s.wide_side_panel;
                s.save();
                Task::none()
            }
            Message::LyricsManual => {
                self.lyrics_follow = false;
                self.lyrics_sync_at = None;
                Task::none()
            }
            Message::LyricsSync => {
                if self.lyrics_sync_at.is_none() {
                    self.lyrics_follow = true;
                    self.lyrics_sync_at = Some(Instant::now());
                }
                Task::none()
            }
            Message::LyricLine(at) => {
                self.lyrics_follow = true;
                self.seek(at)
            }
            Message::RetryLyrics => {
                if let Some(track) = self.now.as_ref().map(|n| n.track.clone()) {
                    crate::tidal::lyrics::forget_missing(&track);
                    self.lyrics.remove(&track.id);
                    self.lyrics_docs.remove(&track.id);
                    return self.load_lyrics(track.id);
                }
                Task::none()
            }
            Message::CoverDoubleClick => {
                let Some(track) = self.now.as_ref().map(|n| n.track.clone()) else { return Task::none() };
                if track.local_path.is_some() {
                    return Task::none();
                }
                self.heart_burst = Some(Instant::now());
                if !self.fav_ids.contains(&track.id) {
                    return self.toggle_favorite(&track);
                }
                Task::none()
            }
            Message::OpenCover(url) => {
                if !url.is_empty() {
                    self.cover_view = Some(url);
                }
                Task::none()
            }
            Message::CloseCover => {
                self.cover_view = None;
                Task::none()
            }
            Message::OpenInfo(target) => self.open_info(target),
            Message::AlbumToPlaylist(id, uuid) => self.add_album_to_playlist(id, uuid),
            Message::QueueAlbum(id, next) => self.queue_album(id, next),
            Message::CloseInfo => {
                self.info_panel = None;
                Task::none()
            }

            // ------------------------------------------------ collection
            Message::ToggleFavorite(track) => {
                self.menu = None;
                self.toggle_favorite(&track)
            }
            Message::ToggleFavoriteAlbum(album) => {
                self.menu = None;
                self.toggle_favorite_album(&album)
            }
            Message::ToggleFavoriteArtist(artist) => {
                self.menu = None;
                self.toggle_favorite_artist(&artist)
            }
            Message::SetRating(id, halves) => {
                self.set_rating(id, halves);
                Task::none()
            }
            Message::SetAlbumRating(id, halves) => {
                self.set_album_rating(id, halves);
                Task::none()
            }
            Message::AddToPlaylist(uuid, tracks) => self.add_tracks_to_playlist(uuid, tracks),
            Message::OpenDialog(d) => self.open_dialog(d),
            Message::DialogTitle(t) => {
                self.dlg_title = t;
                Task::none()
            }
            Message::DialogDesc(t) => {
                self.dlg_desc = t;
                Task::none()
            }
            Message::DialogSubmit => self.submit_dialog(),
            Message::DialogClose => {
                self.dialog = None;
                Task::none()
            }
            Message::ToggleSavedPlaylist(playlist) => self.toggle_saved_playlist(playlist),

            // ------------------------------------------------ menus
            Message::OpenMenu(super::MenuTarget::Sort(section)) => super::menus::open_sort_at_button(section),
            Message::OpenSortMenu(section, bounds) => {
                let at = super::menus::sort_menu_origin(bounds, self.viewport.height, super::library::sort_options(section).len());
                self.menu = Some((at, super::MenuTarget::Sort(section)));
                Task::none()
            }
            Message::OpenMenu(target) => {
                if matches!(target, super::MenuTarget::Output) {
                    self.devices = crate::audio::output::device_names();
                }
                // Inside a menu (e.g. "Add to playlist") the new one opens where the old one was.
                let at = self
                    .menu
                    .as_ref()
                    .map(|(p, _)| *p)
                    .filter(|_| matches!(target, super::MenuTarget::AddToPlaylist(_) | super::MenuTarget::AddAlbumToPlaylist(_)))
                    .unwrap_or_else(super::chrome::cursor);
                self.menu = Some((at, target));
                Task::none()
            }
            Message::CloseMenu => {
                // A click elsewhere: the menu fades out rather than vanishing
                // (a chosen option or Esc closes it at once, the answer is the action).
                self.menu_exit = self.menu.take().map(|(at, target)| (at, target, Instant::now()));
                Task::none()
            }
            Message::CopyLink(url) => {
                self.menu = None;
                self.notify_toast(crate::i18n::t("link_copied"));
                iced::clipboard::write(url)
            }
            Message::OpenUrl(url) => {
                let _ = open::that(url);
                Task::none()
            }
            Message::SidebarHover(on) => {
                // GPUI folds the sidebar only 150 ms after the pointer left.
                let delay = if on { std::time::Duration::ZERO } else { super::motion::SIDEBAR_CLOSE_DELAY };
                self.sidebar_reveal.set_after(on, delay);
                Task::none()
            }

            // ------------------------------------------------ settings
            Message::SettingsTab(tab) => {
                self.settings_tab = tab;
                if tab == super::settings::EQ_TAB {
                    let _ = self.player.take_peaks();
                    self.eq_meter = Default::default();
                }
                Task::none()
            }
            Message::SetQuality(q) => self.set_quality(q),
            Message::ToggleSetting(which) => {
                self.menu = None;
                self.toggle_setting(which)
            }
            Message::SetLight(light) => {
                self.set_light_theme(light);
                self.paint_border();
                Task::none()
            }
            Message::SetAccent(rgb) => {
                self.set_accent(rgb);
                Task::none()
            }
            Message::StepZoom(0) => Task::none(),
            Message::StepZoom(dir) => self.step_zoom(dir),
            Message::AccentHue(hue) => {
                self.preview_accent(super::settings::hue_color(hue));
                Task::none()
            }
            Message::AccentSave => {
                self.settings.lock().save();
                Task::none()
            }
            Message::ToggleOutputList => {
                self.output_list_open = !self.output_list_open;
                Task::none()
            }
            Message::SetOutput(device, exclusive, force) => {
                self.menu = None;
                self.output_list_open = false;
                self.set_output(device, exclusive, force);
                Task::none()
            }
            Message::ChooseService(service) => {
                let mut s = self.settings.lock();
                s.preferred_service = service.to_string();
                s.save();
                Task::none()
            }
            Message::WelcomeBack => {
                self.welcome_step = self.welcome_step.map(|step| step.saturating_sub(1));
                Task::none()
            }
            Message::LastfmCancel => {
                self.scrobbler.cancel();
                Task::none()
            }
            Message::SetLanguage(code) => {
                {
                    let mut s = self.settings.lock();
                    s.language = code.clone();
                    s.save();
                }
                crate::i18n::set_language(code.as_deref());
                Task::none()
            }
            Message::SetSort(section, code) => {
                self.menu = None;
                self.set_sort(section, super::library::SortKey::from_code(&code));
                Task::none()
            }
            Message::ChooseLocal(folders) => {
                self.menu = None;
                self.choose_local_files(folders)
            }
            Message::LocalAdded(paths) => self.import_local_paths(paths),
            Message::RemoveLocalTrack(id) => {
                self.menu = None;
                self.remove_local_track(id);
                Task::none()
            }
            Message::RemoveLocalAlbum(id) => {
                self.menu = None;
                self.remove_local_album(id)
            }
            Message::RescanLocal => self.rescan_local_folders(),
            Message::CheckUpdates => {
                self.update_check = Some(Load::Loading);
                Task::perform(super::blocking(crate::updates::latest_release), |r| {
                    Message::Done(super::Callback::new(move |app: &mut App| {
                        app.update_check = Some(match r {
                            Ok(v) if crate::updates::is_newer(&v, env!("CARGO_PKG_VERSION")) => Load::Ready(Some(v)),
                            Ok(_) => Load::Ready(None),
                            Err(e) => {
                                log::warn!("update check failed: {e:#}");
                                Load::Failed(format!("{e:#}"))
                            }
                        });
                        Task::none()
                    }))
                })
            }
            Message::ExportData => {
                let settings = self.settings.lock().clone();
                self.notify_toast(crate::i18n::t("export.working"));
                Task::perform(
                    super::blocking(move || {
                        let parent = dirs::document_dir().unwrap_or_else(crate::config::config_dir);
                        crate::export::export_all(&parent, &settings).map_err(|e| format!("{e:#}"))
                    }),
                    |r| {
                        Message::Done(super::Callback::new(move |app: &mut App| {
                            match r {
                                Ok(dir) => {
                                    app.notify_toast_for(crate::i18n::text("export.done", &[dir.display().to_string()]), 8);
                                    let _ = open::that(dir);
                                }
                                Err(e) => app.notify_toast(crate::i18n::text("export.failed", &[e])),
                            }
                            Task::none()
                        }))
                    },
                )
            }
            Message::Replay(msg) => self.replay_update(msg),
            Message::LastfmConnect => {
                self.lastfm_connect();
                Task::none()
            }
            Message::LastfmDisconnect => {
                self.lastfm_disconnect();
                Task::none()
            }
            Message::LastfmToggle => {
                self.lastfm_toggle();
                Task::none()
            }
            Message::LastfmSync => {
                self.lastfm_sync();
                Task::none()
            }
            Message::DiscordId(text) => {
                let id: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
                {
                    let mut s = self.settings.lock();
                    s.discord_client_id = id.clone();
                    s.save();
                }
                self.discord.set_client_id(id);
                Task::none()
            }
            Message::Local(msg) => self.local_update(msg),
            Message::Eq(msg) => {
                self.eq_update(msg);
                Task::none()
            }

            // ------------------------------------------------ session
            Message::StartLogin => self.start_login(),
            Message::CancelLogin => {
                self.login = super::LoginState::Idle;
                Task::none()
            }
            Message::UseLocalOnly => {
                {
                    let mut s = self.settings.lock();
                    s.preferred_service = "local".into();
                    s.save();
                }
                self.signed_in = false;
                self.page = Page::MyTracks;
                self.reset_data();
                self.load_page_data()
            }
            Message::WelcomeNext => {
                let step = self.welcome_step.unwrap_or(0);
                if step >= 3 {
                    return self.finish_welcome();
                }
                self.welcome_step = Some(step + 1);
                Task::none()
            }
            Message::WelcomeSkip => self.finish_welcome(),
            Message::WelcomeRepeat => {
                self.welcome_step = Some(0);
                self.menu = None;
                self.search_open = false;
                if self.now_playing_open {
                    return self.toggle_now_playing();
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn finish_welcome(&mut self) -> Task<Message> {
        let local = {
            let mut s = self.settings.lock();
            s.welcome_complete = true;
            s.save();
            s.preferred_service == "local"
        };
        self.welcome_step = None;
        if local {
            // Saved service tokens are kept: choosing local never signs the account out.
            self.signed_in = false;
            self.page = Page::MyTracks;
            self.reset_data();
            return self.load_page_data();
        }
        if !self.signed_in && self.settings.lock().signed_in() {
            self.signed_in = true;
            self.page = Page::Home;
            return self.after_login();
        }
        self.load_page_data()
    }
}
