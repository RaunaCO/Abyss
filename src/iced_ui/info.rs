//! Song, album and artist information: the artwork and the facts Abyss has.
//! Credits come from TIDAL, the technical data from the stream itself (see
//! `tidal::probe`); anything unknown says so. Playback is never touched.

use std::collections::hash_map::Entry;

use iced::widget::{column, container, row, scrollable, text, Space};
use iced::{Alignment, Element, Length, Task};

use super::icons::Icon;
use super::widgets::{self, cover, fmt_date, fmt_long, fmt_time};
use super::{theme, App, Message};
use crate::shared::{into_load, Load};
use crate::tidal::probe::TechInfo;
use crate::tidal::{Album, Artist, Track};

#[derive(Debug, Clone)]
pub enum InfoTarget {
    Song(Track),
    Album(Album),
    Artist(Artist),
}

impl App {
    pub(super) fn open_info(&mut self, target: InfoTarget) -> Task<Message> {
        self.menu = None;
        let mut tasks = Vec::new();
        match &target {
            InfoTarget::Song(track) if track.local_path.is_some() => {}
            InfoTarget::Song(track) => {
                tasks.push(self.load_credits(track.id));
                if let Some(album) = &track.album {
                    tasks.push(self.ensure_album(album.id));
                }
                let id = track.id;
                if let Entry::Vacant(slot) = self.tech_info.entry(id) {
                    slot.insert(Load::Loading);
                    let quality = self.settings.lock().quality;
                    tasks.push(self.fetch(
                        move |t| {
                            let stream = t.stream(id, quality)?;
                            let mut info = t.probe_stream(&stream)?;
                            if info.codec.is_empty() {
                                info.codec = stream.codec.to_uppercase();
                            }
                            Ok((info, stream.audio_quality))
                        },
                        move |app, r| {
                            app.tech_info.insert(id, into_load(r));
                            Task::none()
                        },
                    ));
                }
            }
            InfoTarget::Album(album) => tasks.push(self.ensure_album(album.id)),
            InfoTarget::Artist(artist) => {
                let id = artist.id;
                if crate::local::is_local_id(id) {
                    if !self.artists.contains_key(&id) {
                        self.local_changed();
                    }
                } else if let Entry::Vacant(slot) = self.artists.entry(id) {
                    slot.insert(Load::Loading);
                    tasks.push(self.fetch(
                        move |t| t.artist(id),
                        move |app, r| {
                            app.artists.insert(id, into_load(r));
                            Task::none()
                        },
                    ));
                }
            }
        }
        self.info_panel = Some(target);
        Task::batch(tasks)
    }

    /// Load an album (details and tracks) into the cache once.
    fn ensure_album(&mut self, id: u64) -> Task<Message> {
        if crate::local::is_local_id(id) {
            if !self.albums.contains_key(&id) {
                self.local_changed();
            }
            return Task::none();
        }
        if let Entry::Vacant(slot) = self.albums.entry(id) {
            slot.insert(Load::Loading);
            return self.fetch(
                move |t| t.album(id),
                move |app, r| {
                    app.albums.insert(id, into_load(r));
                    Task::none()
                },
            );
        }
        Task::none()
    }

    /// Add an album's tracks to a playlist, loading them if needed.
    pub(super) fn add_album_to_playlist(&mut self, id: u64, uuid: String) -> Task<Message> {
        self.menu = None;
        if let Some((_, tracks)) = self.albums.get(&id).and_then(Load::ready) {
            let tracks = tracks.clone();
            return self.add_tracks_to_playlist(uuid, tracks);
        }
        self.fetch(
            move |t| t.album(id),
            move |app, r| match r {
                Ok((album, tracks)) => {
                    app.albums.insert(id, Load::Ready((album, tracks.clone())));
                    app.add_tracks_to_playlist(uuid, tracks)
                }
                Err(e) => {
                    app.notify_toast(crate::i18n::text("could_not_load_the_album", &[format!("{e:#}")]));
                    Task::none()
                }
            },
        )
    }

    /// Queue every track of an album, keeping the album's order.
    pub(super) fn queue_album(&mut self, id: u64, next: bool) -> Task<Message> {
        self.menu = None;
        let mut tracks = self.albums.get(&id).and_then(Load::ready).map(|(_, t)| t.clone()).unwrap_or_default();
        // With nothing playing the first one added starts, so it must be the album's first.
        if next && self.now.is_some() {
            tracks.reverse();
        }
        let tasks: Vec<_> = tracks.into_iter().map(|track| self.add_to_queue(track, next)).collect();
        Task::batch(tasks)
    }
}

fn labelled<'a>(label: &'a str, value: Element<'a, Message>) -> Element<'a, Message> {
    column![text(label.to_uppercase()).size(11).font(widgets::semibold()).color(theme::text_dim()), value]
        .spacing(2)
        .width(Length::Fill)
        .into()
}

