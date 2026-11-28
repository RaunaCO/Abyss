//! Replay: the dashboard of listening statistics and its actions (play the
//! top tracks, make a playlist of them, save an image of the dashboard).

use std::rc::Rc;

use iced::widget::{column, container, mouse_area, row, scrollable, text, Space};
use iced::{alignment, Alignment, Border, Color, Element, Length, Rectangle, Task};

use super::icons::Icon;
use super::widgets::{self, cover, line, pill};
use super::{blocking, theme, App, Callback, Dialog, Message};
use crate::history::{civil_from_days, unix_now};
use crate::shared::replay::{compute, fmt_listen, full_date, times, Period, Ranked, Stats};
use crate::shared::{Load, Page};
use crate::tidal::{RowKind, Track};

/// Songs listed before "Show all".
const COLLAPSED: usize = 8;

/// What the statistics were computed from: (entries, seconds of the last one, period, minute).
pub type CacheKey = (usize, u64, Period, u64);

#[derive(Debug, Clone)]
pub enum ReplayMsg {
    Period(Period),
    PlayTop(usize),
    MakePlaylist,
    ToggleAll,
    ShareImage,
    Captured(iced::window::Screenshot),
}

impl App {
    /// Statistics for the period, recomputed only when the history or the
    /// period changed (or once a minute, for the passing of days).
    pub(super) fn replay_stats(&self) -> Rc<Stats> {
        let last = self.history.last().map_or(0, |e| e.secs as u64);
        let key = (self.history.len(), last, self.replay_period, unix_now() / 60);
        let mut cache = self.replay_cache.borrow_mut();
        if let Some((cached, stats)) = cache.as_ref() {
            if *cached == key {
                return stats.clone();
            }
        }
        let stats = Rc::new(compute(&self.history, self.replay_period));
        *cache = Some((key, stats.clone()));
        stats
    }

    pub(super) fn replay_update(&mut self, msg: ReplayMsg) -> Task<Message> {
        match msg {
            ReplayMsg::Period(period) => {
                if self.replay_period != period {
                    self.replay_period = period;
                    self.replay_all_tracks = false;
                    return self.load_page_data();
                }
            }
            ReplayMsg::PlayTop(limit) => {
                let ids: Vec<u64> = self.replay_stats().top_tracks.iter().take(limit).map(|r| r.id).collect();
                if ids.is_empty() {
                    self.notify_toast(crate::i18n::t("no_plays_in_this_period"));
                    return Task::none();
                }
                self.notify_toast(crate::i18n::t("loading_your_top_songs"));
                return self.fetch(
                    move |t| Ok(ids.iter().filter_map(|id| t.track(*id).ok()).collect::<Vec<Track>>()),
                    |app, r: anyhow::Result<Vec<Track>>| match r {
                        Ok(tracks) if !tracks.is_empty() => {
                            app.shuffle = false;
                            app.play_tracks(tracks, 0)
                        }
                        Ok(_) => {
                            app.notify_toast(crate::i18n::t("could_not_load_tracks"));
                            Task::none()
                        }
                        Err(e) => {
                            app.notify_toast(crate::i18n::text("tracks.load_failed", &[format!("{e:#}")]));
                            Task::none()
                        }
                    },
                );
            }
            ReplayMsg::MakePlaylist => {
                let stats = self.replay_stats();
                let ids: Vec<u64> = stats.top_tracks.iter().map(|r| r.id).collect();
                if ids.is_empty() {
                    self.notify_toast(crate::i18n::t("no_plays_in_this_period"));
                    return Task::none();
                }
                let today = full_date(stats.today);
                let name = crate::i18n::text("replay.title_period", &[self.replay_period.label().to_string(), today.clone()]);
                let about = crate::i18n::text("replay.playlist_description", &[ids.len().to_string(), full_date(stats.start_day), today]);
                let task = self.open_dialog(Dialog::NewPlaylist { add: ids });
                self.dlg_title = name;
                self.dlg_desc = about;
                return task;
            }
            ReplayMsg::ToggleAll => self.replay_all_tracks = !self.replay_all_tracks,
            ReplayMsg::ShareImage => {
                let Some(id) = self.window else {
                    self.notify_toast(crate::i18n::t("could_not_capture_image"));
                    return Task::none();
                };
                return iced::window::screenshot(id).map(|shot| Message::Replay(ReplayMsg::Captured(shot)));
            }
            ReplayMsg::Captured(shot) => return self.save_replay_image(shot),
        }
        Task::none()
    }

