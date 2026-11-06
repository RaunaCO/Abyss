---
tags: [tide, process]
---
# Decision Log

| Date | Decision | Why |
| --- | --- | --- |
| 2026-11 | Build from scratch in Rust + GPUI, no forks | User requirement |
| 2026-11 | Inter instead of SF | Apple license forbids SF on Windows |
| 2026-11 | Keep device-code login | PKCE web login failed; FLAC for Hi-Res only is accepted: [[Audio Pipeline]] |
| 2026-11 | Zoom via global scale `sc()` | One place to apply it: [[UI System]] |
| 2026-11-06 | Rename `tidal-gpui` → **TIDE**, own repo + vault | Brand identity: [[Brand Identity]] |
| 2026-11-06 | Copy UI values from TIDAL's cached CSS/JS, not by eye | Exact colours, easings, layouts: [[UI System]] |
| 2026-11-06 | Seek DASH by restarting at a segment | Instant seeks: [[Seeking]] |
| 2026-11-06 | Home from `v2/home/feed`, plus local recents | Same content as TIDAL; recents update without playback reports: [[Home Feed]] |
| 2026-11-06 | Ratings stored locally | TIDAL has no rating API: [[Ratings]] |
| 2026-11-06 | Built-in Discord Application ID | No setup for users: [[Discord Integration]] |
| 2026-11-06 | Background = TIDAL's rotating blurred cover, no audio reaction | Audio-reactive versions looked erratic; copied the real effect: [[Now Playing]] |
| 2026-11-06 | Post (not send) the window-drag message | Crash fix: [[Window Drag Crash]] |
| 2026-11-06 | Code map with graphify | Navigable architecture in Obsidian: [[Code Map]] |

Back to [[Home]].
