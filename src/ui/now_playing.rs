//! Full-window "Now playing", like the official app: a wash of the cover's
//! colour behind everything, the cover very large on the left, and on the right
//! the lyrics in big type (or credits / similar tracks) chosen with the tabs in
//! the top right. The player bar stays underneath, transparent.

use super::sc;
use gpui::{canvas, div, prelude::*, px, relative, AnimationExt, AnyElement, Context, FontWeight, SharedString, Window};

use super::icons::Icon;
use super::widgets::{cover, fmt_time};
use super::{theme, AppView, Load};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NpTab {
    #[default]
    Lyrics,
    Credits,
    Similar,
    Queue,
}

impl NpTab {
    fn label(self) -> &'static str {
        match self {
            NpTab::Queue => "Cola",
            NpTab::Similar => "Similares",
            NpTab::Credits => "Créditos",
            NpTab::Lyrics => "Letra",
        }
    }
}

/// Human labels for TIDAL credit roles.
fn role_label(role: &str) -> String {
    match role {
        "Main Artist" | "MAIN_ARTIST" => "Artista principal".into(),
        "Featured Artist" | "FEATURED_ARTIST" => "Artista invitado".into(),
        "Producer" => "Producción".into(),
        "Composer" => "Composición".into(),
        "Lyricist" => "Letra".into(),
        "Songwriter" | "Writer" => "Autoría".into(),
        "Mixing Engineer" | "Mixer" => "Mezcla".into(),
        "Mastering Engineer" => "Masterización".into(),
        "Engineer" | "Recording Engineer" => "Ingeniería de sonido".into(),
        "Record Label" => "Sello".into(),
        "Vocals" | "Vocal" => "Voz".into(),
        "Guitar" => "Guitarra".into(),
        "Bass" | "Bass Guitar" => "Bajo".into(),
        "Drums" => "Batería".into(),
        "Keyboards" => "Teclados".into(),
        other => other.to_string(),
    }
}

impl AppView {
    pub fn toggle_now_playing(&mut self, cx: &mut Context<Self>) {
        if self.now.is_none() {
            return;
        }
        let shown = self.now_playing_amount();
        let opening = !(self.now_playing_open && self.np_anim.map(|(o, _)| o).unwrap_or(true));
        // Start from where a running animation currently is.
        let start = std::time::Instant::now()
            - super::motion::PANEL_ANIM.mul_f32(if opening { shown } else { 1.0 - shown }).min(super::motion::PANEL_ANIM);
        self.np_anim = Some((opening, start));
        self.now_playing_open = true;
        if opening {
            self.load_np_data(cx);
        }
        cx.notify();
    }

    /// Open Now Playing on `tab`, or close it if that tab is already showing.
    pub fn open_np_tab(&mut self, tab: NpTab, cx: &mut Context<Self>) {
        let showing = self.now_playing_open && self.np_anim.map(|(o, _)| o).unwrap_or(true) && self.np_tab == tab;
        self.np_tab = tab;
        if showing || !self.now_playing_open {
            self.toggle_now_playing(cx);
        } else {
            self.load_np_data(cx);
            cx.notify();
        }
    }

    pub fn wants_lyrics(&self) -> bool {
        self.show_lyrics || (self.now_playing_open && self.np_tab == NpTab::Lyrics)
    }

    /// Fetch what the visible tab needs for the playing track.
    pub fn load_np_data(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.now.as_ref().map(|n| n.track.id) else { return };
        match self.np_tab {
            NpTab::Lyrics => self.load_lyrics(id, cx),
            NpTab::Queue => {}
            NpTab::Credits => self.load_credits(id, cx),
            NpTab::Similar => {
                if !self.similar.contains_key(&id) {
                    self.similar.insert(id, Load::Loading);
                    self.fetch(cx, move |t| t.track_radio(id), move |this, r, _| {
                        this.similar.insert(id, super::into_load(r));
                    });
                }
            }
        }
    }

    pub fn load_credits(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.credits.contains_key(&id) {
            return;
        }
        self.credits.insert(id, Load::Loading);
        self.fetch(cx, move |t| t.credits(id), move |this, r, _| {
            this.credits.insert(id, super::into_load(r));
        });
    }
}

