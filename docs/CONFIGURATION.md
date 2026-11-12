# Configuration and diagnostics

## Files

| Path | Content |
| --- | --- |
| `%APPDATA%\abyss\settings.json` | Session (tokens DPAPI-encrypted as `protected_tokens`), profile, preferences |
| `%APPDATA%\abyss\listening.sqlite3` | Replay history and star ratings for tracks and albums |
| `%APPDATA%\abyss\recent.json` | Albums played in Abyss |
| `%APPDATA%\abyss\history.json`, `ratings.json` | Legacy files, imported once per database and kept as they were |
| `%APPDATA%\abyss\crash.log` | Last panic, if any |
| `%APPDATA%\abyss\local-library.json`, `local-folders.json` | Imported music metadata and watched folders |
| `%APPDATA%\abyss\local-originals\` | One-time originals for restoring edited tags, including full audio copies |
| `%APPDATA%\abyss\local-covers\` | Extracted and edited covers keyed by content |
| `%APPDATA%\abyss\lyrics-missing.json` | Bounded 15-minute cache of unsuccessful lyric lookups |
| `%LOCALAPPDATA%\abyss\images\` | Artwork cache (trimmed to ~512 MB at start-up, oldest first) |

On first start, Abyss copies the complete `%APPDATA%\tide` tree to
`%APPDATA%\abyss`, including backups, local artwork, encrypted settings and
SQLite data. If tide is absent, it also accepts the older tidal-gpui folder.
Close older players before the first launch. The original tree is never moved
or deleted. A complete staging copy is renamed into place only when copying
succeeds. Existing Abyss data is never overwritten. A failed copy stops startup
and leaves both the original and the staging directory for recovery. Do not
run the old and new versions together during migration.

`ABYSS_CONFIG_DIR` and `ABYSS_CACHE_DIR` isolate data and bypass migration.
The legacy TIDE overrides still work, with ABYSS overrides taking precedence.
The old DPAPI entropy is retained so copied sessions remain decryptable on the
same Windows account. Copied local-library artwork resolves in the new configuration tree; paths to
your original audio files are unchanged. Artwork cache is rebuilt in the new
cache directory.
`settings.json` is written atomically with a temporary file and rename.
Unknown or missing fields take defaults. Edit it only while Abyss is closed.

To back up Replay and ratings, copy `listening.sqlite3` with Abyss closed.
Older builds still use the legacy JSON files, which contain the data from
before the import.

### `settings.json` fields

| Field | Default | Meaning |
| --- | --- | --- |
| `protected_tokens` | - | Encrypted session, see [SECURITY.md](SECURITY.md). Do not edit. |
| `expires_at`, `user_id`, `country_code`, `username`, `user_picture` | - | Session / profile |
| `language` | `null` | Follow Windows UI language; overrides: `en`, `es`, `pt`, `fr`, `de`, `ja`. Settings > Appearance > Language applies immediately. |
| `quality` | `Max` | `Low` (AAC 320), `High` (FLAC 16/44.1), `Max` (up to 24/192) |
| `exclusive_mode` | `false` | WASAPI exclusive output |
| `force_volume` | `false` | With exclusive mode: fixed 100 % volume, no ReplayGain (bit-perfect) |
| `normalize` | `true` | ReplayGain normalisation |
| `autoplay` | `true` | Continue with track radio after the queue |
| `output_device` | `null` | Device name; `null` = system default |
| `volume`, `shuffle`, `repeat` | `0.8`, `false`, `Off` | Player state |
| `zoom`, `sorts` | `1.0`, `{}` | UI state |
| `sidebar_collapsed` | - | No longer used since 0.9.4c (the sidebar is always a rail that unfolds on hover); kept so older files load |
| `recent_searches` | `[]` | Last 12 searches |
| `discord` | `true` | Discord Rich Presence |
| `discord_client_id` | `""` | Application ID supplied by the owner, empty means no activity is published |
| `lastfm_enabled` | `false` | Send listens to Last.fm (turns on when you connect; Settings → Integrations) |
| `lastfm_user` | - | Connected Last.fm user name, shown in Settings |
| `lastfm_protected_session` | - | Last.fm session key, DPAPI-encrypted, base64. Never stored in plaintext (`lastfm_session` is only read from older plaintext files). |
| `external_lyrics` | `true` | Ask LRCLIB / NetEase / lyrics.ovh when TIDAL has no lyrics |
| `lyrics_romanize` | `true` | Romaji / pinyin / Korean romanization under lyrics |
| `np_blur`, `audio_metadata`, `show_ratings`, `hide_uploads` | `true`, `true`, `true`, `false` | Display options |
| `light_theme` | `false` | Light interface theme (Ajustes → Pantalla → Tema). Now Playing keeps its dark look over the artwork. |
| `lib_last` | `"tracks"` | Library category shown when Library is opened (`tracks`, `albums`, `artists`) |
| `lib_grid` | `{}` | List (`false`) or grid (`true`) per Library category; albums and artists default to grid, tracks to list |
| `user_email` | - | Account e-mail as TIDAL reports it, shown in the sidebar's account panel (removed on sign-out) |
| `accent` | `3407854` (`0x33FFEE`) | Accent colour as a 0xRRGGBB number (Ajustes → Pantalla → Color de acento). Abyss's own colour, independent of Windows; the light theme darkens it as needed for contrast. |
| `client_id`, `client_secret` | TV client | OAuth client of the session. See [LEGAL.md](LEGAL.md). |

## Environment variables

| Variable | Effect |
| --- | --- |
| `ABYSS_LASTFM_API_KEY`, `ABYSS_LASTFM_API_SECRET` | **Build time**: Last.fm application credentials compiled into the binary. Without them, Connect says Last.fm is not available in this build. |
| `RUST_LOG` | Log filter, default `Abyss=info,tide=info` (e.g. `Abyss=debug,tide=debug`) |
| `ABYSS_DISCORD_ID` | Use the owner’s Abyss Discord application ID; `TIDE_DISCORD_ID` remains a fallback |
| `TIDE_ZOOM` | Override UI zoom (e.g. `1.25`) |
| `TIDE_PAGE` | Development: open on a page (`home`, `explore`, `genre`, `tracks`, `albums`, `albums-open`, `artists`, `playlists`, `mixes`, `settings`, `menu`, `quicksearch`, `playlist`, `album`, `nowplaying`, `nowplaying-lyrics`, `nowplaying-credits`, `nowplaying-queue`) |
| `TIDE_DEMO_PLAY`, `TIDE_DEMO_TRACK` | Development: start silent playback on Now Playing pages |
| `TIDE_TILT=x,y` | Development: pin the cover light (pointer position, 0..1) for screenshots |
| `TIDE_ICON_SHEET=1` | Development: show every icon at 14, 18 and 24 px over the app |
| `TIDE_LYRICS_UNFOLLOW=1` | Development: leave lyrics following after load (shows the floating "Sincronizar") |
| `TIDE_SHOW_KNOBS=1` | Development: show slider knobs without hovering |
| `TIDE_PERF` | Development: log frames per second, render time, busy animations and image cache usage (every 5 s) |
| `ABYSS_CONFIG_DIR`, `ABYSS_CACHE_DIR` (legacy `TIDE_CONFIG_DIR`, `TIDE_CACHE_DIR`) | Use another folder for settings/ratings/recents and for caches (isolated tests, side-by-side runs) |
| `TIDE_DEMO_NP_CLOSED=1` | Development: demo playback without opening Now Playing |
| `TIDE_DEMO_IDS=1,2,3` | Development: with a `nowplaying*` page, play these track ids instead of the favourites |
| `TIDE_PROBE_EXPIRE=1` | Development: `--probe` breaks every link's signature first, to test link renewal against the CDN |

## Command line

```
Abyss --probe [track id | search text]
```

Prints what TIDAL offers for a track (codec, rate, depth, ReplayGain,
credits, lyrics, home rows) and runs a download/decode/seek/resample test.
Signed URLs are replaced by `<url>` in the manifest dump. See
[AUDIO.md](AUDIO.md#checking-the-pipeline).

## Cargo features

| Feature | Default | Effect |
| --- | --- | --- |
| `kanji-romaji` | on | Embeds the IPADIC dictionary (~46 MiB of the 71 MiB binary) for kanji readings. Without it, kana are still romanized and kanji pass through unchanged. |
