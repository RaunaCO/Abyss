//! Replay: a dashboard of personal listening statistics, computed from the
//! history Abyss records while it plays (see `crate::history`). Every figure
//! comes from those entries and each period states its dates and coverage.

use super::OneLine;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{div, prelude::*, relative, AnyElement, Context, Div, FontWeight, SharedString, Window};

use super::icons::Icon;
use super::widgets::{cover, pill, widget_panel, PANEL_GAP};
use super::{interact, sc, theme, AppView, Load, Page};
use crate::history::{civil_from_days, days_from_civil, local_day_hour, local_offset, unix_now, Entry};
use crate::tidal::image_url;

fn months() -> [&'static str; 12] {
    [
        crate::i18n::t("jan"),
        crate::i18n::t("feb"),
        crate::i18n::t("mar"),
        crate::i18n::t("apr"),
        crate::i18n::t("may"),
        crate::i18n::t("jun"),
        crate::i18n::t("jul"),
        crate::i18n::t("aug"),
        crate::i18n::t("sep"),
        crate::i18n::t("oct"),
        crate::i18n::t("nov"),
        crate::i18n::t("dec"),
    ]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Period {
    #[default]
    ThisYear,
    Last30,
    ThisMonth,
    AllTime,
}

impl Period {
    const ALL: [Period; 4] = [Period::ThisYear, Period::Last30, Period::ThisMonth, Period::AllTime];
    fn label(self) -> &'static str {
        match self {
            Period::ThisYear => crate::i18n::t("this_year"),
            Period::Last30 => crate::i18n::t("last_30_days"),
            Period::ThisMonth => crate::i18n::t("this_month"),
            Period::AllTime => crate::i18n::t("all_time"),
        }
    }
}

/// Parts of the page, one list block each.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Part {
    Header,
    Dashboard,
}

/// A grouped item (track, artist or album) with its plays and time.
#[derive(Clone)]
pub struct Ranked {
    pub id: u64,
    pub name: String,
    pub sub: String,
    pub cover: Option<String>,
    pub plays: u32,
    pub secs: u64,
}

pub struct Stats {
    pub start_day: i64,
    pub today: i64,
    /// First day with any recorded listening (all time), if any.
    pub first_day: Option<i64>,
    pub total_secs: u64,
    pub plays: u32,
    pub distinct_tracks: usize,
    pub distinct_artists: usize,
    pub active_days: usize,
    /// (label, plays) per bucket and the bucket unit ("día", "mes", "año").
    pub timeline: Vec<(String, u64)>,
    pub unit: &'static str,
    /// Tracks by plays (up to 50, for "Play Top Tracks" and the playlist).
    pub top_tracks: Vec<Ranked>,
    pub top_artists: Vec<Ranked>,
    pub top_albums: Vec<Ranked>,
}

fn day_label(day: i64) -> String {
    let (_, m, d) = civil_from_days(day);
    crate::i18n::text("date.short", &[d.to_string(), months()[(m - 1) as usize].into()])
}

fn full_date(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    crate::i18n::text("date.full", &[d.to_string(), months()[(m - 1) as usize].into(), y.to_string()])
}

fn rank(items: impl Iterator<Item = Ranked>, n: usize) -> Vec<Ranked> {
    let mut v: Vec<Ranked> = items.filter(|r| r.plays > 0).collect();
    v.sort_by(|a, b| b.plays.cmp(&a.plays).then(b.secs.cmp(&a.secs)).then(a.name.cmp(&b.name)));
    v.truncate(n);
    v
}

/// Statistics of `history` for `period`, as of now.
pub fn compute(history: &[Entry], period: Period) -> Stats {
    compute_at(history, period, unix_now(), local_offset())
}