    /// Save the dashboard as it is on screen to a PNG in Pictures\Abyss.
    fn save_replay_image(&mut self, shot: iced::window::Screenshot) -> Task<Message> {
        let area = super::pages::page_area(self);
        let Some(region) = pixel_region(area, self.viewport.width, (shot.size.width, shot.size.height)) else {
            self.notify_toast(crate::i18n::t("could_not_capture_image"));
            return Task::none();
        };
        let (y, m, d) = civil_from_days(self.replay_stats().today);
        let secs = unix_now() % 86400;
        let file = format!("Replay-{y}-{m:02}-{d:02}-{:02}{:02}{:02}.png", secs / 3600, secs / 60 % 60, secs % 60);
        let save = blocking(move || -> anyhow::Result<(std::path::PathBuf, String)> {
            let cropped = shot.crop(region).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            let image = image::RgbaImage::from_raw(cropped.size.width, cropped.size.height, cropped.rgba.to_vec())
                .ok_or_else(|| anyhow::anyhow!("incomplete screenshot"))?;
            let home = dirs::picture_dir().ok_or_else(|| anyhow::anyhow!("no Pictures folder"))?;
            let dir = home.join("Abyss");
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(&file);
            image.save(&path)?;
            Ok((path, file))
        });
        Task::perform(save, |result| {
            Message::Done(Callback::new(move |app: &mut App| match result {
                Ok((path, file)) => {
                    app.notify_toast(crate::i18n::text("replay.image_saved", &[file]));
                    iced::clipboard::write(path.display().to_string())
                }
                Err(e) => {
                    app.notify_toast(crate::i18n::text("could_not_save_image", &[format!("{e:#}")]));
                    Task::none()
                }
            }))
        })
    }
}

/// `area` (design px in a window `window_w` wide) as a pixel region of an image.
fn pixel_region(area: Rectangle, window_w: f32, (w, h): (u32, u32)) -> Option<Rectangle<u32>> {
    if window_w <= 0.0 {
        return None;
    }
    let k = w as f32 / window_w;
    let x = ((area.x * k).max(0.0) as u32).min(w);
    let y = ((area.y * k).max(0.0) as u32).min(h);
    let width = ((area.width * k).max(0.0) as u32).min(w - x);
    let height = ((area.height * k).max(0.0) as u32).min(h - y);
    (width > 0 && height > 0).then_some(Rectangle { x, y, width, height })
}

fn per_day(plays: u64, days: i64) -> String {
    let rate = plays as f64 / days.max(1) as f64;
    if plays > 0 && rate < 0.1 {
        crate::i18n::t("less_than_0_1_per_day").into()
    } else {
        crate::i18n::text("per_day", &[format!("{rate:.1}")])
    }
}

/// GPUI's `PANEL_GAP`, between the dashboard's panels.
const GAP: f32 = 12.0;

