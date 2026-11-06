//! Playlist management: create, edit, delete, add/remove/reorder tracks,
//! save other people's playlists, plus the dialogs for it.

use super::sc;
use gpui::{div, prelude::*, px, AnyElement, Context, Focusable, FontWeight, Window};

use super::input::TextInput;
use super::widgets::pill;
use super::{theme, AppView, Load, Page};
use crate::tidal::{Playlist, Track};

#[derive(Clone)]
pub enum Dialog {
    /// New playlist, optionally filled with these tracks.
    NewPlaylist { add: Vec<u64> },
    EditPlaylist { uuid: String },
    DeletePlaylist { uuid: String, title: String },
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
            .rounded(sc(6.))
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::accent())
            .text_color(theme::text())
            .text_size(sc(13.))
            .font_family(theme::FONT)
            .shadow_lg()
            .child(self.title.clone())
    }
}

impl AppView {
    pub fn user_id(&self) -> Option<u64> {
        self.settings.lock().user_id
    }

    /// Playlists the user created (editable).
    pub fn own_playlists(&self) -> Vec<Playlist> {
        let uid = self.user_id();
        self.my_playlists
            .ready()
            .map(|l| l.iter().filter(|p| Some(p.creator_id) == uid || p.kind == "USER").cloned().collect())
            .unwrap_or_default()
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
        let check = |p: &Playlist| p.uuid == uuid && (Some(p.creator_id) == uid || p.kind == "USER");
        self.my_playlists.ready().map(|l| l.iter().any(check)).unwrap_or(false)
            || self.playlists.get(uuid).and_then(Load::ready).map(|(p, _)| Some(p.creator_id) == uid).unwrap_or(false)
    }

    pub fn is_saved_playlist(&self, uuid: &str) -> bool {
        self.my_playlists.ready().map(|l| l.iter().any(|p| p.uuid == uuid)).unwrap_or(false)
    }

