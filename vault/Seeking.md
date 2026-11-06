---
tags: [tide, tech, audio]
---
# Seeking

**Problem (fixed):** Hi-Res streams are fragmented MP4 over DASH; symphonia
cannot seek them while downloading, so a seek decoded from the start and
waited for the download to reach the target ("no carga").

**Fix:** the DASH manifest gives each segment's duration (`dash.rs`,
`seg_secs`). A seek restarts the download at the right segment
(`Buffer::restart_at`, init segment first), opens a new decoder and drops the
few samples before the target (`audio::do_seek`). Segments download with 4
parallel connections, appended in order. Measured: jump to 150 s in ~0.43 s.

While seeking, the player bar keeps showing the target position, duration and
format (`seek_hint` in the engine) instead of blanking.

Progressive files (AAC) still use the demuxer's seek.

Related: [[Audio Pipeline]].
