//! Playlist management: create, edit, delete, add/remove/reorder tracks,
//! save other people's playlists, plus the dialogs for it.

use super::sc;
use gpui::AnimationExt;
use gpui::{div, prelude::*, px, AnyElement, Context, Focusable, FontWeight, Window};

use super::input::TextInput;
use super::widgets::pill;
use super::{theme, AppView, Load, Page};
use crate::tidal::{Playlist, Track};

#[derive(Clone)]
pub enum Dialog {
    /// New playlist, optionally filled with these tracks.
    NewPlaylist {
        add: Vec<u64>,
    },
    EditPlaylist {
        uuid: String,
    },
    DeletePlaylist {
        uuid: String,
        title: String,
    },
    /// Confirm signing out (from the sidebar's account panel).
    SignOut,
    /// Confirm a "Remove" from a context menu (changes the TIDAL account).
    ConfirmRemove(RemoveKind),
}

/// What a context menu's "Remove" takes away.
#[derive(Clone)]
pub enum RemoveKind {
    /// A track from one of the user's playlists (by position).
    PlaylistTrack {
        uuid: String,
        index: usize,
        title: String,
    },
    /// A track, album or artist from the collection (favourites / follows).
    FavTrack(Track),
    FavAlbum(crate::tidal::Album),
    Unfollow(u64, String),
}

/// What is being dragged when reordering a playlist.
#[derive(Clone)]
pub struct DraggedTrack {
    pub uuid: String,
    pub index: usize,
    pub title: String,
}

impl Render for DraggedTrack {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(sc(14.))
            .py(sc(8.))
            .rounded(sc(10.))
            .bg(theme::liquid_glass(0.3))
            .border_1()
            .border_color(theme::accent())
            .text_color(theme::text())
            .text_size(sc(13.))
            .font_family(theme::FONT)
            .shadow_lg()
            .child(self.title.clone())
    }
}

fn playlist_is_editable(playlist: &Playlist, uid: Option<u64>) -> bool {
    Some(playlist.creator_id) == uid || playlist.kind == "USER"
}

impl AppView {
    pub fn user_id(&self) -> Option<u64> {
        self.settings.lock().user_id
    }

    /// Playlists the user created (editable).
    pub fn own_playlists(&self) -> Vec<Playlist> {
        let uid = self.user_id();
        self.my_playlists.ready().map(|l| l.iter().filter(|p| playlist_is_editable(p, uid)).cloned().collect()).unwrap_or_default()
    }

    /// A playlist's picture: its own image, or a mosaic of its first albums.
    pub fn playlist_cover(&mut self, pl: &Playlist, size: f32, cx: &mut Context<Self>) -> AnyElement {
        let px_size = (size * 2.0) as u32;
        if let Some(url) = pl.cover(px_size) {
            return super::widgets::cover(Some(url), size, false, cx);
        }
        let mut urls: Vec<String> = Vec::new();
        if let Some((_, tracks)) = self.playlists.get(&pl.uuid).and_then(Load::ready) {
            for t in tracks {
                if let Some(u) = t.cover(320) {
                    if !urls.contains(&u) {
                        urls.push(u);
                    }
                }
                if urls.len() == 4 {
                    break;
                }
            }
        }
        match urls.len() {
            4 => super::widgets::mosaic(&urls, size, cx),
            0 => super::widgets::cover(None, size, false, cx),
            _ => super::widgets::cover(urls.into_iter().next(), size, false, cx),
        }
    }

    pub fn is_own_playlist(&self, uuid: &str) -> bool {
        let uid = self.user_id();
        let check = |p: &Playlist| p.uuid == uuid && playlist_is_editable(p, uid);
        self.my_playlists.ready().map(|l| l.iter().any(check)).unwrap_or(false)
            || self.playlists.get(uuid).and_then(Load::ready).map(|(p, _)| Some(p.creator_id) == uid).unwrap_or(false)
    }

    pub fn is_saved_playlist(&self, uuid: &str) -> bool {
        self.my_playlists.ready().map(|l| l.iter().any(|p| p.uuid == uuid)).unwrap_or(false)
    }