fn tab_button(tab: NpTab, view: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let active = view.np_tab == tab;
    div()
        .id(SharedString::from(format!("np-tab-{}", tab.label())))
        .h(sc(26.))
        .px(sc(13.))
        .flex()
        .items_center()
        .rounded(sc(13.))
        .cursor_pointer()
        .text_size(sc(11.))
        .font_weight(FontWeight::BOLD)
        .bg(if active { theme::text() } else { gpui::rgba(0x00000000) })
        .text_color(if active { theme::bg() } else { gpui::rgba(0xffffffb3) })
        .hover(|s| if active { s.opacity(0.9) } else { s.text_color(theme::text()) })
        .active(|s| s.opacity(0.7))
        .child(tab.label())
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.np_tab = tab;
            this.load_np_data(cx);
            cx.notify();
        }))
        .into_any_element()
}

/// "Cola": docks the play queue beside whichever tab is open, so lyrics and queue
/// can be seen together.
fn queue_toggle(view: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let active = view.np_queue_side;
    div()
        .id("np-queue-toggle")
        .h(sc(26.))
        .px(sc(13.))
        .flex()
        .items_center()
        .rounded(sc(13.))
        .cursor_pointer()
        .text_size(sc(11.))
        .font_weight(FontWeight::BOLD)
        .bg(if active { theme::text() } else { gpui::rgba(0x00000000) })
        .text_color(if active { theme::bg() } else { gpui::rgba(0xffffffb3) })
        .hover(|s| if active { s.opacity(0.9) } else { s.text_color(theme::text()) })
        .active(|s| s.opacity(0.7))
        .child("Cola")
        .on_click(cx.listener(|this, _, _, cx| {
            cx.stop_propagation();
            this.np_queue_side = !this.np_queue_side;
            cx.notify();
        }))
        .into_any_element()
}

fn small_button(id: &'static str, label: &'static str, active: bool, cx: &mut Context<AppView>, f: impl Fn(&mut AppView, &mut Context<AppView>) + 'static) -> AnyElement {
    div()
        .id(id)
        .h(sc(24.))
        .px(sc(11.))
        .flex()
        .items_center()
        .rounded(sc(12.))
        .cursor_pointer()
        .text_size(sc(10.5))
        .font_weight(FontWeight::BOLD)
        .bg(if active { theme::text() } else { gpui::rgba(0xffffff1f) })
        .text_color(if active { theme::bg() } else { gpui::rgba(0xffffffd9) })
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.6))
        .child(label)
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            f(this, cx)
        }))
        .into_any_element()
}

