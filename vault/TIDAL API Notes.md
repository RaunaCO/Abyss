---
tags: [tide, tech, api]
---
# TIDAL API Notes

- Base: `api.tidal.com/v1`; auth: `auth.tidal.com/v1/oauth2`.
- `/playlists/{uuid}/tracks` includes `dateAdded`; `/items` does not.
- `playlistsAndFavoritePlaylists` page size max **50** (else HTTP 400).
- Playlist writes need ETag + `If-None-Match`.
- Promo images are only served at `resources.tidal.com/images/{id with /}/550x400.jpg`.
- Explore modules: `FEATURED_PROMOTIONS`, `PAGE_LINKS_CLOUD`, `PAGE_LINKS`.
  Featured promo: label = `header`, title = `shortHeader`.
- Wrong track id on playbackinfo gives HTTP 500.
- Lyrics: TIDAL first, then LRCLIB.

See [[Audio Pipeline]], [[Architecture]].

## v2 home feed
- `GET https://api.tidal.com/v2/home/feed/{static|editorial|uploads}` with
  headers `x-tidal-client-version`, `x-tidal-authenticated-userid`,
  `x-tidal-authenticated-subscription`. Details: [[Home Feed]].
- Mix items there use `titleTextInfo`/`subtitleTextInfo` and `mixImages`.
- `/v1/pages/home` is the old editorial page (The Hits, New Tracks…).
