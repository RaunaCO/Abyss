//! Fullscreen playback with artwork and controls beside a tabbed detail panel.

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
            NpTab::Queue => crate::i18n::t("queue"),
            NpTab::Similar => crate::i18n::t("similar"),
            NpTab::Credits => crate::i18n::t("credits"),
            NpTab::Lyrics => crate::i18n::t("lyrics"),
        }
    }
}

/// Human labels for TIDAL credit roles.
fn role_label(role: &str) -> String {
    match role {
        "Main Artist" | "MAIN_ARTIST" => crate::i18n::t("main_artist").into(),
        "Featured Artist" | "FEATURED_ARTIST" => crate::i18n::t("featured_artist").into(),
        "Producer" => crate::i18n::t("production").into(),
        "Composer" => crate::i18n::t("composition").into(),
        "Lyricist" => crate::i18n::t("lyrics").into(),
        "Songwriter" | "Writer" => crate::i18n::t("songwriting").into(),
        "Mixing Engineer" | "Mixer" => crate::i18n::t("mixing").into(),
        "Mastering Engineer" => crate::i18n::t("mastering").into(),
        "Engineer" | "Recording Engineer" => crate::i18n::t("sound_engineering").into(),
        "Record Label" => crate::i18n::t("record_label").into(),
        "Vocals" | "Vocal" => crate::i18n::t("vocals").into(),
        "Guitar" => crate::i18n::t("guitar").into(),
        "Bass" | "Bass Guitar" => crate::i18n::t("bass").into(),
        "Drums" => crate::i18n::t("drums").into(),
        "Keyboards" => crate::i18n::t("keyboards").into(),
        other => other.to_string(),
    }
}

impl AppView {
    pub fn toggle_now_playing(&mut self, cx: &mut Context<Self>) {
        let opening = !self.np_fullscreen_request.unwrap_or(self.now_playing_open);
        self.np_fullscreen_request = Some(opening);
        cx.notify();
    }

    pub fn toggle_lyrics(&mut self, cx: &mut Context<Self>) {
        if self.now_playing_open {
            self.np_tab = NpTab::Lyrics;
            self.load_np_data(cx);
        } else {
            self.show_lyrics = !self.show_lyrics;
            if self.show_lyrics {
                self.show_queue = false;
                if let Some(id) = self.now.as_ref().map(|n| n.track.id) {
                    self.load_lyrics(id, cx);
                }
            }
        }
        cx.notify();
    }

    pub fn wants_lyrics(&self) -> bool {
        if self.now_playing_open {
            self.np_tab == NpTab::Lyrics || self.now.as_ref().is_some_and(|n| n.track.local_path.is_some())
        } else {
            self.show_lyrics
        }
    }

    /// Fetch what the visible tab needs for the playing track.
    pub fn load_np_data(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.now.as_ref().map(|n| n.track.id) else { return };
        if self.now.as_ref().is_some_and(|n| n.track.local_path.is_some()) {
            self.load_lyrics(id, cx);
            return;
        }
        match self.np_tab {
            NpTab::Lyrics => self.load_lyrics(id, cx),
            NpTab::Queue => {}
            NpTab::Credits => self.load_credits(id, cx),
            NpTab::Similar => {
                if let std::collections::hash_map::Entry::Vacant(e) = self.similar.entry(id) {
                    e.insert(Load::Loading);
                    self.fetch(
                        cx,
                        move |t| t.track_radio(id),
                        move |this, r, _| {
                            this.similar.insert(id, super::into_load(r));
                        },
                    );
                }
            }
        }
    }

