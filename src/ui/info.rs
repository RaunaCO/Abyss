//! Song Info / Album Info / Artist Info: a compact floating panel with the
//! artwork and the facts Abyss actually has. Credits come from TIDAL, the year
//! from the album, and the technical data from the stream itself (see
//! `tidal::probe`). Anything unknown says so. Escape or a click outside
//! closes it; playback is never touched.

use gpui::AnimationExt;
use gpui::{div, prelude::*, AnyElement, Context, FontWeight, MouseButton, SharedString, Window};

use super::widgets::{cover, fmt_long, fmt_time};
use super::{sc, theme, AppView, Load};
use crate::tidal::probe::TechInfo;
use crate::tidal::{Album, Artist, Track};

#[derive(Clone)]
pub enum InfoTarget {
    Song(Track),
    Album(Album),
    Artist(Artist),
}

impl AppView {
    pub fn open_info(&mut self, target: InfoTarget, cx: &mut Context<Self>) {
        match &target {
            InfoTarget::Song(t) => {
                if t.local_path.is_some() {
                    return;
                }
                self.load_credits(t.id, cx);
                if let Some(al) = &t.album {
                    self.ensure_album(al.id, cx);
                }
                let id = t.id;
                if let std::collections::hash_map::Entry::Vacant(e) = self.tech_info.entry(id) {
                    e.insert(Load::Loading);
                    let q = self.settings.lock().quality;
                    self.fetch(
                        cx,
                        move |t| {
                            let s = t.stream(id, q)?;
                            let mut info = t.probe_stream(&s)?;
                            if info.codec.is_empty() {
                                info.codec = s.codec.to_uppercase();
                            }
                            Ok((info, s.audio_quality))
                        },
                        move |this, r, _| {
                            this.tech_info.insert(id, super::into_load(r));
                        },
                    );
                }
            }
            InfoTarget::Album(a) => self.ensure_album(a.id, cx),
            InfoTarget::Artist(a) => {
                let id = a.id;
                if crate::local::is_local_id(id) && !self.artists.contains_key(&id) {
                    self.local_changed();
                }
                if let std::collections::hash_map::Entry::Vacant(e) = self.artists.entry(id) {
                    e.insert(Load::Loading);
                    self.fetch(
                        cx,
                        move |t| t.artist(id),
                        move |this, r, _| {
                            this.artists.insert(id, super::into_load(r));
                        },
                    );
                }
            }
        }
        self.info_panel = Some(target);
        cx.notify();
    }

    pub fn close_info(&mut self, cx: &mut Context<Self>) -> bool {
        if self.info_panel.take().is_some() {
            cx.notify();
            true
        } else {
            false
        }
    }

    /// Load an album (details and tracks) into the cache once.
    pub fn ensure_album(&mut self, id: u64, cx: &mut Context<Self>) {
        if crate::local::is_local_id(id) {
            if !self.albums.contains_key(&id) {
                self.local_changed();
            }
            return;
        }
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
}

fn kv(label: &str, value: String, dim: bool) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(sc(2.))
        .min_w(sc(0.))
        .child(div().text_size(sc(10.5)).font_weight(FontWeight::SEMIBOLD).text_color(theme::text_dim()).child(label.to_uppercase()))
        .child(div().text_size(sc(13.)).text_color(if dim { theme::text_dim() } else { theme::text() }).line_clamp(3).child(value))
        .into_any_element()
}

fn or_na(v: Option<String>) -> (String, bool) {
    match v {
        Some(s) if !s.is_empty() => (s, false),
        _ => (crate::i18n::t("unavailable").to_string(), true),
    }
}

fn grid(cells: Vec<AnyElement>, cols: usize) -> AnyElement {
    let mut rows = Vec::new();
    let mut it = cells.into_iter().peekable();
    while it.peek().is_some() {
        let row: Vec<AnyElement> = it.by_ref().take(cols).collect();
        let n = row.len();
        rows.push(
            div()
                .flex()
                .gap(sc(16.))
                .children(row.into_iter().map(|c| div().flex_1().min_w(sc(0.)).child(c)))
                .children((n..cols).map(|_| div().flex_1()))
                .into_any_element(),
        );
    }
    div().flex().flex_col().gap(sc(12.)).children(rows).into_any_element()
}

