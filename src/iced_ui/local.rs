//! Settings > Local files: watched folders, the tag and cover editor, and the
//! tracks of the local library. Same rules as the GPUI reference
//! (`src/ui/local.rs`): nothing is written while Abyss has the file open, and
//! the original can be restored.

use std::path::PathBuf;

use iced::widget::{column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length, Task};

use super::icons::Icon;
use super::widgets::{self, cover, line, pill};
use super::{blocking, theme, App, Callback, Message};
use crate::shared::Page;

const LABELS: [&str; 8] =
    ["title", "artist", "local.album_artist", "album", "local.genre", "local.year", "local.track_number", "local.disc_number"];
const TRACK_LIST_H: f32 = 460.0;
const TRACK_H: f32 = 64.0;

/// The track being edited and what was typed so far.
pub struct Editor {
    pub id: u64,
    fields: [String; 8],
    /// None keeps the cover, an empty path removes it.
    cover: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum LocalMsg {
    OpenSettings,
    /// Open Settings on this track's tags (from its menu or the list).
    Edit(u64),
    Field(usize, String),
    ChooseCover,
    CoverChosen(u64, PathBuf),
    RemoveCover,
    Save,
    Cancel,
    Restore(u64),
    Unwatch(PathBuf),
    Scrolled(f32),
}

impl App {
    pub(super) fn local_update(&mut self, msg: LocalMsg) -> Task<Message> {
        match msg {
            LocalMsg::OpenSettings => return self.open_local_settings(),
            LocalMsg::Edit(id) => {
                let open = self.open_local_settings();
                return Task::batch([open, self.begin_local_edit(id)]);
            }
            LocalMsg::Field(i, value) => {
                if let Some(field) = self.local_editor.as_mut().and_then(|e| e.fields.get_mut(i)) {
                    *field = value;
                }
            }
            LocalMsg::ChooseCover => {
                let Some(id) = self.local_editor.as_ref().map(|e| e.id) else { return Task::none() };
                let title = crate::i18n::t("local.choose_cover");
                let pick =
                    blocking(move || rfd::FileDialog::new().set_title(title).add_filter("PNG, JPEG", &["png", "jpg", "jpeg"]).pick_file());
                return Task::perform(pick, move |path| match path {
                    Some(path) => Message::Local(LocalMsg::CoverChosen(id, path)),
                    None => Message::Wake,
                });
            }
            LocalMsg::CoverChosen(id, path) => {
                if let Some(editor) = self.local_editor.as_mut().filter(|e| e.id == id) {
                    editor.cover = Some(path);
                }
            }
            LocalMsg::RemoveCover => {
                if let Some(editor) = &mut self.local_editor {
                    editor.cover = Some(PathBuf::new());
                }
            }
            LocalMsg::Save => {
                if let Some(id) = self.local_editor.as_ref().map(|e| e.id) {
                    return self.write_local_edit(id, false);
                }
            }
            LocalMsg::Cancel => {
                if !self.local_busy {
                    self.local_editor = None;
                }
            }
            LocalMsg::Restore(id) => return self.write_local_edit(id, true),
            LocalMsg::Unwatch(path) => {
                if self.local_busy {
                    return Task::none();
                }
                if let Err(e) = crate::local::forget_folder(&path) {
                    self.notify_toast(crate::i18n::text("local.operation_failed", &[format!("{e:#}")]));
                }
            }
            LocalMsg::Scrolled(y) => self.local_scroll = y,
        }
        Task::none()
    }

    fn open_local_settings(&mut self) -> Task<Message> {
        self.menu = None;
        self.local_scroll = 0.0;
        self.settings_tab = super::settings::LOCAL_TAB;
        let task = if self.page == Page::Settings { Task::none() } else { self.navigate(Page::Settings) };
        // The editor sits at the top of the section.
        self.page_scroll = 0.0;
        let top = iced::widget::operation::scroll_to(super::pages::PAGE_SCROLL, scrollable::AbsoluteOffset { x: 0.0, y: 0.0 });
        Task::batch([task, top])
    }

