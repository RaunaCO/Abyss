---
tags: [tide, brand]
---
# Brand Identity

| Item | Value |
| --- | --- |
| Name | **TIDE** (all caps in the UI, `tide` for crate, binary, paths) |
| Tagline | A native TIDAL client, built on GPUI |
| Logo | White rounded square with black dots (inverse of TIDAL's mark). Source: `assets/make_logo.py`, outputs `assets/logo.png`, `assets/icon.ico` |
| Typeface | Inter (OFL). SF is not licensable on Windows |
| Palette | Black background, white text, translucent whites for surfaces (see `ui/theme.rs`) |

**Naming notes**: "TIDE" avoids using the TIDAL trademark in the product name
while staying evocative. The README carries an unaffiliated disclaimer: see
[[Legal Notes]].

**Rename checklist (done)**: crate/binary `tide`, config dir `%APPDATA%\tide`
(auto-migrated from `tidal-gpui`), env vars `TIDE_*`, window title, UA string.

Related: [[Project Overview]], [[Decision Log]].
