# Privacy

Abyss has **no telemetry, no analytics, no crash upload and no server of its
own**. Everything below is either stored on your computer or sent to a
service as a direct consequence of something you use.

## Network destinations

| Destination | When | What is sent |
| --- | --- | --- |
| `auth.tidal.com`, `login.tidal.com` | Signing in, refreshing the session | OAuth client ID/secret, device code or authorisation code, refresh token |
| `api.tidal.com` (`/v1`, `/v2`) | Browsing, searching, playing, editing the library | Access token, your user ID, country code, search text, track/album/playlist IDs, library changes |
| TIDAL's audio CDN (hosts come from the stream manifest) | Playing | Requests for the signed stream / segment URLs |
| `resources.tidal.com` | Showing artwork | Image IDs (no token) |
| `lrclib.net` | Lyrics chain, only while external providers are enabled, including local music | Track title, first artist, album, duration |
| `music.163.com` (NetEase, China) | External lyrics enabled; before LRCLIB for CJK metadata, otherwise after TIDAL | Track title and first artist |
| `api.lyrics.ovh` | Same condition, last fallback | First artist and track title |
| Discord desktop app (local pipe `\\.\pipe\discord-ipc-N`) | While playing, if the Discord setting is on and Discord is running | Title, artist, album, artwork URL, start/end time, a `tidal.com/browse/track/<id>` link |

Every request carries the User-Agent `Abyss/<version>
(https://github.com/RaunaCO/TIDE)` and your IP address, as any HTTP request
does. Nothing is sent to the Abyss project.

Opening links from the menu (*Buscar actualizaciones*, *Soporte*, TIDAL
account and terms pages) hands the URL to your default browser.

## Disclosures to review

- **Discord Rich Presence is on by default** (`discord: true`). While Discord
  runs, your friends and servers can see what you are listening to. It is
  switched off in Settings → Discord. Whether it should be off until enabled
  is an open product decision (see the audit report).
- **Third-party lyrics lookups are on by default** (`external_lyrics: true`).
  They tell LRCLIB, NetEase and lyrics.ovh which tracks you play, including
  local music, in the order described in [lyrics](LYRICS.md). Switch off in Settings → "Letras de otros proveedores".

Local tag edits keep a one-time full original under `local-originals` for
restoration. `lyrics-missing.json` keeps unsuccessful lookup hashes for 15 minutes.
See [local files](LOCAL-FILES.md) and [lyrics](LYRICS.md).

## Data stored locally

| Path | Content | Sensitivity |
| --- | --- | --- |
| `%APPDATA%\abyss\settings.json` | Session tokens (DPAPI-encrypted on Windows), user ID, user name, picture URL, country, preferences, the last 12 searches | **Sensitive**: account access, search history |
| `%APPDATA%\abyss\listening.sqlite3` | Replay timestamps and seconds listened, track metadata and your star ratings. Stored locally | Listening history and ratings |
| `%APPDATA%\abyss\recent.json` | Albums played in Abyss (home shortcuts) | Listening history |
| `%APPDATA%\abyss\history.json`, `ratings.json` | Legacy Replay and ratings files kept after the one-time import | Listening history and ratings |
| `%APPDATA%\abyss\crash.log` | Last panic message and backtrace (only after a crash) | Local paths |
| `%LOCALAPPDATA%\abyss\images\` | Cached artwork, trimmed to ~512 MB | Reveals what you browsed |

Signing out removes the tokens and your profile fields. Ratings, recents,
searches and the image cache stay until you delete them. *Ayuda → Solución de
problemas* opens both folders. Deleting `%APPDATA%\abyss` and
`%LOCALAPPDATA%\abyss` removes everything Abyss stores.

Audio is never written to disk.

To clear Replay and ratings, close Abyss and remove `listening.sqlite3` together
with the two legacy JSON files if present. A fresh database imports any legacy
files it finds.