/// A fact; None says it is unavailable.
fn fact<'a>(label: &'a str, value: Option<String>) -> Element<'a, Message> {
    let value = value.filter(|v| !v.is_empty());
    let color = if value.is_some() { theme::text() } else { theme::text_dim() };
    let shown = value.unwrap_or_else(|| crate::i18n::t("unavailable").to_string());
    labelled(label, text(shown).size(13).color(color).into())
}

/// A fact that comes from something still loading, loaded, or failed.
fn loaded<'a, T>(label: &'a str, data: Option<&Load<T>>, read: impl FnOnce(&T) -> Option<String>) -> Element<'a, Message> {
    match data {
        Some(Load::Loading) => labelled(label, widgets::loading_dots()),
        other => fact(label, other.and_then(Load::ready).and_then(read)),
    }
}

fn grid<'a>(cells: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut rows = column![].spacing(12);
    let mut cells = cells.into_iter().peekable();
    while cells.peek().is_some() {
        rows = rows.push(row(cells.by_ref().take(3)).spacing(16));
    }
    rows.into()
}

fn section<'a>(title: &'a str) -> Element<'a, Message> {
    column![
        container(Space::new().width(Length::Fill).height(1))
            .style(|_| container::Style { background: Some(theme::divider().into()), ..Default::default() }),
        text(title).size(theme::TYPE_LABEL).font(widgets::bold()).color(theme::text()),
    ]
    // GPUI: 14 px above the rule, 14 under it, 10 under the title.
    .spacing(14)
    .padding(iced::Padding { top: 14.0, bottom: 10.0, ..Default::default() })
    .into()
}

fn song<'a>(app: &'a App, t: &'a Track) -> Element<'a, Message> {
    let local = t.local_path.is_some();
    let featured = (t.artists.len() > 1).then(|| t.artists[1..].iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", "));
    let mut facts = column![
        text(t.display_title()).size(theme::TYPE_SECTION).font(widgets::bold()).color(theme::text()),
        fact(crate::i18n::t("main_artist"), t.artists.first().map(|a| a.name.clone())),
        fact(crate::i18n::t("featured_artist"), Some(featured.unwrap_or_else(|| crate::i18n::t("none").to_string()))),
    ]
    .spacing(12)
    .width(Length::Fill);
    if !local {
        facts = facts.push(loaded(crate::i18n::t("production"), app.credits.get(&t.id), |list| {
            let names: Vec<String> =
                list.iter().filter(|(role, _)| role.to_lowercase().contains("producer")).flat_map(|(_, n)| n.clone()).collect();
            Some(names.join(", "))
        }));
    }
    let album = t.album.as_ref().and_then(|a| app.albums.get(&a.id));
    let numbers = grid(vec![
        loaded(crate::i18n::t("year"), album, |(a, _)| Some(a.year().to_string())),
        fact(crate::i18n::t("disc"), (t.volume_number > 0).then(|| t.volume_number.to_string())),
        fact(crate::i18n::t("track"), (t.track_number > 0).then(|| t.track_number.to_string())),
    ]);
    let technical: Element<'a, Message> = match &t.local_path {
        Some(path) => column![
            fact(crate::i18n::t("duration"), Some(fmt_time(t.duration as f64))),
            text(path.display().to_string()).size(12).color(theme::text_secondary()),
        ]
        .spacing(12)
        .into(),
        None => match app.tech_info.get(&t.id) {
            Some(Load::Ready((info, tier))) => technical_grid(info, tier, t),
            Some(Load::Failed(e)) => {
                let reason = crate::i18n::text("could_not_read_stream_data", std::slice::from_ref(e));
                text(reason).size(theme::TYPE_LABEL).color(theme::text_dim()).into()
            }
            _ => container(widgets::loading_dots()).height(40).center_x(Length::Fill).into(),
        },
    };
    column![
        row![widgets::cover_rounded(t.cover(320), 132.0, 8.0), facts].spacing(18),
        section(crate::i18n::t("track")),
        numbers,
        section(crate::i18n::t("technical")),
        technical,
    ]
    .into()
}

fn technical_grid<'a>(info: &TechInfo, tier: &str, t: &Track) -> Element<'a, Message> {
    let format = match (info.codec.as_str(), info.bits) {
        ("FLAC", Some(bits)) => crate::i18n::text("flac_bit", &[bits.to_string()]),
        (codec, _) => codec.to_string(),
    };
    // Exact length from the samples when the header has them.
    let seconds = match (info.total_samples, info.sample_rate) {
        (Some(n), Some(rate)) if rate > 0 => n as f64 / rate as f64,
        _ => t.duration as f64,
    };
    let segmented = || info.segmented.then(|| crate::i18n::t("unavailable_segmented_dash_stream").to_string());
    let bitrate = match (info.file_size, seconds > 0.0) {
        (Some(size), true) => Some(crate::i18n::text("kbps_file_average", &[format!("{:.0}", size as f64 * 8.0 / seconds / 1000.0)])),
        _ if info.codec == "AAC" && tier == "HIGH" => Some(crate::i18n::t("320_kbps_tidal_nominal").to_string()),
        _ if info.codec == "AAC" && tier == "LOW" => Some(crate::i18n::t("96_kbps_tidal_nominal").to_string()),
        _ => segmented(),
    };
    let channels = info.channels.map(|c| match c {
        1 => crate::i18n::t("1_mono").to_string(),
        2 => crate::i18n::t("2_stereo").to_string(),
        n => n.to_string(),
    });
    let rate = info.sample_rate.map(|r| crate::i18n::text("khz", &[crate::audio::output::fmt_khz(r).to_string()]));
    let size = info.file_size.map(|s| crate::i18n::text("mb", &[format!("{:.1}", s as f64 / 1_048_576.0)])).or_else(segmented);
    grid(vec![
        fact(crate::i18n::t("audio_format"), Some(format)),
        fact(crate::i18n::t("bitrate"), bitrate),
        fact(crate::i18n::t("sample_rate"), rate),
        fact(crate::i18n::t("channels"), channels),
        fact(crate::i18n::t("duration"), Some(fmt_time(seconds))),
        fact(crate::i18n::t("file_size"), size),
    ])
}

