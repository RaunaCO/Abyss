# Roadmap

## Done
Playback engine, Hi-Res DASH, library pages, playlist editing, lyrics,
Now Playing, zoom, mouse navigation, sorting, inline album unfolding.

## Next (UI parity with TIDAL)
- [x] Home feed with shortcuts, *Staff Picks* / *Uploads* tabs
- [x] Settings page and hamburger menu
- [x] Rotating blurred Now Playing background
- [ ] Artist page and Search page comparison
- [ ] Genre / decade pages visual check
- [ ] Horizontally scrolling chips
- [ ] "View all" pages for home rows
- [ ] Report playback to TIDAL (its recents)

## Improvements beyond TIDAL
- [ ] Parametric EQ / DSP chain
- [ ] Loudness-aware crossfade
- [ ] Offline cache (not planned without explicit permission from TIDAL, see LEGAL.md)
- [ ] Linux / macOS output backends
- [ ] TIDE's own OAuth client from TIDAL (LEGAL.md R1)

## Security / compliance (from the 2026-11-07 audit)
- [x] Tokens encrypted at rest (DPAPI)
- [x] Single token refresh under concurrency
- [x] Signed URLs kept out of logs; DASH limits; HTTPS-only streams
- [x] Optional third-party lyrics; IPADIC licence text shipped
- [ ] Decide on the unused Android/PKCE login (remove before a public release)
- [ ] THIRD_PARTY_NOTICES for binary releases; code signing
- [ ] Revoke tokens on logout; rotate `crash.log`
- [ ] Bounded audio buffer instead of whole-track RAM buffering

## Cleanup
- [ ] Remove unused PKCE code, `play_pulse`, `SortBar` and other dead code clippy reports
