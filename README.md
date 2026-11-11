<p align="center"><img src="assets/logo.png" width="96" alt="Abyss logo"></p>

# Abyss

**An unofficial, native TIDAL desktop client written from scratch in Rust with [GPUI](https://www.gpui.rs/).**

Abyss aims for the look and the features of the official TIDAL desktop app, but
runs as a single native binary: no Electron, no web view, fast start-up and low
memory use. Its audio path is built for quality: Hi-Res FLAC over DASH, a sinc
resampler and WASAPI exclusive (bit-perfect) output.

> **Read this first.** Abyss is an independent project. It is not affiliated
> with, endorsed by or sponsored by TIDAL, and "TIDAL" is a trademark of its
> owner. You need your own TIDAL subscription. Abyss refuses encrypted (DRM)
> streams and never saves audio to disk.
>
> Abyss has **no client credentials of its own**: it signs in with OAuth
> clients that belong to TIDAL's apps, and it uses undocumented TIDAL
> endpoints. This may break TIDAL's terms and may get accounts suspended or
> the app blocked. Use it at your own risk. [docs/LEGAL.md](docs/LEGAL.md)
> lists the known legal risks and open questions. It is not legal advice.

## Features

- **Playback**: Hi-Res FLAC (up to 24-bit) via DASH (CD-quality tracks come
  as AAC or FLAC depending on the login client, see [docs/AUDIO.md](docs/AUDIO.md)),
  gapless queue, fast seeking (DASH segments restart), shuffle/repeat, ReplayGain normalization.
- **Audio engine**: symphonia decoding, rubato sinc resampling, shared (cpal)
  or WASAPI exclusive output, lock-free ring buffer.
- **Library**: Home, Explore, genres, mixes, playlists (create/edit/reorder),
  albums, tracks, artists; sorting everywhere; inline album unfolding in the
  collection (MusicBee / foobar2000 style).
- **Home**: the same "For you" / Staff Picks / Uploads feed as the official app, with local recents.
- **Now Playing**: full-window view with TIDAL's rotating blurred-cover background,
  synced lyrics that follow smoothly (click to seek, sync offset, glow on the
  current line; TIDAL first, optionally LRCLIB / NetEase / lyrics.ovh), romaji /
  pinyin / Korean romanization, and tabs for credits, similar tracks and queue.
  Progress, transport, volume and utility controls sit beneath the artwork;
  the desktop player bar is hidden in fullscreen.
- **Settings & menu**: settings page and hamburger menu (File, Navigation,
  Playback, View, Help) laid out like the official app, with keyboard shortcuts.
- **Interface**: collapsible icon rail, a single top bar with search and window
  controls, a player bar with transport and volume, shared motion on every panel
  (reduced when Windows asks for it), smooth wheel scrolling, UI zoom, mouse
  back/forward buttons.
- **Ratings**: 0 to 5 stars in half-star steps for any track (lists, album panel, player bar), sortable. Stored locally in `%APPDATA%\abyss\ratings.json` since TIDAL has no rating API.
- **Discord**: optional Rich Presence with cover, title, artist, progress and a TIDAL link. Inactive until an Abyss application ID is configured. See [docs/DISCORD.md](docs/DISCORD.md).
- **Languages**: English, Spanish, Portuguese, French, German and Japanese. Follows the system by default; change it in Settings > Appearance.
- **System**: Windows media controls (SMTC), Inter font bundled.

Current version: **1.0.0** (see [CHANGELOG.md](CHANGELOG.md)). Abyss is free and open
source (MIT); it is not sold and must not be bundled with a paid offering.

## Getting started

Requirements: Windows 10/11 and Rust (stable, MSVC toolchain).

```bash
git clone https://github.com/RaunaCO/TIDE.git
cd TIDE
cargo run --release
```

The Windows binary is `Abyss.exe`. Existing `%APPDATA%\tide` data is copied to `%APPDATA%\abyss` on first launch, preserving the original. See [configuration](docs/CONFIGURATION.md).

Manage local music and edit file tags in Settings > Local files. MP3, FLAC and
M4A changes are written safely to the file with a one-time original backup;
WAV changes stay in the index. See [local files](docs/LOCAL-FILES.md).

On first launch choose *Log in*: Abyss uses TIDAL's device-code flow, and you
authorise it in your browser. Tokens are stored in
`%APPDATA%\abyss\settings.json`, encrypted with Windows DPAPI for your user
account ([docs/SECURITY.md](docs/SECURITY.md)). Abyss sends no telemetry. Every
host it contacts is listed in [docs/PRIVACY.md](docs/PRIVACY.md).

Build without the ~46 MiB Japanese dictionary (71 → 25 MiB binary) (kanji stay unromanized):
`cargo build --release --no-default-features`.

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
- [Security](docs/SECURITY.md) · [Privacy](docs/PRIVACY.md) · [Performance](docs/PERFORMANCE.md)
- [Dependencies & licences](docs/DEPENDENCIES.md)
- [Roadmap](docs/ROADMAP.md)
- [Discord integration](docs/DISCORD.md)
- [Legal notes and risks](docs/LEGAL.md)

## Project name

Formerly `tidal-gpui`. Settings from `%APPDATA%\tidal-gpui` are migrated
automatically to `%APPDATA%\abyss` on first run.

## License

MIT, see [LICENSE](LICENSE). The bundled Inter font is under the SIL OFL
(`assets/fonts/Inter-LICENSE.txt`). The embedded IPADIC dictionary is under
its own licence (`assets/licenses/IPADIC-COPYING.txt`). Third-party crate
licences are summarised in [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md).
