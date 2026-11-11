# Lyrics selection and motion

The chain is evaluated when lyrics are opened or a visible lyrics view changes
track. A provider with synchronized lines wins immediately. Plain text is kept
as a fallback while later providers are checked; the first plain result wins
only if no synchronized result is found.

| Track metadata | Provider order |
| --- | --- |
| Local file | Embedded tags, sibling `<audio-stem>.lrc`, then the language chain below without TIDAL |
| Han, kana or Hangul in title/artist | NetEase, LRCLIB, TIDAL, lyrics.ovh |
| Other scripts or ambiguous language | LRCLIB, TIDAL, NetEase, lyrics.ovh |

Service metadata has no dependable song-language field. Script detection in the
title and artist is a heuristic, independent of the interface language. Latin
transliterations of CJK names use the general chain. TIDAL is skipped for local
IDs. NetEase results must match the normalized title, artist and duration within
three seconds. LRCLIB first uses exact metadata, then a search constrained by
title, artist and a three-second duration tolerance.

Each provider gets **8 seconds total**, including search and lyric retrieval,
not eight seconds per HTTP request. HTTP calls consume the remaining deadline
with a small return margin. A separate receive deadline also bounds waiting on
local I/O. A stalled filesystem read can finish later on its worker, but its
late result is ignored. At most two local reads can remain in flight; further
local reads are skipped while those slots are occupied. There are no automatic
retry loops in this chain. The
TIDAL lyrics request uses the current token and the same eight-second bound;
normal authenticated requests still handle session refresh independently.
Worst-case sequential waiting can reach 40 seconds for local music or 32 seconds
for service music. Local providers usually finish immediately or fail quickly.

Embedded lyrics use Lofty's USLT/UnsyncLyrics and Vorbis/MP4 Lyrics mappings.
Sibling LRC files accept UTF-8 (with optional BOM) and BOM-marked UTF-16, up to
1 MiB. Multiple timestamps per line and `[offset:milliseconds]` are supported;
timestamps are validated and sorted. Files are read without modifying them.

Settings > Library and lyrics > external providers controls **all** LRCLIB,
NetEase and lyrics.ovh calls, including local music. With it off, local music
uses embedded/LRC lyrics and service tracks use only TIDAL. Changing the setting
clears displayed provider results, discards older background completions and
stops the chain from starting further external providers. An already-started
request may finish within its original deadline. The setting defaults remain
unchanged. Remote lookups disclose title, artist and sometimes album/duration;
see [privacy](PRIVACY.md).

Missing results, including temporary network failures, are remembered for
15 minutes in `lyrics-missing.json`, capped at 2048 hashes. Keys include track
metadata, external-provider mode and local audio/LRC modification information.
No plaintext local paths or titles are stored in this cache. Expiry permits a
new lookup when the view is reopened or the track is selected again. Search
again clears the current miss immediately, including its persisted copy.
Metadata edits invalidate the displayed results. A plain result is not a miss.

Both sidebar and fullscreen select the active line 150 ms early, interpolate
its emphasis over 340 ms and blend toward the next measured layout position
during the last 350 ms before its activation. Scroll position follows with
time-based exponential interpolation, including wrapped lines. Clicking a line
seeks and resumes following; manual scrolling pauses it. The Windows reduced
animation preference disables lyric interpolation and glow transitions.

Sources: [LRCLIB API](https://lrclib.net/docs/api.md),
[Lofty source](https://github.com/Serial-ATA/lofty-rs/tree/0.25.5).
The existing free NetEase endpoint is undocumented and may change; there are
no fabricated credentials or keys. Live provider coverage, real audio/LRC
files, seeks and motion/layout in six languages still require owner review.
No automated tests or real-file editing sessions were run in phase B.
