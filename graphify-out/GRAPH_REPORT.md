# Graph Report - tidal-gpui  (2026-11-06)

## Corpus Check
- 60 files · ~50,167 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 5 file(s) not represented in the graph (top: (none) 3, .ico 1, .rc 1)

## Summary
- 1278 nodes · 3064 edges · 58 communities (48 shown, 10 thin omitted)
- Extraction: 97% EXTRACTED · 3% INFERRED · 0% AMBIGUOUS · INFERRED: 106 edges (avg confidence: 0.85)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `c8cbc162`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- TextInput
- sc
- output.rs
- Icon
- Tidal
- Quality
- Page
- Buffer
- Block
- theme.rs
- now_playing.rs
- Playlist
- Home.md
- decode.rs
- images.rs
- appmenu.rs
- discord.rs
- Item
- audio/mod.rs
- models.rs
- sidebar.rs
- tidal/mod.rs
- AppView
- settings.rs
- ui/mod.rs
- fx.rs
- .fetch
- winctl.rs
- smooth.rs
- .play_index
- .render
- make_logo.py
- duration
- titlebar.rs
- Context
- media.rs
- Engine
- px
- Rgba
- MediaAction
- README.md
- Cmd
- RowKind
- decode_loop
- BgSpin
- TIDE
- StreamInfo
- LoginState
- .toggle_album_inline
- Roadmap
- DEVELOPMENT.md
- Architecture
- Configuration & diagnostics
- Discord Rich Presence
- OutState
- tide

## God Nodes (most connected - your core abstractions)
1. `AppView` - 142 edges
2. `sc()` - 85 edges
3. `Tidal` - 63 edges
4. `Icon` - 60 edges
5. `TextInput` - 59 edges
6. `Block` - 39 edges
7. `Buffer` - 31 edges
8. `Item` - 28 edges
9. `Page` - 28 edges
10. `Engine` - 27 edges

## Surprising Connections (you probably didn't know these)
- `Contributing` --references--> `main()`  [INFERRED]
  CONTRIBUTING.md → assets/make_logo.py
- `lrclib()` --calls--> `close()`  [INFERRED]
  src/tidal/lyrics.rs → src/ui/winctl.rs
- `render()` --calls--> `row()`  [INFERRED]
  src/ui/album_inline.rs → src/ui/settings.rs
- `panel()` --calls--> `sc()`  [INFERRED]
  src/ui/appmenu.rs → src/ui/mod.rs
- `overlay()` --calls--> `sc()`  [INFERRED]
  src/ui/appmenu.rs → src/ui/mod.rs

## Import Cycles
- 2-file cycle: `src/ui/input.rs -> src/ui/mod.rs -> src/ui/input.rs`
- 2-file cycle: `src/audio/mod.rs -> src/audio/stream.rs -> src/audio/mod.rs`
- 3-file cycle: `src/audio/decode.rs -> src/audio/stream.rs -> src/audio/mod.rs -> src/audio/decode.rs`

## Communities (58 total, 10 thin omitted)

### Community 0 - "TextInput"
Cohesion: 0.05
Nodes (8): bindings(), InputEvent, Changed, Escape, Submit, PrepaintState, TextElement, TextInput

### Community 1 - "sc"
Cohesion: 0.07
Nodes (49): fmt_khz(), AppView, render(), MenuTarget, AddToPlaylist, PlaylistTrack, QueueItem, Sort (+41 more)

### Community 2 - "output.rs"
Cohesion: 0.07
Nodes (21): ExclusiveHandle, fill(), find_device(), Keep, Cpal, Exclusive, open_exclusive(), open_shared() (+13 more)

### Community 3 - "Icon"
Cohesion: 0.04
Nodes (46): Icon, Album, Artist, Back, Calendar, ChevDown, Clock, Close (+38 more)

### Community 5 - "Quality"
Cohesion: 0.08
Nodes (20): config_dir(), DEVICE_CLIENT_ID, DEVICE_CLIENT_SECRET, DISCORD_APP_ID, Quality, High, Low, Max (+12 more)

### Community 6 - "Page"
Cohesion: 0.07
Nodes (30): Page, Album, Artist, Browse, Explore, Home, Mix, MyAlbums (+22 more)

### Community 7 - "Buffer"
Cohesion: 0.11
Nodes (12): Seek, Buffer, fetch(), get_bytes(), PARALLEL_SEGMENTS, Reader, segment_worker(), start() (+4 more)

### Community 8 - "Block"
Cohesion: 0.09
Nodes (36): Block, Actions, AlbumHeader, AlbumInline, ArtistHeader, Cards, Chips, Featured (+28 more)

