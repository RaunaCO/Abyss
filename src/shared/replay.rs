//! Replay statistics: computed from the history Abyss records while it plays
//! (see `crate::history`). Shared by every interface.

use std::collections::HashMap;

use crate::history::{civil_from_days, days_from_civil, local_day_hour, local_offset, unix_now, Entry};
use crate::tidal::image_url;

pub fn months() -> [&'static str; 12] {
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
    pub const ALL: [Period; 4] = [Period::ThisYear, Period::Last30, Period::ThisMonth, Period::AllTime];
    pub fn label(self) -> &'static str {
        match self {
            Period::ThisYear => crate::i18n::t("this_year"),
            Period::Last30 => crate::i18n::t("last_30_days"),
            Period::ThisMonth => crate::i18n::t("this_month"),
            Period::AllTime => crate::i18n::t("all_time"),
        }
    }
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

pub fn day_label(day: i64) -> String {
    let (_, m, d) = civil_from_days(day);
    crate::i18n::text("date.short", &[d.to_string(), months()[(m - 1) as usize].into()])
}

pub fn full_date(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    crate::i18n::text("date.full", &[d.to_string(), months()[(m - 1) as usize].into(), y.to_string()])
}

pub fn rank(items: impl Iterator<Item = Ranked>, n: usize) -> Vec<Ranked> {
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

pub fn times(n: u32) -> String {
    if n == 1 {
        crate::i18n::t("1_play").into()
    } else {
        crate::i18n::text("plays.count", &[format!("{}", n)])
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
