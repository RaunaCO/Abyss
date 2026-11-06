//! Sorting of the collection pages (tracks, albums, artists, playlists,
//! mixes). The original "date added" order is remembered per item, so
//! "Recientes" always restores it.

use super::sc;
use std::collections::HashMap;

use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight, SharedString};

use super::{theme, AppView, Load, Page};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Added,
    Name,
    Artist,
    Album,
    Year,
    Duration,
    Count,
}

impl SortKey {
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Added => "Recientes",
            SortKey::Name => "Nombre",
            SortKey::Artist => "Artista",
            SortKey::Album => "Álbum",
            SortKey::Year => "Año",
            SortKey::Duration => "Duración",
            SortKey::Count => "Nº de pistas",
        }
    }
    fn code(self) -> &'static str {
        match self {
            SortKey::Added => "added",
            SortKey::Name => "name",
            SortKey::Artist => "artist",
            SortKey::Album => "album",
            SortKey::Year => "year",
            SortKey::Duration => "duration",
            SortKey::Count => "count",
        }
    }
    fn from_code(s: &str) -> Self {
        match s {
            "name" => SortKey::Name,
            "artist" => SortKey::Artist,
            "album" => SortKey::Album,
            "year" => SortKey::Year,
            "duration" => SortKey::Duration,
            "count" => SortKey::Count,
            _ => SortKey::Added,
        }
    }
}

/// Which collection list a sort applies to.
pub fn section(page: &Page) -> Option<&'static str> {
    Some(match page {
        Page::MyTracks => "tracks",
        Page::MyAlbums => "albums",
        Page::MyArtists => "artists",
        Page::MyPlaylists => "playlists",
        Page::MyMixes => "mixes",
        _ => return None,
    })
}

pub fn options(section: &str) -> &'static [SortKey] {
    match section {
        "tracks" => &[SortKey::Added, SortKey::Name, SortKey::Artist, SortKey::Album, SortKey::Duration],
        "albums" => &[SortKey::Added, SortKey::Name, SortKey::Artist, SortKey::Year],
        "artists" => &[SortKey::Added, SortKey::Name],
        "playlists" => &[SortKey::Added, SortKey::Name, SortKey::Count],
        _ => &[SortKey::Added, SortKey::Name],
    }
}

fn text_key(s: &str) -> String {
    let s = s.trim().to_lowercase();
    s.strip_prefix("the ").map(str::to_string).unwrap_or(s)
}

/// Remember the API ("date added") order of a list the first time each item is seen.
fn rank<T>(ranks: &mut HashMap<String, i64>, list: &[T], id: impl Fn(&T) -> String) {
    let fresh: Vec<String> = list.iter().map(&id).filter(|k| !ranks.contains_key(k)).collect();
    if fresh.is_empty() {
        return;
    }
    if ranks.is_empty() {
        for (i, k) in list.iter().map(&id).enumerate() {
            ranks.insert(k, i as i64);
        }
    } else {
        // Items added later (e.g. a new favourite) count as the newest.
        let min = ranks.values().copied().min().unwrap_or(0);
        for (i, k) in fresh.into_iter().enumerate() {
            ranks.insert(k, min - 1 - i as i64);
        }
    }
}

impl AppView {
    pub fn sort_of(&self, section: &str) -> (SortKey, bool) {
        self.settings
            .lock()
            .sorts
            .get(section)
            .map(|(k, d)| (SortKey::from_code(k), *d))
            .unwrap_or((SortKey::Added, false))
    }

    pub fn set_sort(&mut self, section: &str, key: SortKey, cx: &mut Context<Self>) {
        let (cur, desc) = self.sort_of(section);
        // Clicking the active option flips the direction.
        let desc = if cur == key { !desc } else { false };
        {
            let mut s = self.settings.lock();
            s.sorts.insert(section.to_string(), (key.code().to_string(), desc));
            s.save();
        }
        self.apply_sorts();
        self.content_changed_pub();
        self.page_anim += 1;
        cx.notify();
    }