/// A filled fraction of a thin track (GPUI's `meter`: 4 px, the text colour on a divider).
fn meter<'a>(fraction: f32) -> Element<'a, Message> {
    let part = (fraction.clamp(0.0, 1.0) * 1000.0).round() as u16;
    let fill = container(Space::new().width(Length::Fill).height(4)).width(Length::FillPortion(part.max(1))).style(|_| container::Style {
        background: Some(theme::text().into()),
        border: Border { radius: 2.0.into(), ..Default::default() },
        ..Default::default()
    });
    let mut r = row![];
    if part > 0 {
        r = r.push(fill);
    }
    if part < 1000 {
        r = r.push(Space::new().width(Length::FillPortion(1000 - part)).height(4));
    }
    container(r)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(theme::divider().into()),
            border: Border { radius: 2.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}

/// A figure in its own sub-card (GPUI's `stat`): label, a 32 px value
/// (22 when narrow), a detail and room for a meter, so cards side by side line up.
fn stat<'a>(label: &'a str, value: String, detail: String, fraction: Option<f32>, width: f32) -> Element<'a, Message> {
    let compact = width < 198.0;
    let inset = if compact { 14.0 } else { 20.0 };
    let meter_w = (width - 2.0 * inset - 2.0).clamp(1.0, 150.0);
    let bar: Element<'a, Message> = match fraction {
        Some(f) => container(meter(f)).width(meter_w).into(),
        None => Space::new().width(meter_w).height(4).into(),
    };
    let body = column![
        // A label as the others ("plays" is also the lower-case note of a list).
        text(sentence_case(label)).size(theme::TYPE_LABEL).color(theme::text_secondary()),
        text(value).size(if compact { 22 } else { 32 }).font(widgets::bold()).color(theme::text()),
        text(detail).size(11).color(theme::text_secondary()),
        bar,
    ]
    .spacing(5);
    // Content height: a Fill height inside the page's scroll squeezed the
    // card and clipped the value and detail.
    container(body)
        .padding(inset)
        .width(Length::Fill)
        .align_y(alignment::Vertical::Center)
        .style(|_| container::Style {
            background: Some(theme::ink(0.04).into()),
            border: Border { radius: 11.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}

/// `s` with its first letter in upper case.
fn sentence_case(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The surface of the dashboard's panels (GPUI's `flat_panel`, 18 px
/// corners): a pane of liquid glass over the scene (see `liquid_glass`).
fn flat<'a>(content: impl Into<Element<'a, Message>>, inset: f32) -> Element<'a, Message> {
    super::liquid_glass::glass(container(content).padding(inset).width(Length::Fill), super::liquid_glass::PANEL_R).into()
}

/// [`flat`] as tall as the row it sits in, its content centred.
fn flat_fill<'a>(content: impl Into<Element<'a, Message>>, inset: f32) -> Element<'a, Message> {
    let body = container(content).padding(inset).width(Length::Fill).height(Length::Fill).align_y(alignment::Vertical::Center);
    super::liquid_glass::glass(body, super::liquid_glass::PANEL_R).into()
}

/// The top track beside the overview, both as tall as the taller one: the
/// left one has a fixed width and fills the height, so Iced's row lays it out
/// last, with the overview's height (see `iced_core::layout::flex`).
fn top_pair<'a>(most: Element<'a, Message>, overview: Element<'a, Message>, section_w: f32, wide: bool) -> Element<'a, Message> {
    if wide {
        row![container(most).width(section_w.floor()).height(Length::Fill), container(overview).width(Length::Fill)].spacing(GAP).into()
    } else {
        column![most, overview].spacing(GAP).into()
    }
}

/// A titled part of the dashboard (GPUI's `section`): a rule on top, a 17 px
/// title with a note at the other end, 14 px above the body.
fn section<'a>(title: &'a str, note: Option<String>, inset: f32, body: Element<'a, Message>) -> Element<'a, Message> {
    let head = row![text(title).size(17).font(widgets::bold()).color(theme::text()), Space::new().width(Length::Fill)]
        .push(note.map(|n| text(n).size(11).color(theme::text_secondary())))
        .spacing(8)
        .align_y(Alignment::Center);
    let rule = container(Space::new().width(Length::Fill).height(1))
        .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() });
    column![rule, container(column![head, body].spacing(14)).padding(inset).width(Length::Fill)].width(Length::Fill).into()
}

fn empty<'a>(note: &'a str) -> Element<'a, Message> {
    container(text(note).size(theme::TYPE_LABEL).color(theme::text_dim())).padding([8, 0]).into()
}

/// Two widgets side by side in equal halves 12 px apart, or one over the other.
fn pair<'a>(left: Element<'a, Message>, right: Element<'a, Message>, wide: bool) -> Element<'a, Message> {
    if wide {
        row![container(left).width(Length::FillPortion(1)), container(right).width(Length::FillPortion(1))].spacing(GAP).into()
    } else {
        column![left, right].spacing(GAP).into()
    }
}

