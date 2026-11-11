//! Right-click menus for tracks, albums and artists: one visual language
//! (solid floating panel, a header with the artwork, a row of main actions,
//! grouped options, an expandable "Add to playlist" and a red "Remove" at the
//! end), with only the actions that make sense for each kind of content.

use gpui::{div, prelude::*, AnyElement, ClipboardItem, Context, FontWeight, SharedString, Window};

use super::icons::Icon;
use super::info::InfoTarget;
use super::playlists::{Dialog, RemoveKind};
use super::widgets::cover;
use super::{interact, sc, theme, AppView, Item, Load, MenuTarget, Page};
use crate::tidal::{Album, Artist, Track};

type Act = std::rc::Rc<dyn Fn(&mut AppView, &mut Window, &mut Context<AppView>)>;

const W: f32 = 286.0;

fn header(art: AnyElement, title: String, sub: String) -> AnyElement {
    div()
        .px(sc(12.))
        .pt(sc(12.))
        .pb(sc(10.))
        .flex()
        .items_center()
        .gap(sc(12.))
        .child(art)
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .flex()
                .flex_col()
                .gap(sc(2.))
                .child(div().truncate().text_size(sc(13.5)).font_weight(FontWeight::SEMIBOLD).child(title))
                .when(!sub.is_empty(), |d| d.child(div().truncate().text_size(sc(11.5)).text_color(theme::text_secondary()).child(sub))),
        )
        .into_any_element()
}

fn sep() -> AnyElement {
    div().mx(sc(10.)).my(sc(5.)).h(sc(1.)).bg(theme::divider()).into_any_element()
}

/// The three main actions, side by side.
fn actions(items: Vec<(&'static str, Icon, String, bool, Act)>, cx: &mut Context<AppView>) -> AnyElement {
    div()
        .px(sc(8.))
        .pb(sc(4.))
        .flex()
        .gap(sc(6.))
        .children(items.into_iter().map(|(id, icon, label, on, act)| {
            let key = format!("cm-{id}");
            let (h, p) = (interact::hover(&key), interact::press(&key));
            let color = if on { theme::accent() } else { interact::mix(theme::text_secondary(), theme::text(), h) };
            let el = div()
                .id(id)
                .flex_1()
                .h(sc(54.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(sc(5.))
                .rounded(sc(8.))
                .cursor_pointer()
                .bg(theme::ink(0.04 + 0.06 * h + 0.05 * p))
                .child(super::glass::sheen("action-glass", 8., 0.8))
                .child(icon.el_scaled(17., color, 1.0 - 0.07 * p))
                .child(div().text_size(sc(11.)).font_weight(FontWeight::MEDIUM).text_color(color).child(label))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.menu = None;
                    act(this, window, cx);
                    cx.notify();
                }));
            interact::track(el, key, cx).into_any_element()
        }))
        .into_any_element()
}

/// How a vertical option looks and behaves.
#[derive(Clone, Copy, Default)]
struct Style {
    /// Red text and icon (Remove).
    danger: bool,
    /// Small icon at the end (the expander's chevron).
    trailing: Option<Icon>,
    /// Leave the menu open after the click (expanders).
    keep_open: bool,
}