/// Big lyrics (synced lines you can click, or plain text).
fn lyrics_view(view: &mut AppView, id: u64, window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    let pos = view.display_position();
    let glow_color = {
        let c = view.ambient(cx);
        // Lift the cover colour towards white so the halo reads on any cover.
        gpui::Rgba { r: c.r * 0.6 + 0.4, g: c.g * 0.6 + 0.4, b: c.b * 0.6 + 0.4, a: 1.0 }
    };
    match view.lyrics.get(&id) {
        Some(Load::Ready(l)) if !l.synced.is_empty() => {
            let l = l.clone();
            let roman = l.roman.clone();
            let show_roman = view.settings.lock().lyrics_romanize;
            let offset = view.lyrics_offset.get(&id).copied().unwrap_or(0.0);
            let current = l.synced.iter().rposition(|(t, _)| *t + offset <= pos + 0.2);
            if current != view.lyrics_line {
                view.lyrics_prev = view.lyrics_line;
                view.lyrics_changes += 1;
                view.lyrics_line = current;
            }
            // Follow the song: ease the scroll so the current line sits about a third
            // of the way down. Measured from the laid-out lines, so wrapped lines
            // and stanza gaps are handled. Any manual scroll switches this off until
            // "Sincronizar" or a click on a line.
            if view.lyrics_follow {
                let handle = view.lyrics_scroll.clone();
                // Child 0 is the intro dots, line i is child i + 1.
                let child = current.map(|c| c + 1).unwrap_or(0);
                if let Some(b) = handle.bounds_for_item(child) {
                    let vp = handle.bounds();
                    // Child bounds are layout positions, before the scroll offset is applied:
                    // the offset that puts the line at 32 % of the height is absolute.
                    let line_top = f32::from(b.top()) - f32::from(vp.top());
                    let off = handle.offset();
                    let max = f32::from(handle.max_offset().height);
                    let target = -(line_top - f32::from(vp.size.height) * 0.32).clamp(0.0, max);
                    // Time-based ease (tau 140 ms), so it glides the same at any refresh rate.
                    let dt = view.lyrics_scroll_at.elapsed().as_secs_f32().min(0.05);
                    view.lyrics_scroll_at = std::time::Instant::now();
                    let step = (target - f32::from(off.y)) * (1.0 - (-dt / 0.14).exp());
                    if (target - f32::from(off.y)).abs() > 0.6 {
                        handle.set_offset(gpui::point(off.x, gpui::px(f32::from(off.y) + step)));
                        window.request_animation_frame();
                    } else if (target - f32::from(off.y)).abs() > 0.0 {
                        handle.set_offset(gpui::point(off.x, gpui::px(target)));
                    }
                } else {
                    window.request_animation_frame();
                }
            }
            div()
                .id("np-lyrics")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&view.lyrics_scroll)
                .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                    if this.lyrics_follow {
                        this.lyrics_follow = false;
                        cx.notify();
                    }
                }))
                // Room around the lines so the glow of the current one is not cut by the box.
                .pl(sc(80.))
                .ml(sc(-80.))
                .pt(sc(70.))
                .pr(sc(80.))
                .pb(sc(140.))
                .flex()
                .flex_col()
                // Lines hug their text, so the glow surrounds the words and not the whole column.
                .items_start()
                // Three dots while the intro plays.
                .child(
                    div()
                        .h(sc(54.))
                        .flex()
                        .items_center()
                        .gap(sc(9.))
                        .px(sc(10.))
                        .children((0..3).map(|i| {
                            div()
                                .size(sc(9.))
                                .rounded(sc(5.))
                                .bg(if current.is_none() { theme::text() } else { gpui::rgba(0xffffff59) })
                                .opacity(1.0 - 0.0 * i as f32)
                        })),
                )
                .children(l.synced.iter().enumerate().map(|(i, (t, line))| {
                    let active = Some(i) == current;
                    let was_active = Some(i) == view.lyrics_prev;
                    let changes = view.lyrics_changes;
                    let at = *t + offset;
                    // A longer pause before a line opens a gap, like stanzas.
                    let gap_before = i > 0 && (*t - l.synced[i - 1].0) > 5.5;
                    // Official app: 32 px / 600 / 48 px line height, inactive lines at 55 % opacity.
                    let base = div()
                        .id(("np-line", i))
                        .when(gap_before, |d| d.mt(sc(26.)))
                        .px(sc(10.))
                        .py(sc(8.))
                        .rounded(sc(10.))
                        .cursor_pointer()
                        .text_size(sc(32.))
                        .line_height(sc(46.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .flex()
                        .flex_col()
                        .child(if line.is_empty() { "♪".to_string() } else { line.clone() })
                        // Pronunciation in Latin letters under Asian scripts.
                        .when_some(roman.get(i).cloned().flatten().filter(|_| show_roman), |d, r| {
                            d.child(div().text_size(sc(19.)).line_height(sc(26.)).font_weight(FontWeight::MEDIUM).opacity(0.8).child(r))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.lyrics_follow = true;
                            this.seek(at.max(0.0), cx)
                        }));
                    const INACTIVE: f32 = 0.55;
                    let white = |a: f32| super::fx::with_alpha(gpui::rgb(0xffffff), a);
                    if active || was_active {
                        // .46 s, cubic-bezier(.4, 0, .2, 1), like `--lyrics-duration/--lyrics-ease`.
                        base.with_animation(
                            ("np-line-fade", i * 10_000 + changes % 10_000),
                            gpui::Animation::new(std::time::Duration::from_millis(460)).with_easing(super::motion::lyrics_ease),
                            move |d, t| {
                                let a = if active { INACTIVE + (1.0 - INACTIVE) * t } else { 1.0 - (1.0 - INACTIVE) * t };
                                // 0 (dim) .. 1 (current): drives the glow around the line.
                                let k = (a - INACTIVE) / (1.0 - INACTIVE);
                                d.text_color(white(a)).shadow(vec![
                                    gpui::BoxShadow {
                                        color: gpui::Hsla::from(super::fx::with_alpha(glow_color, 0.30 * k)),
                                        offset: gpui::point(sc(0.), sc(0.)),
                                        blur_radius: sc(34.),
                                        spread_radius: sc(2. * k),
                                    },
                                    gpui::BoxShadow {
                                        color: gpui::Hsla::from(super::fx::with_alpha(glow_color, 0.16 * k)),
                                        offset: gpui::point(sc(0.), sc(0.)),
                                        blur_radius: sc(48.),
                                        spread_radius: sc(4. * k),
                                    },
                                ])
                            },
                        )
                        .into_any_element()
                    } else {
                        base.text_color(white(INACTIVE)).hover(|s| s.text_color(white(0.85))).into_any_element()
                    }
                }))
                .into_any_element()
        }
        Some(Load::Ready(l)) if !l.text.is_empty() => div()
            .id("np-lyrics")
            .size_full()
            .overflow_y_scroll()
            .pr(sc(40.))
            .pb(sc(140.))
            .text_size(sc(24.))
            .line_height(sc(34.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(gpui::rgba(0xffffffd9))
            .child(l.text.clone())
            .into_any_element(),
        Some(Load::Loading) => div().text_size(sc(16.)).text_color(gpui::rgba(0xffffff99)).child("Buscando letra…").into_any_element(),
        _ => div().text_size(sc(16.)).text_color(gpui::rgba(0xffffff99)).child("Ningún proveedor tiene letra para esta pista.").into_any_element(),
    }
}