fn album<'a>(app: &'a App, shown: &'a Album) -> Element<'a, Message> {
    let full = app.albums.get(&shown.id).and_then(Load::ready);
    let album = full.map_or(shown, |(a, _)| a);
    let count = full.map_or(album.number_of_tracks as usize, |(_, tracks)| tracks.len());
    let quality: Vec<String> = album.tags.iter().map(|t| widgets::quality_name(t)).collect();
    let facts = column![
        text(album.title.clone()).size(theme::TYPE_SECTION).font(widgets::bold()).color(theme::text()),
        fact(crate::i18n::t("artist"), Some(album.artist_names())),
        grid(vec![
            fact(crate::i18n::t("released"), Some(fmt_date(&album.release_date))),
            fact(crate::i18n::t("tracks"), Some(count.to_string())),
            fact(crate::i18n::t("duration"), Some(fmt_long(album.duration as u64))),
        ]),
        fact(crate::i18n::t("quality"), Some(quality.join(", "))),
    ]
    .push((!album.copyright.is_empty()).then(|| fact(crate::i18n::t("copyright"), Some(album.copyright.clone()))))
    .spacing(12)
    .width(Length::Fill);
    row![widgets::cover_rounded(album.cover(320), 132.0, 8.0), facts].spacing(18).into()
}

fn artist<'a>(app: &'a App, shown: &'a Artist) -> Element<'a, Message> {
    let biography = |s: String, dim: bool| -> Element<'a, Message> {
        let color = if dim { theme::text_dim() } else { theme::text_secondary() };
        let body = text(s).size(13).line_height(iced::widget::text::LineHeight::Absolute(19.0.into())).color(color);
        container(scrollable(body).style(super::pages::scroll_style)).max_height(260).into()
    };
    let bio = match app.artists.get(&shown.id) {
        Some(Load::Ready(page)) => match &page.bio {
            Some(bio) => biography(bio.clone(), false),
            None => biography(crate::i18n::t("tidal_has_no_biography_for_this_artist").to_string(), true),
        },
        Some(Load::Failed(_)) => biography(crate::i18n::t("unavailable").to_string(), true),
        _ => widgets::loading_dots(),
    };
    let facts = column![text(shown.name.clone()).size(18).font(widgets::bold()).color(theme::text()), bio].spacing(10).width(Length::Fill);
    row![cover(shown.picture(320), 120.0, true), facts].spacing(18).into()
}

pub fn overlay<'a>(app: &'a App, target: &'a InfoTarget) -> Element<'a, Message> {
    let (icon, title, body) = match target {
        InfoTarget::Song(t) => (Icon::Track, crate::i18n::t("song_info"), song(app, t)),
        InfoTarget::Album(a) => (Icon::Album, crate::i18n::t("album_info"), album(app, a)),
        InfoTarget::Artist(a) => (Icon::Artist, crate::i18n::t("artist_info"), artist(app, a)),
    };
    let head = row![
        widgets::panel_heading(icon, title, None),
        widgets::icon_button(Icon::Close, 14.0, theme::text_secondary(), Message::CloseInfo),
    ]
    .align_y(Alignment::Center);
    // GPUI: the heading, 12 px, a rule, 14 px, then the body.
    let rule = container(Space::new().width(Length::Fill).height(1))
        .style(|_| container::Style { background: Some(super::wash::faded(theme::divider()).into()), ..Default::default() });
    let head = column![head, Space::new().height(12), rule, Space::new().height(14)];
    let panel = container(super::glide::glide(scrollable(column![head, body]).style(super::pages::scroll_style)))
        .width(560)
        .max_height(app.viewport.height * 0.9)
        .padding(20);
    // A pane of glass, as the menus and the dialogs.
    let panel = super::liquid_glass::glass(panel, 16.0).optics(super::liquid_glass::Optics::POPOVER).shadow(theme::popover_shadow());
    // GPUI: the info panel fades in over `FAST` (`motion::enter`).
    super::pages::scrim(super::wash::enter(panel, super::motion::FAST), Message::CloseInfo, 0.35)
}