fn section(title: &str) -> AnyElement {
    div()
        .pt(sc(14.))
        .mt(sc(14.))
        .border_t_1()
        .border_color(theme::divider())
        .pb(sc(10.))
        .text_size(sc(theme::TYPE_LABEL))
        .font_weight(FontWeight::BOLD)
        .child(title.to_string())
        .into_any_element()
}

fn song(view: &AppView, t: &Track, cx: &mut Context<AppView>) -> AnyElement {
    let primary = t.artists.first().map(|a| a.name.clone());
    let featured = (t.artists.len() > 1).then(|| t.artists[1..].iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", "));
    let producers = match view.credits.get(&t.id) {
        Some(Load::Ready(list)) => {
            let p: Vec<String> = list.iter().filter(|(r, _)| r.to_lowercase().contains("producer")).flat_map(|(_, n)| n.clone()).collect();
            if p.is_empty() {
                (crate::i18n::t("unavailable").to_string(), true)
            } else {
                (p.join(", "), false)
            }
        }
        Some(Load::Loading) => (crate::i18n::t("loading").into(), true),
        _ => (crate::i18n::t("unavailable").to_string(), true),
    };
    let year = t
        .album
        .as_ref()
        .and_then(|a| view.albums.get(&a.id))
        .map(|l| match l {
            Load::Ready((al, _)) => or_na(Some(al.year().to_string())),
            Load::Loading => (crate::i18n::t("loading").into(), true),
            _ => (crate::i18n::t("unavailable").to_string(), true),
        })
        .unwrap_or((crate::i18n::t("unavailable").to_string(), true));
    let (pa, pa_d) = or_na(primary);
    let (fe, fe_d) = match featured {
        Some(f) => (f, false),
        None => (crate::i18n::t("none").into(), true),
    };
    let top = div()
        .flex()
        .gap(sc(18.))
        .child(div().rounded(sc(8.)).overflow_hidden().flex_none().child(cover(t.cover(320), 132., false, cx)))
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .flex()
                .flex_col()
                .gap(sc(12.))
                .child(div().text_size(sc(theme::TYPE_SECTION)).font_weight(FontWeight::BOLD).line_clamp(2).child(t.display_title()))
                .child(kv(crate::i18n::t("main_artist"), pa, pa_d))
                .child(kv(crate::i18n::t("featured_artist"), fe, fe_d))
                .child(kv(crate::i18n::t("production"), producers.0, producers.1)),
        );
    let numbers = grid(
        vec![
            kv(crate::i18n::t("year"), year.0, year.1),
            kv(
                crate::i18n::t("disc"),
                if t.volume_number > 0 { t.volume_number.to_string() } else { crate::i18n::t("unavailable").into() },
                t.volume_number == 0,
            ),
            kv(
                crate::i18n::t("track"),
                if t.track_number > 0 { t.track_number.to_string() } else { crate::i18n::t("unavailable").into() },
                t.track_number == 0,
            ),
        ],
        3,
    );
    // Technical data, from the stream as Abyss would play it now.
    let tech: AnyElement = match view.tech_info.get(&t.id) {
        Some(Load::Ready((ti, tier))) => tech_grid(ti, tier, t),
        Some(Load::Failed(e)) => div()
            .text_size(sc(theme::TYPE_LABEL))
            .text_color(theme::text_dim())
            .child(crate::i18n::text("could_not_read_stream_data", &[e.to_string()]))
            .into_any_element(),
        _ => {
            div().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_dim()).child(crate::i18n::t("reading_stream")).into_any_element()
        }
    };
    div()
        .flex()
        .flex_col()
        .child(top)
        .child(section(crate::i18n::t("track")))
        .child(numbers)
        .child(section(crate::i18n::t("technical")))
        .child(tech)
        .into_any_element()
}