pub fn view(app: &App, width: f32) -> Element<'_, Message> {
    let mut page = column![];
    if app.history.is_empty() {
        // GPUI: the page's title block, then the explanation.
        let head = column![
            text(crate::i18n::t("replay")).size(30).font(widgets::bold()).color(theme::text()),
            text(crate::i18n::t("your_listening_statistics")).size(theme::TYPE_LABEL).color(theme::text_secondary()),
        ]
        .spacing(5)
        .padding(iced::Padding { top: 4.0, bottom: 14.0, ..Default::default() });
        page = page.push(head).push(widgets::notice(Icon::Stats, crate::i18n::t("replay.empty_explanation")));
    } else {
        let stats = app.replay_stats();
        page = page.push(header(app, &stats, width)).push(dashboard(app, &stats, width));
    }
    // TIDAL's own monthly summaries, if its Home feed has them (no extra query).
    if let Load::Ready(rows) = &app.home {
        let summaries = rows.iter().filter(|r| r.kind == RowKind::Cards).filter(|r| {
            let title = r.title.to_lowercase();
            ["historial", "history", "most listened"].iter().any(|word| title.contains(word))
        });
        for summary in summaries {
            let tag =
                super::pages::source_tag(crate::i18n::t("tidal_summaries").into(), crate::i18n::t("replay.tidal_monthly_playlists").into());
            page = page.push(tag).push(super::pages::card_row(summary.title.clone(), &summary.items));
        }
    }
    page.into()
}