fn credits_view(view: &AppView, id: u64) -> AnyElement {
    match view.credits.get(&id) {
        Some(Load::Ready(list)) if !list.is_empty() => div()
            .id("np-credits")
            .size_full()
            .overflow_y_scroll()
            .pr(sc(40.))
            .pb(sc(140.))
            .flex()
            .flex_col()
            .gap(sc(20.))
            .children(list.iter().map(|(role, names)| {
                div()
                    .flex()
                    .flex_col()
                    .gap(sc(4.))
                    .child(div().text_size(sc(11.)).font_weight(FontWeight::BOLD).text_color(gpui::rgba(0xffffff80)).child(role_label(role).to_uppercase()))
                    .child(div().text_size(sc(20.)).font_weight(FontWeight::BOLD).child(names.join(", ")))
            }))
            .into_any_element(),
        Some(Load::Loading) => div().text_size(sc(16.)).text_color(gpui::rgba(0xffffff99)).child("Cargando créditos…").into_any_element(),
        _ => div().text_size(sc(16.)).text_color(gpui::rgba(0xffffff99)).child("No hay créditos para esta pista.").into_any_element(),
    }
}

fn similar_view(view: &mut AppView, id: u64, cx: &mut Context<AppView>) -> AnyElement {
    let tracks = match view.similar.get(&id) {
        Some(Load::Ready(t)) => t.clone(),
        Some(Load::Loading) => {
            return div().text_size(sc(16.)).text_color(gpui::rgba(0xffffff99)).child("Buscando pistas similares…").into_any_element()
        }
        _ => return div().text_size(sc(16.)).text_color(gpui::rgba(0xffffff99)).child("No hay pistas similares.").into_any_element(),
    };
    let playing = view.now.as_ref().map(|n| n.track.id);
    div()
        .id("np-similar")
        .size_full()
        .overflow_y_scroll()
        .pr(sc(40.))
        .pb(sc(140.))
        .flex()
        .flex_col()
        .children(tracks.iter().cloned().enumerate().map(|(i, t)| {
            let list = tracks.clone();
            let current = playing == Some(t.id);
            div()
                .id(("np-sim", i))
                .h(sc(52.))
                .px(sc(8.))
                .flex()
                .items_center()
                .gap(sc(12.))
                .rounded(sc(8.))
                .cursor_pointer()
                .hover(|s| s.bg(gpui::rgba(0xffffff14)))
                .active(|s| s.bg(gpui::rgba(0xffffff24)))
                .child(cover(t.cover(80), 38., false, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w(sc(0.))
                        .flex()
                        .flex_col()
                        .gap(sc(2.))
                        .child(div().truncate().text_size(sc(13.)).font_weight(FontWeight::BOLD).text_color(if current { theme::accent() } else { theme::text() }).child(t.display_title()))
                        .child(div().truncate().text_size(sc(11.5)).text_color(gpui::rgba(0xffffff99)).child(t.artist_names())),
                )
                .child(div().text_size(sc(11.)).text_color(gpui::rgba(0xffffff80)).child(fmt_time(t.duration as f64)))
                .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                    if ev.click_count() >= 2 {
                        this.play_tracks(list.clone(), i, cx)
                    }
                }))
        }))
        .into_any_element()
}

