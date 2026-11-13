# Roadmap

## Done (1.0.0)
Playback engine, Hi-Res DASH, library (TIDAL and local files together),
playlist editing, lyrics from several providers, Now Playing and a clean
fullscreen view, Replay, context menus and Song Info, six languages, guided
introduction, local file tag editing, Windows media controls.

## Done (1.3.0)
Equaliser v2: no clipping inside the chain, filters per channel, automatic
pre-amp with a peak meter, dither for processed 16-bit output, frequency
response graph and an APO-style filter list.

## Done (1.2.0)
Parametric equaliser (chain of filters and pre-amp, bit-perfect when idle).

## Done (1.1.0)
Last.fm scrobbling and history sync, plain-language install and account-risk
notices (README and welcome guide), automatic draft releases from a tag.

## Before or after 1.0 (open)
- Remove the unused Android/PKCE login code (LEGAL.md R1).
- Remove the remaining dead code that clippy reports in the audio engine.
- Code signing for binaries.
- "View all" pages for home rows.
- Discord: register an Abyss application and ship its ID.
- Visual comparison of artist and search pages with TIDAL.

## Improvements beyond TIDAL (not planned for 1.0)
- Parametric EQ / DSP chain, loudness-aware crossfade.
- Linux / macOS output backends.
- Offline cache: not planned without explicit permission from TIDAL (see LEGAL.md).
- Abyss's own OAuth client from TIDAL (LEGAL.md R1).

## Security and compliance
- [x] Tokens encrypted at rest (DPAPI); single token refresh under concurrency.
- [x] Signed URLs and bearer tokens kept out of logs and `crash.log` (rotated).
- [x] DASH limits, HTTPS-only streams, bounded audio buffer, byte-limited caches.
- [x] Third-party notices (`THIRD_PARTY_NOTICES.md`) and licence texts ship with binaries.
- [ ] Revoke tokens on sign-out.
- [ ] Third-party lyrics off by default for public releases.