    pub fn load_credits(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.credits.contains_key(&id) {
            return;
        }
        self.credits.insert(id, Load::Loading);
        self.fetch(
            cx,
            move |t| t.credits(id),
            move |this, r, _| {
                this.credits.insert(id, super::into_load(r));
            },
        );
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

/// Space kept left of the lyrics for the glow of the current line.
const LYRICS_GUTTER: f32 = 36.0;

/// Follow the song: ease the lyrics scroll so `child` sits at `fraction` of the
/// height. Measured from the laid-out children, so wrapped lines and stanza
/// gaps are handled. Time-based (tau 140 ms), so it glides the same at any
/// refresh rate; used both when following and when "Sincronizar" brings the
/// view back to the current line.
pub fn follow_lyrics(view: &mut AppView, child: usize, fraction: f32, next: Option<(usize, f32)>) {
    super::smooth::stop("lyrics");
    let handle = view.lyrics_scroll.clone();
    let Some(b) = handle.bounds_for_item(child) else {
        super::motion::want_frame("lyrics-layout");
        return;
    };
    let vp = handle.bounds();
    // Child bounds are layout positions, before the scroll offset is applied:
    // the offset that puts the line at `fraction` of the height is absolute.
    let mut line_top = f32::from(b.top()) - f32::from(vp.top());
    if !super::motion::reduced_motion() {
        if let Some((index, blend)) = next {
            if let Some(next) = handle.bounds_for_item(index) {
                line_top += (f32::from(next.top()) - f32::from(b.top())) * blend;
            }
        }
    }
    let off = handle.offset();
    let max = f32::from(handle.max_offset().height);
    let target = -(line_top - f32::from(vp.size.height) * fraction).clamp(0.0, max);
    let dt = view.lyrics_scroll_at.elapsed().as_secs_f32().min(0.05);
    view.lyrics_scroll_at = std::time::Instant::now();
    let step =
        if super::motion::reduced_motion() { target - f32::from(off.y) } else { (target - f32::from(off.y)) * (1.0 - (-dt / 0.14).exp()) };
    // "Sincronizar" ends once the glide has arrived (and the spin was seen).
    if let Some(at) = view.lyrics_sync_at {
        if (target - f32::from(off.y)).abs() <= 0.6 && at.elapsed().as_secs_f32() >= 0.5 {
            view.lyrics_sync_at = None;
        } else {
            super::motion::want_frame("lyrics-sync");
        }
    }
    if (target - f32::from(off.y)).abs() > 0.6 {
        handle.set_offset(gpui::point(off.x, gpui::px(f32::from(off.y) + step)));
        super::motion::want_frame("lyrics-scroll");
    } else if (target - f32::from(off.y)).abs() > 0.0 {
        handle.set_offset(gpui::point(off.x, gpui::px(target)));
    }
}

/// Artwork fits the available column and leaves room for metadata and controls.
pub fn cover_size(_view: &AppView, window: &Window) -> f32 {
    let vw = f32::from(window.viewport_size().width) / super::zoom();
    let vh = f32::from(window.viewport_size().height) / super::zoom();
    let stacked = vw < 820.;
    let width = if stacked { (vw - 48.).min(520.) } else { (vw * 0.46 - 80.).min(640.) };
    width.min((vh - 400.).max(120.)).max(80.)
}

/// Opacity of lyrics child `child` from where it sits in the visible area: lines
/// fade out as they approach the top and bottom edges, so no line is ever
/// shown cut in half against the panel edges or the tabs. Measured from the
/// last layout (paint-only, no masks).
pub fn edge_fade(handle: &gpui::ScrollHandle, child: usize, top_zone: f32, bottom_zone: f32) -> f32 {
    let Some(b) = handle.bounds_for_item(child) else { return 1.0 };
    let vp = handle.bounds();
    let h = f32::from(vp.size.height);
    if h <= 1.0 {
        return 1.0;
    }
    // Child bounds are layout positions; add the scroll offset to get the
    // position inside the visible area.
    let top = f32::from(b.top()) - f32::from(vp.top()) + f32::from(handle.offset().y);
    let mid = top + f32::from(b.size.height) / 2.0;
    let z = super::zoom();
    let smooth = |t: f32| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    smooth(mid / (top_zone * z)).min(smooth((h - mid) / (bottom_zone * z)))
}

/// Big lyrics (synced lines you can click, or plain text).
pub(super) fn lyrics_view(view: &mut AppView, id: u64, window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    let lyric_scale = (f32::from(window.viewport_size().height) / super::zoom() / 900.).clamp(0.55, 2.5);
    let pos = view.display_position();
    let glow_color = {
        let c = view.ambient(cx);
        // Lift the cover colour towards white so the halo reads on any cover.
        gpui::Rgba { r: c.r * 0.6 + 0.4, g: c.g * 0.6 + 0.4, b: c.b * 0.6 + 0.4, a: 1.0 }
    };
    let show_roman = view.settings.lock().lyrics_romanize;
    match view.lyrics.get(&id) {
        Some(Load::Ready(l)) if !l.synced.is_empty() => {
            let l = l.clone();
            let roman = l.roman.clone();
            let current = l.synced.iter().rposition(|(t, _)| *t <= pos + 0.15);
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
                // Child 0 is the intro dots, line i is child i + 1.
                follow_lyrics(view, current.map(|c| c + 1).unwrap_or(0), 0.32, upcoming(&l.synced, current, pos, 1));
            }
            super::smooth::register("lyrics", &view.lyrics_scroll);
            let list = div()
                .id("np-lyrics")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&view.lyrics_scroll)
                .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                    if this.lyrics_follow {
                        this.lyrics_follow = false;
                        this.lyrics_sync_at = None;
                        cx.notify();
                    }
                }))
                // Room around the lines so the glow of the current one is not cut by the
                // scroll box (the cover column is narrower by the same amount).
                .pl(sc(LYRICS_GUTTER))
                .pt(sc(32.))
                .pr(sc(36.))
                .pb(sc(64.))
                .flex()
                .flex_col()
                // Lines hug their text, so the glow surrounds the words and not the whole column.
                .items_start()
                // Three dots while the intro plays.
                .child(div().h(sc(54.)).flex().items_center().gap(sc(9.)).px(sc(10.)).children((0..3).map(|i| {
                    div()
                        .size(sc(9.))
                        .rounded(sc(5.))
                        .bg(if current.is_none() { theme::text() } else { gpui::rgba(0xffffff59) })
                        .opacity(1.0 - 0.0 * i as f32)
                })))
                .children(l.synced.iter().enumerate().map(|(i, (t, line))| {
                    let active = Some(i) == current;
                    let emphasis = line_emphasis(id, i, active);
                    let reading = roman.get(i).and_then(|r| r.as_ref()).filter(|_| show_roman);
                    let at = *t;
                    // A longer pause before a line opens a gap, like stanzas.
                    let gap_before = i > 0 && (*t - l.synced[i - 1].0) > 5.5;
                    // Official app: 32 px / 600 / 48 px line height, inactive lines at 55 % opacity.
                    let edge = edge_fade(&view.lyrics_scroll, i + 1, 60.0, 130.0);
                    let base = div()
                        .id(("np-line", i))
                        .opacity(edge)
                        .when(gap_before, |d| d.mt(sc(26.)))
                        .px(sc(10.))
                        .py(sc(8.))
                        .rounded(sc(10.))
                        .cursor_pointer()
                        .text_size(sc(44. * lyric_scale))
                        .line_height(sc(58. * lyric_scale))
                        .font_weight(FontWeight::SEMIBOLD)
                        .flex()
                        .flex_col()
                        .child(reading.cloned().unwrap_or_else(|| if line.is_empty() { "♪".into() } else { line.clone() }))
                        .when(reading.is_some(), |d| {
                            d.child(
                                div()
                                    .text_size(sc(26. * lyric_scale))
                                    .line_height(sc(34. * lyric_scale))
                                    .font_weight(FontWeight::MEDIUM)
                                    .opacity(0.8)
                                    .child(line.clone()),
                            )
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.lyrics_follow = true;
                            this.seek(at.max(0.0), cx)
                        }));
                    const INACTIVE: f32 = 0.55;
                    let white = |a: f32| super::fx::with_alpha(gpui::rgb(0xffffff), a);
                    base.text_color(white(INACTIVE + (1. - INACTIVE) * emphasis))
                        .shadow(vec![
                            gpui::BoxShadow {
                                color: gpui::Hsla::from(super::fx::with_alpha(glow_color, 0.30 * emphasis)),
                                offset: gpui::point(sc(0.), sc(0.)),
                                blur_radius: sc(34.),
                                spread_radius: sc(2. * emphasis),
                            },
                            gpui::BoxShadow {
                                color: gpui::Hsla::from(super::fx::with_alpha(glow_color, 0.16 * emphasis)),
                                offset: gpui::point(sc(0.), sc(0.)),
                                blur_radius: sc(48.),
                                spread_radius: sc(4. * emphasis),
                            },
                        ])
                        .hover(|s| s.text_color(white(0.85)))
                        .into_any_element()
                }))
                .child(
                    div()
                        .pt(sc(24.))
                        .text_size(sc(11.))
                        .text_color(theme::text_secondary())
                        .child(crate::i18n::text("lyrics.provider", std::slice::from_ref(&l.provider))),
                )
                .child(super::smooth::glide("lyrics"));
            super::widgets::with_sync_button(list.into_any_element(), view, "np", false, cx)
        }
        Some(Load::Ready(l)) if !l.text.is_empty() => div()
            .id("np-lyrics")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&super::smooth::handle("np-lyrics-text"))
            .child(super::smooth::glide("np-lyrics-text"))
            .pr(sc(24.))
            .pb(sc(64.))
            .text_size(sc(36. * lyric_scale))
            .line_height(sc(48. * lyric_scale))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(gpui::rgba(0xffffffd9))
            .children(l.text.lines().enumerate().map(|(i, line)| {
                let reading = l.roman.get(i).and_then(|r| r.as_ref()).filter(|_| show_roman);
                div()
                    .child(reading.cloned().unwrap_or_else(|| if line.is_empty() { "\u{a0}".into() } else { line.to_owned() }))
                    .when(reading.is_some(), |d| d.child(div().text_size(sc(16.)).line_height(sc(23.)).opacity(0.8).child(line.to_owned())))
            }))
            .into_any_element(),
        Some(Load::Loading) => {
            div().text_size(sc(16.)).text_color(gpui::rgba(0xffffff99)).child(crate::i18n::t("searching_for_lyrics")).into_any_element()
        }
        _ => missing_lyrics(cx),
    }
}

