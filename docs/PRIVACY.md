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
| `ws.audioscrobbler.com` (Last.fm) | Only after you connect a Last.fm account and while scrobbling is on; the history sync is manual | Your Last.fm session key, and for each listen the artist, title, album, length and start time. "Now playing" is sent when a track starts. Local music is included. |
| `api.github.com` | Only when you click *Check for updates* in Settings > About | Nothing but the request itself (User-Agent and IP address) |
| Discord desktop app (local pipe `\\.\pipe\discord-ipc-N`) | While playing, if the Discord setting is on and Discord is running | Title, artist, album, artwork URL, start/end time, a `tidal.com/browse/track/<id>` link |

Every request carries the User-Agent `Abyss/<version>
(https://github.com/RaunaCO/Abyss)` and your IP address, as any HTTP request
does. Nothing is sent to the Abyss project.

Opening links from the menu (*Buscar actualizaciones*, *Soporte*, TIDAL
account and terms pages) hands the URL to your default browser.

## Disclosures to review

- **Discord Rich Presence is off until you turn it on** (`discord: false`). Once
  on, while Discord runs, your friends and servers can see what you are listening
  to. Switch it in Settings → Privacy.
- **Last.fm is off until you connect an account** (`lastfm_enabled: false`). Once
  connected, every track you play, from TIDAL or local files, is sent to your Last.fm
  profile after half of it (or 4 minutes) has been heard. Last.fm shows it on your
  profile according to your Last.fm privacy settings. *Sync history* sends the
  saved Replay history (TIDAL listens only, Replay does not record local files).
  Disconnect in Settings → Integrations.
- **Third-party lyrics lookups are off until you turn them on** (`external_lyrics: false`).
  Once on, they tell LRCLIB, NetEase and lyrics.ovh which tracks you play, including
  local music, in the order described in [lyrics](LYRICS.md). Switch them in Settings →
  Library and lyrics, or with *Turn on other providers* when a track has no lyrics.
- **Upgrading from 1.4.0 or earlier**: both used to be on by default. The first launch
  of 1.5.0 turns both off once (`privacy_defaults` in `settings.json`) and says so in a
  notice; turning them back on afterwards is kept.

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
| `%APPDATA%\abyss\queue.json` | The play queue and position | What you were listening to |

In portable mode (a `data` folder next to `Abyss.exe`) these files live in
`data\config` and `data\cache` instead. *Export my data* (Settings > About) writes
your listening history and ratings as CSV, and your settings without the session,
the Last.fm key or the e-mail, to a new folder in Documents. Uninstalling asks
whether to delete the data folders too (No by default).

Signing out removes the tokens and your profile fields. Ratings, recents,
searches and the image cache stay until you delete them. *Ayuda → Solución de
problemas* opens both folders. Deleting `%APPDATA%\abyss` and
`%LOCALAPPDATA%\abyss` removes everything Abyss stores.

Audio is never written to disk.

To clear Replay and ratings, close Abyss and remove `listening.sqlite3` together
with the two legacy JSON files if present. A fresh database imports any legacy
files it finds.