/// A vertical option of the menu.
fn option(id: &str, icon: Icon, label: &str, st: Style, act: Act, cx: &mut Context<AppView>) -> AnyElement {
    let Style { danger, trailing, keep_open } = st;
    let key = format!("cmo-{id}");
    let (h, p) = (interact::hover(&key), interact::press(&key));
    let color = if danger { theme::danger() } else { theme::text() };
    let el = div()
        .id(SharedString::from(key.clone()))
        .mx(sc(6.))
        .h(sc(34.))
        .px(sc(10.))
        .flex()
        .items_center()
        .gap(sc(10.))
        .rounded(sc(7.))
        .cursor_pointer()
        .bg(if danger { super::fx::with_alpha(theme::danger(), 0.12 * h + 0.06 * p) } else { theme::ink(0.07 * h + 0.05 * p) })
        .child(super::glass::sheen("option-glass", 7., if danger { 0.42 } else { 0.74 }))
        .child(icon.el(15., if danger { theme::danger() } else { theme::text_secondary() }))
        .child(div().flex_1().min_w(sc(0.)).truncate().text_size(sc(13.)).text_color(color).child(label.to_string()))
        .children(trailing.map(|t| t.el(12., theme::text_dim())))
        .on_click(cx.listener(move |this, _, window, cx| {
            if !keep_open {
                this.menu = None;
            }
            act(this, window, cx);
            cx.notify();
        }));
    interact::track(el, key, cx).into_any_element()
}

fn act(f: impl Fn(&mut AppView, &mut Window, &mut Context<AppView>) + 'static) -> Act {
    std::rc::Rc::new(f)
}

fn share(kind: &'static str, id: String) -> Act {
    act(move |v, _, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(crate::tidal::share_url(kind, &id)));
        v.notify_toast(crate::i18n::t("link_copied"), cx);
    })
}

/// "Add to playlist", unfolding the user's playlists (cover and name) in a
/// scrolling list; `tracks` resolves the tracks to add (an album loads them).
fn add_to_playlist(
    view: &mut AppView,
    open: bool,
    add: std::rc::Rc<dyn Fn(&mut AppView, String, &mut Context<AppView>)>,
    new_ids: Option<Vec<u64>>,
    cx: &mut Context<AppView>,
) -> Vec<AnyElement> {
    let mut out = vec![option(
        "addpl",
        Icon::Plus,
        crate::i18n::t("add_to_playlist"),
        Style { trailing: Some(if open { Icon::ChevUp } else { Icon::ChevDown }), keep_open: true, ..Default::default() },
        act(|v, _, cx| {
            v.ctx_pl_open = !v.ctx_pl_open;
            cx.notify();
        }),
        cx,
    )];
    if open {
        let lists = view.own_playlists();
        let mut rows: Vec<AnyElement> = Vec::new();
        if let Some(ids) = new_ids {
            rows.push(option(
                "addpl-new",
                Icon::Plus,
                crate::i18n::t("new_playlist"),
                Style::default(),
                act(move |v, window, cx| v.open_dialog(Dialog::NewPlaylist { add: ids.clone() }, window, cx)),
                cx,
            ));
        }
        for (i, pl) in lists.into_iter().enumerate() {
            let key = format!("cmpl-{i}");
            let h = interact::hover(&key);
            let uuid = pl.uuid.clone();
            let add = add.clone();
            let el = div()
                .id(SharedString::from(key.clone()))
                .mx(sc(6.))
                .h(sc(40.))
                .px(sc(8.))
                .flex()
                .items_center()
                .gap(sc(10.))
                .rounded(sc(7.))
                .cursor_pointer()
                .bg(theme::ink(0.07 * h))
                .child(div().rounded(sc(4.)).overflow_hidden().child(view.playlist_cover(&pl, 28., cx)))
                .child(div().flex_1().min_w(sc(0.)).truncate().text_size(sc(12.5)).child(pl.title.clone()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.menu = None;
                    add(this, uuid.clone(), cx);
                    cx.notify();
                }));
            rows.push(interact::track(el, key, cx).into_any_element());
        }
        if rows.is_empty() {
            rows.push(
                div()
                    .px(sc(16.))
                    .py(sc(8.))
                    .text_size(sc(12.))
                    .text_color(theme::text_dim())
                    .child(crate::i18n::t("you_have_no_playlists_of_your_own"))
                    .into_any_element(),
            );
        }
        out.push(
            div()
                .id("cm-pl-list")
                .max_h(sc(200.))
                .overflow_y_scroll()
                .track_scroll(&super::smooth::handle("ctx-pl"))
                .child(super::smooth::glide("ctx-pl"))
                .flex()
                .flex_col()
                .children(rows)
                .into_any_element(),
        );
    }
    out
}