pub fn compute_at(history: &[Entry], period: Period, now: u64, off: i64) -> Stats {
    let (today, _) = local_day_hour(now, off);
    let (ty, tm, _) = civil_from_days(today);
    let first_day = history.iter().map(|e| local_day_hour(e.ts, off).0).min();
    let start_day = match period {
        Period::ThisYear => days_from_civil(ty, 1, 1),
        Period::Last30 => today - 29,
        Period::ThisMonth => days_from_civil(ty, tm, 1),
        Period::AllTime => first_day.unwrap_or(today),
    };
    let mut total_secs = 0u64;
    let mut plays = 0u32;
    let mut days: HashMap<i64, (u64, u32)> = HashMap::new();
    let mut tracks: HashMap<u64, Ranked> = HashMap::new();
    let mut artists: HashMap<String, Ranked> = HashMap::new();
    let mut albums: HashMap<u64, Ranked> = HashMap::new();
    for e in history {
        let (d, _) = local_day_hour(e.ts, off);
        if d < start_day || d > today {
            continue;
        }
        let secs = e.secs as u64;
        let play = e.counts_as_play() as u32;
        total_secs += secs;
        plays += play;
        let slot = days.entry(d).or_default();
        slot.0 += secs;
        slot.1 += play;
        let cover = e.cover.as_deref().map(|c| image_url(c, 320));
        let t = tracks.entry(e.id).or_insert_with(|| Ranked {
            id: e.id,
            name: e.title.clone(),
            sub: e.artist.clone(),
            cover: cover.clone(),
            plays: 0,
            secs: 0,
        });
        t.plays += play;
        t.secs += secs;
        let main = e.artist.split(", ").next().unwrap_or("").to_string();
        if !main.is_empty() {
            let a = artists.entry(main.clone()).or_insert_with(|| Ranked {
                id: e.artist_id,
                name: main.clone(),
                sub: String::new(),
                cover: None,
                plays: 0,
                secs: 0,
            });
            a.plays += play;
            a.secs += secs;
        }
        if e.album_id != 0 {
            let al = albums.entry(e.album_id).or_insert_with(|| Ranked {
                id: e.album_id,
                name: e.album.clone(),
                sub: main.clone(),
                cover: cover.clone(),
                plays: 0,
                secs: 0,
            });
            al.plays += play;
            al.secs += secs;
        }
    }
    // Plays over time: months for this year, days for 30 days / this month;
    // all time adapts to the span of the history (days, months or years).
    let span = today - start_day + 1;
    let by_day =
        |s: i64| -> Vec<(String, u64)> { (s..=today).map(|d| (day_label(d), days.get(&d).map(|v| v.1 as u64).unwrap_or(0))).collect() };
    let by_month = |s: i64| -> Vec<(String, u64)> {
        let (sy, sm, _) = civil_from_days(s);
        let (mut y, mut m) = (sy, sm);
        let mut out: Vec<((i64, u32), u64)> = Vec::new();
        while (y, m) <= (ty, tm) {
            out.push(((y, m), 0));
            (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
        }
        for (d, (_, p)) in &days {
            let (yy, mm, _) = civil_from_days(*d);
            if let Some(slot) = out.iter_mut().find(|((a, b), _)| *a == yy && *b == mm) {
                slot.1 += *p as u64;
            }
        }
        let multi = sy != ty;
        out.into_iter()
            .map(|((y, m), p)| {
                (if multi { format!("{} {}", months()[(m - 1) as usize], y % 100) } else { months()[(m - 1) as usize].to_string() }, p)
            })
            .collect()
    };
    let (timeline, unit) = match period {
        Period::ThisYear => (by_month(start_day), crate::i18n::t("month")),
        Period::Last30 | Period::ThisMonth => (by_day(start_day), crate::i18n::t("day")),
        Period::AllTime if span <= 31 => (by_day(start_day), crate::i18n::t("day")),
        Period::AllTime if span <= 3 * 365 => (by_month(start_day), crate::i18n::t("month")),
        Period::AllTime => {
            let (sy, _, _) = civil_from_days(start_day);
            let mut out: Vec<(String, u64)> = (sy..=ty).map(|y| (y.to_string(), 0)).collect();
            for (d, (_, p)) in &days {
                let (y, _, _) = civil_from_days(*d);
                if let Some(slot) = out.get_mut((y - sy) as usize) {
                    slot.1 += *p as u64;
                }
            }
            (out, crate::i18n::t("year.singular"))
        }
    };
    let distinct_tracks = tracks.values().filter(|t| t.plays > 0).count();
    let distinct_artists = artists.values().filter(|a| a.plays > 0).count();
    Stats {
        start_day,
        today,
        first_day,
        total_secs,
        plays,
        distinct_tracks,
        distinct_artists,
        active_days: days.values().filter(|v| v.1 > 0).count(),
        timeline,
        unit,
        top_tracks: rank(tracks.into_values(), 50),
        top_artists: rank(artists.into_values(), 10),
        top_albums: rank(albums.into_values(), 10),
    }
}

/// Listening time, exact to the second for short spans.
pub fn fmt_listen(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h >= 24 {
        crate::i18n::text("d_h", &[format!("{}", h / 24), format!("{}", h % 24)])
    } else if h > 0 {
        crate::i18n::text("h_min", &[format!("{}", h), format!("{}", m)])
    } else if m >= 10 {
        crate::i18n::text("min", &[format!("{}", m)])
    } else if m > 0 {
        if s > 0 {
            crate::i18n::text("min_s", &[format!("{}", m), format!("{}", s)])
        } else {
            crate::i18n::text("min", &[format!("{}", m)])
        }
    } else {
        crate::i18n::text("s", &[format!("{}", s)])
    }
}

fn times(n: u32) -> String {
    if n == 1 {
        crate::i18n::t("1_play").into()
    } else {
        crate::i18n::text("plays.count", &[format!("{}", n)])
    }
}

impl AppView {
    /// Statistics for the chosen period, recomputed only when the history or
    /// the period changed (or once a minute, for the passing of days).
    pub fn replay_stats(&mut self) -> Rc<Stats> {
        let last: u64 = self.history.last().map(|e| e.secs as u64).unwrap_or(0);
        let key = (self.history.len(), last, self.replay_period, unix_now() / 60);
        if let Some((k, s)) = &self.replay_cache {
            if *k == key {
                return s.clone();
            }
        }
        let s = Rc::new(compute(&self.history, self.replay_period));
        self.replay_cache = Some((key, s.clone()));
        s
    }

    fn set_replay_period(&mut self, p: Period, cx: &mut Context<Self>) {
        if self.replay_period != p {
            self.replay_period = p;
            self.replay_all_tracks = false;
            self.content_changed_pub();
            cx.notify();
        }
    }

    /// Play the most played tracks of the period, in order.
    fn replay_play_top(&mut self, limit: usize, cx: &mut Context<Self>) {
        let ids: Vec<u64> = self.replay_stats().top_tracks.iter().take(limit).map(|r| r.id).collect();
        if ids.is_empty() {
            return self.notify_toast(crate::i18n::t("no_plays_in_this_period"), cx);
        }
        self.notify_toast(crate::i18n::t("loading_your_top_songs"), cx);
        self.fetch(
            cx,
            move |t| Ok(ids.iter().filter_map(|id| t.track(*id).ok()).collect::<Vec<_>>()),
            |this, r: anyhow::Result<Vec<crate::tidal::Track>>, cx| match r {
                Ok(tracks) if !tracks.is_empty() => {
                    this.shuffle = false;
                    this.play_tracks(tracks, 0, cx)
                }
                Ok(_) => this.notify_toast(crate::i18n::t("could_not_load_tracks"), cx),
                Err(e) => this.notify_toast(crate::i18n::text("tracks.load_failed", &[format!("{:#}", e)]), cx),
            },
        );
    }

    /// The "New playlist" dialog, filled with the period's top tracks and a name.
    fn replay_make_playlist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let st = self.replay_stats();
        let ids: Vec<u64> = st.top_tracks.iter().map(|r| r.id).collect();
        if ids.is_empty() {
            return self.notify_toast(crate::i18n::t("no_plays_in_this_period"), cx);
        }
        let name = crate::i18n::text("replay.title_period", &[self.replay_period.label().to_string(), full_date(st.today).to_string()]);
        let desc = crate::i18n::text(
            "replay.playlist_description",
            &[format!("{}", ids.len()), full_date(st.start_day).to_string(), full_date(st.today).to_string()],
        );
        self.open_dialog(super::playlists::Dialog::NewPlaylist { add: ids }, window, cx);
        self.dlg_title.update(cx, |i, cx| i.set_text(&name, cx));
        self.dlg_desc.update(cx, |i, cx| i.set_text(&desc, cx));
    }

    /// Save the dashboard as it is on screen to a PNG in Pictures\Abyss.
    fn replay_share_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // From the top of the header (title, dates) to the end of the
        // dashboard, cut where the page panel ends (never the player bar).
        let (hb, b) = (self.replay_head_bounds, self.replay_bounds);
        let scale = window.scale_factor();
        let top = f32::from(hb.origin.y).min(f32::from(b.origin.y));
        let panel_bottom = f32::from(window.viewport_size().height) - (theme::PLAYER_H + 2.0 * theme::FRAME_GAP) * super::zoom();
        let bottom = f32::from(b.origin.y + b.size.height).min(panel_bottom);
        let rect = (
            (f32::from(b.origin.x) * scale).max(0.0) as i32,
            (top * scale).max(0.0) as i32,
            (f32::from(b.size.width) * scale) as i32,
            ((bottom - top) * scale).max(1.0) as i32,
        );
        let Some(img) = super::winctl::capture_client(window) else {
            return self.notify_toast(crate::i18n::t("could_not_capture_image"), cx);
        };
        let (y, m, d) = civil_from_days(self.replay_stats().today);
        let secs = unix_now() % 86400;
        let file = format!("Replay-{y}-{m:02}-{d:02}-{:02}{:02}{:02}.png", secs / 3600, secs / 60 % 60, secs % 60);
        let dir = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from).unwrap_or_default().join("Pictures").join("Abyss");
        let path = dir.join(file);
        cx.spawn(async move |this, cx| {
            let p2 = path.clone();
            let r = cx
                .background_executor()
                .spawn(async move {
                    let (w, h, bgra) = img;
                    let mut rgba = image::RgbaImage::new(w, h);
                    for (i, px) in rgba.pixels_mut().enumerate() {
                        let o = i * 4;
                        *px = image::Rgba([bgra[o + 2], bgra[o + 1], bgra[o], 255]);
                    }
                    let (x, y, cw, ch) = rect;
                    let x = (x as u32).min(w.saturating_sub(1));
                    let y = (y as u32).min(h.saturating_sub(1));
                    let cw = (cw.max(1) as u32).min(w - x);
                    let ch = (ch.max(1) as u32).min(h - y);
                    let crop = image::imageops::crop_imm(&rgba, x, y, cw, ch).to_image();
                    std::fs::create_dir_all(p2.parent().unwrap_or(std::path::Path::new(".")))?;
                    crop.save(&p2)?;
                    anyhow::Ok(())
                })
                .await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(()) => {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.display().to_string()));
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    this.notify_toast(crate::i18n::text("replay.image_saved", &[name.to_string()]), cx);
                }
                Err(e) => this.notify_toast(crate::i18n::text("could_not_save_image", &[format!("{:#}", e)]), cx),
            });
        })
        .detach();
    }

    /// Pictures of the top artists (one light request each, once).
    fn replay_artist_pics(&mut self, ids: &[u64], cx: &mut Context<Self>) {
        for &id in ids {
            if id == 0 || self.artist_pics.contains_key(&id) {
                continue;
            }
            if let Some(a) = self.fav_artists.ready().and_then(|l| l.iter().find(|a| a.id == id)) {
                let pic = a.picture(320);
                self.artist_pics.insert(id, Load::Ready(pic));
                continue;
            }
            self.artist_pics.insert(id, Load::Loading);
            self.fetch(
                cx,
                move |t| t.artist_basic(id).map(|a| a.picture(320)),
                move |this, r, _| {
                    this.artist_pics.insert(id, super::into_load(r));
                },
            );
        }
    }
}