/// GPUI's `Part::Header`: "Replay" over the period's name with the actions
/// beside it (centred when narrow), the period switch with the dates, and how
/// far back the history goes.
fn header<'a>(app: &'a App, stats: &Stats, width: f32) -> Element<'a, Message> {
    let period = app.replay_period;
    let centered = width < 780.0;
    let name = match period {
        Period::ThisYear => format!("{} · {}", period.label(), civil_from_days(stats.today).0),
        _ => period.label().to_string(),
    };
    let align = if centered { Alignment::Center } else { Alignment::Start };
    let names = column![
        text(crate::i18n::t("replay")).size(theme::TYPE_LABEL).color(theme::text_secondary()),
        text(name).size(32).font(widgets::bold()).color(theme::text()),
    ]
    .spacing(3)
    .align_x(align);
    let action =
        |label: &'static str, icon: Icon, primary: bool, msg: ReplayMsg| pill(label, Some(icon), primary, Some(Message::Replay(msg)));
    let actions = row![
        action(crate::i18n::t("play_top_tracks"), Icon::Play, true, ReplayMsg::PlayTop(50)),
        action(crate::i18n::t("create_playlist"), Icon::Plus, false, ReplayMsg::MakePlaylist),
        action(crate::i18n::t("share_image"), Icon::Upload, false, ReplayMsg::ShareImage),
    ]
    .spacing(8)
    .wrap()
    .vertical_spacing(8);
    let top: Element<'a, Message> = if centered {
        column![names, actions].spacing(12).align_x(Alignment::Center).width(Length::Fill).into()
    } else {
        row![container(names).width(Length::Fill), actions].spacing(12).align_y(Alignment::Center).into()
    };
    let medium = super::font(iced::font::Weight::Medium);
    let options = Period::ALL.into_iter().map(|p| {
        let on = p == period;
        let label = text(p.label()).size(13).font(if on { widgets::semibold() } else { medium }).color(if on {
            theme::text()
        } else {
            theme::text_secondary()
        });
        let b = iced::widget::button(container(label).center_y(Length::Fill))
            .height(28)
            .padding([0, if width < 440.0 { 8 } else { 12 }])
            .style(|_, _| iced::widget::button::Style { text_color: theme::text(), ..Default::default() })
            .on_press(Message::Replay(ReplayMsg::Period(p)));
        // The hover and the selection glide in as GPUI's `interact` (see `wash`).
        super::wash::surface(b, |ph| super::wash::Look {
            background: Some(theme::ink(0.04 * ph.hover + 0.055 * ph.active).into()),
            border: Border {
                color: super::wash::mix(iced::Color::TRANSPARENT, theme::divider(), ph.active),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .active(on)
        .into()
    });
    let switch = container(row(options.collect::<Vec<Element<'a, Message>>>()).spacing(2).wrap())
        .padding(3)
        .style(|_| container::Style { border: Border { color: theme::divider(), width: 1.0, radius: 9.0.into() }, ..Default::default() });
    let range = if stats.start_day == stats.today {
        full_date(stats.today)
    } else {
        format!("{} – {}", full_date(stats.start_day), full_date(stats.today))
    };
    let periods =
        row![switch, text(range).size(theme::TYPE_LABEL).color(theme::text_secondary())].spacing(10).align_y(Alignment::Center).wrap();
    let periods: Element<'a, Message> = if centered { container(periods).center_x(Length::Fill).into() } else { periods.into() };
    let coverage = stats.first_day.map(|first| {
        let key = if first > stats.start_day { "replay.history_coverage" } else { "replay.history_since" };
        let t = text(crate::i18n::text(key, &[full_date(first)])).size(theme::TYPE_LABEL).color(theme::text_secondary());
        container(t).width(Length::Fill).align_x(if centered { alignment::Horizontal::Center } else { alignment::Horizontal::Left })
    });
    // With the column's own 18 px, 22 px above; 16 below.
    column![top, periods].push(coverage).spacing(14).padding(iced::Padding { top: 4.0, bottom: 16.0, ..Default::default() }).into()
}

/// GPUI's `Part::Dashboard`: the top track beside the overview, the repeats
/// beside the chart, the artists beside the albums.
fn dashboard<'a>(app: &'a App, stats: &Stats, width: f32) -> Element<'a, Message> {
    let wide = width >= 760.0;
    let section_w = if wide { (width - GAP) / 2.0 } else { width };
    let inset = if section_w < 520.0 { 14.0 } else { 20.0 };
    let body_w = (section_w - 2.0 * inset - 2.0).max(1.0);
    let compact = section_w < 410.0;
    let stat_w = ((body_w - GAP) / 2.0).max(1.0);
    let days = (stats.today - stats.start_day + 1).max(1);
    let overview = column![
        widgets::panel_heading(Icon::Stats, crate::i18n::t("overview"), None),
        pair(
            stat(crate::i18n::t("plays"), stats.plays.to_string(), per_day(stats.plays as u64, days), None, stat_w),
            stat(
                crate::i18n::t("listening_time"),
                fmt_listen(stats.total_secs),
                crate::i18n::text("minutes", &[(stats.total_secs / 60).to_string()]),
                None,
                stat_w,
            ),
            true,
        ),
        pair(
            stat(
                crate::i18n::t("tracks"),
                stats.distinct_tracks.to_string(),
                crate::i18n::text("artists.count", &[stats.distinct_artists.to_string()]),
                None,
                stat_w,
            ),
            stat(
                crate::i18n::t("listening_days"),
                stats.active_days.to_string(),
                crate::i18n::text("of_days", &[days.to_string()]),
                Some(stats.active_days as f32 / days as f32),
                stat_w,
            ),
            true,
        ),
    ]
    .spacing(GAP);
    let overview = flat(overview, inset);

    let most: Element<'a, Message> = match stats.top_tracks.first() {
        None => {
            let note = container(empty(crate::i18n::t("replay.period_empty"))).center_x(Length::Fill);
            if wide {
                flat_fill(note, inset)
            } else {
                flat(note, inset)
            }
        }
        Some(top) => {
            let vertical = !wide && width < 420.0;
            let size = (body_w * if vertical { 0.55 } else { 0.42 }).clamp(76.0, 220.0).min(body_w);
            let title_size = if compact { 20.0 } else { 26.0 };
            let align = if vertical { Alignment::Center } else { Alignment::Start };
            let about = column![
                text(crate::i18n::t("your_top_track")).size(theme::TYPE_LABEL).color(theme::text_secondary()),
                container(text(top.name.clone()).size(title_size).font(widgets::bold()).color(theme::text()))
                    .max_height(2.0 * 1.3 * title_size)
                    .clip(true),
                line(top.sub.clone(), theme::TYPE_LABEL, theme::text_secondary()),
                text(format!("{} · {}", times(top.plays), fmt_listen(top.secs))).size(theme::TYPE_LABEL).font(widgets::semibold()),
                container(pill(crate::i18n::t("play"), Some(Icon::Play), true, Some(Message::Replay(ReplayMsg::PlayTop(1)))))
                    .padding(iced::Padding { top: 4.0, ..Default::default() }),
            ]
            .spacing(6)
            .align_x(align)
            .width(Length::Fill)
            .clip(true);
            let body: Element<'a, Message> = if vertical {
                column![cover(top.cover.clone(), size, false), about].spacing(20).align_x(Alignment::Center).into()
            } else {
                row![cover(top.cover.clone(), size, false), about].spacing(20).align_y(Alignment::Center).into()
            };
            if wide {
                flat_fill(body, inset)
            } else {
                flat(body, inset)
            }
        }
    };

    let peak = stats
        .timeline
        .iter()
        .max_by_key(|(_, plays)| *plays)
        .filter(|(_, plays)| *plays > 0)
        .map(|(label, plays)| crate::i18n::text("peak", &[label.clone(), plays.to_string()]))
        .unwrap_or_else(|| crate::i18n::text("per", &[stats.unit.to_string()]));
    let chart = if stats.plays == 0 { empty(crate::i18n::t("replay.period_empty")) } else { bars(&stats.timeline, body_w) };

    let artists: Vec<Tile> = stats
        .top_artists
        .iter()
        .map(|r| (app.artist_pics.get(&r.id).and_then(Load::ready).cloned().flatten(), r, (r.id != 0).then_some(Page::Artist(r.id))))
        .collect();
    let albums: Vec<Tile> = stats.top_albums.iter().map(|r| (r.cover.clone(), r, Some(Page::Album(r.id)))).collect();

    let note = container(text(crate::i18n::t("replay.counting_explanation")).size(11).color(theme::text_secondary()))
        .padding(iced::Padding { top: 14.0, ..Default::default() });
    column![
        column![
            top_pair(most, overview, section_w, wide),
            pair(
                section(crate::i18n::t("also_on_repeat"), Some(crate::i18n::t("plays").to_string()), inset, repeats(app, stats, body_w)),
                section(crate::i18n::t("your_listening_over_time"), Some(peak), inset, chart),
                wide,
            ),
            pair(
                section(crate::i18n::t("top_artists"), None, inset, carousel(artists, true, body_w)),
                section(crate::i18n::t("top_albums"), None, inset, carousel(albums, false, body_w)),
                wide,
            ),
        ]
        .spacing(GAP),
        note,
    ]
    .padding(iced::Padding { bottom: 30.0, ..Default::default() })
    .into()
}