fn local_track_menu(view: &AppView, track: Track, cx: &mut Context<AppView>) -> Vec<AnyElement> {
    let mut rows = vec![header(cover(track.cover(160), 44., false, cx), track.display_title(), crate::i18n::t("local_file").into())];
    let (play, queue, next) = (track.clone(), track.clone(), track.clone());
    if let Some(id) = track.album.as_ref().map(|a| a.id) {
        rows.push(option(
            "local-view-album",
            Icon::Album,
            crate::i18n::t("view_album"),
            Style::default(),
            act(move |v, _, cx| v.navigate(Page::Album(id), cx)),
            cx,
        ));
    }
    if let Some(id) = track.artists.first().map(|a| a.id) {
        rows.push(option(
            "local-view-artist",
            Icon::Artist,
            crate::i18n::t("view_artist"),
            Style::default(),
            act(move |v, _, cx| v.navigate(Page::Artist(id), cx)),
            cx,
        ));
    }
    rows.push(option(
        "local-play",
        Icon::Play,
        crate::i18n::t("play"),
        Style::default(),
        act(move |v, _, cx| v.play_tracks(vec![play.clone()], 0, cx)),
        cx,
    ));
    rows.push(option(
        "local-next",
        Icon::Queue,
        crate::i18n::t("play_next"),
        Style::default(),
        act(move |v, _, cx| v.add_to_queue(next.clone(), true, cx)),
        cx,
    ));
    rows.push(option(
        "local-queue",
        Icon::Queue,
        crate::i18n::t("add_to_queue"),
        Style::default(),
        act(move |v, _, cx| v.add_to_queue(queue.clone(), false, cx)),
        cx,
    ));
    if view.local_tracks.iter().any(|t| t.id == track.id) {
        let id = track.id;
        rows.push(sep());
        rows.push(option(
            "local-settings",
            Icon::Edit,
            crate::i18n::t("local.edit_metadata"),
            Style::default(),
            act(move |v, _, cx| v.edit_local_in_settings(id, cx)),
            cx,
        ));
    }
    rows
}