    fn begin_local_edit(&mut self, id: u64) -> Task<Message> {
        if self.local_busy {
            return Task::none();
        }
        let Some(track) = self.local_tracks.iter().find(|t| t.id == id).cloned() else { return Task::none() };
        self.local_busy = true;
        Task::perform(blocking(move || crate::local::Edit::read(&track)), move |result| {
            Message::Done(Callback::new(move |app: &mut App| {
                app.local_busy = false;
                match result {
                    Ok(e) => {
                        let fields = [e.title, e.artist, e.album_artist, e.album, e.genre, e.year, e.track, e.disc];
                        app.local_editor = Some(Editor { id, fields, cover: None });
                    }
                    Err(e) => app.notify_toast(crate::i18n::text("local.operation_failed", &[format!("{e:#}")])),
                }
                Task::none()
            }))
        })
    }

    /// Write the edited tags, or put the original back (`undo`).
    fn write_local_edit(&mut self, id: u64, undo: bool) -> Task<Message> {
        if self.local_busy {
            return Task::none();
        }
        // Warn before any write while Abyss has the file open, even when paused.
        if self.now.as_ref().is_some_and(|n| n.track.id == id) {
            self.notify_toast(crate::i18n::t("local.write_blocked"));
            return Task::none();
        }
        let Some(track) = self.local_tracks.iter().find(|t| t.id == id).cloned() else { return Task::none() };
        let edit = if undo {
            None
        } else {
            let Some(editor) = self.local_editor.as_ref().filter(|e| e.id == id) else { return Task::none() };
            let [title, artist, album_artist, album, genre, year, track, disc] = editor.fields.clone();
            Some(crate::local::Edit { title, artist, album_artist, album, genre, year, track, disc, cover: editor.cover.clone() })
        };
        // The engine may already hold this file as the gapless next track.
        if self.queued_next.and_then(|(_, i)| self.queue.get(i)).is_some_and(|q| q.track.id == id) {
            self.notify_toast(crate::i18n::t("local.write_blocked"));
            return Task::none();
        }
        self.local_busy = true;
        Task::perform(blocking(move || crate::local::edit(&track, edit)), move |result| {
            Message::Done(Callback::new(move |app: &mut App| {
                app.local_busy = false;
                match result {
                    Ok(track) => {
                        let slots = app.local_tracks.iter_mut().chain(app.queue.iter_mut().map(|q| &mut q.track));
                        for item in slots.filter(|t| t.id == id) {
                            *item = track.clone();
                        }
                        // The order shuffle restores keeps the same entries.
                        for item in app.unshuffled.iter_mut().flatten().filter(|q| q.track.id == id) {
                            item.track = track.clone();
                        }
                        if let Some(now) = app.now.as_mut().filter(|n| n.track.id == id) {
                            now.track = track.clone();
                        }
                        app.lyrics_epoch = app.lyrics_epoch.wrapping_add(1);
                        app.lyrics.clear();
                        app.lyrics_docs.clear();
                        crate::tidal::lyrics::forget_missing(&track);
                        // The file itself is already written; the index follows in the background.
                        crate::local::save_async(app.local_tracks.clone());
                        app.local_changed();
                        app.local_editor = None;
                        app.notify_toast(crate::i18n::t(if undo { "local.restored" } else { "local.saved" }));
                        if app.wants_lyrics() {
                            if let Some(playing) = app.now.as_ref().map(|n| n.track.id) {
                                return app.load_lyrics(playing);
                            }
                        }
                    }
                    Err(e) => app.notify_toast(crate::i18n::text("local.operation_failed", &[format!("{e:#}")])),
                }
                Task::none()
            }))
        })
    }
}

fn heading<'a>(s: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    text(s).size(14).font(widgets::semibold()).color(theme::text()).into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    // The same cards as the other Settings sections: a heading with its icon
    // tile over a quiet surface, one row per entry with the rule between.
    let mut import = vec![row_pad(
        column![
            text(crate::i18n::t("local.edit_safety"))
                .size(12)
                .line_height(text::LineHeight::Absolute(17.0.into()))
                .color(theme::text_secondary()),
            row![
                pill(crate::i18n::t("add_files"), Some(Icon::Plus), true, Some(Message::ChooseLocal(false))),
                pill(crate::i18n::t("add_folder"), Some(Icon::Upload), false, Some(Message::ChooseLocal(true))),
                pill(crate::i18n::t("local.rescan"), Some(Icon::Sync), false, Some(Message::RescanLocal)),
            ]
            .spacing(8)
            .wrap()
            .vertical_spacing(8),
        ]
        .spacing(12)
        .into(),
    )];
    if app.local_busy {
        import.push(row_pad(text(crate::i18n::t("local.working")).size(12).color(theme::text_secondary()).into()));
    }
    let mut body =
        column![super::settings::group(Icon::Upload, crate::i18n::t("local.import"), "", import)].spacing(super::settings::GROUP_GAP);
    if let Some(editor) = &app.local_editor {
        body = body.push(editor_form(app, editor));
    }
    let folders: Vec<Element<'_, Message>> = crate::local::folders()
        .into_iter()
        .map(|path| {
            let shown = path.to_string_lossy().into_owned();
            row_pad(
                row![
                    line(shown, 12.0, theme::text_secondary()).width(Length::Fill),
                    widgets::pill(crate::i18n::t("local.unwatch"), None, false, Some(Message::Local(LocalMsg::Unwatch(path)))),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into(),
            )
        })
        .collect();
    let folders = if folders.is_empty() { vec![row_pad(empty_note(crate::i18n::t("local.no_folders")))] } else { folders };
    body = body.push(super::settings::group(Icon::Library, crate::i18n::t("local.watched_folders"), "", folders));
    if !app.local_albums.is_empty() {
        let albums = column(app.local_albums.iter().map(|album| {
            row![
                line(album.title.clone(), 13.0, theme::text()).font(widgets::semibold()).width(Length::Fill),
                pill(crate::i18n::t("remove_from_library"), None, false, Some(Message::RemoveLocalAlbum(album.id))),
            ]
            .spacing(10)
            .padding([4, 20])
            .align_y(Alignment::Center)
            .into()
        }))
        .padding([8, 0]);
        let list = container(super::glide::glide(scrollable(albums).style(super::pages::scroll_style))).max_height(260).into();
        body = body.push(super::settings::group(Icon::Album, crate::i18n::t("albums"), "", vec![list]));
    }
    let count = crate::i18n::text("tracks_in_local_files", &[app.local_tracks.len().to_string()]);
    let tracks = if app.local_tracks.is_empty() { row_pad(empty_note(crate::i18n::t("local.drop_instructions"))) } else { track_list(app) };
    body.push(group_owned(Icon::Track, count, tracks)).into()
}

/// One entry of a card, with the settings rows' padding.
fn row_pad(content: Element<'_, Message>) -> Element<'_, Message> {
    container(content).width(Length::Fill).padding([13, 20]).into()
}

/// A quiet line inside a card when it has nothing to list.
fn empty_note<'a>(s: &'a str) -> Element<'a, Message> {
    text(s).size(12).line_height(text::LineHeight::Absolute(17.0.into())).color(theme::text_dim()).into()
}

