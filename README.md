<p align="center"><img src="assets/logo.png" width="96" alt="TIDE logo"></p>

# TIDE

**An unofficial, native TIDAL desktop client written from scratch in Rust with [GPUI](https://www.gpui.rs/).**

TIDE aims for the look and the features of the official TIDAL desktop app, but
runs as a single native binary: no Electron, no web view, fast start-up and low
memory use. Its audio path is built for quality: Hi-Res FLAC over DASH, a sinc
resampler and WASAPI exclusive (bit-perfect) output.

> **Disclaimer.** TIDE is an independent project. It is not affiliated with,
> endorsed by or sponsored by TIDAL. "TIDAL" is a trademark of its owner. You
> need your own TIDAL subscription; TIDE does not bypass any DRM or paywall.
> See [docs/LEGAL.md](docs/LEGAL.md).

## Features

- **Playback**: Hi-Res FLAC (up to 24-bit) via DASH, AAC for CD-quality tracks,
  gapless queue, seeking, shuffle/repeat, ReplayGain normalization.
- **Audio engine**: symphonia decoding, rubato sinc resampling, shared (cpal)
  or WASAPI exclusive output, lock-free ring buffer.
- **Library**: Home, Explore, genres, mixes, playlists (create/edit/reorder),
  albums, tracks, artists; sorting everywhere; inline album unfolding in the
  collection (MusicBee / foobar2000 style).
- **Now Playing**: full-window view with synced lyrics (click to seek, sync
  offset, TIDAL + LRCLIB providers), credits and similar tracks.
- **Interface**: TIDAL-style icon rail, two-row header with working window
  controls, 68 px player bar, smooth wheel scrolling, haptic-style press
  effects, UI zoom, mouse back/forward buttons.
- **System**: Windows media controls (SMTC), Inter font bundled.

## Getting started

Requirements: Windows 10/11 and Rust (stable, MSVC toolchain).

```bash
git clone https://github.com/RaunaCO/TIDE.git
cd TIDE
cargo run --release
```

On first launch choose *Log in*; TIDE uses TIDAL's device-code flow (you
authorize it in your browser). Tokens are stored locally in
`%APPDATA%\tide\settings.json`.

## Keyboard & mouse

| Action | Shortcut |
| --- | --- |
| Back / forward | Mouse back/forward buttons, `Alt+Left` / `Alt+Right` |
| Zoom in / out / reset | `Ctrl +` / `Ctrl -` / `Ctrl 0`, `Ctrl + wheel` |
| Volume | Mouse wheel over the volume slider (1 % steps) |
| Open Now Playing | Click the player bar |

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Audio pipeline](docs/AUDIO.md)
- [Configuration & diagnostics](docs/CONFIGURATION.md)
- [Development guide](docs/DEVELOPMENT.md)
- [Roadmap](docs/ROADMAP.md)
- [Legal notes](docs/LEGAL.md)
- Knowledge base as an [Obsidian](https://obsidian.md) vault: open the [`vault/`](vault) folder.

## Project name

Formerly `tidal-gpui`. Settings from `%APPDATA%\tidal-gpui` are migrated
automatically to `%APPDATA%\tide` on first run.

## License

MIT, see [LICENSE](LICENSE). The bundled Inter font is under the SIL OFL
(`assets/fonts/Inter-LICENSE.txt`).