    pub fn open_dialog(&mut self, dialog: Dialog, window: &mut Window, cx: &mut Context<Self>) {
        let (title, desc) = match &dialog {
            Dialog::EditPlaylist { uuid } => {
                self.playlists.get(uuid).and_then(Load::ready).map(|(p, _)| (p.title.clone(), p.description.clone())).unwrap_or_default()
            }
            _ => (String::new(), String::new()),
        };
        self.dlg_title.update(cx, |i, cx| i.set_text(&title, cx));
        self.dlg_desc.update(cx, |i, cx| i.set_text(&desc, cx));
        // Confirmations have no text field: keep the focus on the window so
        // Escape reaches it (the hidden field would swallow it).
        let has_fields = matches!(dialog, Dialog::NewPlaylist { .. } | Dialog::EditPlaylist { .. });
        self.dialog = Some(dialog);
        self.menu = None;
        if has_fields {
            let fh = self.dlg_title.read(cx).focus_handle(cx);
            window.focus(&fh);
        } else {
            window.focus(&self.focus);
        }
        cx.notify();
    }

    pub fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog = None;
        window.focus(&self.focus);
        cx.notify();
    }

    pub fn submit_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.dialog.clone() else { return };
        let title = self.dlg_title.read(cx).text().trim().to_string();
        let desc = self.dlg_desc.read(cx).text().trim().to_string();
        match dialog {
            Dialog::NewPlaylist { add } => {
                if title.is_empty() {
                    return self.notify_toast(crate::i18n::t("enter_a_playlist_name"), cx);
                }
                self.create_playlist(title, desc, add, cx);
            }
            Dialog::EditPlaylist { uuid } => {
                if title.is_empty() {
                    return self.notify_toast(crate::i18n::t("the_name_cannot_be_empty"), cx);
                }
                self.edit_playlist(uuid, title, desc, cx);
            }
            Dialog::DeletePlaylist { uuid, .. } => self.delete_playlist(uuid, cx),
            Dialog::SignOut => self.sign_out(cx),
            Dialog::ConfirmRemove(k) => match k {
                RemoveKind::PlaylistTrack { uuid, index, .. } => self.remove_playlist_track(uuid, index, cx),
                RemoveKind::FavTrack(t) => {
                    if self.fav_ids.contains(&t.id) {
                        self.toggle_favorite(&t, cx)
                    }
                }
                RemoveKind::FavAlbum(a) => {
                    if self.fav_album_ids.contains(&a.id) {
                        self.toggle_favorite_album(&a, cx)
                    }
                }
                RemoveKind::Unfollow(id, _) => {
                    if self.fav_artist_ids.contains(&id) {
                        self.toggle_follow_artist(id, cx)
                    }
                }
            },
        }
        self.close_dialog(window, cx);
    }

    fn create_playlist(&mut self, title: String, desc: String, add: Vec<u64>, cx: &mut Context<Self>) {
        self.fetch(
            cx,
            move |t| {
                let pl = t.create_playlist(&title, &desc)?;
                if !add.is_empty() {
                    t.add_to_playlist(&pl.uuid, &add)?;
                }
                Ok(pl)
            },
            |this, r, cx| match r {
                Ok(pl) => {
                    if let Load::Ready(list) = &mut this.my_playlists {
                        list.insert(0, pl.clone());
                    }
                    this.notify_toast(crate::i18n::text("playlist_created", &[pl.title.to_string()]), cx);
                    this.navigate(Page::Playlist(pl.uuid), cx);
                }
                Err(e) => this.notify_toast(crate::i18n::text("could_not_create_the_playlist", &[format!("{:#}", e)]), cx),
            },
        );
    }

    fn edit_playlist(&mut self, uuid: String, title: String, desc: String, cx: &mut Context<Self>) {
        let (u, t2, d2) = (uuid.clone(), title.clone(), desc.clone());
        self.fetch(
            cx,
            move |t| t.edit_playlist(&u, &t2, &d2),
            move |this, r, cx| match r {
                Ok(()) => {
                    if let Some(Load::Ready((p, _))) = this.playlists.get_mut(&uuid) {
                        p.title = title.clone();
                        p.description = desc.clone();
                    }
                    if let Load::Ready(list) = &mut this.my_playlists {
                        if let Some(p) = list.iter_mut().find(|p| p.uuid == uuid) {
                            p.title = title.clone();
                            p.description = desc.clone();
                        }
                    }
                    this.notify_toast(crate::i18n::t("playlist_updated"), cx);
                }
                Err(e) => this.notify_toast(crate::i18n::text("could_not_edit", &[format!("{:#}", e)]), cx),
            },
        );
    }

    fn delete_playlist(&mut self, uuid: String, cx: &mut Context<Self>) {
        let u = uuid.clone();
        self.fetch(
            cx,
            move |t| t.delete_playlist(&u),
            move |this, r, cx| match r {
                Ok(()) => {
                    if let Load::Ready(list) = &mut this.my_playlists {
                        list.retain(|p| p.uuid != uuid);
                    }
                    this.playlists.remove(&uuid);
                    if this.page == Page::Playlist(uuid.clone()) {
                        this.navigate(Page::MyPlaylists, cx);
                    }
                    this.notify_toast(crate::i18n::t("playlist_deleted"), cx);
                }
                Err(e) => this.notify_toast(crate::i18n::text("could_not_delete", &[format!("{:#}", e)]), cx),
            },
        );
    }

    pub fn add_tracks_to_playlist(&mut self, uuid: String, tracks: Vec<Track>, cx: &mut Context<Self>) {
        if tracks.iter().any(|track| track.local_path.is_some()) {
            self.notify_toast(crate::i18n::text("local_files_cannot_be_added_to_playlists", &[crate::i18n::t("tidal").to_string()]), cx);
            return;
        }
        let ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
        let name = self
            .my_playlists
            .ready()
            .and_then(|l| l.iter().find(|p| p.uuid == uuid))
            .map(|p| p.title.clone())
            .unwrap_or_else(|| crate::i18n::t("the_playlist").into());
        let u = uuid.clone();
        self.fetch(
            cx,
            move |t| t.add_to_playlist(&u, &ids),
            move |this, r, cx| match r {
                Ok(()) => {
                    if let Some(Load::Ready((p, list))) = this.playlists.get_mut(&uuid) {
                        list.extend(tracks.iter().cloned());
                        p.number_of_tracks = list.len() as u32;
                    }
                    if let Load::Ready(all) = &mut this.my_playlists {
                        if let Some(p) = all.iter_mut().find(|p| p.uuid == uuid) {
                            p.number_of_tracks += tracks.len() as u32;
                        }
                    }
                    let n = tracks.len();
                    this.notify_toast(
                        if n == 1 {
                            crate::i18n::text("added_to", &[name.to_string()])
                        } else {
                            crate::i18n::text("tracks_added_to", &[format!("{}", n), name.to_string()])
                        },
                        cx,
                    );
                }
                Err(e) => this.notify_toast(crate::i18n::text("could_not_add", &[format!("{:#}", e)]), cx),
            },
        );
    }

    pub fn remove_playlist_track(&mut self, uuid: String, index: usize, cx: &mut Context<Self>) {
        // Optimistic: update the list now, reload it if TIDAL refuses.
        if let Some(Load::Ready((p, list))) = self.playlists.get_mut(&uuid) {
            if index < list.len() {
                list.remove(index);
                p.number_of_tracks = list.len() as u32;
            }
        }
        let u = uuid.clone();
        self.fetch(
            cx,
            move |t| t.remove_from_playlist(&u, index),
            move |this, r, cx| match r {
                Ok(()) => this.notify_toast(crate::i18n::t("removed_from_playlist"), cx),
                Err(e) => {
                    this.playlists.remove(&uuid);
                    this.load_page_data(cx);
                    this.notify_toast(crate::i18n::text("could_not_remove", &[format!("{:#}", e)]), cx);
                }
            },
        );
        cx.notify();
    }

    pub fn move_playlist_track(&mut self, uuid: String, from: usize, to: usize, cx: &mut Context<Self>) {
        if from == to {
            return;
        }
        if let Some(Load::Ready((_, list))) = self.playlists.get_mut(&uuid) {
            if from < list.len() && to < list.len() {
                let t = list.remove(from);
                list.insert(to, t);
            }
        }
        let u = uuid.clone();
        self.fetch(
            cx,
            move |t| t.move_in_playlist(&u, from, to),
            move |this, r, cx| {
                if let Err(e) = r {
                    this.playlists.remove(&uuid);
                    this.load_page_data(cx);
                    this.notify_toast(crate::i18n::text("could_not_move", &[format!("{:#}", e)]), cx);
                }
            },
        );
        cx.notify();
    }

    pub fn toggle_saved_playlist(&mut self, pl: Playlist, cx: &mut Context<Self>) {
        let save = !self.is_saved_playlist(&pl.uuid);
        if let Load::Ready(list) = &mut self.my_playlists {
            if save {
                list.insert(0, pl.clone());
            } else {
                list.retain(|p| p.uuid != pl.uuid);
            }
        }
        let u = pl.uuid.clone();
        self.fetch(
            cx,
            move |t| t.set_favorite_playlist(&u, save),
            move |this, r, cx| match r {
                Ok(()) => this.notify_toast(
                    if save { crate::i18n::t("saved_to_my_collection") } else { crate::i18n::t("removed_from_my_collection") },
                    cx,
                ),
                Err(e) => this.notify_toast(crate::i18n::text("error", &[format!("{:#}", e)]), cx),
            },
        );
        cx.notify();
    }

    pub fn toggle_follow_artist(&mut self, id: u64, cx: &mut Context<Self>) {
        let follow = !self.fav_artist_ids.contains(&id);
        if follow {
            self.fav_artist_ids.insert(id);
        } else {
            self.fav_artist_ids.remove(&id);
        }
        self.fav_artists = Load::Idle;
        self.fetch(
            cx,
            move |t| t.set_favorite_artist(id, follow),
            move |this, r, cx| match r {
                Ok(()) => {
                    this.notify_toast(if follow { crate::i18n::t("following_artist") } else { crate::i18n::t("unfollowed_artist") }, cx)
                }
                Err(e) => this.notify_toast(crate::i18n::text("error", &[format!("{:#}", e)]), cx),
            },
        );
        cx.notify();
    }
}

