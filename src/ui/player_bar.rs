//! The bottom bar, laid out like the official app: track info with like and
//! menu on the left; shuffle / previous / play / next / repeat with the time
//! and progress line below them in the centre; queue, lyrics, volume and the
//! full-screen button on the right.

use super::sc;
use gpui::{canvas, div, prelude::*, px, relative, AnimationExt, AnyElement, Context, FontWeight, MouseButton, MouseDownEvent, Window};

use super::icons::Icon;
use super::widgets::{cover, explicit_badge, fmt_time, icon_button};
use super::{theme, AppView, Drag, MenuTarget, Page};
use crate::config::Repeat;

pub fn render(view: &mut AppView, window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    // Width in logical px (after zoom): narrow windows or big zoom levels get a
    // more compact bar instead of controls running into each other.
    // The bar sits beside the sidebar rail, inside the frame.
    let width = f32::from(window.viewport_size().width) / super::zoom() - 2.0 * theme::FRAME_GAP;
    let compact = width < 1150.0;
    let wing = (width * 0.28).min(400.);

    // Bar thumbnail: the new cover fades in over the old one.
    let bar_cover = {
        let url = view.now.as_ref().and_then(|n| n.track.cover(160));
        let src = url.as_deref().and_then(|u| super::images::sized(u, 42., 42., cx));
        let (prev, now) = view.bar_cover_fade.frame(url.as_deref(), src);
        if !view.now_playing_open {
            view.prefetch_next_covers(None, cx);
            // Have the big cover ready, so opening Now Playing shows it at once.
            if let Some(u) = view.now.as_ref().and_then(|n| n.track.cover(1280)) {
                let size = super::now_playing::cover_size(view, window);
                let _ = super::images::sized(&u, size, size, cx);
            }
        }
        super::widgets::cover_layers(prev, now, 42.)
    };
    let now = view.now.as_ref().map(|n| n.track.clone());
    let fav = now.as_ref().map(|t| view.fav_ids.contains(&t.id)).unwrap_or(false);

    // ---------------------------------------------------------- left
    let left = div()
        .w(sc(wing))
        .flex_shrink()
        .flex()
        .items_center()
        .gap(sc(10.))
        .min_w(sc(0.))
        .when(now.is_none(), |d| d.child(super::widgets::panel_heading(Icon::Track, crate::i18n::t("nothing_playing"), None)))
        .when_some(now.clone(), |d, track| {
            let album = track.album.as_ref().filter(|a| !a.title.trim().is_empty());
            let local = track.local_path.is_some();
            let album_id = track.album.as_ref().map(|a| a.id).filter(|id| *id != 0);
            let artists = track.artist_names();
            let artist = track.artists.first().map(|a| a.id).filter(|id| *id != 0);
            let t_fav = track.clone();
            let t_more = track.clone();
            d.child(div().flex_none().child(bar_cover))
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
                                        .when(album_id.is_some(), |d| d.cursor_pointer().hover(|s| s.underline()))
                                        .child(track.display_title())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            if let Some(id) = album_id {
                                                this.navigate(Page::Album(id), cx)
                                            }
                                        })),
                                )
                                .when(track.explicit, |d| d.child(explicit_badge())),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(sc(5.))
                                .min_w(sc(0.))
                                .text_size(sc(11.))
                                .text_color(theme::text_secondary())
                                .when_some(album, |d, a| {
                                    d.child(
                                        div()
                                            .id("np-album")
                                            .min_w(sc(0.))
                                            .max_w(relative(if artists.is_empty() { 1.0 } else { 0.60 }))
                                            .truncate()
                                            .when(album_id.is_some(), |d| {
                                                d.cursor_pointer().hover(|s| s.underline().text_color(theme::text()))
                                            })
                                            .child(a.title.clone())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(id) = album_id {
                                                    this.navigate(Page::Album(id), cx);
                                                }
                                            })),
                                    )
                                })
                                .when(album.is_some() && !artists.is_empty(), |d| d.child(div().flex_none().child("·")))
                                .when(!artists.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .id("np-artist")
                                            .min_w(sc(0.))
                                            .truncate()
                                            .when(artist.is_some(), |d| {
                                                d.cursor_pointer().hover(|s| s.underline().text_color(theme::text()))
                                            })
                                            .child(artists)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(id) = artist {
                                                    this.navigate(Page::Artist(id), cx);
                                                }
                                            })),
                                    )
                                }),
                        ),
                )
                .when(!local, |d| {
                    d.child(icon_button(
                        "np-fav",
                        if fav { Icon::HeartFilled } else { Icon::Heart },
                        15. * view.heart_scale(track.id),
                        if fav { theme::text() } else { theme::text_secondary() },
                        move |this, _, _, cx| this.toggle_favorite(&t_fav, cx),
                        cx,
                    ))
                })
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
                .when(!compact && !local, |d| d.child(super::widgets::stars("bar", track.id, 13., view, cx)))
        });

    let colors = if now.as_ref().is_some_and(|t| t.cover(640).is_some()) { Some(view.palette(cx)) } else { None };

    div()
        .id("player-bar")
        .h(sc(theme::PLAYER_H))
        // Its corners follow the panel it sits in (inside the window frame).
        .rounded(sc(theme::PANEL_R - 1.0))
        .flex_none()
        .px(sc(16.))
        .flex()
        .items_center()
        .gap(sc(if compact { 10. } else { 18. }))
        .bg(theme::liquid_glass(0.0))
        .text_color(theme::text())
        .relative()
        .when_some(colors, |d, palette| d.child(bar_light(palette[0], palette[1])))
        .child(super::glass::sheen("player-bar-glass", theme::PANEL_R - 1.0, 0.72))
        .child(left)
        .child(controls(view, false, width, cx))
        .into_any_element()
}

