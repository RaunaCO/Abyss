---
tags: [tide, feature]
---
# Ratings

0 to 5 stars in half-star steps, for **tracks** and **albums** (TIDAL has no
rating API, so they are local). File: `%APPDATA%\tide\ratings.json`
(`{"tracks":{…},"albums":{…}}`; the first, flat format still loads).

Control: `widgets::stars` / `widgets::album_stars`. Each star has a left half
(½) and right half (full); hover previews, clicking the current value clears.

Where: track lists (column "Valoración", sortable), album panel in the
collection, player bar, album page header, album cards (shown when rated or on
hover). Collection sort "Valoración" for tracks and albums (best first).
Setting "Valoraciones" hides the column.

Code: `src/ratings.rs`, `AppView::set_rating`, `set_album_rating`.
