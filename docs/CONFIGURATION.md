# Configuration & diagnostics

## Files
| Path | Content |
| --- | --- |
| `%APPDATA%\tide\settings.json` | tokens, quality, exclusive mode, normalize, zoom, sort orders, volume... |
| `%LOCALAPPDATA%\tide\` | image and stream cache |

**Treat `settings.json` as a secret**: it contains your TIDAL tokens. It is
git-ignored.

## Environment variables (development)
| Variable | Effect |
| --- | --- |
| `RUST_LOG=tide=debug` | verbose logging |
| `TIDE_PAGE=<page>` | open on a page: `home explore genre tracks albums artists playlists mixes settings playlist album nowplaying nowplaying-lyrics nowplaying-credits` (playback starts paused) |
| `TIDE_ZOOM=1.25` | override UI zoom |
| `TIDE_RAIL=1` | force the icon rail |

## CLI
```
tide --probe [track id | search text]
```
Prints stream info (codec, sample rate, bit depth), runs a decode/seek test,
credits, ReplayGain, lyrics and Explore modules. Useful to verify quality.