/// Shared playback controls, stacked beneath the artwork in Now Playing.
pub fn controls(view: &mut AppView, over_fullscreen: bool, width: f32, cx: &mut Context<AppView>) -> AnyElement {
    let entity = cx.entity();
    let compact = width < 1150.;
    let wing = (width * 0.28).min(400.);
    let duration = view.duration().max(0.);
    let position = view.display_position().clamp(0., duration);
    let frac = if duration > 0. { (position / duration) as f32 } else { 0. };
    let playing = view.now.is_some() && !view.snap.paused && !view.ended;
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
        // A generous hit area around the 4 px line.
        .h(sc(20.))
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
        // Knob that fades in on hover (and while dragging) at the play position.
        .children(knob(frac, hp, 12.))
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
            cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                if this.now.is_some() {
                    this.drag = Some(Drag::Progress);
                    super::winctl::capture_mouse(window);
                    this.drag_to(ev.position, cx);
                }
            }),
        );

    let progress = super::interact::track(progress, "progress", cx);
    let center = div()
        .flex_1()
        // The transport controls always keep their room; the sides give way.
        .min_w(sc(0.))
        .when(over_fullscreen, |d| d.w_full().flex_none())
        .flex()
        .flex_col()
        .items_center()
        .when(over_fullscreen, |d| d.flex_col_reverse())
        .gap(sc(if over_fullscreen { 12. } else { 1. }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(sc(if over_fullscreen {
                    18.
                } else if compact {
                    5.
                } else {
                    9.
                }))
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
                    // The primary control is distinct from the rest of the transport.
                    let pop = view.pulse_scale("playpause");
                    div()
                        .id("playpause")
                        .flex_none()
                        .size(sc(if over_fullscreen { 52. } else { 38. }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .rounded(sc(if over_fullscreen { 26. } else { 19. }))
                        .bg(theme::button())
                        .hover(|d| d.bg(theme::button_hover()))
                        .child(super::glass::sheen("playpause-glass", 19., 0.58))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child((if playing { Icon::PauseRound } else { Icon::PlayRound }).el_scaled(24., theme::button_text(), pop))
                                .with_animation(("pp-morph", playing as usize), super::motion::animation(super::motion::FAST), |d, t| {
                                    d.opacity(0.25 + 0.75 * t)
                                }),
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
        .children(knob(vol, hv, 10.))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                this.drag = Some(Drag::Volume);
                super::winctl::capture_mouse(window);
                this.drag_to(ev.position, cx);
            }),
        );

    let volume = super::interact::track(volume, "volume", cx);
    // Toggles that are on get a rounded highlight, as in the original.
    let toggle = move |id: &'static str,
                       icon: Icon,
                       on: bool,
                       label: &'static str,
                       cx: &mut Context<AppView>,
                       f: fn(&mut AppView, &mut Context<AppView>)| {
        let key = format!("bar-toggle-{id}");
        let (h, p) = (super::interact::hover(&key), super::interact::press(&key));
        let rest = if on { theme::elevated() } else { super::fx::with_alpha(theme::elevated(), 0.0) };
        let el = div()
            .id(id)
            .flex_none()
            .size(sc(30.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(sc(8.))
            .cursor_pointer()
            .bg(super::interact::mix(rest, theme::hover(), h * 0.85))
            .child(super::glass::sheen("player-toggle-glass", 8., 1.05))
            .child(icon.el_scaled(16., if on { theme::accent() } else { theme::text_secondary() }, 1.0 + 0.06 * h - 0.09 * p))
            .tooltip(super::sidebar::tip(label))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                f(this, cx)
            }));
        super::interact::track(el, key, cx)
    };
    let right = div()
        .when(!over_fullscreen, |d| d.w(sc(wing)))
        .when(over_fullscreen, |d| d.w_full().flex_wrap())
        .flex_shrink()
        .min_w(sc(0.))
        .flex()
        .items_center()
        .when(!over_fullscreen, |d| d.justify_end())
        .when(over_fullscreen, |d| d.justify_center())
        .gap(sc(4.))
        .child(toggle(
            "queue",
            Icon::Queue,
            if over_fullscreen { view.np_tab == super::now_playing::NpTab::Queue } else { view.show_queue },
            crate::i18n::t("queue"),
            cx,
            |this, cx| {
                if this.now_playing_open {
                    this.np_tab = super::now_playing::NpTab::Queue;
                    this.load_np_data(cx);
                } else {
                    this.show_queue = !this.show_queue;
                    if this.show_queue {
                        this.show_lyrics = false;
                    }
                }
                cx.notify();
            },
        ))
        .child(toggle(
            "lyrics",
            Icon::Lyrics,
            if over_fullscreen { view.np_tab == super::now_playing::NpTab::Lyrics } else { view.show_lyrics },
            crate::i18n::t("lyrics"),
            cx,
            AppView::toggle_lyrics,
        ))
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
                .when(show_pct, |d| d.child(crate::i18n::text("format.percent", &[format!("{}", (vol * 100.0).round() as i32)]))),
        )
        .child(toggle(
            "fullscreen",
            if over_fullscreen { Icon::ExitFullscreen } else { Icon::Fullscreen },
            over_fullscreen,
            if over_fullscreen { crate::i18n::t("exit_full_screen_esc") } else { crate::i18n::t("full_screen") },
            cx,
            AppView::toggle_now_playing,
        ));

    div()
        .flex_1()
        .min_w(sc(0.))
        .flex()
        .when(over_fullscreen, |d| d.flex_col().flex_none().gap(sc(16.)))
        .when(!over_fullscreen, |d| d.items_center().gap(sc(if compact { 10. } else { 18. })))
        .child(center)
        .child(right)
        .into_any_element()
}