/// The modal dialog (new / edit / delete playlist).
pub fn dialog_overlay(view: &mut AppView, cx: &mut Context<AppView>) -> Option<AnyElement> {
    let dialog = view.dialog.clone()?;
    let field = |label: &'static str, input: gpui::Entity<TextInput>| {
        div()
            .flex()
            .flex_col()
            .gap(sc(6.))
            .child(div().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_secondary()).child(label))
            .child(
                div()
                    .h(sc(40.))
                    .px(sc(12.))
                    .flex()
                    .items_center()
                    .rounded(sc(6.))
                    .bg(theme::surface())
                    .border_1()
                    .border_color(theme::border())
                    .child(input),
            )
    };
    let (heading, body, confirm, danger): (String, AnyElement, &str, bool) = match &dialog {
        Dialog::NewPlaylist { add } => (
            crate::i18n::t("playlist.create").into(),
            div()
                .flex()
                .flex_col()
                .gap(sc(14.))
                .child(field(crate::i18n::t("name"), view.dlg_title.clone()))
                .child(field(crate::i18n::t("description_optional"), view.dlg_desc.clone()))
                .when(!add.is_empty(), |d| {
                    d.child(div().text_size(sc(13.)).text_color(theme::text_secondary()).child(if add.len() == 1 {
                        crate::i18n::t("one_track_will_be_added").to_string()
                    } else {
                        crate::i18n::text("tracks_will_be_added", &[add.len().to_string()])
                    }))
                })
                .into_any_element(),
            crate::i18n::t("create"),
            false,
        ),
        Dialog::EditPlaylist { .. } => (
            crate::i18n::t("edit_playlist").into(),
            div()
                .flex()
                .flex_col()
                .gap(sc(14.))
                .child(field(crate::i18n::t("name"), view.dlg_title.clone()))
                .child(field(crate::i18n::t("description"), view.dlg_desc.clone()))
                .into_any_element(),
            crate::i18n::t("save"),
            false,
        ),
        Dialog::SignOut => (
            crate::i18n::t("sign_out").into(),
            div()
                .text_color(theme::text_secondary())
                .child(crate::i18n::text("account.signout_explanation", &[crate::i18n::t("tidal").to_string()]))
                .into_any_element(),
            crate::i18n::t("sign_out"),
            true,
        ),
        Dialog::ConfirmRemove(k) => {
            let (title, text, button) = match k {
                RemoveKind::PlaylistTrack { title, .. } => (
                    crate::i18n::t("remove_from_playlist"),
                    crate::i18n::text("playlist.confirm_remove", &[title.to_string(), crate::i18n::t("tidal").to_string()]),
                    crate::i18n::t("remove"),
                ),
                RemoveKind::FavTrack(t) => (
                    crate::i18n::t("remove_from_your_collection"),
                    crate::i18n::text(
                        "remove_from_your_favorite_tracks_in",
                        &[t.display_title().to_string(), crate::i18n::t("tidal").to_string()],
                    ),
                    crate::i18n::t("remove"),
                ),
                RemoveKind::FavAlbum(a) => (
                    crate::i18n::t("remove_from_your_collection"),
                    crate::i18n::text("remove_from_your_albums_in", &[a.title.to_string(), crate::i18n::t("tidal").to_string()]),
                    crate::i18n::t("remove"),
                ),
                RemoveKind::Unfollow(_, name) => (
                    crate::i18n::t("unfollow"),
                    crate::i18n::text("unfollow_in", &[name.to_string(), crate::i18n::t("tidal").to_string()]),
                    crate::i18n::t("unfollow"),
                ),
            };
            (title.into(), div().text_color(theme::text_secondary()).child(text).into_any_element(), button, true)
        }
        Dialog::DeletePlaylist { title, .. } => (
            crate::i18n::t("delete_playlist").into(),
            div()
                .text_color(theme::text_secondary())
                .child(crate::i18n::text("delete_this_cannot_be_undone", &[title.to_string()]))
                .into_any_element(),
            crate::i18n::t("delete"),
            true,
        ),
    };
    Some(
        gpui::deferred(
            div()
                .id("dialog-backdrop")
                .absolute()
                .top(sc(0.))
                .left(sc(0.))
                .size_full()
                .occlude()
                .bg(gpui::rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, window, cx| this.close_dialog(window, cx)))
                .child(
                    super::widgets::floating_panel(16.)
                        .id("dialog")
                        .w(sc(460.))
                        .max_w_full()
                        .max_h(gpui::relative(0.9))
                        .overflow_y_scroll()
                        .p(sc(super::widgets::PANEL_PAD))
                        .flex()
                        .flex_col()
                        .gap(sc(18.))
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(super::glass::sheen("playlist-dialog-glass", 16., 0.75))
                        .child(div().text_size(sc(20.)).font_weight(FontWeight::BOLD).child(heading))
                        .child(body)
                        .child(
                            div()
                                .pt(sc(12.))
                                .border_t_1()
                                .border_color(theme::divider())
                                .flex()
                                .justify_end()
                                .gap(sc(10.))
                                .child(pill(
                                    "dlg-cancel",
                                    crate::i18n::t("cancel"),
                                    None,
                                    false,
                                    |this, _, window, cx| this.close_dialog(window, cx),
                                    cx,
                                ))
                                .child(div().when(danger, |d| d.text_color(theme::danger())).child(pill(
                                    "dlg-ok",
                                    confirm.to_string(),
                                    None,
                                    true,
                                    |this, _, window, cx| this.submit_dialog(window, cx),
                                    cx,
                                ))),
                        )
                        .with_animation("dialog-entrance", super::motion::animation(super::motion::PANEL), super::motion::enter),
                ),
        )
        .with_priority(3)
        .into_any_element(),
    )
}