    /// Sort every loaded collection list by its chosen key.
    pub fn apply_sorts(&mut self) {
        let get = |s: &Self, k: &str| s.sort_of(k);
        let (tk, td) = get(self, "tracks");
        let (ak, ad) = get(self, "albums");
        let (rk, rd) = get(self, "artists");
        let (pk, pd) = get(self, "playlists");
        let (mk, md) = get(self, "mixes");

        if let Load::Ready(list) = &mut self.fav_tracks {
            let ranks = self.sort_ranks.entry("tracks").or_default();
            rank(ranks, list, |t| t.id.to_string());
            list.sort_by(|a, b| {
                let o = match tk {
                    SortKey::Name => text_key(&a.title).cmp(&text_key(&b.title)),
                    SortKey::Artist => text_key(&a.artist_names()).cmp(&text_key(&b.artist_names())),
                    SortKey::Album => {
                        let t = |x: &crate::tidal::Track| x.album.as_ref().map(|a| text_key(&a.title)).unwrap_or_default();
                        t(a).cmp(&t(b)).then(a.track_number.cmp(&b.track_number))
                    }
                    SortKey::Duration => a.duration.cmp(&b.duration),
                    _ => ranks.get(&a.id.to_string()).cmp(&ranks.get(&b.id.to_string())),
                };
                if td { o.reverse() } else { o }
            });
        }
        if let Load::Ready(list) = &mut self.fav_albums {
            let ranks = self.sort_ranks.entry("albums").or_default();
            rank(ranks, list, |a| a.id.to_string());
            list.sort_by(|a, b| {
                let o = match ak {
                    SortKey::Name => text_key(&a.title).cmp(&text_key(&b.title)),
                    SortKey::Artist => text_key(&a.artist_names()).cmp(&text_key(&b.artist_names())).then(a.release_date.cmp(&b.release_date)),
                    // Newest first feels natural for years.
                    SortKey::Year => b.release_date.cmp(&a.release_date),
                    _ => ranks.get(&a.id.to_string()).cmp(&ranks.get(&b.id.to_string())),
                };
                if ad { o.reverse() } else { o }
            });
        }
        if let Load::Ready(list) = &mut self.fav_artists {
            let ranks = self.sort_ranks.entry("artists").or_default();
            rank(ranks, list, |a| a.id.to_string());
            list.sort_by(|a, b| {
                let o = match rk {
                    SortKey::Name => text_key(&a.name).cmp(&text_key(&b.name)),
                    _ => ranks.get(&a.id.to_string()).cmp(&ranks.get(&b.id.to_string())),
                };
                if rd { o.reverse() } else { o }
            });
        }
        if let Load::Ready(list) = &mut self.my_playlists {
            let ranks = self.sort_ranks.entry("playlists").or_default();
            rank(ranks, list, |p| p.uuid.clone());
            list.sort_by(|a, b| {
                let o = match pk {
                    SortKey::Name => text_key(&a.title).cmp(&text_key(&b.title)),
                    SortKey::Count => b.number_of_tracks.cmp(&a.number_of_tracks),
                    _ => ranks.get(&a.uuid).cmp(&ranks.get(&b.uuid)),
                };
                if pd { o.reverse() } else { o }
            });
        }
        if let Load::Ready(list) = &mut self.my_mixes {
            let ranks = self.sort_ranks.entry("mixes").or_default();
            rank(ranks, list, |m| m.id.clone());
            list.sort_by(|a, b| {
                let o = match mk {
                    SortKey::Name => text_key(&a.title).cmp(&text_key(&b.title)),
                    _ => ranks.get(&a.id).cmp(&ranks.get(&b.id)),
                };
                if md { o.reverse() } else { o }
            });
        }
    }
}

/// Row of sort options under a collection title.
pub fn bar(view: &mut AppView, section: &'static str, cx: &mut Context<AppView>) -> AnyElement {
    let (cur, desc) = view.sort_of(section);
    div()
        .pb(sc(14.))
        .flex()
        .items_center()
        .gap(sc(8.))
        .child(div().text_size(sc(12.)).text_color(theme::text_dim()).mr(sc(4.)).child("Ordenar por"))
        .children(options(section).iter().map(|&k| {
            let active = k == cur;
            let label: SharedString = if active {
                format!("{} {}", k.label(), if desc { "↑" } else { "↓" }).into()
            } else {
                k.label().into()
            };
            div()
                .id(SharedString::from(format!("sort-{section}-{}", k.code())))
                .px(sc(12.))
                .py(sc(5.))
                .rounded(sc(999.))
                .text_size(sc(12.))
                .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                .cursor_pointer()
                .bg(if active { theme::text() } else { theme::surface() })
                .text_color(if active { theme::bg() } else { theme::text_secondary() })
                .hover(|s| if active { s.opacity(0.9) } else { s.bg(theme::elevated()).text_color(theme::text()) })
                .active(|s| s.opacity(0.7))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.set_sort(section, k, cx)))
        }))
        .into_any_element()
}