    pub fn open_dialog(&mut self, dialog: Dialog, window: &mut Window, cx: &mut Context<Self>) {
        let (title, desc) = match &dialog {
            Dialog::EditPlaylist { uuid } => self
                .playlists
                .get(uuid)
                .and_then(Load::ready)
                .map(|(p, _)| (p.title.clone(), p.description.clone()))
                .unwrap_or_default(),
            _ => (String::new(), String::new()),
        };
        self.dlg_title.update(cx, |i, cx| i.set_text(&title, cx));
        self.dlg_desc.update(cx, |i, cx| i.set_text(&desc, cx));
        self.dialog = Some(dialog);
        self.menu = None;
        let fh = self.dlg_title.read(cx).focus_handle(cx);
        window.focus(&fh);
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
                    return self.notify_toast("Escribe un nombre para la playlist", cx);
                }
                self.create_playlist(title, desc, add, cx);
            }
            Dialog::EditPlaylist { uuid } => {
                if title.is_empty() {
                    return self.notify_toast("El nombre no puede estar vacío", cx);
                }
                self.edit_playlist(uuid, title, desc, cx);
            }
            Dialog::DeletePlaylist { uuid, .. } => self.delete_playlist(uuid, cx),
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
                    this.notify_toast(format!("Playlist «{}» creada", pl.title), cx);
                    this.navigate(Page::Playlist(pl.uuid), cx);
                }
                Err(e) => this.notify_toast(format!("No se pudo crear la playlist: {e:#}"), cx),
            },
        );
    }

    fn edit_playlist(&mut self, uuid: String, title: String, desc: String, cx: &mut Context<Self>) {
        let (u, t2, d2) = (uuid.clone(), title.clone(), desc.clone());
        self.fetch(cx, move |t| t.edit_playlist(&u, &t2, &d2), move |this, r, cx| match r {
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
                this.notify_toast("Playlist actualizada", cx);
            }
            Err(e) => this.notify_toast(format!("No se pudo editar: {e:#}"), cx),
        });
    }

    fn delete_playlist(&mut self, uuid: String, cx: &mut Context<Self>) {
        let u = uuid.clone();
        self.fetch(cx, move |t| t.delete_playlist(&u), move |this, r, cx| match r {
            Ok(()) => {
                if let Load::Ready(list) = &mut this.my_playlists {
                    list.retain(|p| p.uuid != uuid);
                }
                this.playlists.remove(&uuid);
                if this.page == Page::Playlist(uuid.clone()) {
                    this.navigate(Page::MyPlaylists, cx);
                }
                this.notify_toast("Playlist eliminada", cx);
            }
            Err(e) => this.notify_toast(format!("No se pudo eliminar: {e:#}"), cx),
        });
    }

    pub fn add_tracks_to_playlist(&mut self, uuid: String, tracks: Vec<Track>, cx: &mut Context<Self>) {
        let ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
        let name = self
            .my_playlists
            .ready()
            .and_then(|l| l.iter().find(|p| p.uuid == uuid))
            .map(|p| p.title.clone())
            .unwrap_or_else(|| "la playlist".into());
        let u = uuid.clone();
        self.fetch(cx, move |t| t.add_to_playlist(&u, &ids), move |this, r, cx| match r {
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
                    if n == 1 { format!("Añadida a «{name}»") } else { format!("{n} pistas añadidas a «{name}»") },
                    cx,
                );
            }
            Err(e) => this.notify_toast(format!("No se pudo añadir: {e:#}"), cx),
        });
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
        self.fetch(cx, move |t| t.remove_from_playlist(&u, index), move |this, r, cx| match r {
            Ok(()) => this.notify_toast("Eliminada de la playlist", cx),
            Err(e) => {
                this.playlists.remove(&uuid);
                this.load_page_data(cx);
                this.notify_toast(format!("No se pudo quitar: {e:#}"), cx);
            }
        });
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
        self.fetch(cx, move |t| t.move_in_playlist(&u, from, to), move |this, r, cx| {
            if let Err(e) = r {
                this.playlists.remove(&uuid);
                this.load_page_data(cx);
                this.notify_toast(format!("No se pudo mover: {e:#}"), cx);
            }
        });
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
        self.fetch(cx, move |t| t.set_favorite_playlist(&u, save), move |this, r, cx| match r {
            Ok(()) => this.notify_toast(if save { "Guardada en Mi colección" } else { "Quitada de Mi colección" }, cx),
            Err(e) => this.notify_toast(format!("Error: {e:#}"), cx),
        });
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
        self.fetch(cx, move |t| t.set_favorite_artist(id, follow), move |this, r, cx| match r {
            Ok(()) => this.notify_toast(if follow { "Siguiendo al artista" } else { "Has dejado de seguir al artista" }, cx),
            Err(e) => this.notify_toast(format!("Error: {e:#}"), cx),
        });
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
            .child(div().text_size(sc(12.)).text_color(theme::text_secondary()).child(label))
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
            "Nueva playlist".into(),
            div()
                .flex()
                .flex_col()
                .gap(sc(14.))
                .child(field("Nombre", view.dlg_title.clone()))
                .child(field("Descripción (opcional)", view.dlg_desc.clone()))
                .when(!add.is_empty(), |d| {
                    d.child(div().text_size(sc(13.)).text_color(theme::text_secondary()).child(format!(
                        "Se añadirá{} {} pista{}.",
                        if add.len() == 1 { "" } else { "n" },
                        add.len(),
                        if add.len() == 1 { "" } else { "s" }
                    )))
                })
                .into_any_element(),
            "Crear",
            false,
        ),
        Dialog::EditPlaylist { .. } => (
            "Editar playlist".into(),
            div()
                .flex()
                .flex_col()
                .gap(sc(14.))
                .child(field("Nombre", view.dlg_title.clone()))
                .child(field("Descripción", view.dlg_desc.clone()))
                .into_any_element(),
            "Guardar",
            false,
        ),
        Dialog::DeletePlaylist { title, .. } => (
            "Eliminar playlist".into(),
            div()
                .text_color(theme::text_secondary())
                .child(format!("¿Seguro que quieres eliminar «{title}»? No se puede deshacer."))
                .into_any_element(),
            "Eliminar",
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
                    div()
                        .id("dialog")
                        .w(sc(460.))
                        .p(sc(24.))
                        .rounded(sc(10.))
                        .bg(theme::elevated())
                        .border_1()
                        .border_color(theme::border())
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .gap(sc(18.))
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(div().text_size(sc(20.)).font_weight(FontWeight::BOLD).child(heading))
                        .child(body)
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(sc(10.))
                                .child(pill("dlg-cancel", "Cancelar", None, false, |this, _, window, cx| this.close_dialog(window, cx), cx))
                                .child(
                                    div()
                                        .when(danger, |d| d.text_color(theme::danger()))
                                        .child(pill("dlg-ok", confirm.to_string(), None, true, |this, _, window, cx| {
                                            this.submit_dialog(window, cx)
                                        }, cx)),
                                ),
                        ),
                ),
        )
        .with_priority(3)
        .into_any_element(),
    )
}