fn credits_view(view: &AppView, id: u64) -> AnyElement {
    match view.credits.get(&id) {
        Some(Load::Ready(list)) if !list.is_empty() => div()
            .id("np-credits")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&super::smooth::handle("np-credits"))
            .child(super::smooth::glide("np-credits"))
            .pr(sc(24.))
            .pb(sc(64.))
            .flex()
            .flex_col()
            .gap(sc(20.))
            .children(list.iter().map(|(role, names)| {
                div()
                    .flex()
                    .flex_col()
                    .gap(sc(4.))
                    .child(
                        div()
                            .text_size(sc(11.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(gpui::rgba(0xffffff80))
                            .child(role_label(role).to_uppercase()),
                    )
                    .child(div().text_size(sc(20.)).font_weight(FontWeight::BOLD).child(names.join(", ")))
            }))
            .into_any_element(),
        Some(Load::Loading) => {
            div().text_size(sc(16.)).text_color(gpui::rgba(0xffffff99)).child(crate::i18n::t("loading_credits")).into_any_element()
        }
        _ => div()
            .text_size(sc(16.))
            .text_color(gpui::rgba(0xffffff99))
            .child(crate::i18n::t("no_credits_for_this_track"))
            .into_any_element(),
    }
}

fn similar_view(view: &mut AppView, id: u64, cx: &mut Context<AppView>) -> AnyElement {
    let tracks = match view.similar.get(&id) {
        Some(Load::Ready(t)) => t.clone(),
        Some(Load::Loading) => {
            return div()
                .text_size(sc(16.))
                .text_color(gpui::rgba(0xffffff99))
                .child(crate::i18n::t("finding_similar_tracks"))
                .into_any_element()
        }
        _ => {
            return div()
                .text_size(sc(16.))
                .text_color(gpui::rgba(0xffffff99))
                .child(crate::i18n::t("no_similar_tracks"))
                .into_any_element()
        }
    };
    let playing = view.now.as_ref().map(|n| n.track.id);
    div()
        .id("np-similar")
        .size_full()
        .overflow_y_scroll()
        .track_scroll(&super::smooth::handle("np-similar"))
        .child(super::smooth::glide("np-similar"))
        .pr(sc(24.))
        .pb(sc(64.))
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
                        .child(
                            div()
                                .truncate()
                                .text_size(sc(13.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(if current { theme::accent() } else { theme::text() })
                                .child(t.display_title()),
                        )
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
    let vw = f32::from(window.viewport_size().width) / super::zoom();
    let stacked = vw < 820.;
    let margin = (vw * 0.035).clamp(20., 64.);
    let Some(track) = view.now.as_ref().map(|n| n.track.clone()) else {
        return div()
            .id("now-playing-empty")
            .absolute()
            .size_full()
            .occlude()
            .bg(theme::bg())
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(sc(28.))
            .p(sc(margin))
            .text_color(theme::text_secondary())
            .child(crate::i18n::t("play_a_track_to_view_it_in_full_screen"))
            .child(div().w(sc(360.)).max_w_full().child(super::player_bar::controls(view, true, vw, cx)))
            .into_any_element();
    };
    let ambient = view.ambient(cx);
    let palette = view.palette(cx);
    let view_blur = view.settings.lock().np_blur;
    if view.np_tilt.step() {
        super::motion::want_frame("tilt");
    }
    let frames_url = if view_blur { track.cover(640) } else { None };
    let frames = frames_url.as_deref().and_then(|u| super::images::frames(u, cx));
    let (frames_prev, frames_now) = view.np_frames_fade.frame(frames_url.as_deref(), frames);
    let angle = view.bg_angle_now;
    let size = cover_size(view, window);
    let local = track.local_path.is_some();
    let id = track.id;
    if local && matches!(view.np_tab, NpTab::Credits | NpTab::Similar) {
        view.np_tab = NpTab::Lyrics;
    }
    let tab = view.np_tab;
    let body = match tab {
        NpTab::Lyrics => lyrics_view(view, id, window, cx),
        NpTab::Queue => super::pages::queue_panel(view, "np-queue", cx),
        NpTab::Credits => credits_view(view, id),
        NpTab::Similar => similar_view(view, id, cx),
    };
    let tabs = div()
        .flex_none()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_center()
        .gap(sc(6.))
        .pb(sc(20.))
        .child(tab_button(NpTab::Queue, view, cx))
        .when(!local, |d| d.child(tab_button(NpTab::Similar, view, cx)).child(tab_button(NpTab::Credits, view, cx)))
        .child(tab_button(NpTab::Lyrics, view, cx));
    let artwork = div()
        .id("np-big-cover")
        .relative()
        .flex_none()
        .size(sc(size))
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
        // One continuous surface that follows the mouse: same
        // drawing at rest and tilted, so nothing switches.
        .child({
            let url = track.cover(1280);
            let src = url.as_deref().and_then(|u| super::images::sized(u, size, size, cx));
            let (prev, now) = view.np_cover_fade.frame(url.as_deref(), src);
            view.prefetch_next_covers(Some(size), cx);
            view.np_tilt.surface(prev, now, size, ambient, theme::surface())
        })
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
                if !local && ev.click_count() >= 2 {
                    if !this.fav_ids.contains(&t.id) {
                        this.toggle_favorite(&t, cx);
                    }
                    this.heart_burst();
                    cx.notify();
                }
            })
        });
    let left = div()
        .id("np-artwork-column")
        .min_w(sc(0.))
        .flex_none()
        .when(stacked, |d| d.w_full())
        .when(!stacked, |d| d.w(relative(0.46)).h_full().overflow_y_scroll())
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(sc(24.))
        .child(artwork)
        .child(
            div()
                .w_full()
                .max_w(sc(520.))
                .flex_none()
                .flex()
                .flex_col()
                .items_center()
                .gap(sc(8.))
                .text_center()
                .child(
                    div()
                        .w_full()
                        .text_size(sc(if stacked { 24. } else { 30. }))
                        .line_height(sc(36.))
                        .font_weight(FontWeight::BOLD)
                        .line_clamp(2)
                        .child(track.display_title()),
                )
                .child(
                    div()
                        .w_full()
                        .text_size(sc(17.))
                        .line_height(sc(24.))
                        .text_color(gpui::rgba(0xffffffb3))
                        .line_clamp(2)
                        .child(track.artist_names()),
                ),
        )
        .child(div().w_full().max_w(sc(500.)).flex_none().child(super::player_bar::controls(view, true, vw, cx)));
    let details = div()
        .min_w(sc(0.))
        .min_h(sc(0.))
        .flex()
        .flex_col()
        .when(stacked, |d| d.w_full().h(sc(480.)).flex_none())
        .when(!stacked, |d| d.flex_1().h_full())
        .child(tabs)
        .child(div().relative().flex_1().min_h(sc(0.)).flex().flex_col().child(body).with_animation(
            ("np-detail", tab as usize),
            super::motion::animation(super::motion::PANEL),
            super::motion::enter,
        ));
    div()
        .id("now-playing")
        // Dark palette in both themes (see theme::dark_scope).
        .text_color(theme::text())
        .absolute()
        .left(sc(0.))
        .top(sc(0.))
        .size_full()
        .occlude()
        .bg(theme::bg())
        // The official background: the cover, blurred and saturated, turning slowly about
        // its centre (see `fx::rotating_backdrop`). Flat colour when that setting is chosen.
        .child(match (view_blur, frames_now) {
            // New cover's background fades in over the old one (never over black).
            (true, Some((f, a))) => div()
                .absolute()
                .top(sc(0.))
                .left(sc(0.))
                .size_full()
                .children(frames_prev.map(|p| super::fx::rotating_backdrop(&p, angle)))
                .child(div().absolute().top(sc(0.)).left(sc(0.)).size_full().opacity(a).child(super::fx::rotating_backdrop(&f, angle)))
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
        .child(super::motion::looping(
            div().absolute().top(sc(0.)).left(sc(0.)).size_full(),
            std::time::Duration::from_secs(28),
            "np-light",
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
        ))
        // A soft veil keeps the text and controls readable over the artwork.
        .child(div().absolute().top(sc(0.)).left(sc(0.)).size_full().bg(gpui::linear_gradient(
            180.,
            gpui::linear_color_stop(gpui::rgba(0x00000057), 0.),
            gpui::linear_color_stop(gpui::rgba(0x000000c2), 1.),
        )))
        .child(
            div()
                .id("np-layout")
                .relative()
                .size_full()
                .p(sc(margin))
                .flex()
                .gap(sc(margin))
                .when(stacked, |d| d.flex_col().overflow_y_scroll())
                .child(left)
                .child(details),
        )
        .into_any_element()
}

/// Shared transition state keeps both lyric presentations consistent.
pub fn line_emphasis(id: u64, line: usize, active: bool) -> f32 {
    let target = if active { 1. } else { 0. };
    if super::motion::reduced_motion() {
        return target;
    }
    let key = format!("lyric-emphasis-{id}-{line}");
    super::interact::set(&key, target, super::motion::seconds(super::motion::PANEL));
    let value = super::interact::get(&key);
    if (value - target).abs() > 0.001 {
        super::motion::want_frame("lyrics-highlight");
    }
    value
}

pub fn upcoming(lines: &[(f64, String)], current: Option<usize>, position: f64, first: usize) -> Option<(usize, f32)> {
    let next = current.map(|i| i + 1).unwrap_or(0);
    // Move toward the next layout position in the last 350 ms, with the same
    // 150 ms lead used by the active-line selection.
    let at = lines.get(next)?.0;
    let blend = ((position + 0.15 - at + 0.35) / 0.35).clamp(0., 1.) as f32;
    let blend = super::motion::lyrics_ease(blend);
    Some((next + first, blend))
}

pub fn missing_lyrics(cx: &mut Context<AppView>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(sc(12.))
        .p(sc(16.))
        .child(div().text_size(sc(14.)).text_color(theme::text_secondary()).child(crate::i18n::t("no_provider_has_lyrics_for_this_track")))
        .child(super::widgets::pill(
            "retry-lyrics",
            crate::i18n::t("lyrics.retry"),
            None,
            false,
            |v, _, _, cx| {
                if let Some(track) = v.now.as_ref().map(|n| n.track.clone()) {
                    crate::tidal::lyrics::forget_missing(&track);
                    v.lyrics.remove(&track.id);
                    v.load_lyrics(track.id, cx);
                    cx.notify();
                }
            },
            cx,
        ))
        .into_any_element()
}