### Community 9 - "theme.rs"
Cohesion: 0.09
Nodes (32): accent(), accent_dim(), active(), bg(), border(), button(), button_text(), CAPTION_H (+24 more)

### Community 10 - "now_playing.rs"
Cohesion: 0.14
Nodes (14): AppView, credits_view(), lyrics_view(), NpTab, Credits, Lyrics, Queue, Similar (+6 more)

### Community 11 - "Playlist"
Cohesion: 0.13
Nodes (8): Playlist, AppView, Dialog, DeletePlaylist, EditPlaylist, NewPlaylist, dialog_overlay(), DraggedTrack

### Community 12 - "Home.md"
Cohesion: 0.09
Nodes (20): Architecture, Audio Pipeline, Facts learned, Verify, Brand Identity, {{date}}, Done, Findings (+12 more)

### Community 13 - "decode.rs"
Cohesion: 0.08
Nodes (4): Decoder, interleave(), Resampler2, do_seek()

### Community 14 - "images.rs"
Cohesion: 0.15
Nodes (21): blurred(), blurred_png(), color(), dominant_color(), file_for(), format_of(), FRAME_COUNT, FRAME_STEP_DEG (+13 more)

### Community 15 - "appmenu.rs"
Cohesion: 0.11
Nodes (18): Repeat, All, Off, One, AppView, entries_height_before(), Entry, Item (+10 more)

### Community 16 - "discord.rs"
Cohesion: 0.11
Nodes (12): Activity, Conn, connect(), Discord, frame(), Msg, Clear, ClientId (+4 more)

### Community 17 - "Item"
Cohesion: 0.10
Nodes (16): ArtistPage, promotion_item(), Album, Artist, image_url(), Item, Album, Artist (+8 more)

### Community 18 - "audio/mod.rs"
Cohesion: 0.15
Nodes (10): EngineView, Event, Error, Finished, gain_for(), Mark, OutputPrefs, Player (+2 more)

### Community 19 - "models.rs"
Cohesion: 0.24
Nodes (8): AlbumRef, ArtistRef, Mix, opt_s(), s(), tags(), Track, u()

### Community 20 - "sidebar.rs"
Cohesion: 0.20
Nodes (9): AppView, divider(), NAV_COLLECTION, nav_item(), NAV_MAIN, RAIL_W, render(), Tip (+1 more)

### Community 21 - "tidal/mod.rs"
Cohesion: 0.10
Nodes (11): API_URL, AUTH_URL, now(), PKCE_CLIENT_ID, PKCE_CLIENT_SECRET, PKCE_REDIRECT, PkceLogin, PollResult (+3 more)

### Community 22 - "AppView"
Cohesion: 0.15
Nodes (4): AppView, Drag, Progress, Volume

### Community 23 - "settings.rs"
Cohesion: 0.30
Nodes (8): AppView, card(), CARD_BG, heading(), row(), row_text(), small_button(), switch()

### Community 24 - "ui/mod.rs"
Cohesion: 0.12
Nodes (3): bindings(), ZOOM_BITS, ZOOM_STEPS

### Community 25 - "fx.rs"
Cohesion: 0.13
Nodes (10): AMBIENT_FADE, blobs(), COAST_DEG, COAST_S, delayed(), glow(), HEART_POP, rotating_backdrop() (+2 more)

### Community 26 - ".fetch"
Cohesion: 0.18
Nodes (7): into_load(), Load, Failed, Idle, Loading, Ready, Load<T>

### Community 27 - "winctl.rs"
Cohesion: 0.21
Nodes (16): close(), HTCAPTION, hwnd(), is_maximized(), IsZoomed(), minimize(), PostMessageW(), ReleaseCapture() (+8 more)

### Community 28 - "smooth.rs"
Cohesion: 0.16
Nodes (6): AppView, EASE, FRAME, PX_PER_LINE, wheel_catcher(), zoom_catcher()

### Community 29 - ".play_index"
Cohesion: 0.17
Nodes (3): now_secs(), NowPlaying, shuffle()

### Community 31 - "make_logo.py"
Cohesion: 0.18
Nodes (7): dots(), ico(), inside_rounded(), main(), png(), render(), Contributing

### Community 32 - "duration"
Cohesion: 0.16
Nodes (7): AppView, cubic_bezier(), ease_out(), lyrics_ease(), PANEL_ANIM, progress(), SIDEBAR_ANIM

### Community 33 - "titlebar.rs"
Cohesion: 0.24
Nodes (7): drag_area(), render(), WinButton, Close, Max, Min, window_button()

