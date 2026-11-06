//! Album browser in "My Collection → Albums": clicking a cover unfolds its
//! tracks right under that row (MusicBee / foobar2000 style) instead of
//! opening the album page.

use super::sc;
use gpui::{div, prelude::*, px, AnyElement, Animation, AnimationExt, ClickEvent, Context, FontWeight, MouseButton, MouseDownEvent, SharedString};

use super::icons::Icon;
use super::widgets::{cover, fmt_long, fmt_time, icon_button, pill, quality_badge};
use super::{theme, AppView, Load, MenuTarget, Page};
use crate::tidal::Album;

impl AppView {
    /// Toggle the inline track list of an album in the collection grid.
    pub fn toggle_album_inline(&mut self, album: &Album, cx: &mut Context<Self>) {
        if self.expanded_album == Some(album.id) {
            self.expanded_album = None;
        } else {
            self.expanded_album = Some(album.id);
            self.inline_anim += 1;
            let id = album.id;
            if !self.albums.contains_key(&id) {
                self.albums.insert(id, Load::Loading);
                self.fetch(cx, move |t| t.album(id), move |this, r, _| {
                    this.albums.insert(id, super::into_load(r));
                });
            }
        }
        self.content_changed_pub();
        cx.notify();
    }
}

/// The unfolded panel: cover and details on the left, tracks on the right.
pub fn render(view: &mut AppView, album: Album, column: usize, cols: usize, cx: &mut Context<AppView>) -> AnyElement {
    let id = album.id;
    let tracks = view.albums.get(&id).and_then(Load::ready).map(|(_, t)| t.clone());
    let playing_id = view.now.as_ref().map(|n| n.track.id);
    let playing = view.now.is_some() && !view.snap.paused && !view.ended;
    let artist_id = album.artists.first().map(|a| a.id);
    // Caret under the clicked card.
    let caret_x = 24.0 + 8.0 + column as f32 * (theme::CARD_W + 8.0) + theme::CARD_W / 2.0 - 10.0;

    let total: u64 = tracks.as_ref().map(|t| t.iter().map(|x| x.duration as u64).sum()).unwrap_or(album.duration as u64);
    let n = tracks.as_ref().map(|t| t.len()).unwrap_or(album.number_of_tracks as usize);
    let meta = format!(
        "{}{} pistas · {}",
        if album.year().is_empty() { String::new() } else { format!("{} · ", album.year()) },
        n,
        fmt_long(total)
    );

    let list: AnyElement = match &tracks {
        None => div().p(sc(20.)).text_color(theme::text_dim()).child("Cargando pistas…").into_any_element(),
        Some(tracks) => {
            let multi_disc = tracks.iter().any(|t| t.volume_number > 1);
            let row = |i: usize, cx: &mut Context<AppView>| -> AnyElement {
                let t = tracks[i].clone();
                let current = playing_id == Some(t.id);
                let fav = view.fav_ids.contains(&t.id);
                let t_menu = t.clone();
                let t_fav = t.clone();
                let rated = view.ratings.get(t.id);
                let group: SharedString = format!("inl-{i}").into();
                div()
                    .id(("inline-track", i))
                    .h(sc(34.))
                    .px(sc(10.))
                    .flex()
                    .items_center()
                    .gap(sc(10.))
                    .rounded(sc(6.))
                    .cursor_pointer()
                    .group(group.clone())
                    .when(current, |d| d.bg(theme::accent_dim()))
                    .hover(|s| s.bg(theme::hover()))
                    .active(|s| s.bg(theme::active()))
                    // Track number, replaced by a play arrow on hover.
                    .child(
                        div()
                            .relative()
                            .w(sc(26.))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .text_size(sc(12.))
                            .text_color(if current { theme::accent() } else { theme::text_dim() })
                            .child(
                                div()
                                    .group_hover(group.clone(), |s| s.invisible())
                                    .child(if current {
                                        super::widgets::eq_bars(playing, theme::accent()).into_any_element()
                                    } else if multi_disc {
                                        div().child(format!("{}.{}", t.volume_number, t.track_number)).into_any_element()
                                    } else {
                                        div().child(t.track_number.to_string()).into_any_element()
                                    }),
                            )
                            .child(
                                div()
                                    .id(("inline-play", i))
                                    .absolute()
                                    .invisible()
                                    .group_hover(group.clone(), |s| s.visible())
                                    .cursor_pointer()
                                    .child(if current && playing { Icon::Pause } else { Icon::Play }.el(15., theme::text()))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        if current {
                                            this.toggle_play(cx);
                                            return;
                                        }
                                        if let Some(list) = this.albums.get(&id).and_then(Load::ready).map(|(_, t)| t.clone()) {
                                            this.shuffle = false;
                                            this.play_tracks(list, i, cx);
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(sc(0.))
                            .truncate()
                            .text_size(sc(13.))
                            .text_color(if current { theme::accent() } else { theme::text() })
                            .child(t.display_title()),
                    )
                    // Stars: always shown when rated, otherwise on hover.
                    .child(
                        div()
                            .flex_none()
                            .when(rated == 0, |d| d.invisible().group_hover(group.clone(), |s| s.visible()))
                            .child(super::widgets::stars("inl", t.id, 12., view, cx)),
                    )
                    // Like: always shown when liked, otherwise on hover.
                    .child(
                        div()
                            .id(("inline-fav", i))
                            .flex_none()
                            .cursor_pointer()
                            .when(!fav, |d| d.invisible().group_hover(group.clone(), |s| s.visible()))
                            .active(|s| s.opacity(0.5))
                            .child(if fav {
                                Icon::HeartFilled.el(14. * view.heart_scale(t.id), theme::text())
                            } else {
                                Icon::Heart.el(14., theme::text_secondary())
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle_favorite(&t_fav, cx)
                            })),
                    )
                    .child(div().flex_none().text_size(sc(12.)).text_color(theme::text_secondary()).child(fmt_time(t.duration as f64)))
                    .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        if ev.click_count() >= 2 {
                            if let Some(list) = this.albums.get(&id).and_then(Load::ready).map(|(_, t)| t.clone()) {
                                this.shuffle = false;
                                this.play_tracks(list, i, cx);
                            }
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                            this.menu = Some((ev.position, MenuTarget::Track(t_menu.clone())));
                            cx.notify();
                        }),
                    )
                    .into_any_element()
            };
            // Adapt to the room: as many columns as fit (>= 260 px each), but only as
            // many as the album needs (about 10 rows per column), so long albums
            // spread out instead of growing very tall.
            let grid_w = cols as f32 * (theme::CARD_W + 8.0);
            let list_w = (grid_w - 200.0 - 24.0 - 40.0).max(260.0);
            let fit = ((list_w + 16.0) / 276.0).floor().clamp(1.0, 4.0) as usize;
            let need = ((tracks.len() + 9) / 10).clamp(1, 4);
            let ncols = fit.min(need);
            // Flat list with a header before each disc of a multi-disc album.
            let mut flat: Vec<Result<usize, u32>> = Vec::new();
            let mut disc = 0u32;
            for (i, t) in tracks.iter().enumerate() {
                if multi_disc && t.volume_number != disc {
                    disc = t.volume_number;
                    flat.push(Err(disc));
                }
                flat.push(Ok(i));
            }
            let per = (flat.len() + ncols - 1) / ncols;
            let columns: Vec<AnyElement> = flat
                .chunks(per.max(1))
                .map(|chunk| {
                    let rows: Vec<AnyElement> = chunk
                        .iter()
                        .map(|e| match e {
                            Ok(i) => row(*i, cx),
                            Err(d) => div()
                                .h(sc(30.))
                                .px(sc(10.))
                                .flex()
                                .items_end()
                                .pb(sc(4.))
                                .text_size(sc(10.5))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme::text_dim())
                                .child(format!("DISCO {d}"))
                                .into_any_element(),
                        })
                        .collect();
                    div().flex_1().min_w(sc(0.)).flex().flex_col().children(rows).into_any_element()
                })
                .collect();
            div().flex_1().min_w(sc(0.)).flex().gap(sc(16.)).children(columns).into_any_element()
        }
    };

    let a_open = album.clone();
    div()
        .pb(sc(18.))
        .child(
            // Caret pointing at the open album.
            div().h(sc(10.)).relative().child(
                div()
                    .absolute()
                    .left(sc(caret_x - 24.0))
                    .top(sc(2.))
                    .size(sc(16.))
                    .rounded(sc(3.))
                    .bg(theme::surface()),
            ),
        )
        .child(
            div()
                .id(("inline-panel", id as usize))
                .p(sc(20.))
                .rounded(sc(12.))
                .bg(theme::surface())
                .border_1()
                .border_color(theme::border())
                .shadow_lg()
                .flex()
                .gap(sc(24.))
                .child(
                    div()
                        .w(sc(200.))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap(sc(8.))
                        .child(div().shadow_lg().child(cover(album.cover(640), 200., false, cx)))
                        .child(div().pt(sc(6.)).text_size(sc(17.)).font_weight(FontWeight::BOLD).line_clamp(2).child(album.title.clone()))
                        .child(
                            div()
                                .id("inline-artist")
                                .text_size(sc(13.))
                                .text_color(theme::text_secondary())
                                .cursor_pointer()
                                .hover(|s| s.underline().text_color(theme::text()))
                                .child(album.artist_names())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(a) = artist_id {
                                        this.navigate(Page::Artist(a), cx)
                                    }
                                })),
                        )
                        .child(div().text_size(sc(12.)).text_color(theme::text_dim()).child(meta))
                        .child(super::widgets::album_stars("inl-album", id, 15., view, cx))
                        .child(div().flex().gap(sc(6.)).children(quality_badge(&album.tags)))
                        .child(
                            div()
                                .pt(sc(8.))
                                .flex()
                                .items_center()
                                .gap(sc(6.))
                                .child(pill("inline-play", "Reproducir", Some(Icon::Play), true, move |this, _, _, cx| {
                                    if let Some(t) = this.albums.get(&id).and_then(Load::ready).map(|(_, t)| t.clone()) {
                                        this.shuffle = false;
                                        this.play_tracks(t, 0, cx)
                                    }
                                }, cx))
                                .child(icon_button("inline-shuffle", Icon::Shuffle, 20., theme::text_secondary(), move |this, _, _, cx| {
                                    if let Some(t) = this.albums.get(&id).and_then(Load::ready).map(|(_, t)| t.clone()) {
                                        this.play_shuffled(t, cx)
                                    }
                                }, cx))
                                .child(icon_button("inline-open", Icon::Forward, 20., theme::text_secondary(), move |this, _, _, cx| {
                                    this.navigate(Page::Album(a_open.id), cx)
                                }, cx))
                                .child(icon_button("inline-close", Icon::Close, 18., theme::text_secondary(), move |this, _, _, cx| {
                                    this.expanded_album = None;
                                    this.content_changed_pub();
                                    cx.notify();
                                }, cx)),
                        ),
                )
                .child(list),
        )
        .with_animation(
            ("inline-anim", view.inline_anim),
            Animation::new(std::time::Duration::from_millis(260)).with_easing(super::motion::ease_out),
            |el, t| el.opacity(t).top(sc((1.0 - t) * -10.0)),
        )
        .into_any_element()
}