/// The songs after the first (GPUI: 50 px rows with a rule between them,
/// rank, cover, names, a meter where there is room and the plays).
fn repeats<'a>(app: &App, stats: &Stats, width: f32) -> Element<'a, Message> {
    if stats.top_tracks.len() < 2 {
        return empty(crate::i18n::t("no_more_songs_with_plays_yet"));
    }
    let shown = if app.replay_all_tracks { stats.top_tracks.len() } else { COLLAPSED };
    let most = stats.top_tracks[0].plays.max(1) as f32;
    let meter_w = (width * 0.19).min(112.0);
    let mut list = column![];
    for (i, track) in stats.top_tracks.iter().enumerate().take(shown).skip(1) {
        let names = column![
            line(track.name.clone(), 13.0, theme::text()).font(widgets::semibold()),
            line(track.sub.clone(), 11.0, theme::text_secondary())
        ];
        let entry = row![
            text((i + 1).to_string()).size(11).color(theme::text_dim()).width(18),
            cover(track.cover.clone(), 32.0, false),
            container(names).width(Length::Fill).clip(true),
        ]
        .push((width >= 290.0).then(|| container(meter(track.plays as f32 / most)).width(meter_w)))
        .push(text(track.plays.to_string()).size(11).color(theme::text_secondary()).width(38).align_x(alignment::Horizontal::Right))
        .spacing(8)
        .height(if i > 1 { 49 } else { 50 })
        .align_y(Alignment::Center);
        if i > 1 {
            let rule = container(Space::new().width(Length::Fill).height(1))
                .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() });
            list = list.push(rule);
        }
        list = list.push(entry);
    }
    if stats.top_tracks.len() > COLLAPSED {
        let label = if app.replay_all_tracks {
            crate::i18n::t("show_less").to_string()
        } else {
            crate::i18n::text("show_all_songs", &[stats.top_tracks.len().to_string()])
        };
        let glyph = if app.replay_all_tracks { Icon::ChevUp } else { Icon::ChevDown };
        let more = iced::widget::button(
            container(
                row![super::icons::icon(glyph, 14.0, theme::text()), text(label).size(theme::TYPE_LABEL).color(theme::text())]
                    .spacing(6)
                    .align_y(Alignment::Center),
            )
            .center_y(32),
        )
        .padding([0, 6])
        .on_press(Message::Replay(ReplayMsg::ToggleAll));
        // The hover fades in like GPUI's (see `widgets::soft`).
        let more = widgets::soft(more, 6.0);
        list = list.push(container(more).padding(iced::Padding { top: 8.0, ..Default::default() }));
    }
    list.into()
}

