# Discord Rich Presence

TIDE shows what you are listening to on your Discord profile, the same card
Spotify uses ("Listening to ..."): track title, artist, album cover (album name
on hover), a progress bar and an *Escuchar en TIDAL* button. The card is hidden
while paused, like Spotify.

## Setup
None: TIDE ships with the Application ID of its own Discord application (named
**TIDE**, which is what Discord shows after "Listening to"). Use the switch in
Settings -> Discord to turn it off. Forks can use their own application via the
`TIDE_DISCORD_ID` environment variable or `discord_client_id` in `settings.json`.

**Privacy:** the card is visible to your Discord contacts and servers. It is
on by default. Turn it off in Settings → Discord. Any local program could open
a pipe with Discord's name before Discord does and receive the same activity
data. That is a limitation of Discord's IPC design.

Discord desktop must be running. TIDE talks to it only through its local IPC
pipe (`\.\pipe\discord-ipc-N`); nothing else is sent. Cover art is passed to
Discord as the TIDAL image URL. Updates happen on track change, pause/resume
and seeking (Discord allows about 5 updates per 20 s).

## Code
`src/discord.rs`: IPC framing, reconnect loop, activity (`type: 2`).
