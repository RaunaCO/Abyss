# Development guide

```bash
cargo build            # debug
cargo build --release  # optimized (use this for daily use)
cargo run -- --probe
```

- Rust stable, MSVC toolchain, Windows 10/11.
- `build.rs` embeds `assets/icon.ico` via `assets/icon.rc`; regenerate the logo
  with `python assets/make_logo.py` (needs Pillow).
- A release `.exe` cannot be overwritten while TIDE is running: close it first.
- When comparing the UI with the official app, take screenshots of both and
  compare spacing and typography; use `TIDE_PAGE` to open a page directly.
- Tests that simulate input must never run while another TIDE instance is open
  (they would click in the real window).

## Style
- `cargo fmt`; keep warnings low; one small module per widget/page.
- Use `sc()` for every pixel size; use `theme::*` for colours and metrics.

## Commits
Conventional-style messages (`feat:`, `fix:`, `docs:`, `refactor:`).