/// [`super::settings::group`] for a heading made at run time (a count).
fn group_owned<'a>(i: Icon, title: String, content: Element<'a, Message>) -> Element<'a, Message> {
    column![
        widgets::panel_heading(i, title, None),
        container(content).width(Length::Fill).style(|_| container::Style {
            background: Some(theme::ink(0.036).into()),
            border: iced::Border { color: theme::ink(0.09), width: 1.0, radius: 16.0.into() },
            ..Default::default()
        }),
    ]
    .spacing(12)
    .into()
}

fn editor_form<'a>(app: &'a App, editor: &'a Editor) -> Element<'a, Message> {
    let preview = match &editor.cover {
        Some(path) => (!path.as_os_str().is_empty()).then(|| format!("local:{}", path.to_string_lossy())),
        None => app.local_tracks.iter().find(|t| t.id == editor.id).and_then(|t| t.cover(160)),
    };
    let mut form = column![heading(crate::i18n::t("local.edit_metadata")), cover(preview, 80.0, false)].spacing(10);
    for (i, (key, value)) in LABELS.iter().zip(&editor.fields).enumerate() {
        form = form.push(
            column![
                text(crate::i18n::t(key)).size(12).color(theme::text_secondary()),
                text_input("", value)
                    .on_input(move |v| Message::Local(LocalMsg::Field(i, v)))
                    .size(13)
                    .padding(8)
                    .style(super::pages::field_style),
            ]
            .spacing(4),
        );
    }
    let chosen = editor.cover.as_ref().map(|p| {
        let label =
            if p.as_os_str().is_empty() { crate::i18n::t("local.cover_removed").to_string() } else { p.to_string_lossy().into_owned() };
        text(label).size(12).color(theme::text_secondary())
    });
    let action = |msg: LocalMsg| (!app.local_busy).then_some(Message::Local(msg));
    let actions = row![
        pill(crate::i18n::t("local.choose_cover"), None, false, action(LocalMsg::ChooseCover)),
        pill(crate::i18n::t("local.remove_cover"), None, false, action(LocalMsg::RemoveCover)),
        pill(crate::i18n::t("save"), None, true, action(LocalMsg::Save)),
        pill(crate::i18n::t("cancel"), None, false, action(LocalMsg::Cancel)),
    ]
    .spacing(8)
    .wrap();
    container(form.push(chosen).push(actions)).padding(16).width(Length::Fill).style(widgets::panel_style).into()
}

