# Architecture

TIDE is a single Rust binary (`src/main.rs`) with three layers.

```
 ui/  (GPUI views)  -->  tidal/ (API client)  -->  api.tidal.com
   |                         |
   +-------->  audio/  <-----+  (stream + decode + resample + output)
               config.rs (settings.json)
```

## `src/tidal/`
- `mod.rs`: blocking HTTP client (reqwest + rustls), OAuth device-code login,
  token refresh, library, search, playlists (writes use ETag / `If-None-Match`),
  Explore page modules, album extras, credits.
- `models.rs`: `Track`, `Album`, `Playlist`, `Item`, `Row`, `StreamInfo`...
- `dash.rs`: DASH MPD parser for Hi-Res streams.
- `lyrics.rs`: lyrics from TIDAL, falling back to LRCLIB.

## `src/audio/`
See [AUDIO.md](AUDIO.md). `mod.rs` is the engine (queue, gapless, seek),
`stream.rs` the progressive downloader, `decode.rs` symphonia + rubato,
`output.rs` cpal / WASAPI exclusive.

## `src/ui/`
- `mod.rs`: `AppView` (all state), `Page` enum, navigation history, actions and
  key bindings, zoom (`sc(v)` = `px(v * zoom)`), `fetch()` helper that runs a
  request on a background thread and updates the view.
- `pages.rs`: pages are lists of `Block`s rendered in a virtualized `list`
  inside a centered column (`theme::CONTENT_W`).
- `widgets.rs`, `icons.rs`, `theme.rs`, `images.rs` (cover cache + dominant
  colour), `input.rs` (text input), `sort.rs`, `playlists.rs`, `album_inline.rs`.
- Chrome: `titlebar.rs`, `sidebar.rs`, `player_bar.rs`, `now_playing.rs`.
- Motion: `fx.rs`, `motion.rs`, `smooth.rs` (smooth wheel + Ctrl+wheel zoom).
- Platform: `winctl.rs` (Win32 caption buttons / drag), `media.rs` (SMTC).
- `debug.rs`: screenshot helper, see [CONFIGURATION.md](CONFIGURATION.md).

## Conventions
- All UI sizes go through `sc()` so zoom applies everywhere.
- Never block the UI thread: use `fetch()` or `cx.spawn`.
- Pages never re-scroll on content updates (`splice` + restore scroll top).