fn tech_grid(ti: &TechInfo, tier: &str, t: &Track) -> AnyElement {
    let format = if ti.codec.is_empty() {
        (crate::i18n::t("unavailable").to_string(), true)
    } else {
        (
            match (ti.codec.as_str(), ti.bits) {
                ("FLAC", Some(b)) => crate::i18n::text("flac_bit", &[format!("{}", b)]),
                (c, _) => c.to_string(),
            },
            false,
        )
    };
    // Exact length from the samples when the header has them.
    let dur_s = match (ti.total_samples, ti.sample_rate) {
        (Some(n), Some(r)) if r > 0 => n as f64 / r as f64,
        _ => t.duration as f64,
    };
    let bitrate = match (ti.file_size, dur_s > 0.0) {
        (Some(size), true) => (crate::i18n::text("kbps_file_average", &[format!("{:.0}", size as f64 * 8.0 / dur_s / 1000.0)]), false),
        _ if ti.codec == "AAC" && tier == "HIGH" => (crate::i18n::t("320_kbps_tidal_nominal").into(), false),
        _ if ti.codec == "AAC" && tier == "LOW" => (crate::i18n::t("96_kbps_tidal_nominal").into(), false),
        _ if ti.segmented => (crate::i18n::t("unavailable_segmented_dash_stream").into(), true),
        _ => (crate::i18n::t("unavailable").into(), true),
    };
    let rate = ti.sample_rate.map(|r| crate::i18n::text("khz", &[crate::audio::output::fmt_khz(r).to_string()]));
    let channels = ti.channels.map(|c| match c {
        1 => crate::i18n::t("1_mono").to_string(),
        2 => crate::i18n::t("2_stereo").to_string(),
        n => n.to_string(),
    });
    let size = match ti.file_size {
        Some(s) => (crate::i18n::text("mb", &[format!("{:.1}", s as f64 / 1_048_576.0)]), false),
        None if ti.segmented => (crate::i18n::t("unavailable_segmented_dash_stream").into(), true),
        None => (crate::i18n::t("unavailable").into(), true),
    };
    let (r, r_d) = or_na(rate);
    let (c, c_d) = or_na(channels);
    grid(
        vec![
            kv(crate::i18n::t("audio_format"), format.0, format.1),
            kv(crate::i18n::t("bitrate"), bitrate.0, bitrate.1),
            kv(crate::i18n::t("sample_rate"), r, r_d),
            kv(crate::i18n::t("channels"), c, c_d),
            kv(crate::i18n::t("duration"), fmt_time(dur_s), false),
            kv(crate::i18n::t("file_size"), size.0, size.1),
        ],
        3,
    )
}

fn album(view: &AppView, a: &Album, cx: &mut Context<AppView>) -> AnyElement {
    let full = view.albums.get(&a.id).and_then(Load::ready).map(|(al, tr)| (al.clone(), tr.len()));
    let al = full.as_ref().map(|(al, _)| al.clone()).unwrap_or_else(|| a.clone());
    let n = full.as_ref().map(|(_, n)| *n).unwrap_or(al.number_of_tracks as usize);
    let quality: Vec<String> = al.tags.iter().map(|t| super::widgets::quality_name(t)).collect();
    div()
        .flex()
        .gap(sc(18.))
        .child(div().rounded(sc(8.)).overflow_hidden().flex_none().child(cover(al.cover(320), 132., false, cx)))
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .flex()
                .flex_col()
                .gap(sc(12.))
                .child(div().text_size(sc(theme::TYPE_SECTION)).font_weight(FontWeight::BOLD).line_clamp(2).child(al.title.clone()))
                .child(kv(crate::i18n::t("artist"), al.artist_names(), false))
                .child(grid(
                    vec![
                        kv(
                            crate::i18n::t("released"),
                            if al.release_date.is_empty() {
                                crate::i18n::t("unavailable").into()
                            } else {
                                super::widgets::fmt_date(&al.release_date)
                            },
                            al.release_date.is_empty(),
                        ),
                        kv(crate::i18n::t("tracks"), n.to_string(), false),
                        kv(crate::i18n::t("duration"), fmt_long(al.duration as u64), false),
                    ],
                    3,
                ))
                .child(kv(
                    crate::i18n::t("quality"),
                    if quality.is_empty() { crate::i18n::t("unavailable").into() } else { quality.join(", ") },
                    quality.is_empty(),
                ))
                .when(!al.copyright.is_empty(), |d| d.child(kv(crate::i18n::t("copyright"), al.copyright.clone(), false))),
        )
        .into_any_element()
}