### Community 38 - "Rgba"
Cohesion: 0.29
Nodes (5): AppView, lerp(), play_pulse(), tint(), with_alpha()

### Community 39 - "MediaAction"
Cohesion: 0.24
Nodes (9): MediaAction, Next, Pause, Play, Prev, SeekBy, SeekTo, Stop (+1 more)

### Community 40 - "README.md"
Cohesion: 0.28
Nodes (4): Audio pipeline, Exclusive mode, Quality reality check, Legal notes

### Community 41 - "Cmd"
Cohesion: 0.22
Nodes (9): Cmd, ClearQueue, Output, Pause, Play, Queue, Resume, Stop (+1 more)

### Community 42 - "RowKind"
Cohesion: 0.22
Nodes (7): Row, RowKind, Cards, Chips, Featured, Links, Shortcuts

### Community 43 - "decode_loop"
Cohesion: 0.32
Nodes (7): DecMsg, Chunk, End, Error, Format, Seeked, decode_loop()

### Community 44 - "BgSpin"
Cohesion: 0.29
Nodes (5): BgSpin, Coasting, Scrub, Spinning, Stopped

### Community 45 - "TIDE"
Cohesion: 0.29
Nodes (7): Documentation, Features, Getting started, Keyboard & mouse, License, Project name, TIDE

### Community 46 - "StreamInfo"
Cohesion: 0.43
Nodes (3): spawn_worker(), Track, StreamInfo

### Community 47 - "LoginState"
Cohesion: 0.43
Nodes (5): DeviceCode, LoginState, Failed, Idle, Waiting

### Community 49 - "Roadmap"
Cohesion: 0.33
Nodes (5): Cleanup, Done, Improvements beyond TIDAL, Next (UI parity with TIDAL), Roadmap

### Community 51 - "DEVELOPMENT.md"
Cohesion: 0.40
Nodes (3): Commits, Development guide, Style

### Community 52 - "Architecture"
Cohesion: 0.40
Nodes (5): Architecture, Conventions, `src/audio/`, `src/tidal/`, `src/ui/`

### Community 53 - "Configuration & diagnostics"
Cohesion: 0.50
Nodes (4): CLI, Configuration & diagnostics, Environment variables (development), Files

### Community 54 - "Discord Rich Presence"
Cohesion: 0.50
Nodes (3): Code, Discord Rich Presence, Setup

### Community 55 - "OutState"
Cohesion: 0.50
Nodes (4): OutState, Failed, Ready, Wait

## Knowledge Gaps
- **151 isolated node(s):** `tide`, `Play`, `Queue`, `ClearQueue`, `Pause` (+146 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 397 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **10 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `AppView` connect `AppView` to `TextInput`, `sc`, `output.rs`, `Tidal`, `Quality`, `Page`, `Block`, `now_playing.rs`, `Playlist`, `appmenu.rs`, `discord.rs`, `Item`, `audio/mod.rs`, `models.rs`, `sidebar.rs`, `settings.rs`, `ui/mod.rs`, `fx.rs`, `.fetch`, `smooth.rs`, `.play_index`, `.render`, `duration`, `titlebar.rs`, `Context`, `media.rs`, `RowKind`, `BgSpin`, `StreamInfo`, `LoginState`, `.toggle_album_inline`?**
  _High betweenness centrality (0.392) - this node is a cross-community bridge._
- **Are the 65 inferred relationships involving `sc()` (e.g. with `render()` and `overlay()`) actually correct?**
  _`sc()` has 65 INFERRED edges - model-reasoned connections that need verification._
- **What connects `tide`, `Play`, `Queue` to the rest of the system?**
  _151 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `TextInput` be split into smaller, more focused modules?**
  _Cohesion score 0.05006662859318484 - nodes in this community are weakly interconnected._
- **Why does `sc()` connect `sc` to `TextInput`, `titlebar.rs`, `px`, `Rgba`, `Page`, `now_playing.rs`, `Playlist`, `appmenu.rs`, `sidebar.rs`, `AppView`, `settings.rs`, `ui/mod.rs`, `fx.rs`, `smooth.rs`, `.render`?**
  _High betweenness centrality (0.120) - this node is a cross-community bridge._
- **Should `sc` be split into smaller, more focused modules?**
  _Cohesion score 0.06507731958762887 - nodes in this community are weakly interconnected._
- **Why does `TextInput` connect `TextInput` to `ui/mod.rs`, `Context`, `Playlist`, `AppView`?**
  _High betweenness centrality (0.114) - this node is a cross-community bridge._