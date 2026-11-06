---
tags: [tide, process]
---
# Testing Workflow

1. Close any running TIDE.
2. `cargo build` (debug) for screenshots; `TIDE_PAGE=<page>` opens a page.
3. Screenshot scripts must verify the TIDE window is on top before capturing
   (an earlier capture grabbed a private chat by accident).
4. Input-simulating tests refuse to run if any `tide` process exists.
5. Tests may overwrite `settings.json` (zoom, sidebar): reset afterwards.
6. `cargo build --release` only when the app is closed.

See [[UI System]], [[Audio Pipeline]].