/// Keep the dashboard's columns, with room between independent widgets.
fn pair(left: Div, right: Div, wide: bool, width: f32) -> Div {
    let half = ((width - PANEL_GAP) / 2.).max(1.);
    // Size the outer cells equally. The cards' own padding must not change
    // the split between the left and right columns.
    let cell = |card: Div| div().min_w(sc(0.)).flex().when(wide, |d| d.w(sc(half)).flex_none()).child(card.min_w(sc(0.)).flex_1());
    div().flex().gap(sc(PANEL_GAP)).when(!wide, |d| d.flex_col()).child(cell(left)).child(cell(right))
}

fn section(title: &'static str, note: Option<String>, inset: f32, body: impl IntoElement) -> Div {
    div()
        .p(sc(inset))
        .border_t_1()
        .border_color(theme::divider())
        .flex()
        .flex_col()
        .gap(sc(super::widgets::PANEL_HEAD_GAP))
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .flex_wrap()
                .gap(sc(8.))
                .child(div().text_size(sc(17.)).font_weight(FontWeight::BOLD).child(title))
                .children(note.map(|n| div().text_size(sc(11.)).text_color(theme::text_secondary()).child(n))),
        )
        .child(body)
}

fn stat(label: &'static str, value: String, detail: String, fraction: Option<f32>, width: f32) -> Div {
    let compact = width < 198.;
    let inset = if compact { super::widgets::PANEL_PAD_SM } else { super::widgets::PANEL_PAD };
    let meter_width = (width - 2. * inset - 2.).clamp(1., 150.);
    div()
        .p(sc(inset))
        .rounded(sc(11.))
        .bg(theme::ink(0.04))
        .flex()
        .flex_col()
        .items_start()
        .justify_center()
        .gap(sc(5.))
        .child(div().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_secondary()).child(label))
        .child(div().text_size(sc(if compact { 22. } else { 32. })).font_weight(FontWeight::BOLD).child(value))
        .child(div().text_size(sc(11.)).text_color(theme::text_secondary()).child(detail))
        // Every card keeps the meter's room, so the labels of cards side by side line up.
        .child(div().w(sc(meter_width)).flex_none().when(fraction.is_none(), |d| d.invisible()).child(meter(fraction.unwrap_or(0.))))
}

