# Discord Rich Presence

Abyss shows what you are listening to on your Discord profile. The card shows
the track title, artist, album cover (album name on hover), a progress bar and
an *Escuchar en TIDAL* button. The card is hidden while paused.

## Setup
Abyss does not ship a verified Discord application ID under its new name.
Activity publishing stays inactive until the owner supplies one. Enter the
Application ID in Settings > Privacy, set `discord_client_id` in
`settings.json`, or use `ABYSS_DISCORD_ID`. The legacy `TIDE_DISCORD_ID`
is accepted as a fallback. The owner must register or rename their Discord
application to **Abyss**; the client cannot rename it through local IPC.

**Privacy:** when configured and enabled, the card is visible to contacts and
servers. The saved Discord switch is preserved during migration. Disable it
in Settings > Privacy to hide activity. Any local program could impersonate
the IPC pipe and receive activity data, a limitation of Discord's IPC design.

Discord desktop must be running. Abyss talks to it only through its local IPC
pipe (`\.\pipe\discord-ipc-N`); nothing else is sent. Cover art is passed to
Discord as the TIDAL image URL. Updates happen on track change, pause/resume
and seeking (Discord allows about 5 updates per 20 s).

## Code
`src/discord.rs`: IPC framing, reconnect loop, activity (`type: 2`).
