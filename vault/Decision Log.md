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

Back to [[Home]].