fn artist(view: &AppView, a: &Artist, cx: &mut Context<AppView>) -> AnyElement {
    let bio = match view.artists.get(&a.id) {
        Some(Load::Ready(p)) => {
            p.bio.clone().map(|b| (b, false)).unwrap_or((crate::i18n::t("tidal_has_no_biography_for_this_artist").into(), true))
        }
        Some(Load::Loading) | Some(Load::Idle) | None => (crate::i18n::t("loading").into(), true),
        Some(Load::Failed(_)) => (crate::i18n::t("unavailable").into(), true),
    };
    div()
        .flex()
        .gap(sc(18.))
        .child(div().flex_none().child(cover(a.picture(320), 120., true, cx)))
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .flex()
                .flex_col()
                .gap(sc(10.))
                .child(div().text_size(sc(18.)).font_weight(FontWeight::BOLD).child(a.name.clone()))
                .child(
                    div()
                        .id("artist-bio")
                        .max_h(sc(260.))
                        .overflow_y_scroll()
                        .text_size(sc(12.5))
                        .line_height(sc(19.))
                        .text_color(if bio.1 { theme::text_dim() } else { theme::text_secondary() })
                        .child(bio.0),
                ),
        )
        .into_any_element()
}

pub fn overlay(view: &mut AppView, _window: &mut Window, cx: &mut Context<AppView>) -> Option<AnyElement> {
    let target = view.info_panel.clone()?;
    use super::icons::Icon;
    let (icon, title, body) = match &target {
        InfoTarget::Song(t) => (Icon::Track, crate::i18n::t("song_info"), song(view, t, cx)),
        InfoTarget::Album(a) => (Icon::Album, crate::i18n::t("album_info"), album(view, a, cx)),
        InfoTarget::Artist(a) => (Icon::Artist, crate::i18n::t("artist_info"), artist(view, a, cx)),
    };
    Some(
        gpui::deferred(
            div()
                .id("info-backdrop")
                .absolute()
                .top(sc(0.))
                .left(sc(0.))
                .size_full()
                .occlude()
                .bg(gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.35 })
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.close_info(cx);
                    }),
                )
                .child(
                    super::widgets::floating_panel(16.)
                        .id("info-panel")
                        .w(sc(560.))
                        .max_w_full()
                        .max_h(gpui::relative(0.9))
                        .overflow_y_scroll()
                        .p(sc(20.))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .flex()
                        .flex_col()
                        .child(super::glass::sheen("track-info-glass", 16., 0.7))
                        .child(
                            div()
                                .pb(sc(12.))
                                .mb(sc(14.))
                                .border_b_1()
                                .border_color(theme::divider())
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(super::widgets::panel_heading(icon, title, None))
                                .child(super::widgets::icon_button(
                                    "info-close",
                                    super::icons::Icon::Close,
                                    14.,
                                    theme::text_secondary(),
                                    |this, _, _, cx| {
                                        this.close_info(cx);
                                    },
                                    cx,
                                )),
                        )
                        .child(body)
                        .with_animation("info-entrance", super::motion::animation(super::motion::FAST), super::motion::enter),
                ),
        )
        .with_priority(3)
        .into_any_element(),
    )
}
