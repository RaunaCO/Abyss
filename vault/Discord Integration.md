---
tags: [tide, feature]
---
# Discord Integration

"Listening to TIDE" card like Spotify's: title, artist, cover (album name on
hover), progress bar, button "Escuchar en TIDAL". Hidden while paused.

- Talks to the local Discord client over `\\.\pipe\discord-ipc-N` (activity
  `type: 2`), reconnects every few seconds, rate-limited.
- Application **TIDE** registered once in the owner's Discord developer
  portal; its public Application ID is built in (`config::DISCORD_APP_ID`), so
  users need no setup. Override: `TIDE_DISCORD_ID` or `discord_client_id`.
- Switch in Settings → Conectar.

Code: `src/discord.rs`. Public doc: `docs/DISCORD.md`.