/// Plays per bucket (GPUI's `bars`): text-coloured bars, the busiest solid,
/// over a rule, with a few labels spread across the plot.
fn bars<'a>(values: &[(String, u64)], width: f32) -> Element<'a, Message> {
    let max = values.iter().map(|(_, v)| *v).max().unwrap_or(0).max(1);
    let n = values.len();
    let labels = n.min((width / 52.0).floor().clamp(2.0, 7.0) as usize);
    let gap = if n > 20 || width < 320.0 { 2.0 } else { 4.0 };
    let height = (width * 0.34).clamp(122.0, 224.0);
    let columns = values.iter().map(|(label, v)| {
        let frac = if *v == 0 { 0.0 } else { (*v as f32 / max as f32).max(0.04) };
        let (empty, peak) = (*v == 0, *v == max);
        let bar = container(Space::new().width(Length::Fill).height((height * frac).max(2.0))).style(move |_| container::Style {
            background: Some(
                if empty {
                    theme::divider()
                } else {
                    let hover = super::wash::phase().hover;
                    theme::with_alpha(theme::text(), if peak { 1.0 } else { 0.66 + 0.34 * hover })
                }
                .into(),
            ),
            border: Border { radius: 3.0.into(), ..Default::default() },
            ..Default::default()
        });
        let tip = format!("{label}: {}", if *v == 0 { crate::i18n::t("no_plays").to_string() } else { times(*v as u32) });
        let bar = column![Space::new().height(Length::Fill), bar].width(Length::Fill).height(height);
        let bar: Element<'a, Message> = iced::widget::tooltip(
            super::wash::track(bar),
            // The shared bubble (it gives the glass the finite viewport its label needs).
            widgets::tip_bubble(tip),
            iced::widget::tooltip::Position::Top,
        )
        .into();
        container(bar).width(Length::FillPortion(1)).into()
    });
    let plot = row(columns.collect::<Vec<Element<'a, Message>>>()).spacing(gap).height(height);
    let rule = container(Space::new().width(Length::Fill).height(1))
        .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() });
    // Each label under its own bar (the first and last flush with the plot's
    // edges), at evenly spaced bars: Jan Mar May… rather than Jan Feb Apr….
    let picked = caption_bars(n, labels);
    let captions = values.iter().enumerate().map(|(j, (label, _))| -> Element<'a, Message> {
        if !picked.contains(&j) {
            return Space::new().width(Length::FillPortion(1)).into();
        }
        let align = if j == 0 {
            alignment::Horizontal::Left
        } else if j + 1 == n {
            alignment::Horizontal::Right
        } else {
            alignment::Horizontal::Center
        };
        let caption = text(label.clone())
            .size(10)
            .color(theme::text_secondary())
            .wrapping(iced::widget::text::Wrapping::None)
            .align_x(align)
            .width(Length::Fill);
        container(caption).width(Length::FillPortion(1)).into()
    });
    column![column![plot, rule], row(captions.collect::<Vec<Element<'a, Message>>>()).spacing(gap)].spacing(6).into()
}

/// Which of `n` bars get a caption, at most `max`: the first, the last and
/// evenly spaced ones between (the most that divide the range evenly).
fn caption_bars(n: usize, max: usize) -> Vec<usize> {
    if n <= 1 || max <= 1 {
        return (0..n.min(1)).collect();
    }
    let max = max.min(n);
    let k = (2..=max).rev().find(|k| (n - 1).is_multiple_of(k - 1)).filter(|k| *k * 2 > max).unwrap_or(max);
    (0..k).map(|i| i * (n - 1) / (k - 1)).collect()
}

/// Picture, entry and the page it opens.
type Tile<'s> = (Option<String>, &'s Ranked, Option<Page>);

