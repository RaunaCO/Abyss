//! Local music library and file picker.

use gpui::{div, prelude::*, AnyElement, Context, FontWeight, PathPromptOptions};

use super::{
    icons::Icon,
    sc, theme,
    widgets::{pill, widget_panel},
    AppView, Load, Page,
};
use crate::tidal::Track;

impl AppView {
    pub fn choose_local_files(&mut self, folders: bool, cx: &mut Context<Self>) {
        if self.local_busy {
            return;
        }
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: !folders,
            directories: folders,
            multiple: true,
            prompt: Some(if folders { crate::i18n::t("add_music_folder") } else { crate::i18n::t("add_audio_files") }.into()),
        });
        cx.spawn(async move |this, cx| {
            let choice = prompt.await;
            let _ = this.update(cx, |view, cx| match choice {
                Ok(Ok(Some(paths))) if !paths.is_empty() => view.import_local_paths(paths, cx),
                Ok(Err(_)) => view.notify_toast(crate::i18n::t("could_not_open_the_file_picker"), cx),
                _ => {}
            });
        })
        .detach();
    }

    /// Import files and folders (picker or drag and drop) straight into the
    /// Library: they are listed under Library › Tracks (and their albums under
    /// Albums) as soon as they are read. Folders are remembered and scanned
    /// again for new files at start-up.
    pub fn import_local_paths(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        if self.local_busy || paths.is_empty() {
            return;
        }
        let folders: Vec<_> = paths.iter().filter(|p| p.is_dir()).cloned().collect();
        self.local_busy = true;
        self.notify_toast(crate::i18n::t("adding_to_your_library"), cx);
        cx.notify();
        let known = self.local_tracks.iter().filter_map(|t| t.local_path.clone()).collect();
        cx.spawn(async move |this, cx| {
            let imported = cx
                .background_executor()
                .spawn(async move {
                    crate::local::remember_folders(&folders);
                    crate::local::import(paths, true, known)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.local_busy = false;
                let count = view.merge_local(imported.tracks, cx);
                let mut parts = vec![crate::i18n::text(
                    if count == 1 { "local.track_imported" } else { "local.tracks_imported" },
                    &[count.to_string()],
                )];
                if imported.duplicates > 0 {
                    parts.push(crate::i18n::text("already_present", &[format!("{}", imported.duplicates)]));
                }
                if imported.rejected > 0 {
                    parts.push(crate::i18n::text("unsupported", &[format!("{}", imported.rejected)]));
                }
                view.notify_toast(parts.join(" · "), cx);
                if count > 0 && view.page != Page::Settings {
                    // Show them where they now live: the Library, newest first.
                    view.set_library_cat("tracks", cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Look for new files in the remembered folders (in the background).
    pub fn rescan_local_folders(&mut self, cx: &mut Context<Self>) {
        let folders = crate::local::folders();
        if folders.is_empty() {
            return;
        }
        let known = self.local_tracks.iter().filter_map(|t| t.local_path.clone()).collect();
        cx.spawn(async move |this, cx| {
            let imported = cx.background_executor().spawn(async move { crate::local::import(folders, true, known) }).await;
            let _ = this.update(cx, |view, cx| {
                let count = view.merge_local(imported.tracks, cx);
                if count > 0 {
                    view.notify_toast(crate::i18n::text("new_files_from_your_folders_added_to_the_library", &[format!("{}", count)]), cx);
                }
            });
        })
        .detach();
    }

    /// Add imported tracks, save the index and refresh the Library.
    fn merge_local(&mut self, tracks: Vec<Track>, cx: &mut Context<Self>) -> usize {
        // A startup folder scan can finish after a manual import of the same file.
        let mut known: std::collections::HashSet<_> = self.local_tracks.iter().map(|t| t.id).collect();
        let tracks: Vec<_> = tracks.into_iter().filter(|t| known.insert(t.id)).collect();
        let count = tracks.len();
        if count == 0 {
            return 0;
        }
        let mut combined = self.local_tracks.clone();
        combined.extend(tracks);
        if let Err(e) = crate::local::save(&combined) {
            self.notify_toast(crate::i18n::text("could_not_save_the_local_library", &[format!("{:#}", e)]), cx);
            return 0;
        }
        self.local_tracks = combined;
        self.local_changed();
        self.content_changed_pub();
        cx.notify();
        count
    }

    /// Rebuild the local albums (cached like TIDAL albums so they open in the
    /// album page and inline) and the Library's lists.
    pub fn local_changed(&mut self) {
        for a in &self.local_albums {
            self.albums.remove(&a.id);
        }
        let albums = crate::local::albums(&self.local_tracks);
        self.local_albums = albums.iter().map(|(a, _)| a.clone()).collect();
        for (album, tracks) in albums {
            self.albums.insert(album.id, Load::Ready((album, tracks)));
        }
        for artist in &self.local_artists {
            self.artists.remove(&artist.id);
        }
        let artists = crate::local::artists(&self.local_tracks, &self.local_albums);
        self.local_artists = artists.iter().map(|page| page.artist.clone()).collect();
        for page in artists {
            self.artists.insert(page.artist.id, Load::Ready(page));
        }
        self.apply_sorts();
        let searches: Vec<_> = self.search.iter().filter_map(|(q, r)| r.ready().map(|r| (q.clone(), r.clone()))).collect();
        for (q, r) in searches {
            let result = self.with_local_search(&q, r);
            self.search.insert(q, Load::Ready(result));
        }
    }

    /// Remove every track of a local album from the Library (files stay).
    pub fn remove_local_album(&mut self, album: u64, cx: &mut Context<Self>) {
        if self.local_busy {
            return;
        }
        let remaining: Vec<_> = self.local_tracks.iter().filter(|t| t.album.as_ref().map(|a| a.id) != Some(album)).cloned().collect();
        match crate::local::save(&remaining) {
            Ok(()) => {
                self.local_tracks = remaining;
                if self.local_editor.as_ref().is_some_and(|e| !self.local_tracks.iter().any(|t| t.id == e.id)) {
                    self.local_editor = None;
                }
                self.local_changed();
                self.albums.remove(&album);
                self.expanded_album = None;
                if self.page == Page::Album(album) {
                    self.set_library_cat("albums", cx);
                }
                self.content_changed_pub();
                self.notify_toast(crate::i18n::t("album_removed_from_library_files_kept"), cx);
            }
            Err(e) => self.notify_toast(crate::i18n::text("could_not_save_the_local_library", &[format!("{:#}", e)]), cx),
        }
    }

    pub fn remove_local_track(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.local_busy {
            return;
        }
        let remaining: Vec<_> = self.local_tracks.iter().filter(|t| t.id != id).cloned().collect();
        match crate::local::save(&remaining) {
            Ok(()) => {
                self.local_tracks = remaining;
                self.local_changed();
                self.content_changed_pub();
                self.notify_toast(crate::i18n::t("removed_from_library_file_kept"), cx);
            }
            Err(e) => self.notify_toast(crate::i18n::text("could_not_save_the_local_library", &[format!("{:#}", e)]), cx),
        }
    }
}

pub struct LocalEditor {
    pub id: u64,
    pub fields: Vec<gpui::Entity<super::input::TextInput>>,
    pub cover: Option<std::path::PathBuf>,
}

const EDIT_LABELS: [&str; 8] =
    ["title", "artist", "local.album_artist", "album", "local.genre", "local.year", "local.track_number", "local.disc_number"];

impl AppView {
    pub fn open_local_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_tab = super::settings::LOCAL_TAB;
        self.navigate(Page::Settings, cx);
    }

    pub fn edit_local_in_settings(&mut self, id: u64, cx: &mut Context<Self>) {
        self.open_local_settings(cx);
        self.begin_local_edit(id, cx);
    }

    fn begin_local_edit(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.local_busy {
            return;
        }
        let Some(track) = self.local_tracks.iter().find(|t| t.id == id).cloned() else { return };
        self.local_busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { crate::local::Edit::read(&track) }).await;
            let _ = this.update(cx, |v, cx| {
                v.local_busy = false;
                match result {
                    Ok(e) => {
                        let fields = [e.title, e.artist, e.album_artist, e.album, e.genre, e.year, e.track, e.disc]
                            .into_iter()
                            .map(|value| {
                                cx.new(|cx| {
                                    let mut input = super::input::TextInput::new("", cx);
                                    input.set_text(&value, cx);
                                    input
                                })
                            })
                            .collect();
                        v.local_editor = Some(LocalEditor { id, fields, cover: None });
                    }
                    Err(e) => v.notify_toast(crate::i18n::text("local.operation_failed", &[format!("{e:#}")]), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn choose_local_cover(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &self.local_editor else { return };
        let id = editor.id;
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(crate::i18n::t("local.choose_cover").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = prompt.await {
                let _ = this.update(cx, |v, cx| {
                    if let Some(editor) = v.local_editor.as_mut().filter(|e| e.id == id) {
                        editor.cover = paths.first().cloned();
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    fn write_local_edit(&mut self, id: u64, undo: bool, cx: &mut Context<Self>) {
        if self.local_busy {
            return;
        }
        // Warn before any write while Abyss has the file open, even when paused.
        if self.now.as_ref().is_some_and(|n| n.track.id == id) {
            self.notify_toast(crate::i18n::t("local.write_blocked"), cx);
            return;
        }
        let Some(track) = self.local_tracks.iter().find(|t| t.id == id).cloned() else { return };
        let edit = if undo {
            None
        } else {
            let Some(editor) = self.local_editor.as_ref().filter(|e| e.id == id) else { return };
            let f: Vec<_> = editor.fields.iter().map(|i| i.read(cx).text()).collect();
            Some(crate::local::Edit {
                title: f[0].clone(),
                artist: f[1].clone(),
                album_artist: f[2].clone(),
                album: f[3].clone(),
                genre: f[4].clone(),
                year: f[5].clone(),
                track: f[6].clone(),
                disc: f[7].clone(),
                cover: editor.cover.clone(),
            })
        };
        self.local_busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { crate::local::edit(&track, edit) }).await;
            let _ = this.update(cx, |v, cx| {
                v.local_busy = false;
                match result {
                    Ok(track) => {
                        for item in &mut v.local_tracks {
                            if item.id == id {
                                *item = track.clone();
                            }
                        }
                        for item in &mut v.queue {
                            if item.track.id == id {
                                item.track = track.clone();
                            }
                        }
                        if let Some(now) = v.now.as_mut().filter(|n| n.track.id == id) {
                            now.track = track.clone();
                        }
                        v.lyrics_epoch = v.lyrics_epoch.wrapping_add(1);
                        v.lyrics.clear();
                        crate::tidal::lyrics::forget_missing(&track);
                        let saved = crate::local::save(&v.local_tracks);
                        v.local_changed();
                        v.content_changed_pub();
                        v.local_editor = None;
                        match saved {
                            Ok(()) => v.notify_toast(crate::i18n::t(if undo { "local.restored" } else { "local.saved" }), cx),
                            Err(e) => v.notify_toast(
                                crate::i18n::text("local.operation_failed", &[format!("{}: {e:#}", crate::i18n::t("local.index_retry"))]),
                                cx,
                            ),
                        }
                    }
                    Err(e) => v.notify_toast(crate::i18n::text("local.operation_failed", &[format!("{e:#}")]), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn settings_local(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut body = div()
            .flex()
            .flex_col()
            .gap(sc(14.))
            .child(div().text_size(sc(12.)).text_color(theme::text_secondary()).child(crate::i18n::t("local.edit_safety")))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(sc(8.))
                    .child(pill(
                        "settings-local-files",
                        crate::i18n::t("add_files"),
                        Some(Icon::Plus),
                        true,
                        |v, _, _, cx| v.choose_local_files(false, cx),
                        cx,
                    ))
                    .child(pill(
                        "settings-local-folder",
                        crate::i18n::t("add_folder"),
                        Some(Icon::Upload),
                        false,
                        |v, _, _, cx| v.choose_local_files(true, cx),
                        cx,
                    )),
            );
        if self.local_busy {
            body = body.child(div().child(crate::i18n::t("local.working")));
        }
        body = body.child(div().font_weight(FontWeight::SEMIBOLD).child(crate::i18n::t("local.watched_folders")));
        for (i, path) in crate::local::folders().into_iter().enumerate() {
            let display = path.to_string_lossy().into_owned();
            body =
                body.child(div().flex().items_center().gap(sc(10.)).child(div().flex_1().text_size(sc(12.)).child(display)).child(pill(
                    gpui::ElementId::from(("unwatch", i)),
                    crate::i18n::t("local.unwatch"),
                    None,
                    false,
                    move |v, _, _, cx| {
                        if v.local_busy {
                            return;
                        }
                        match crate::local::forget_folder(&path) {
                            Ok(()) => cx.notify(),
                            Err(e) => v.notify_toast(crate::i18n::text("local.operation_failed", &[format!("{e:#}")]), cx),
                        }
                    },
                    cx,
                )));
        }
        if let Some(editor) = &self.local_editor {
            let id = editor.id;
            let preview = editor
                .cover
                .as_ref()
                .map(|p| (!p.as_os_str().is_empty()).then(|| format!("local:{}", p.to_string_lossy())))
                .unwrap_or_else(|| self.local_tracks.iter().find(|t| t.id == id).and_then(|t| t.cover(160)));
            let mut form = widget_panel("local-tag-editor")
                .p(sc(16.))
                .flex()
                .flex_col()
                .gap(sc(10.))
                .child(div().font_weight(FontWeight::BOLD).child(crate::i18n::t("local.edit_metadata")));
            form = form.child(super::widgets::cover(preview, 80., false, cx));
            for (i, field) in editor.fields.iter().enumerate() {
                form = form.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(sc(4.))
                        .child(div().text_size(sc(12.)).child(crate::i18n::t(EDIT_LABELS[i])))
                        .child(field.clone()),
                );
            }
            let cover = editor.cover.as_ref().map(|p| {
                if p.as_os_str().is_empty() {
                    crate::i18n::t("local.cover_removed").to_string()
                } else {
                    p.to_string_lossy().into_owned()
                }
            });
            form = form.when_some(cover, |d, text| d.child(div().text_size(sc(12.)).child(text))).child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(sc(8.))
                    .child(pill(
                        "local-cover",
                        crate::i18n::t("local.choose_cover"),
                        None,
                        false,
                        |v, _, _, cx| v.choose_local_cover(cx),
                        cx,
                    ))
                    .child(pill(
                        "local-cover-remove",
                        crate::i18n::t("local.remove_cover"),
                        None,
                        false,
                        |v, _, _, cx| {
                            if let Some(e) = &mut v.local_editor {
                                e.cover = Some(Default::default());
                            }
                            cx.notify();
                        },
                        cx,
                    ))
                    .child(pill(
                        "local-save-tags",
                        crate::i18n::t("save"),
                        None,
                        true,
                        move |v, _, _, cx| v.write_local_edit(id, false, cx),
                        cx,
                    ))
                    .child(pill(
                        "local-cancel-tags",
                        crate::i18n::t("cancel"),
                        None,
                        false,
                        |v, _, _, cx| {
                            if !v.local_busy {
                                v.local_editor = None;
                                cx.notify();
                            }
                        },
                        cx,
                    )),
            );
            body = body.child(form);
        }
        if !self.local_albums.is_empty() {
            body = body.child(div().font_weight(FontWeight::SEMIBOLD).child(crate::i18n::t("albums")));
            for (i, album) in self.local_albums.clone().into_iter().enumerate() {
                let id = album.id;
                body = body.child(
                    div().flex().items_center().gap(sc(10.)).child(div().flex_1().text_size(sc(13.)).child(album.title)).child(pill(
                        gpui::ElementId::from(("local-album-remove-settings", i)),
                        crate::i18n::t("remove_from_library"),
                        None,
                        false,
                        move |v, _, _, cx| v.remove_local_album(id, cx),
                        cx,
                    )),
                );
            }
        }
        body = body.child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(crate::i18n::text("tracks_in_local_files", &[self.local_tracks.len().to_string()])),
        );
        for (i, track) in self.local_tracks.clone().into_iter().enumerate() {
            let id = track.id;
            body = body.child(
                div()
                    .py(sc(8.))
                    .border_b_1()
                    .border_color(theme::divider())
                    .flex()
                    .flex_col()
                    .gap(sc(6.))
                    .child(div().text_size(sc(14.)).child(track.display_title()))
                    .child(
                        div()
                            .text_size(sc(11.))
                            .text_color(theme::text_secondary())
                            .child(track.local_path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(sc(8.))
                            .child(pill(
                                gpui::ElementId::from(("local-edit", i)),
                                crate::i18n::t("local.edit_metadata"),
                                Some(Icon::Edit),
                                false,
                                move |v, _, _, cx| v.begin_local_edit(id, cx),
                                cx,
                            ))
                            .when(crate::local::has_original(&track), |d| {
                                d.child(pill(
                                    gpui::ElementId::from(("local-undo", i)),
                                    crate::i18n::t("local.restore_original"),
                                    None,
                                    false,
                                    move |v, _, _, cx| v.write_local_edit(id, true, cx),
                                    cx,
                                ))
                            })
                            .child(pill(
                                gpui::ElementId::from(("local-remove", i)),
                                crate::i18n::t("remove_from_library"),
                                None,
                                false,
                                move |v, _, _, cx| {
                                    if !v.local_busy {
                                        v.remove_local_track(id, cx);
                                    }
                                },
                                cx,
                            )),
                    ),
            );
        }
        body.into_any_element()
    }
}