fn track_menu(view: &mut AppView, t: Track, playlist: Option<(String, usize)>, cx: &mut Context<AppView>) -> Vec<AnyElement> {
    if t.local_path.is_some() {
        return local_track_menu(view, t, cx);
    }
    let fav = view.fav_ids.contains(&t.id);
    let sub = [t.album.as_ref().map(|a| a.title.clone()).unwrap_or_default(), t.artist_names()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let mut out = vec![header(
        div().rounded(sc(5.)).overflow_hidden().child(cover(t.cover(160), 44., false, cx)).into_any_element(),
        t.display_title(),
        sub,
    )];
    let (a, b) = (t.clone(), t.clone());
    out.push(actions(
        vec![
            ("cm-play", Icon::Play, crate::i18n::t("play").into(), false, act(move |v, _, cx| v.play_tracks(vec![a.clone()], 0, cx))),
            (
                "cm-fav",
                if fav { Icon::HeartFilled } else { Icon::Heart },
                crate::i18n::t("add_to_favorites").into(),
                fav,
                act(move |v, _, cx| v.toggle_favorite(&b, cx)),
            ),
            ("cm-share", Icon::Link, crate::i18n::t("share").into(), false, share("track", t.id.to_string())),
        ],
        cx,
    ));
    // Queue (kept from the previous menu).
    let (q1, q2, r) = (t.clone(), t.clone(), t.clone());
    out.push(option(
        "next",
        Icon::Queue,
        crate::i18n::t("play_next"),
        Style::default(),
        act(move |v, _, cx| v.add_to_queue(q1.clone(), true, cx)),
        cx,
    ));
    out.push(option(
        "queue",
        Icon::Queue,
        crate::i18n::t("add_to_queue"),
        Style::default(),
        act(move |v, _, cx| v.add_to_queue(q2.clone(), false, cx)),
        cx,
    ));
    out.push(option("radio", Icon::Radio, crate::i18n::t("start_radio"), Style::default(), act(move |v, _, cx| v.start_radio(&r, cx)), cx));
    out.push(sep());
    if let Some(al) = t.album.clone() {
        out.push(option(
            "view-album",
            Icon::Album,
            crate::i18n::t("view_album"),
            Style::default(),
            act(move |v, _, cx| v.navigate(Page::Album(al.id), cx)),
            cx,
        ));
    }
    let info = t.clone();
    out.push(option(
        "song-info",
        Icon::Track,
        crate::i18n::t("song_info"),
        Style::default(),
        act(move |v, _, cx| v.open_info(InfoTarget::Song(info.clone()), cx)),
        cx,
    ));
    out.push(sep());
    let add_t = t.clone();
    out.extend(add_to_playlist(
        view,
        view.ctx_pl_open,
        std::rc::Rc::new(move |v: &mut AppView, uuid: String, cx: &mut Context<AppView>| {
            v.add_tracks_to_playlist(uuid, vec![add_t.clone()], cx)
        }),
        Some(vec![t.id]),
        cx,
    ));
    // Remove: from this playlist, or from the collection; always confirmed.
    let remove = match playlist {
        Some((uuid, index)) => Some(RemoveKind::PlaylistTrack { uuid, index, title: t.display_title() }),
        None if fav => Some(RemoveKind::FavTrack(t.clone())),
        None => None,
    };
    if let Some(k) = remove {
        let label = if matches!(k, RemoveKind::PlaylistTrack { .. }) {
            crate::i18n::t("remove_from_playlist")
        } else {
            crate::i18n::t("remove_from_your_collection")
        };
        out.push(sep());
        out.push(option(
            "remove",
            Icon::Close,
            label,
            Style { danger: true, ..Default::default() },
            act(move |v, window, cx| v.open_dialog(Dialog::ConfirmRemove(k.clone()), window, cx)),
            cx,
        ));
    }
    out
}

fn album_menu(view: &mut AppView, al: Album, cx: &mut Context<AppView>) -> Vec<AnyElement> {
    let fav = view.fav_album_ids.contains(&al.id);
    let sub = [al.artist_names(), al.year().to_string()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    let mut out = vec![header(
        div().rounded(sc(5.)).overflow_hidden().child(cover(al.cover(160), 48., false, cx)).into_any_element(),
        al.title.clone(),
        sub,
    )];
    if crate::local::is_local_id(al.id) {
        // An album made from local files: playback and the Library only.
        let (id, play) = (al.id, al.clone());
        let tracks = view.albums.get(&id).and_then(Load::ready).map(|(_, t)| t.clone()).unwrap_or_default();
        let (next, queue) = (tracks.clone(), tracks);
        out.push(option(
            "local-album-play",
            Icon::Play,
            crate::i18n::t("play"),
            Style::default(),
            act(move |v, _, cx| v.play_item(&Item::Album(play.clone()), cx)),
            cx,
        ));
        out.push(option(
            "local-album-view",
            Icon::Album,
            crate::i18n::t("view_album"),
            Style::default(),
            act(move |v, _, cx| v.navigate(Page::Album(id), cx)),
            cx,
        ));
        out.push(option(
            "local-album-next",
            Icon::Queue,
            crate::i18n::t("play_next"),
            Style::default(),
            act(move |v, _, cx| next.iter().rev().for_each(|t| v.add_to_queue(t.clone(), true, cx))),
            cx,
        ));
        out.push(option(
            "local-album-queue",
            Icon::Queue,
            crate::i18n::t("add_to_queue"),
            Style::default(),
            act(move |v, _, cx| queue.iter().for_each(|t| v.add_to_queue(t.clone(), false, cx))),
            cx,
        ));
        out.push(sep());
        out.push(option(
            "local-album-settings",
            Icon::Settings,
            crate::i18n::t("settings"),
            Style::default(),
            act(move |v, _, cx| v.open_local_settings(cx)),
            cx,
        ));
        if let Some(id) = al.artists.first().map(|a| a.id) {
            out.push(option(
                "local-album-artist",
                Icon::Artist,
                crate::i18n::t("view_artist"),
                Style::default(),
                act(move |v, _, cx| v.navigate(Page::Artist(id), cx)),
                cx,
            ));
        }
        let info = al.clone();
        out.push(option(
            "local-album-info",
            Icon::Album,
            crate::i18n::t("album_info"),
            Style::default(),
            act(move |v, _, cx| v.open_info(InfoTarget::Album(info.clone()), cx)),
            cx,
        ));
        return out;
    }
    let (a, b) = (al.clone(), al.clone());
    out.push(actions(
        vec![
            ("cm-play", Icon::Play, crate::i18n::t("play").into(), false, act(move |v, _, cx| v.play_item(&Item::Album(a.clone()), cx))),
            (
                "cm-fav",
                if fav { Icon::HeartFilled } else { Icon::Heart },
                crate::i18n::t("add_to_favorites").into(),
                fav,
                act(move |v, _, cx| v.toggle_favorite_album(&b, cx)),
            ),
            ("cm-share", Icon::Link, crate::i18n::t("share").into(), false, share("album", al.id.to_string())),
        ],
        cx,
    ));
    out.push(sep());
    if let Some(ar) = al.artists.first().map(|a| a.id) {
        out.push(option(
            "view-artist",
            Icon::Artist,
            crate::i18n::t("view_artist"),
            Style::default(),
            act(move |v, _, cx| v.navigate(Page::Artist(ar), cx)),
            cx,
        ));
    }
    let info = al.clone();
    out.push(option(
        "album-info",
        Icon::Album,
        crate::i18n::t("album_info"),
        Style::default(),
        act(move |v, _, cx| v.open_info(InfoTarget::Album(info.clone()), cx)),
        cx,
    ));
    out.push(sep());
    let id = al.id;
    out.extend(add_to_playlist(
        view,
        view.ctx_pl_open,
        std::rc::Rc::new(move |v: &mut AppView, uuid: String, cx: &mut Context<AppView>| v.add_album_to_playlist(id, uuid, cx)),
        None,
        cx,
    ));
    if fav {
        let k = RemoveKind::FavAlbum(al.clone());
        out.push(sep());
        out.push(option(
            "remove",
            Icon::Close,
            crate::i18n::t("remove_from_your_collection"),
            Style { danger: true, ..Default::default() },
            act(move |v, window, cx| v.open_dialog(Dialog::ConfirmRemove(k.clone()), window, cx)),
            cx,
        ));
    }
    out
}

fn artist_menu(view: &mut AppView, ar: Artist, cx: &mut Context<AppView>) -> Vec<AnyElement> {
    let following = view.fav_artist_ids.contains(&ar.id);
    let mut out = vec![header(cover(ar.picture(320), 48., true, cx), ar.name.clone(), crate::i18n::t("artist").into())];
    let (a, id) = (ar.clone(), ar.id);
    if crate::local::is_local_id(id) {
        out.push(option(
            "local-artist-play",
            Icon::Play,
            crate::i18n::t("play"),
            Style::default(),
            act(move |v, _, cx| v.play_item(&Item::Artist(a.clone()), cx)),
            cx,
        ));
        out.push(option(
            "local-artist-view",
            Icon::Artist,
            crate::i18n::t("view_artist"),
            Style::default(),
            act(move |v, _, cx| v.navigate(Page::Artist(id), cx)),
            cx,
        ));
        out.push(option(
            "local-artist-info",
            Icon::Lyrics,
            crate::i18n::t("artist_info"),
            Style::default(),
            act(move |v, _, cx| v.open_info(InfoTarget::Artist(ar.clone()), cx)),
            cx,
        ));
        out.push(option(
            "local-artist-settings",
            Icon::Settings,
            crate::i18n::t("settings"),
            Style::default(),
            act(move |v, _, cx| v.open_local_settings(cx)),
            cx,
        ));
        return out;
    }
    out.push(actions(
        vec![
            ("cm-play", Icon::Play, crate::i18n::t("play").into(), false, act(move |v, _, cx| v.play_item(&Item::Artist(a.clone()), cx))),
            (
                "cm-follow",
                if following { Icon::HeartFilled } else { Icon::Heart },
                if following { crate::i18n::t("following").into() } else { crate::i18n::t("follow").into() },
                following,
                act(move |v, _, cx| v.toggle_follow_artist(id, cx)),
            ),
            ("cm-share", Icon::Link, crate::i18n::t("share").into(), false, share("artist", ar.id.to_string())),
        ],
        cx,
    ));
    out.push(sep());
    out.push(option(
        "open-artist",
        Icon::Artist,
        crate::i18n::t("view_artist"),
        Style::default(),
        act(move |v, _, cx| v.navigate(Page::Artist(id), cx)),
        cx,
    ));
    let info = ar.clone();
    out.push(option(
        "artist-info",
        Icon::Lyrics,
        crate::i18n::t("artist_info"),
        Style::default(),
        act(move |v, _, cx| v.open_info(InfoTarget::Artist(info.clone()), cx)),
        cx,
    ));
    if following {
        let k = RemoveKind::Unfollow(ar.id, ar.name.clone());
        out.push(sep());
        out.push(option(
            "remove",
            Icon::Close,
            crate::i18n::t("unfollow"),
            Style { danger: true, ..Default::default() },
            act(move |v, window, cx| v.open_dialog(Dialog::ConfirmRemove(k.clone()), window, cx)),
            cx,
        ));
    }
    out
}

/// The menu panel for `target`, or None for the other (plain) menus.
pub fn panel(view: &mut AppView, target: &MenuTarget, cx: &mut Context<AppView>) -> Option<AnyElement> {
    let rows = match target.clone() {
        MenuTarget::Track(t) => track_menu(view, t, None, cx),
        MenuTarget::PlaylistTrack { uuid, index, track } => track_menu(view, track, Some((uuid, index)), cx),
        MenuTarget::Album(a) => album_menu(view, a, cx),
        MenuTarget::Artist(a) => artist_menu(view, a, cx),
        _ => return None,
    };
    Some(
        super::widgets::floating_panel(12.)
            .id("menu")
            .occlude()
            .w(sc(W))
            .pb(sc(6.))
            .flex()
            .flex_col()
            .child(super::glass::sheen("context-menu-glass", 12., 0.65))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.menu = None;
                cx.notify();
            }))
            .children(rows)
            .into_any_element(),
    )
}

impl AppView {
    /// Add an album's tracks to a playlist (loading them if needed).
    pub fn add_album_to_playlist(&mut self, id: u64, uuid: String, cx: &mut Context<Self>) {
        if let Some((_, tracks)) = self.albums.get(&id).and_then(Load::ready) {
            let t = tracks.clone();
            self.add_tracks_to_playlist(uuid, t, cx);
            return;
        }
        self.fetch(
            cx,
            move |t| t.album(id),
            move |this, r, cx| match r {
                Ok((al, tracks)) => {
                    this.albums.insert(id, Load::Ready((al, tracks.clone())));
                    this.add_tracks_to_playlist(uuid.clone(), tracks, cx);
                }
                Err(e) => this.notify_toast(crate::i18n::text("could_not_load_the_album", &[format!("{:#}", e)]), cx),
            },
        );
    }
}