/// The local tracks in their own scroll area, only the visible rows built:
/// each asks the disk whether an original can be restored.
fn track_list(app: &App) -> Element<'_, Message> {
    let count = app.local_tracks.len();
    let first = (((app.local_scroll / TRACK_H).floor() as usize).saturating_sub(3)).min(count);
    let last = (first + (TRACK_LIST_H / TRACK_H) as usize + 8).min(count);
    let mut rows = column![Space::new().height(first as f32 * TRACK_H)].width(Length::Fill);
    for track in &app.local_tracks[first..last] {
        let id = track.id;
        let path = track.local_path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let restore = crate::local::has_original(track)
            .then(|| pill(crate::i18n::t("local.restore_original"), None, false, Some(Message::Local(LocalMsg::Restore(id)))));
        let names = column![line(track.display_title(), 14.0, theme::text()), line(path, 11.0, theme::text_secondary())].spacing(3);
        rows = rows.push(
            row![
                container(names).width(Length::Fill).clip(true),
                pill(crate::i18n::t("local.edit_metadata"), Some(Icon::Edit), false, Some(Message::Local(LocalMsg::Edit(id)))),
            ]
            .push(restore)
            .push(widgets::icon_button(Icon::Close, 14.0, theme::text_secondary(), Message::RemoveLocalTrack(id)))
            .spacing(8)
            .padding([0, 20])
            .height(TRACK_H)
            .align_y(Alignment::Center),
        );
    }
    rows = rows.push(Space::new().height((count - last) as f32 * TRACK_H));
    let list = scrollable(rows).on_scroll(|v| Message::Local(LocalMsg::Scrolled(v.absolute_offset().y))).style(super::pages::scroll_style);
    container(super::glide::glide(list)).max_height(TRACK_LIST_H).into()
}