/// "2.4 al día"; small non-zero rates do not round down to 0.0.
fn per_day(plays: u64, days: i64) -> String {
    let rate = plays as f64 / days.max(1) as f64;
    if plays > 0 && rate < 0.1 {
        crate::i18n::t("less_than_0_1_per_day").into()
    } else {
        crate::i18n::text("per_day", &[format!("{:.1}", rate)])
    }
}

fn meter(fraction: f32) -> Div {
    div()
        .w_full()
        .h(sc(4.))
        .rounded(sc(2.))
        .bg(theme::divider())
        .child(div().w(relative(fraction.clamp(0., 1.))).h_full().rounded(sc(2.)).bg(theme::text()))
}

fn empty(text: &str) -> AnyElement {
    div().py(sc(8.)).text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_dim()).child(text.to_string()).into_any_element()
}

/// Bar chart of plays per bucket. Axis labels share the full plot width.
fn bars(values: &[(String, u64)], width: f32, cx: &mut Context<AppView>) -> AnyElement {
    let max = values.iter().map(|v| v.1).max().unwrap_or(0).max(1);
    let n = values.len();
    let labels = n.min((width / 52.).floor().clamp(2., 7.) as usize);
    let gap = if n > 20 || width < 320. { 2. } else { 4. };
    let height = (width * 0.34).clamp(122., 224.);
    div()
        .flex()
        .flex_col()
        .gap(sc(6.))
        .child(div().h(sc(height)).flex().items_end().gap(sc(gap)).border_b_1().border_color(theme::divider()).children(
            values.iter().enumerate().map(|(i, (label, v))| {
                let key = format!("rp-bar-{i}");
                let h = interact::hover(&key);
                let frac = *v as f32 / max as f32;
                let tip = format!("{label}: {}", if *v == 0 { crate::i18n::t("no_plays").into() } else { times(*v as u32) });
                let el =
                    div().id(SharedString::from(key.clone())).flex_1().h_full().flex().items_end().tooltip(super::sidebar::tip(tip)).child(
                        div().w_full().h(gpui::relative(if *v == 0 { 0.0 } else { frac.max(0.04) })).min_h(sc(2.)).rounded(sc(3.)).bg(
                            if *v == 0 {
                                theme::divider()
                            } else {
                                super::fx::with_alpha(theme::text(), if *v == max { 1.0 } else { 0.66 + 0.34 * h })
                            },
                        ),
                    );
                interact::track(el, key, cx)
            }),
        ))
        .child(div().flex().gap(sc(4.)).children((0..labels).map(|i| {
            let index = if labels == 1 { 0 } else { i * (n - 1) / (labels - 1) };
            div()
                .flex_1()
                .min_w(sc(0.))
                .one_line()
                .when(i > 0 && i + 1 < labels, |d| d.text_center())
                .when(i > 0 && i + 1 == labels, |d| d.text_right())
                .text_size(sc(10.))
                .text_color(theme::text_secondary())
                .child(values[index].0.clone())
        })))
        .into_any_element()
}

