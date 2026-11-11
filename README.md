<p align="center"><img src="assets/logo.png" width="120" alt="Abyss logo"></p>

<h1 align="center">Abyss</h1>

<p align="center"><b>A fast, native desktop player for your TIDAL account and your own music files.</b><br>
Hi-Res audio, bit-perfect output and a calm interface. One small Windows app. No Electron. No web view.</p>

<p align="center">
  <a href="https://github.com/RaunaCO/TIDE/releases/latest"><img alt="Download" src="https://img.shields.io/badge/Download-Setup%20for%20Windows-111111?style=for-the-badge&logo=windows&logoColor=white"></a>
</p>

<p align="center">
  <img alt="Release" src="https://img.shields.io/github/v/release/RaunaCO/TIDE?color=444444">
  <img alt="License" src="https://img.shields.io/badge/license-MIT-444444">
  <img alt="Platform" src="https://img.shields.io/badge/platform-Windows%2010%2F11-444444">
  <img alt="Rust" src="https://img.shields.io/badge/built%20with-Rust-444444">
  <img alt="Languages" src="https://img.shields.io/badge/languages-6-444444">
</p>

<p align="center">
  <img src="docs/screenshots/settings.png" width="49%" alt="Abyss settings">
  <img src="docs/screenshots/welcome.png" width="49%" alt="Abyss welcome guide">
</p>

## Why Abyss

| | |
|---|---|
| **Sound first** | Hi-Res FLAC up to 24-bit / 192 kHz, sinc resampling, gapless queue, ReplayGain and WASAPI exclusive (bit-perfect) output. |
| **Light and quick** | One native binary. Idle on a cover-heavy page uses about 0.4% of one CPU core and draws 0 frames per second. |
| **Your music, together** | TIDAL favourites and your local files live in one Library: tracks, albums and artists, with tag and artwork editing. |
| **Lyrics that follow you** | Synced lyrics from several open providers, with romanization for Japanese, Chinese and Korean. |
| **Yours to read** | Free and open source (MIT). No telemetry. Tokens are encrypted on your PC. |
| **Speaks your language** | English, Spanish, Portuguese, French, German and Japanese. |

## Performance goals and measurements

Measured on Windows release builds with isolated profiles (method and raw numbers in
[docs/PERFORMANCE.md](docs/PERFORMANCE.md)). Goals are what the project aims to keep.

| Measure | Goal | Measured |
|---|---|---|
| Idle CPU (Home, covers cached) | under 1% of one core | 0.4% at 0 frames per second |
| Fullscreen with animated background | under 10% of one core | 4.8% |
| Peak memory (fullscreen, 24-bit / 192 kHz file) | under 800 MB | 132.5 MB working set |
| Memory growth | bounded | byte-limited image caches, bounded audio buffer |

Streaming, very large libraries and long sessions still need separate measurements.

## Install

Download **Abyss-1.0.1-win-x64-setup.exe** from the [latest release](https://github.com/RaunaCO/TIDE/releases/latest).
It installs for your user, with no administrator rights, and keeps your settings on
uninstall. A portable `.zip` is also available. The binaries are not code-signed, so
Windows SmartScreen may warn. Check the SHA-256 in the release notes.

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

Current version: **1.0.1** (see [CHANGELOG.md](CHANGELOG.md)). Abyss is free and open
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