pub fn render(view: &mut AppView, window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    let Some(track) = view.now.as_ref().map(|n| n.track.clone()) else {
        view.now_playing_open = false;
        return div().into_any_element();
    };
    let ambient = view.ambient(cx);
    let palette = view.palette(cx);
    // The background drifts on its own; it no longer reacts to the audio.
    let vis = [0.5f32; 3];
    let view_blur = view.settings.lock().np_blur;
    if view.np_tilt.step() {
        window.request_animation_frame();
    }
    let frames = if view_blur { track.cover(640).and_then(|u| super::images::frames(&u, cx)) } else { None };
    let angle = view.bg_angle_now;
    let amount = view.now_playing_amount();
    let vw = f32::from(window.viewport_size().width) / super::zoom();
    let vh = f32::from(window.viewport_size().height) / super::zoom();
    let area_h = vh - theme::CAPTION_H;
    let slide = (1.0 - amount) * area_h.max(400.0);
    // The cover takes the space the original gives it: big, about a third of the width.
    let size = (area_h - theme::PLAYER_H - 120.0).min(vw * if view.np_queue_side { 0.27 } else { 0.36 }).clamp(240.0, 760.0);

    let synced_lyrics = view.np_tab == NpTab::Lyrics && view.lyrics.get(&track.id).and_then(Load::ready).is_some_and(|l| !l.synced.is_empty());
    let follow = view.lyrics_follow;
    let offset = view.lyrics_offset.get(&track.id).copied().unwrap_or(0.0);
    let id = track.id;

    let body: AnyElement = match view.np_tab {
        NpTab::Lyrics | NpTab::Queue => lyrics_view(view, id, window, cx),
        NpTab::Credits => credits_view(view, id),
        NpTab::Similar => similar_view(view, id, cx),
    };

    // Tabs in the top right, with sync controls on their left.
    let tabs = div()
        .h(sc(52.))
        .flex_none()
        .px(sc(24.))
        .flex()
        .items_center()
        .justify_end()
        .gap(sc(6.))
        .when(synced_lyrics, |d| {
            d.child(small_button("np-sync", if follow { "Sincronizada" } else { "Sincronizar" }, !follow, cx, |this, cx| {
                this.lyrics_follow = true;
                cx.notify();
            }))
            .child(small_button("np-m", "−0,5 s", false, cx, move |this, cx| {
                let o = this.lyrics_offset.entry(id).or_insert(0.0);
                *o = ((*o - 0.5) * 10.0).round() / 10.0;
                cx.notify();
            }))
            .child(
                div()
                    .w(sc(44.))
                    .text_center()
                    .text_size(sc(10.5))
                    .text_color(if offset == 0.0 { gpui::rgba(0xffffff66) } else { theme::accent().into() })
                    .child(format!("{:+.1} s", offset)),
            )
            .child(small_button("np-p", "+0,5 s", false, cx, move |this, cx| {
                let o = this.lyrics_offset.entry(id).or_insert(0.0);
                *o = ((*o + 0.5) * 10.0).round() / 10.0;
                cx.notify();
            }))
            .child(div().w(sc(18.)))
        })
        .child(queue_toggle(view, cx))
        .child(tab_button(NpTab::Similar, view, cx))
        .child(tab_button(NpTab::Credits, view, cx))
        .child(tab_button(NpTab::Lyrics, view, cx))
        .child(
            div()
                .id("np-collapse")
                .ml(sc(10.))
                .size(sc(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(sc(14.))
                .cursor_pointer()
                .hover(|s| s.bg(gpui::rgba(0xffffff1f)))
                .active(|s| s.opacity(0.6))
                .child(Icon::ChevDown.el(16., gpui::rgba(0xffffffd9)))
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle_now_playing(cx)
                })),
        );

    div()
        .id("now-playing")
        .absolute()
        .left(sc(0.))
        .right(sc(0.))
        .top(sc(theme::CAPTION_H + slide))
        .bottom(sc(0.))
        .opacity(0.35 + 0.65 * amount)
        .occlude()
        .bg(theme::bg())
        // The official background: the cover, blurred and saturated, turning slowly about
        // its centre (see `fx::rotating_backdrop`). Flat colour when that setting is chosen.
        .child(match (view_blur, frames) {
            (true, Some(f)) => div()
                .absolute()
                .top(sc(0.))
                .left(sc(0.))
                .size_full()
                .child(super::fx::rotating_backdrop(&f, angle))
                .with_animation(
                    ("np-backdrop", track.id as usize),
                    gpui::Animation::new(std::time::Duration::from_millis(650)).with_easing(super::motion::ease_out),
                    |d, t| d.opacity(t),
                )
                .into_any_element(),
            (true, None) => div()
                .absolute()
                .top(sc(0.))
                .left(sc(0.))
                .size_full()
                .bg(gpui::linear_gradient(
                    180.,
                    gpui::linear_color_stop(super::fx::with_alpha(ambient, 0.78), 0.),
                    gpui::linear_color_stop(super::fx::with_alpha(theme::bg(), 0.96), 1.),
                ))
                .into_any_element(),
            (false, _) => div().absolute().top(sc(0.)).left(sc(0.)).size_full().bg(super::fx::with_alpha(ambient, 0.9)).into_any_element(),
        })
        // Changing light: soft sweeps of the cover's colours that come and go, as if the
        // lighting of the room slowly changed. Independent of the music.
        .child(
            div()
                .absolute()
                .top(sc(0.))
                .left(sc(0.))
                .size_full()
                .with_animation(
                    "np-light",
                    gpui::Animation::new(std::time::Duration::from_secs(28)).repeat(),
                    {
                        let (c0, c1) = (palette[0], palette[1]);
                        move |d, t| {
                            let tau = std::f32::consts::TAU;
                            let a1 = 0.5 + 0.5 * (t * tau).sin();
                            let a2 = 0.5 + 0.5 * (t * tau * 1.0 + 2.4).sin();
                            d.child(div().absolute().top(sc(0.)).left(sc(0.)).size_full().bg(gpui::linear_gradient(
                                60.0 + 90.0 * (t * tau).cos(),
                                gpui::linear_color_stop(super::fx::with_alpha(c0, 0.34 * a1), 0.),
                                gpui::linear_color_stop(super::fx::with_alpha(c0, 0.0), 0.75),
                            )))
                            .child(div().absolute().top(sc(0.)).left(sc(0.)).size_full().bg(gpui::linear_gradient(
                                240.0 + 90.0 * (t * tau + 1.0).sin(),
                                gpui::linear_color_stop(super::fx::with_alpha(c1, 0.30 * a2), 0.),
                                gpui::linear_color_stop(super::fx::with_alpha(c1, 0.0), 0.75),
                            )))
                        }
                    },
                ),
        )
        // Veil so the lyrics stay readable: lighter at the top, darker towards the player.
        .child(
            div()
                .absolute()
                .top(sc(0.))
                .left(sc(0.))
                .size_full()
                .bg(gpui::linear_gradient(
                    180.,
                    gpui::linear_color_stop(gpui::rgba(0x00000057), 0.),
                    gpui::linear_color_stop(gpui::rgba(0x000000c2), 1.),
                )),
        )
        .child(
            div()
                .absolute()
                .top(sc(0.))
                .left(sc(0.))
                .size_full()
                .flex()
                .flex_col()
                .pb(sc(theme::PLAYER_H))
                .child(tabs)
                .child(
                    div()
                        .flex_1()
                        .min_h(sc(0.))
                        .flex()
                        .child(
                            div()
                                .w(relative(if view.np_queue_side { 0.36 } else { 0.47 }))
                                .flex_none()
                                .h_full()
                                .pl(sc((vw * 0.07).min(180.)))
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .id("np-big-cover")
                                        .relative()
                                        .flex_none()
                                        .size(sc(size))
                                        .rounded(sc(4.))
                                        // The shadow stays on the flat footprint while the cover tilts above it.
                                        .shadow(view.np_tilt.shadow(ambient))
                                        .child(
                                            canvas(
                                                {
                                                    let e = cx.entity();
                                                    move |b, _, cx| e.update(cx, |v, _| v.np_cover_bounds = b)
                                                },
                                                |_, _, _, _| {},
                                            )
                                            .absolute()
                                            .size_full(),
                                        )
                                        .child(match (view.np_tilt.tilted(), track.cover(1280).and_then(|u| super::images::get(&u, cx))) {
                                            (true, Some(image)) => view.np_tilt.cover_3d(image, size),
                                            _ => cover(track.cover(1280), size, false, cx),
                                        })
                                        // The cover follows the mouse (3D tilt and glare).
                                        .child(div().absolute().top(sc(0.)).left(sc(0.)).size(sc(size)).rounded(sc(4.)).overflow_hidden().children(view.np_tilt.overlays(size)))
                                        .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, _, cx| {
                                            // Screenshot tests pin the tilt (TIDE_TILT) and ignore the mouse.
                                            if std::env::var_os("TIDE_TILT").is_some() {
                                                return;
                                            }
                                            let b = this.np_cover_bounds;
                                            let (w, h) = (f32::from(b.size.width).max(1.0), f32::from(b.size.height).max(1.0));
                                            let x = (f32::from(ev.position.x) - f32::from(b.origin.x)) / w;
                                            let y = (f32::from(ev.position.y) - f32::from(b.origin.y)) / h;
                                            this.np_tilt.point(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
                                            cx.notify();
                                        }))
                                        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                            if !*hovered && std::env::var_os("TIDE_TILT").is_none() {
                                                this.np_tilt.leave();
                                                cx.notify();
                                            }
                                        }))
                                        // Gesture: double-click the cover to like the track (big heart).
                                        .children(super::haptics::heart_overlay(view, size))
                                        .on_click({
                                            let t = track.clone();
                                            cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                                                if ev.click_count() >= 2 {
                                                    if !this.fav_ids.contains(&t.id) {
                                                        this.toggle_favorite(&t, cx);
                                                    }
                                                    this.heart_burst();
                                                    cx.notify();
                                                }
                                            })
                                        })
                                        .with_animation(
                                            ("np-cover", track.id as usize),
                                            gpui::Animation::new(std::time::Duration::from_millis(600)).with_easing(super::motion::ease_out),
                                            |d, t| d.opacity(t),
                                        ),
                                ),
                        )
                        .child(div().flex_1().min_w(sc(0.)).h_full().pt(sc(8.)).child(body))
                        .when(view.np_queue_side, |d| {
                            d.child(
                                div()
                                    .w(sc(370.))
                                    .flex_none()
                                    .h_full()
                                    .pt(sc(8.))
                                    .pb(sc(90.))
                                    .pr(sc(24.))
                                    .flex()
                                    .flex_col()
                                    .child(super::pages::queue_panel(view, cx)),
                            )
                        }),
                ),
        )
        .into_any_element()
}
