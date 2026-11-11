//! Album browser in "My Collection → Albums": clicking a cover unfolds its
//! tracks right under that row (MusicBee / foobar2000 style) instead of
//! opening the album page.

use super::sc;
use gpui::{
    div, prelude::*, px, Animation, AnimationExt, AnyElement, ClickEvent, Context, FontWeight, MouseButton, MouseDownEvent, SharedString,
};

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
            if let std::collections::hash_map::Entry::Vacant(e) = self.albums.entry(id) {
                e.insert(Load::Loading);
                self.fetch(
                    cx,
                    move |t| t.album(id),
                    move |this, r, _| {
                        this.albums.insert(id, super::into_load(r));
                    },
                );
            }
        }
        self.content_changed_pub();
        cx.notify();
    }
}

/// The unfolded panel: cover and details on the left, tracks on the right.
pub fn render(view: &mut AppView, album: Album, column: usize, cols: usize, card_w: f32, cx: &mut Context<AppView>) -> AnyElement {
    let id = album.id;
    let tracks = view.albums.get(&id).and_then(Load::ready).map(|(_, t)| t.clone());
    let playing_id = view.now.as_ref().map(|n| n.track.id);
    let playing = view.now.is_some() && !view.snap.paused && !view.ended;
    let artist_id = album.artists.first().map(|a| a.id);
    // Centre of the clicked card, where the tip points.
    // (same metrics as the grid above it: real card width and gap)
    let tip_cx = column as f32 * (card_w + theme::GAP) + card_w / 2.0;
    let grid_w = cols as f32 * card_w + cols.saturating_sub(1) as f32 * theme::GAP;
    let stacked = grid_w < 570.;
    let inset = if stacked { super::widgets::PANEL_PAD_SM } else { super::widgets::PANEL_PAD };
    let cover_size = (grid_w - 2. * inset).clamp(1., if stacked { 144. } else { 200. });
    let list_w = if stacked { grid_w - 2. * inset } else { grid_w - 200. - 24. - 2. * inset };

    let total: u64 = tracks.as_ref().map(|t| t.iter().map(|x| x.duration as u64).sum()).unwrap_or(album.duration as u64);
    let n = tracks.as_ref().map(|t| t.len()).unwrap_or(album.number_of_tracks as usize);
    let meta = crate::i18n::text(
        "tracks.count_duration",
        &[
            (if album.year().is_empty() { String::new() } else { format!("{} · ", album.year()) }).to_string(),
            format!("{}", n),
            fmt_long(total).to_string(),
        ],
    );

    let list: AnyElement = match &tracks {
        None => div().p(sc(20.)).text_color(theme::text_dim()).child(crate::i18n::t("loading_tracks")).into_any_element(),
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
                let rk = format!("irow-{id}-{i}");
                let (h, p) = (super::interact::hover(&rk), super::interact::press(&rk));
                let row = div()
                    .id(("inline-track", i))
                    .h(sc(42.))
                    .px(sc(10.))
                    .border_b_1()
                    .border_color(theme::divider())
                    .flex()
                    .items_center()
                    .gap(sc(10.))
                    .rounded(sc(6.))
                    .cursor_pointer()
                    .group(group.clone())
                    .relative()
                    // Hover fades in, press deepens; the accent strip grows from the middle.
                    .bg(if current {
                        theme::accent_dim()
                    } else {
                        super::interact::mix(super::fx::with_alpha(theme::hover(), h), theme::active(), p)
                    })
                    .child(
                        div()
                            .absolute()
                            .left(sc(0.))
                            .top(sc(9.))
                            .bottom(sc(9.))
                            .w(sc(2.))
                            .rounded(sc(1.))
                            .bg(super::fx::with_alpha(theme::accent(), 0.85 * h)),
                    )
                    // Track number, replaced by a play arrow on hover.
                    .child(
                        div()
                            .relative()
                            .w(sc(26.))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .text_size(sc(theme::TYPE_LABEL))
                            .text_color(if current { theme::accent() } else { theme::text_dim() })
                            .child(div().group_hover(group.clone(), |s| s.invisible()).child(if current {
                                super::widgets::eq_bars(playing, theme::accent()).into_any_element()
                            } else if multi_disc {
                                div().child(format!("{}.{}", t.volume_number, t.track_number)).into_any_element()
                            } else {
                                div().child(t.track_number.to_string()).into_any_element()
                            }))
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
                    // Ratings give way to the title in the narrowest layout.
                    .when(list_w >= 300., |d| {
                        d.child(
                            div()
                                .flex_none()
                                .when(rated == 0, |d| d.invisible().group_hover(group.clone(), |s| s.visible()))
                                .child(super::widgets::stars("inl", t.id, 12., view, cx)),
                        )
                    })
                    // Like: always shown when liked, otherwise on hover.
                    .child(
                        div()
                            .id(("inline-fav", i))
                            .flex_none()
                            // Local files cannot be TIDAL favourites.
                            .when(t_fav.local_path.is_some(), |d| d.invisible())
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
                    .child(
                        div()
                            .flex_none()
                            .text_size(sc(theme::TYPE_LABEL))
                            .text_color(theme::text_secondary())
                            .child(fmt_time(t.duration as f64)),
                    )
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
                    );
                super::interact::track(row, rk, cx).into_any_element()
            };
            // Adapt to the room: as many columns as fit (>= 260 px each), but only as
            // many as the album needs (about 10 rows per column), so long albums
            // spread out instead of growing very tall.
            let fit = ((list_w + 16.0) / 276.0).floor().clamp(1.0, 4.0) as usize;
            let need = tracks.len().div_ceil(10).clamp(1, 4);
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
            let per = flat.len().div_ceil(ncols);
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
                                .child(crate::i18n::text("disc.heading", &[format!("{}", d)]))
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
    // The tip and the panel are one surface: the tip is drawn after the panel,
    // over its top border, in the panel's colour, with a 1 px rim matching the
    // border (the border over the panel colour) along its two sides.
    const TIP_W: f32 = 24.0;
    const TIP_H: f32 = 12.0;
    // The tip continues the glass panel: its top colour and rim.
    let surface = theme::glass_top();
    let rim = theme::glass_rim(0.);
    let tip_left = (tip_cx - TIP_W / 2.0).max(14.0);
    let tip = |w: f32, h: f32, color: gpui::Rgba| gpui::svg().path("shapes/tip.svg").w(sc(w)).h(sc(h)).text_color(color);
    div()
        .pb(sc(18.))
        .relative()
        // Room for the tip above the panel.
        .pt(sc(TIP_H - 1.0))
        .child(
            div()
                .id(("inline-panel", id as usize))
                .p(sc(inset))
                .relative()
                .rounded(sc(14.))
                .bg(theme::liquid_glass(0.))
                .border_1()
                .border_color(theme::glass_rim(0.))
                .shadow(theme::glass_shadow())
                .child(super::glass::sheen(("inline-panel-glass", id as usize), 14., 0.55))
                .flex()
                .when(stacked, |d| d.flex_col())
                .gap(sc(24.))
                .child(
                    div()
                        .when(stacked, |d| d.w_full())
                        .when(!stacked, |d| d.w(sc(200.)))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap(sc(8.))
                        // Click the cover to see it full screen.
                        .child({
                            let big = album.cover(1280).or_else(|| album.cover(640));
                            let small = album.cover(640);
                            div()
                                .id("inline-cover")
                                .cursor_pointer()
                                .shadow_lg()
                                .child(cover(album.cover(640), cover_size, false, cx))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    if let Some(u) = big.clone() {
                                        this.open_cover_view(u, small.clone(), cx);
                                    }
                                }))
                        })
                        .child(
                            super::widgets::nav_link(
                                div().id("inline-title").pt(sc(6.)).text_size(sc(17.)).font_weight(FontWeight::BOLD).line_clamp(2),
                                format!("inl-title-{id}"),
                                cx,
                            )
                            .child(album.title.clone())
                            .on_click(cx.listener(move |this, _, _, cx| this.navigate(Page::Album(id), cx))),
                        )
                        .child(
                            super::widgets::nav_link_secondary(
                                div().id("inline-artist").text_size(sc(13.)),
                                format!("inl-artist-{id}"),
                                cx,
                            )
                            .child(album.artist_names())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(a) = artist_id.filter(|a| *a != 0) {
                                    this.navigate(Page::Artist(a), cx)
                                }
                            })),
                        )
                        .child(div().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_dim()).child(meta))
                        .child(super::widgets::album_stars("inl-album", id, 15., view, cx))
                        .child(div().flex().gap(sc(6.)).children(quality_badge(&album.tags)))
                        .child(
                            div()
                                .pt(sc(8.))
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap(sc(6.))
                                .child(pill(
                                    "inline-play",
                                    crate::i18n::t("play"),
                                    Some(Icon::Play),
                                    true,
                                    move |this, _, _, cx| {
                                        if let Some(t) = this.albums.get(&id).and_then(Load::ready).map(|(_, t)| t.clone()) {
                                            this.shuffle = false;
                                            this.play_tracks(t, 0, cx)
                                        }
                                    },
                                    cx,
                                ))
                                .child(icon_button(
                                    "inline-shuffle",
                                    Icon::Shuffle,
                                    20.,
                                    theme::text_secondary(),
                                    move |this, _, _, cx| {
                                        if let Some(t) = this.albums.get(&id).and_then(Load::ready).map(|(_, t)| t.clone()) {
                                            this.play_shuffled(t, cx)
                                        }
                                    },
                                    cx,
                                ))
                                .child(icon_button(
                                    "inline-open",
                                    Icon::Forward,
                                    20.,
                                    theme::text_secondary(),
                                    move |this, _, _, cx| this.navigate(Page::Album(a_open.id), cx),
                                    cx,
                                ))
                                .child(icon_button(
                                    "inline-close",
                                    Icon::Close,
                                    18.,
                                    theme::text_secondary(),
                                    move |this, _, _, cx| {
                                        this.expanded_album = None;
                                        this.content_changed_pub();
                                        cx.notify();
                                    },
                                    cx,
                                )),
                        ),
                )
                .child(list),
        )
        .child(
            div()
                .absolute()
                .top(sc(0.))
                .left(sc(tip_left - 1.0))
                .w(sc(TIP_W + 2.0))
                .h(sc(TIP_H + 1.0))
                // Rim (one size larger, behind), then the fill reaching 1 px into
                // the panel so it covers the border line under its base.
                // (the rim is cut exactly at the bottom of the panel's border line)
                .child(div().absolute().top(sc(0.)).left(sc(0.)).w(sc(TIP_W + 2.0)).h(sc(TIP_H)).overflow_hidden().child(tip(
                    TIP_W + 2.0,
                    TIP_H + 1.0,
                    rim,
                )))
                .child(div().absolute().top(sc(1.5)).left(sc(1.)).child(tip(TIP_W, TIP_H, surface))),
        )
        .with_animation(("inline-anim", view.inline_anim), super::motion::animation(super::motion::PANEL), |el, t| {
            el.opacity(t).top(sc((1.0 - t) * -10.0))
        })
        .into_any_element()
}
