---
tags: [tide, tech]
---
# Architecture

Three layers: `ui/` (GPUI views) → `tidal/` (API client) and `audio/` (engine),
plus `config.rs`.

- State lives in `AppView` (`ui/mod.rs`); pages are `Block` lists in a
  virtualized list (`ui/pages.rs`).
- Background work via `fetch()` (thread → UI update).
- Everything sized with `sc()` so zoom works: see [[UI System]].
- Win32 glue in `winctl.rs`; SMTC in `media.rs`.

Public version: `docs/ARCHITECTURE.md`. Audio details: [[Audio Pipeline]].
API details: [[TIDAL API Notes]]. Back to [[Home]].
