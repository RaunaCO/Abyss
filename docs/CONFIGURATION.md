# Configuration and diagnostics

## Files

| Path | Content |
| --- | --- |
| `%APPDATA%\tide\settings.json` | Session (tokens DPAPI-encrypted as `protected_tokens`), profile, preferences |
| `%APPDATA%\tide\ratings.json` | Star ratings (tracks and albums) |
| `%APPDATA%\tide\recent.json` | Albums played in TIDE |
| `%APPDATA%\tide\crash.log` | Last panic, if any |
| `%LOCALAPPDATA%\tide\images\` | Artwork cache (trimmed to ~512 MB at start-up, oldest first) |

Settings from the project's former name (`%APPDATA%\tidal-gpui`) are moved
once on first run. `settings.json` is written atomically (temp file and
rename). Unknown or missing fields take defaults, so the file can be edited by
hand while TIDE is closed.

### `settings.json` fields

| Field | Default | Meaning |
| --- | --- | --- |
| `protected_tokens` | — | Encrypted session, see [SECURITY.md](SECURITY.md). Do not edit. |
| `expires_at`, `user_id`, `country_code`, `username`, `user_picture` | — | Session / profile |
| `quality` | `Max` | `Low` (AAC 320), `High` (FLAC 16/44.1), `Max` (up to 24/192) |
| `exclusive_mode` | `false` | WASAPI exclusive output |
| `force_volume` | `false` | With exclusive mode: fixed 100 % volume, no ReplayGain (bit-perfect) |
| `normalize` | `true` | ReplayGain normalisation |
| `autoplay` | `true` | Continue with track radio after the queue |
| `output_device` | `null` | Device name; `null` = system default |
| `volume`, `shuffle`, `repeat` | `0.8`, `false`, `Off` | Player state |
| `zoom`, `sidebar_collapsed`, `sorts` | `1.0`, `true`, `{}` | UI state |
| `recent_searches` | `[]` | Last 12 searches |
| `discord` | `true` | Discord Rich Presence |
| `discord_client_id` | `""` | Override of the built-in Discord application ID |
| `external_lyrics` | `true` | Ask LRCLIB / NetEase / lyrics.ovh when TIDAL has no lyrics |
| `lyrics_romanize` | `true` | Romaji / pinyin / Korean romanization under lyrics |
| `np_blur`, `audio_metadata`, `show_ratings`, `hide_uploads` | `true`, `true`, `true`, `false` | Display options |
| `client_id`, `client_secret` | TV client | OAuth client of the session. See [LEGAL.md](LEGAL.md). |

## Environment variables

| Variable | Effect |
| --- | --- |
| `RUST_LOG` | Log filter, default `tide=info` (e.g. `tide=debug`) |
| `TIDE_DISCORD_ID` | Use another Discord application ID |
| `TIDE_ZOOM` | Override UI zoom (e.g. `1.25`) |
| `TIDE_RAIL` | Force the icon rail |
| `TIDE_PAGE` | Development: open on a page (`home`, `explore`, `genre`, `tracks`, `albums`, `albums-open`, `artists`, `playlists`, `mixes`, `settings`, `menu`, `quicksearch`, `playlist`, `album`, `nowplaying`, `nowplaying-lyrics`, `nowplaying-credits`, `nowplaying-queue`) |
| `TIDE_DEMO_PLAY`, `TIDE_DEMO_TRACK` | Development: start silent playback on Now Playing pages |
| `TIDE_TILT=x,y` | Development: pin the 3D cover tilt for screenshots |
| `TIDE_PERF` | Development: log frames per second, render time and busy animations |

## Command line

```
tide --probe [track id | search text]
```

Prints what TIDAL offers for a track (codec, rate, depth, ReplayGain,
credits, lyrics, home rows) and runs a download/decode/seek/resample test.
Signed URLs are replaced by `<url>` in the manifest dump. See
[AUDIO.md](AUDIO.md#checking-the-pipeline).

## Cargo features

| Feature | Default | Effect |
| --- | --- | --- |
| `kanji-romaji` | on | Embeds the IPADIC dictionary (~46 MiB of the 71 MiB binary) for kanji readings. Without it, kana are still romanized and kanji pass through unchanged. |
