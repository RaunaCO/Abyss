//! The bottom bar, laid out like the official app: track info with like and
//! menu on the left; shuffle / previous / play / next / repeat with the time
//! and progress line below them in the centre; queue, lyrics, volume and the
//! quality pill on the right.

use super::sc;
use gpui::{canvas, div, prelude::*, px, relative, AnimationExt, AnyElement, Context, FontWeight, MouseButton, MouseDownEvent, Window};

use super::icons::Icon;
use super::widgets::{cover, explicit_badge, fmt_time, icon_button};
use super::{theme, AppView, Drag, MenuTarget, Page};
use crate::audio::output::fmt_khz;
use crate::config::Repeat;

pub fn render(view: &mut AppView, _window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    let entity = cx.entity();
    let _ambient = view.ambient(cx);
    let _palette = view.palette(cx);
    let bar_frames = if view.settings.lock().np_blur {
        view.now.as_ref().and_then(|n| n.track.cover(640)).and_then(|u| super::images::frames(&u, cx))
    } else {
        None
    };
    let now = view.now.as_ref().map(|n| (n.track.clone(), n.info.clone()));
    let duration = view.duration();
    let position = view.display_position().min(duration.max(0.0));
    let frac = if duration > 0.0 { (position / duration) as f32 } else { 0.0 };
    let playing = now.is_some() && !view.snap.paused && !view.ended;
    let fav = now.as_ref().map(|(t, _)| view.fav_ids.contains(&t.id)).unwrap_or(false);

    // ---------------------------------------------------------- left
    let left = div().w(sc(410.)).flex_none().flex().items_center().gap(sc(12.)).min_w(sc(0.)).when_some(now.clone(), |d, (track, _)| {
        let album = track.album.as_ref().map(|a| a.id);
        let artist = track.artists.first().map(|a| a.id);
        let t_fav = track.clone();
        let t_more = track.clone();
        d.child(
            div()
                .id("np-cover")
                .relative()
                .flex_none()
                .cursor_pointer()
                .rounded(sc(4.))
                .group("bar-cover")
                .group_hover("bar-cover", |s| s.shadow_lg())
                .child(cover(track.cover(160), 42., false, cx))
                // Hover: the cover dims and shows where a click leads (Now Playing).
                .child(
                    div()
                        .absolute()
                        .top(sc(0.))
                        .left(sc(0.))
                        .size(sc(42.))
                        .rounded(sc(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .invisible()
                        .bg(gpui::rgba(0x0000008c))
                        .group_hover("bar-cover", |s| s.visible())
                        .child(Icon::ChevUp.el(20., theme::text())),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle_now_playing(cx)
                })),
        )
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .flex()
                .flex_col()
                .gap(sc(3.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(sc(6.))
                        .min_w(sc(0.))
                        .child(
                            div()
                                .id("np-title")
                                .min_w(sc(0.))
                                .truncate()
                                .text_size(sc(12.))
                                .font_weight(FontWeight::BOLD)
                                .cursor_pointer()
                                .hover(|s| s.underline())
                                .child(track.display_title())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    if let Some(id) = album {
                                        this.navigate(Page::Album(id), cx)
                                    }
                                })),
                        )
                        .when(track.explicit, |d| d.child(explicit_badge())),
                )
                .child(
                    div()
                        .id("np-artist")
                        .truncate()
                        .text_size(sc(11.))
                        .text_color(theme::text_secondary())
                        .cursor_pointer()
                        .hover(|s| s.underline().text_color(theme::text()))
                        .child(track.artist_names())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if let Some(id) = artist {
                                this.navigate(Page::Artist(id), cx)
                            }
                        })),
                ),
        )
        .child(icon_button(
            "np-fav",
            if fav { Icon::HeartFilled } else { Icon::Heart },
            15. * view.heart_scale(track.id),
            if fav { theme::text() } else { theme::text_secondary() },
            move |this, _, _, cx| this.toggle_favorite(&t_fav, cx),
            cx,
        ))
        .child(icon_button(
            "np-more",
            Icon::More,
            15.,
            theme::text_secondary(),
            move |this, ev: &gpui::ClickEvent, _, cx| {
                this.menu = Some((ev.position(), MenuTarget::Track(t_more.clone())));
                cx.notify();
            },
            cx,
        ))
        .child(super::widgets::stars("bar", track.id, 13., view, cx))
    });

    // -------------------------------------------------------- centre
    let shuffle_color = if view.shuffle { theme::accent() } else { theme::text_secondary() };
    let repeat_icon = if view.repeat == Repeat::One { Icon::RepeatOne } else { Icon::Repeat };
    let repeat_color = if view.repeat == Repeat::Off { theme::text_secondary() } else { theme::accent() };
    let e1 = entity.clone();
    let hp = super::interact::hover("progress").max(if view.drag == Some(Drag::Progress) { 1.0 } else { 0.0 });
    let hv = super::interact::hover("volume").max(if view.drag == Some(Drag::Volume) { 1.0 } else { 0.0 });
    let progress = div()
        .id("progress")
        .relative()
        .flex_1()
        .h(sc(14.))
        .flex()
        .items_center()
        .cursor_pointer()
        .group("progress")
        .child(canvas(move |b, _, cx| e1.update(cx, |v, _| v.progress_bounds = b), |_, _, _, _| {}).absolute().size_full())
        .child(
            div()
                .w_full()
                .h(sc(4.))
                .rounded(sc(2.))
                .bg(interact_track_bg(hp))
                .child(div().h_full().w(relative(frac)).rounded(sc(2.)).bg(theme::text())),
        )
        // Knob that appears on hover (and while dragging) at the play position.
        .child(
            div()
                .absolute()
                .left(relative(frac))
                .ml(sc(-6. * hp))
                .size(sc(12. * hp))
                .rounded(sc(6.))
                .bg(theme::text())
                .shadow_md()
                .when(hp < 0.02, |d| d.invisible()),
        )
        // Gesture: the wheel over the bar seeks 5 s per notch.
        .on_scroll_wheel(cx.listener(|this, ev: &gpui::ScrollWheelEvent, _, cx| {
            let dy = match ev.delta {
                gpui::ScrollDelta::Lines(l) => l.y,
                gpui::ScrollDelta::Pixels(p) => f32::from(p.y) / 40.0,
            };
            if this.now.is_some() && dy != 0.0 {
                cx.stop_propagation();
                let p = (this.snap.position + 5.0 * dy.signum() as f64).clamp(0.0, this.duration().max(0.0));
                this.seek(p, cx);
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                this.bar_click_block = true;
                if this.now.is_some() {
                    this.drag = Some(Drag::Progress);
                    this.drag_to(ev.position, cx);
                }
            }),
        );

    let progress = super::interact::track(progress, "progress", cx);
    let center = div()
        .flex_1()
        .min_w(sc(0.))
        .flex()
        .flex_col()
        .items_center()
        .gap(sc(1.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(sc(10.))
                .child(super::haptics::button(
                    view,
                    "shuffle",
                    Icon::Shuffle,
                    15.,
                    shuffle_color,
                    |this, _, _, cx| this.toggle_shuffle(cx),
                    cx,
                ))
                .child(super::haptics::button(view, "prev", Icon::Prev, 18., theme::text(), |this, _, _, cx| this.prev(cx), cx))
                .child({
                    // Clean play / pause: rounded glyph, no background; it sinks and springs
                    // back when pressed and cross-fades into the other glyph.
                    let pop = view.pulse_scale("playpause");
                    div()
                        .id("playpause")
                        .flex_none()
                        .size(sc(40.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child((if playing { Icon::PauseRound } else { Icon::PlayRound }).el(30. * pop, theme::text()))
                                .with_animation(
                                    ("pp-morph", playing as usize),
                                    gpui::Animation::new(std::time::Duration::from_millis(180)).with_easing(super::motion::ease_out),
                                    |d, t| d.opacity(0.25 + 0.75 * t),
                                ),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.pulse("playpause");
                            this.toggle_play(cx);
                            cx.notify();
                        }))
                })
                .child(super::haptics::button(view, "next", Icon::Next, 18., theme::text(), |this, _, _, cx| this.next(cx), cx))
                .child(super::haptics::button(view, "repeat", repeat_icon, 15., repeat_color, |this, _, _, cx| this.cycle_repeat(cx), cx)),
        )
        .child(
            div()
                .w_full()
                .max_w(sc(580.))
                .flex()
                .items_center()
                .gap(sc(10.))
                .text_size(sc(10.5))
                .text_color(theme::text_secondary())
                .child(div().w(sc(34.)).text_right().child(fmt_time(position)))
                .child(progress)
                .child(div().w(sc(34.)).child(fmt_time(duration))),
        );

    // --------------------------------------------------------- right
    let pill = now.as_ref().and_then(|(_, info)| info.clone()).map(|info| {
        let tier = info.tier();
        let (fg, border) = match tier {
            "MAX" => (theme::gold(), super::fx::with_alpha(theme::gold(), 0.45)),
            "HIGH" => (theme::high(), super::fx::with_alpha(theme::high(), 0.45)),
            _ => (theme::text_secondary(), theme::border()),
        };
        let detail = match view.snap.source {
            Some((bits, rate)) if view.snap.key == view.now.as_ref().map(|n| n.key) => {
                if info.codec.eq_ignore_ascii_case("aac") {
                    "AAC".to_string()
                } else {
                    format!("{bits}-bit {}kHz", fmt_khz(rate))
                }
            }
            _ => info.detail(),
        };
        div()
            .id("quality")
            .flex_none()
            .h(sc(26.))
            .px(sc(12.))
            .flex()
            .items_center()
            .rounded(sc(13.))
            .border_1()
            .border_color(border)
            .bg(super::fx::with_alpha(fg, 0.10))
            .text_color(fg)
            .text_size(sc(10.))
            .font_weight(FontWeight::BOLD)
            .cursor_pointer()
            .hover(move |s| {
                s.bg(super::fx::with_alpha(fg, 0.2)).shadow(vec![gpui::BoxShadow {
                    color: gpui::Hsla::from(super::fx::with_alpha(fg, 0.35)),
                    offset: gpui::point(sc(0.), sc(0.)),
                    blur_radius: sc(14.),
                    spread_radius: sc(0.),
                }])
            })
            .active(|s| s.opacity(0.6))
            .child(detail)
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.show_quality_info = !this.show_quality_info;
                cx.notify();
            }))
    });

    let vol = if view.muted { 0.0 } else { view.volume };
    let show_pct = view.drag == Some(Drag::Volume) || view.volume_hint_until.is_some_and(|t| std::time::Instant::now() < t);
    let e2 = entity.clone();
    let volume = div()
        .id("volume")
        .relative()
        .on_scroll_wheel(cx.listener(|this, ev: &gpui::ScrollWheelEvent, _, cx| {
            // One notch = 1 %; touchpads step every 12 px.
            let steps = match ev.delta {
                gpui::ScrollDelta::Lines(l) => l.y.signum() as i32,
                gpui::ScrollDelta::Pixels(p) => {
                    this.volume_wheel_px += f32::from(p.y);
                    let n = (this.volume_wheel_px / 12.0) as i32;
                    this.volume_wheel_px -= n as f32 * 12.0;
                    n
                }
            };
            if steps != 0 {
                let base = if this.muted { 0.0 } else { this.volume };
                let pct = ((base * 100.0).round() as i32 + steps).clamp(0, 100);
                this.set_volume(pct as f32 / 100.0, cx);
                this.save_settings();
                this.volume_hint_until = Some(std::time::Instant::now() + std::time::Duration::from_millis(1200));
            }
            cx.stop_propagation();
        }))
        .w(sc(84.))
        .h(sc(14.))
        .flex()
        .items_center()
        .cursor_pointer()
        .relative()
        .group("vol")
        .child(canvas(move |b, _, cx| e2.update(cx, |v, _| v.volume_bounds = b), |_, _, _, _| {}).absolute().size_full())
        .child(
            div()
                .w_full()
                .h(sc(4.))
                .rounded(sc(2.))
                .bg(interact_track_bg(hv))
                .child(div().h_full().w(relative(vol)).rounded(sc(2.)).bg(theme::text())),
        )
        .child(
            div()
                .absolute()
                .left(relative(vol))
                .ml(sc(-5. * hv))
                .size(sc(10. * hv))
                .rounded(sc(5.))
                .bg(theme::text())
                .shadow_md()
                .when(hv < 0.02, |d| d.invisible()),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                this.bar_click_block = true;
                this.drag = Some(Drag::Volume);
                this.drag_to(ev.position, cx);
            }),
        );

    let volume = super::interact::track(volume, "volume", cx);
    // Toggles that are on get a rounded highlight, as in the original.
    let (pop_queue, pop_lyrics) = (view.pulse_scale("queue"), view.pulse_scale("lyrics"));
    let toggle = move |id: &'static str, icon: Icon, on: bool, cx: &mut Context<AppView>, f: fn(&mut AppView, &mut Context<AppView>)| {
        let pop = if id == "queue" { pop_queue } else { pop_lyrics };
        div()
            .id(id)
            .flex_none()
            .size(sc(30.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(sc(8.))
            .cursor_pointer()
            .when(on, |d| d.bg(theme::elevated()))
            .hover(|s| s.bg(theme::hover()))
            .active(|s| s.opacity(0.6))
            .child(icon.el(16. * pop, if on { theme::text() } else { theme::text_secondary() }))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.pulse(id);
                f(this, cx)
            }))
    };
    let right = div()
        .w(sc(340.))
        .flex_none()
        .flex()
        .items_center()
        .justify_end()
        .gap(sc(4.))
        .child(toggle("queue", Icon::Queue, if view.now_playing_open { view.np_queue_side } else { view.show_queue }, cx, |this, cx| {
            if this.now_playing_open {
                this.np_queue_side = !this.np_queue_side;
                cx.notify();
                return;
            }
            this.show_queue = !this.show_queue;
            if this.show_queue {
                this.show_lyrics = false;
            }
            cx.notify();
        }))
        // Lyrics live in the full Now Playing view, as in the original.
        .child(toggle("lyrics", Icon::Lyrics, view.now_playing_open && view.np_tab == super::now_playing::NpTab::Lyrics, cx, |this, cx| {
            this.open_np_tab(super::now_playing::NpTab::Lyrics, cx)
        }))
        .child(icon_button(
            "mute",
            if vol <= 0.001 { Icon::VolumeOff } else { Icon::VolumeUp },
            16.,
            theme::text_secondary(),
            |this, _, _, cx| this.toggle_mute(cx),
            cx,
        ))
        .child(volume)
        .child(
            div()
                .w(sc(30.))
                .flex_none()
                .text_size(sc(10.))
                .text_color(theme::text_secondary())
                .when(show_pct, |d| d.child(format!("{}%", (vol * 100.0).round() as i32))),
        )
        .children(pill);

    div()
        .id("player-bar")
        .on_click(cx.listener(|this, _, _, cx| {
            if std::mem::take(&mut this.bar_click_block) {
                return;
            }
            this.toggle_now_playing(cx);
        }))
        .h(sc(theme::PLAYER_H))
        .flex_none()
        .px(sc(16.))
        .flex()
        .items_center()
        .gap(sc(20.))
        .when(!view.now_playing_open, |d| d.bg(theme::player_bar()))
        .relative()
        // The same turning cover as Now Playing, as a strip under a veil, so the bar and the
        // full-screen view share one background.
        .when(view.now.is_some() && !view.now_playing_open && bar_frames.is_some(), |d| {
            let f = bar_frames.clone().unwrap();
            d.child(
                div()
                    .absolute()
                    .top(sc(0.))
                    .left(sc(0.))
                    .size_full()
                    .child(super::fx::rotating_backdrop(&f, view.bg_angle_now))
                    .child(div().absolute().top(sc(0.)).left(sc(0.)).size_full().bg(gpui::rgba(0x000000b8))),
            )
        })
        .child(left)
        .child(center)
        .child(right)
        .into_any_element()
}

/// Slider track colour: a touch lighter while hovered.
fn interact_track_bg(h: f32) -> gpui::Rgba {
    super::interact::mix(theme::active(), gpui::rgb(0x45454d), h)
}