/// Two broad, dim pools of light from the cover. A long, shallow cycle keeps
/// the controls readable and leaves the light independent of playback and scrubbing.
fn bar_light(left: gpui::Rgba, right: gpui::Rgba) -> impl IntoElement {
    let layer = div().absolute().top(sc(0.)).left(sc(0.)).size_full();
    let gradient = |color, angle, alpha| {
        div().absolute().top(sc(0.)).left(sc(0.)).size_full().bg(gpui::linear_gradient(
            angle,
            gpui::linear_color_stop(super::fx::with_alpha(color, alpha), 0.),
            gpui::linear_color_stop(super::fx::with_alpha(color, 0.), 0.74),
        ))
    };
    super::motion::looping(layer, std::time::Duration::from_secs(64), "bar-light", |d, t| {
        let phase = t * std::f32::consts::TAU;
        let drift = phase.sin();
        d.child(gradient(left, 82.0 + 5.0 * drift, 0.17 + 0.025 * drift)).child(gradient(
            right,
            268.0 + 5.0 * drift,
            0.14 + 0.025 * (phase + 2.2).sin(),
        ))
    })
}

/// Slider track colour: a touch lighter while hovered.
fn interact_track_bg(h: f32) -> gpui::Rgba {
    super::interact::mix(theme::active(), theme::active_strong(), h)
}

/// Slider knob: fixed size (no whole-pixel growth steps), centred on the
/// position by a zero-width anchor (no negative margin) and faded in by
/// opacity, which is paint-only and therefore smooth.
fn knob(frac: f32, shown: f32, size: f32) -> Option<gpui::AnyElement> {
    use gpui::IntoElement;
    // Diagnostics: TIDE_SHOW_KNOBS=1 shows them without hovering (screenshots).
    static FORCE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let shown = if *FORCE.get_or_init(|| std::env::var_os("TIDE_SHOW_KNOBS").is_some()) { 1.0 } else { shown };
    if shown < 0.01 {
        return None;
    }
    Some(
        div()
            .absolute()
            .top(sc(0.))
            .bottom(sc(0.))
            .left(relative(frac.clamp(0.0, 1.0)))
            .w(sc(0.))
            .flex()
            .items_center()
            .justify_center()
            .child(div().flex_none().size(sc(size)).rounded(sc(size / 2.0)).bg(theme::text()).opacity(shown).shadow(vec![
                gpui::BoxShadow {
                    color: gpui::Hsla::from(gpui::rgba(0x00000066)),
                    offset: gpui::point(sc(0.), sc(1.)),
                    blur_radius: sc(4.),
                    spread_radius: sc(0.),
                },
            ]))
            .into_any_element(),
    )
}
