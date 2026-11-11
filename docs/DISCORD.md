# Discord Rich Presence

Abyss shows what you are listening to on your Discord profile. The "Listening to" header shows the artist (not the app name). The card also shows
the track title, artist, album cover (album name on hover), a progress bar and
an *Escuchar en TIDAL* button. The card is hidden while paused.

## Setup
Abyss ships with its own Discord application ID, so Rich Presence works as soon as
Discord is open and "Display current activity" is enabled in its settings. To use
your own application, enter its Application ID in Settings > Privacy, set
`discord_client_id` in `settings.json`, or use `ABYSS_DISCORD_ID`. The legacy
`TIDE_DISCORD_ID` is accepted as a fallback.

## Code
`src/discord.rs`: IPC framing, reconnect loop, activity (`type: 2`).