/// Covers or portraits with their rank, name and plays (GPUI's `carousel`):
/// up to six that share the width, 136 px at most, centred when they fit.
fn carousel<'a>(items: Vec<Tile<'_>>, round: bool, width: f32) -> Element<'a, Message> {
    if items.is_empty() {
        return empty(crate::i18n::t("replay.period_empty"));
    }
    let gap = 12.0;
    let columns = (((width + gap) / (104.0 + gap)).floor() as usize).clamp(1, 6).min(items.len());
    let size = ((width - gap * (columns - 1) as f32) / columns as f32).floor().clamp(1.0, 136.0);
    let fits = size * items.len() as f32 + gap * (items.len() - 1) as f32 <= width;
    let tiles = items.into_iter().enumerate().map(|(i, (picture, entry, page))| {
        let rank = container(text((i + 1).to_string()).size(10).font(widgets::bold()).color(theme::text())).center(20).style(|_| {
            container::Style {
                background: Some(theme::frame().into()),
                border: Border { radius: 10.0.into(), ..Default::default() },
                ..Default::default()
            }
        });
        let shade = container(Space::new().width(size).height(size)).style(move |_| container::Style {
            background: Some(theme::with_alpha(theme::bg(), 0.1 * (1.0 - super::wash::phase().hover)).into()),
            border: Border { radius: (if round { size / 2.0 } else { 4.0 }).into(), ..Default::default() },
            ..Default::default()
        });
        let art = iced::widget::stack![cover(picture, size, round), shade, container(rank).padding(4)];
        let brighten = |c: Color| theme::with_alpha(c, c.a * (0.9 + 0.1 * super::wash::phase().hover));
        let tile = column![
            art,
            container(line(entry.name.clone(), theme::TYPE_LABEL, theme::text()).font(widgets::semibold()).live(brighten)).center_x(size),
            container(line(times(entry.plays), 11.0, theme::text_dim()).live(brighten)).center_x(size),
        ]
        .spacing(5)
        .width(size)
        .clip(true);
        match page {
            Some(page) => {
                mouse_area(super::wash::track(tile)).on_press(Message::Navigate(page)).interaction(iced::mouse::Interaction::Pointer).into()
            }
            None => tile.into(),
        }
    });
    let strip = row(tiles.collect::<Vec<Element<'a, Message>>>()).spacing(gap);
    if fits {
        return container(strip).center_x(Length::Fill).padding(iced::Padding { bottom: 4.0, ..Default::default() }).into();
    }
    let strip = container(strip).padding(iced::Padding { bottom: 10.0, ..Default::default() });
    scrollable(strip)
        .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(4).scroller_width(4)))
        .style(super::pages::scroll_style)
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::{caption_bars, pixel_region};

    #[test]
    fn chart_captions_sit_under_evenly_spaced_bars() {
        // Eleven months, room for seven: Jan Mar May Jul Sep Nov.
        assert_eq!(caption_bars(11, 7), vec![0, 2, 4, 6, 8, 10]);
        // 31 days: every fifth.
        assert_eq!(caption_bars(31, 7), vec![0, 5, 10, 15, 20, 25, 30]);
        // Nothing divides 23 evenly: as many as fit, spread out.
        assert_eq!(caption_bars(24, 4), vec![0, 7, 15, 23]);
        assert_eq!(caption_bars(1, 7), vec![0]);
    }
    use iced::Rectangle;

    #[test]
    fn the_shared_image_is_the_page_area_in_pixels_and_never_leaves_the_capture() {
        let area = Rectangle { x: 80.0, y: 60.0, width: 900.0, height: 700.0 };
        // 125 %: every design px is 1.25 physical ones.
        let region = pixel_region(area, 1000.0, (1250, 1000)).unwrap();
        assert_eq!((region.x, region.y, region.width, region.height), (100, 75, 1125, 875));
        let clipped = pixel_region(area, 1000.0, (1000, 500)).unwrap();
        assert_eq!((clipped.y + clipped.height, clipped.x + clipped.width), (500, 980));
        assert!(pixel_region(area, 0.0, (1000, 500)).is_none());
    }
}
