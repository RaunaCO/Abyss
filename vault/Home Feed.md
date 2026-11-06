---
tags: [tide, feature, api]
---
# Home Feed ("Para ti")

Same source as the official app: `GET https://api.tidal.com/v2/home/feed/{vibe}`
with `vibe` = `static` (Para ti), `editorial` (Staff Picks) or `uploads`.

Params: `locale` (es_ES), `deviceType=DESKTOP`, `platform=DESKTOP`,
`countryCode`, `timeOffset`, `limit`. Headers it needs:
`x-tidal-client-version`, `x-tidal-authenticated-userid`,
`x-tidal-authenticated-subscription`. Code: `Tidal::home_feed`.

Modules: `SHORTCUT_LIST` (6 quick links), `HORIZONTAL_LIST`,
`HORIZONTAL_LIST_WITH_CONTEXT` ("Because you listened to …", header item),
`TRACK_LIST`. Items: ALBUM, PLAYLIST, MIX (v2 shape: `titleTextInfo`,
`mixImages`), ARTIST, TRACK, DEEP_LINK (`tidal://my-collection/tracks`).

## Recents
TIDAL only updates its shortcut list from the official app's playback reports,
so TIDE keeps its own: albums played here (`src/recents.rs`,
`%APPDATA%\tide\recent.json`) come first in **Escuchado recientemente**, TIDAL's
shortcuts fill the rest. The grid sits below the first two recommendation rows.
The feed refreshes quietly when you return to Home after 90 s.

Related: [[TIDAL API Notes]].