/// Horizontal carousel of covers / pictures with a name and plays.
fn carousel(
    id: &'static str,
    items: Vec<(Option<String>, String, String, Page)>,
    round: bool,
    width: f32,
    cx: &mut Context<AppView>,
) -> AnyElement {
    if items.is_empty() {
        return empty(crate::i18n::t("replay.period_empty"));
    }
    let gap = 12.0;
    let columns = (((width + gap) / (104.0 + gap)).floor() as usize).clamp(1, 6).min(items.len());
    let size = ((width - gap * (columns - 1) as f32) / columns as f32).floor().clamp(1., 136.);
    let fits = size * items.len() as f32 + gap * (items.len() - 1) as f32 <= width;
    div()
        .id(id)
        .w_full()
        .flex()
        .gap(sc(gap))
        .when(fits, |d| d.justify_center())
        .overflow_x_scroll()
        .pb(sc(4.))
        .children(items.into_iter().enumerate().map(|(i, (pic, name, sub, page))| {
            let key = format!("{id}-{i}");
            let h = interact::hover(&key);
            let el = div()
                .id(SharedString::from(key.clone()))
                .w(sc(size))
                .flex_none()
                .flex()
                .flex_col()
                .gap(sc(5.))
                .text_center()
                .cursor_pointer()
                .opacity(0.9 + 0.1 * h)
                .child(
                    div().relative().size(sc(size)).child(cover(pic, size, round, cx)).child(
                        div()
                            .absolute()
                            .top(sc(4.))
                            .left(sc(4.))
                            .size(sc(20.))
                            .rounded(sc(10.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(theme::frame())
                            .text_size(sc(10.))
                            .font_weight(FontWeight::BOLD)
                            .child((i + 1).to_string()),
                    ),
                )
                .child(div().one_line().text_size(sc(theme::TYPE_LABEL)).font_weight(FontWeight::SEMIBOLD).child(name))
                .child(div().one_line().text_size(sc(11.)).text_color(theme::text_dim()).child(sub))
                .on_click(cx.listener(move |this, _, _, cx| this.navigate(page.clone(), cx)));
            interact::track(el, key, cx)
        }))
        .into_any_element()
}

pub fn render(view: &mut AppView, part: Part, window: &Window, cx: &mut Context<AppView>) -> AnyElement {
    let st = view.replay_stats();
    let col_w = view.column_width(window);
    match part {
        Part::Header => {
            let period = view.replay_period;
            let centered = col_w < 780.;
            let range = if st.start_day == st.today {
                full_date(st.today)
            } else {
                format!("{} – {}", full_date(st.start_day), full_date(st.today))
            };
            let coverage = match st.first_day {
                Some(f) if f > st.start_day => crate::i18n::text("replay.history_coverage", &[full_date(f).to_string()]),
                Some(f) => crate::i18n::text("replay.history_since", &[full_date(f).to_string()]),
                None => String::new(),
            };
            let options: Vec<AnyElement> = Period::ALL
                .iter()
                .map(|p| {
                    let on = *p == period;
                    let key = format!("rp-{p:?}");
                    let h = interact::hover(&key);
                    let a = interact::follow(
                        &format!("rpsel-{p:?}"),
                        if on { 1.0 } else { 0.0 },
                        super::motion::seconds(super::motion::CONTROL),
                    );
                    let p2 = *p;
                    let el = div()
                        .id(SharedString::from(key.clone()))
                        .h(sc(28.))
                        .px(sc(if col_w < 440. { 8. } else { 12. }))
                        .flex()
                        .items_center()
                        .rounded(sc(6.))
                        .cursor_pointer()
                        .whitespace_nowrap()
                        .text_size(sc(12.5))
                        .font_weight(if on { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                        .text_color(if on { theme::text() } else { theme::text_secondary() })
                        .border_1()
                        .border_color(super::fx::with_alpha(theme::divider(), a))
                        .bg(theme::ink(0.04 * h + 0.055 * a))
                        .child(super::glass::sheen(SharedString::from(format!("glass-{key}")), 6., 0.95))
                        .child(p.label())
                        .on_click(cx.listener(move |this, _, _, cx| this.set_replay_period(p2, cx)));
                    interact::track(el, key, cx).into_any_element()
                })
                .collect();
            div()
                .relative()
                .pt(sc(22.))
                .pb(sc(16.))
                .flex()
                .flex_col()
                .gap(sc(14.))
                .child(
                    gpui::canvas(
                        {
                            let e = cx.entity();
                            move |b, _, cx| e.update(cx, |v, _| v.replay_head_bounds = b)
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .when(centered, |d| d.flex_col().justify_center())
                        .when(!centered, |d| d.justify_between())
                        .gap(sc(12.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .when(centered, |d| d.items_center().text_center())
                                .gap(sc(3.))
                                .child(
                                    div()
                                        .text_size(sc(theme::TYPE_LABEL))
                                        .text_color(theme::text_secondary())
                                        .child(crate::i18n::t("replay")),
                                )
                                .child(div().text_size(sc(32.)).font_weight(FontWeight::BOLD).child(if period == Period::ThisYear {
                                    format!("{} · {}", period.label(), civil_from_days(st.today).0)
                                } else {
                                    period.label().into()
                                })),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .when(centered, |d| d.justify_center())
                                .gap(sc(8.))
                                .child(pill(
                                    "rp-play",
                                    crate::i18n::t("play_top_tracks"),
                                    Some(Icon::Play),
                                    true,
                                    |v, _, _, cx| v.replay_play_top(50, cx),
                                    cx,
                                ))
                                .child(pill(
                                    "rp-pl",
                                    crate::i18n::t("create_playlist"),
                                    Some(Icon::Plus),
                                    false,
                                    |v, _, w, cx| v.replay_make_playlist(w, cx),
                                    cx,
                                ))
                                .child(pill(
                                    "rp-img",
                                    crate::i18n::t("share_image"),
                                    Some(Icon::Upload),
                                    false,
                                    |v, _, w, cx| v.replay_share_image(w, cx),
                                    cx,
                                )),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .when(centered, |d| d.justify_center())
                        .gap(sc(10.))
                        .child(
                            div()
                                .max_w_full()
                                .p(sc(3.))
                                .flex()
                                .flex_wrap()
                                .when(centered, |d| d.justify_center())
                                .gap(sc(2.))
                                .rounded(sc(9.))
                                .border_1()
                                .border_color(theme::divider())
                                .children(options),
                        )
                        .child(div().text_size(sc(theme::TYPE_LABEL)).text_color(theme::text_secondary()).child(range)),
                )
                .when(!coverage.is_empty(), |d| {
                    d.child(
                        div()
                            .when(centered, |d| d.text_center())
                            .text_size(sc(theme::TYPE_LABEL))
                            .text_color(theme::text_secondary())
                            .child(coverage),
                    )
                })
                .into_any_element()
        }
        Part::Dashboard => {
            let wide = col_w >= 760.;
            let section_w = if wide { (col_w - PANEL_GAP) / 2. } else { col_w };
            let inset = super::widgets::panel_pad(section_w);
            let body_w = (section_w - 2. * inset - 2.).max(1.);
            let compact = section_w < 410.;
            let stat_w = ((body_w - PANEL_GAP) / 2.).max(1.);
            let days = (st.today - st.start_day + 1).max(1);
            let stats = widget_panel("replay-summary")
                .p(sc(inset))
                .min_w(sc(0.))
                .flex()
                .flex_col()
                .gap(sc(PANEL_GAP))
                .child(div().flex().child(super::widgets::panel_heading(Icon::Stats, crate::i18n::t("overview"), None)))
                .child(
                    pair(
                        stat(crate::i18n::t("plays"), st.plays.to_string(), per_day(st.plays as u64, days), None, stat_w),
                        stat(
                            crate::i18n::t("listening_time"),
                            fmt_listen(st.total_secs),
                            crate::i18n::text("minutes", &[format!("{}", st.total_secs / 60)]),
                            None,
                            stat_w,
                        ),
                        true,
                        body_w,
                    )
                    .flex_1(),
                )
                .child(
                    pair(
                        stat(
                            crate::i18n::t("tracks"),
                            st.distinct_tracks.to_string(),
                            crate::i18n::text("artists.count", &[format!("{}", st.distinct_artists)]),
                            None,
                            stat_w,
                        ),
                        stat(
                            crate::i18n::t("listening_days"),
                            st.active_days.to_string(),
                            crate::i18n::text("of_days", &[format!("{}", days)]),
                            Some(st.active_days as f32 / days as f32),
                            stat_w,
                        ),
                        true,
                        body_w,
                    )
                    .flex_1(),
                );

            let most = match st.top_tracks.first() {
                None => widget_panel("replay-empty-glass").p(sc(inset)).text_center().child(empty(crate::i18n::t("replay.period_empty"))),
                Some(track) => {
                    let vertical = !wide && col_w < 420.;
                    let size = (body_w * if vertical { 0.55 } else { 0.42 }).clamp(76., 220.).min(body_w);
                    widget_panel("replay-featured-glass")
                        .p(sc(inset))
                        .flex()
                        .items_center()
                        // Artwork starts at the panel's padding, like every other panel.
                        .when(vertical, |d| d.flex_col().justify_center())
                        .gap(sc(20.))
                        .child(cover(track.cover.clone(), size, false, cx))
                        .child(
                            div()
                                .min_w(sc(0.))
                                .flex_1()
                                .flex()
                                .flex_col()
                                .items_start()
                                .gap(sc(6.))
                                .when(vertical, |d| d.items_center().text_center())
                                .child(
                                    div()
                                        .text_size(sc(theme::TYPE_LABEL))
                                        .text_color(theme::text_secondary())
                                        .child(crate::i18n::t("your_top_track")),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .line_clamp(2)
                                        .text_size(sc(if compact { 20. } else { 26. }))
                                        .font_weight(FontWeight::BOLD)
                                        .child(track.name.clone()),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .one_line()
                                        .text_size(sc(theme::TYPE_LABEL))
                                        .text_color(theme::text_secondary())
                                        .child(track.sub.clone()),
                                )
                                .child(div().text_size(sc(theme::TYPE_LABEL)).font_weight(FontWeight::SEMIBOLD).child(format!(
                                    "{} · {}",
                                    times(track.plays),
                                    fmt_listen(track.secs)
                                )))
                                .child(div().mt(sc(4.)).child(pill(
                                    "rp-play-most",
                                    crate::i18n::t("play"),
                                    Some(Icon::Play),
                                    true,
                                    |v, _, _, cx| v.replay_play_top(1, cx),
                                    cx,
                                ))),
                        )
                }
            };

            let visible = if view.replay_all_tracks { st.top_tracks.len() } else { 8 };
            let max_plays = st.top_tracks.first().map_or(1, |t| t.plays) as f32;
            let repeat = section(
                crate::i18n::t("also_on_repeat"),
                Some(crate::i18n::t("plays").into()),
                inset,
                if st.top_tracks.len() < 2 {
                    empty(crate::i18n::t("no_more_songs_with_plays_yet"))
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .children(st.top_tracks.iter().take(visible).enumerate().skip(1).map(|(i, t)| {
                            div()
                                .h(sc(50.))
                                .flex()
                                .items_center()
                                .gap(sc(8.))
                                .when(i > 1, |d| d.border_t_1().border_color(theme::divider()))
                                .child(
                                    div()
                                        .w(sc(18.))
                                        .flex_none()
                                        .text_size(sc(11.))
                                        .text_color(theme::text_dim())
                                        .child((i + 1).to_string()),
                                )
                                .child(cover(t.cover.clone(), 32., false, cx))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(sc(0.))
                                        .flex()
                                        .flex_col()
                                        .child(div().one_line().text_size(sc(12.5)).font_weight(FontWeight::SEMIBOLD).child(t.name.clone()))
                                        .child(
                                            div().one_line().text_size(sc(11.)).text_color(theme::text_secondary()).child(t.sub.clone()),
                                        ),
                                )
                                .when(body_w >= 290., |d| {
                                    d.child(div().w(relative(0.19)).max_w(sc(112.)).flex_none().child(meter(t.plays as f32 / max_plays)))
                                })
                                .child(
                                    div()
                                        .w(sc(38.))
                                        .flex_none()
                                        .text_right()
                                        .text_size(sc(11.))
                                        .text_color(theme::text_secondary())
                                        .child(t.plays.to_string()),
                                )
                        }))
                        .when(st.top_tracks.len() > 8, |d| {
                            let expanded = view.replay_all_tracks;
                            d.child(
                                div()
                                    .id("rp-more-tracks")
                                    .mt(sc(8.))
                                    .px(sc(6.))
                                    .h(sc(32.))
                                    .flex()
                                    .items_center()
                                    .gap(sc(6.))
                                    .rounded(sc(6.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme::hover()))
                                    .child((if expanded { Icon::ChevUp } else { Icon::ChevDown }).el(14., theme::text()))
                                    .child(div().text_size(sc(theme::TYPE_LABEL)).child(if expanded {
                                        crate::i18n::t("show_less").into()
                                    } else {
                                        crate::i18n::text("show_all_songs", &[format!("{}", st.top_tracks.len())])
                                    }))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.replay_all_tracks = !this.replay_all_tracks;
                                        this.content_changed_pub();
                                        cx.notify();
                                    })),
                            )
                        })
                        .into_any_element()
                },
            );

            let peak = st
                .timeline
                .iter()
                .max_by_key(|(_, plays)| *plays)
                .filter(|(_, plays)| *plays > 0)
                .map(|(label, plays)| crate::i18n::text("peak", &[label.to_string(), format!("{}", plays)]));
            let over_time = section(
                crate::i18n::t("your_listening_over_time"),
                peak.or_else(|| Some(crate::i18n::text("per", &[st.unit.to_string()]))),
                inset,
                if st.plays == 0 { empty(crate::i18n::t("replay.period_empty")) } else { bars(&st.timeline, body_w, cx) },
            )
            .when(wide, |d| d.justify_center());

            let artist_ids: Vec<u64> = st.top_artists.iter().map(|r| r.id).collect();
            view.replay_artist_pics(&artist_ids, cx);
            let artists = section(
                crate::i18n::t("top_artists"),
                None,
                inset,
                carousel(
                    "rp-artists",
                    st.top_artists
                        .iter()
                        .map(|r| {
                            let pic = view.artist_pics.get(&r.id).and_then(|l| l.ready().cloned()).flatten();
                            (pic, r.name.clone(), times(r.plays), if r.id != 0 { Page::Artist(r.id) } else { Page::Replay })
                        })
                        .collect(),
                    true,
                    body_w,
                    cx,
                ),
            );
            let albums = section(
                crate::i18n::t("top_albums"),
                None,
                inset,
                carousel(
                    "rp-albums",
                    st.top_albums.iter().map(|r| (r.cover.clone(), r.name.clone(), times(r.plays), Page::Album(r.id))).collect(),
                    false,
                    body_w,
                    cx,
                ),
            );

            let body = div()
                .flex()
                .flex_col()
                .gap(sc(PANEL_GAP))
                .child(pair(most, stats, wide, col_w))
                .child(pair(repeat, over_time, wide, col_w))
                .child(pair(artists, albums, wide, col_w));
            let note = div()
                .pt(sc(14.))
                .text_size(sc(11.))
                .text_color(theme::text_secondary())
                .child(crate::i18n::t("replay.counting_explanation"));
            div()
                .pb(sc(30.))
                // Where the dashboard is on screen (Share Image saves this area).
                .child(
                    gpui::canvas(
                        {
                            let e = cx.entity();
                            move |b, _, cx| e.update(cx, |v, _| v.replay_bounds = b)
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .relative()
                .child(body)
                .child(note)
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(ts: u64, secs: u32, id: u64) -> Entry {
        Entry {
            ts,
            secs,
            id,
            title: format!("t{id}"),
            artist: "A".into(),
            artist_id: 1,
            album: crate::i18n::t("to").into(),
            album_id: 9,
            cover: None,
            dur: 200,
        }
    }

    #[test]
    fn periods_and_buckets() {
        // 2026-11-08 12:00 UTC.
        let now = days_from_civil(2026, 11, 8) as u64 * 86400 + 12 * 3600;
        let day = 86400;
        let h = vec![e(now - 40 * day, 60, 1), e(now - 2 * day, 60, 2), e(now - 2 * day, 10, 2), e(now - 3600, 120, 1)];
        let y = compute_at(&h, Period::ThisYear, now, 0);
        assert_eq!((y.plays, y.unit, y.timeline.len()), (3, crate::i18n::t("month"), 11));
        let m = compute_at(&h, Period::ThisMonth, now, 0);
        assert_eq!((m.plays, m.unit, m.timeline.len()), (2, crate::i18n::t("day"), 8));
        let l = compute_at(&h, Period::Last30, now, 0);
        assert_eq!((l.plays, l.timeline.len(), l.active_days), (2, 30, 2));
        // The 10 s listen is time but not a play.
        assert_eq!(l.total_secs, 190);
        assert_eq!(l.top_tracks[0].id, 1);
    }
}
