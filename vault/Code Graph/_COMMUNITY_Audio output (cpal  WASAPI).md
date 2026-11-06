---
type: community
members: 48
---

# Audio output (cpal / WASAPI)

**Members:** 48 nodes

## Members
- [[dot-analyse()]] - code - src/audio/output.rs
- [[dot-decay_levels()]] - code - src/audio/output.rs
- [[dot-drop()_2]] - code - src/audio/output.rs
- [[dot-gain()]] - code - src/audio/output.rs
- [[dot-levels()_1]] - code - src/audio/output.rs
- [[dot-lyrics()]] - code - src/tidal/mod.rs
- [[dot-lyrics_any()]] - code - src/tidal/lyrics.rs
- [[dot-set_gain()]] - code - src/audio/output.rs
- [[AtomicBool]] - code
- [[AtomicU32]] - code
- [[AtomicU64]] - code
- [[Consumer]] - code
- [[Cpal]] - code - src/audio/output.rs
- [[Device]] - code
- [[Exclusive]] - code - src/audio/output.rs
- [[ExclusiveHandle]] - code - src/audio/output.rs
- [[JoinHandle]] - code
- [[Keep]] - code - src/audio/output.rs
- [[LRCLIB]] - code - src/tidal/lyrics.rs
- [[Lyrics]] - code - src/tidal/mod.rs
- [[Mpd]] - code - src/tidal/dash.rs
- [[OutShared]] - code - src/audio/output.rs
- [[Output]] - code - src/audio/output.rs
- [[Producer]] - code
- [[Stream]] - code
- [[Tidal_1]] - code - src/tidal/lyrics.rs
- [[Track_8]] - code
- [[Value_2]] - code
- [[anyhow]] - concept
- [[dash.rs]] - code - src/tidal/dash.rs
- [[device_names()]] - code - src/audio/output.rs
- [[devicetrait]] - concept
- [[fill()]] - code - src/audio/output.rs
- [[find_device()]] - code - src/audio/output.rs
- [[from_lrclib()]] - code - src/tidal/lyrics.rs
- [[hosttrait]] - concept
- [[lrclib()]] - code - src/tidal/lyrics.rs
- [[lyrics.rs]] - code - src/tidal/lyrics.rs
- [[open_exclusive()]] - code - src/audio/output.rs
- [[open_shared()]] - code - src/audio/output.rs
- [[output.rs]] - code - src/audio/output.rs
- [[parse()]] - code - src/tidal/dash.rs
- [[parse_lrc()]] - code - src/tidal/mod.rs
- [[parses_timeline()]] - code - src/tidal/dash.rs
- [[result]] - concept
- [[ring()]] - code - src/audio/output.rs
- [[streamtrait]] - concept
- [[write_sample()]] - code - src/audio/output.rs

## Live Query (requires Dataview plugin)

```dataview
TABLE source_file, type FROM #community/Audio_output_cpal_/_WASAPI
SORT file.name ASC
```

## Connections to other communities
- 6 edges to [[_COMMUNITY_Player engine API]]
- 5 edges to [[_COMMUNITY_TIDAL client & auth]]
- 4 edges to [[_COMMUNITY_Community 4]]
- 3 edges to [[_COMMUNITY_Community 36]]
- 2 edges to [[_COMMUNITY_Decoder & resampler]]
- 1 edge to [[_COMMUNITY_AppView core]]
- 1 edge to [[_COMMUNITY_Text input]]
- 1 edge to [[_COMMUNITY_Pages & overlays (ui)]]
- 1 edge to [[_COMMUNITY_Motion & easing]]
- 1 edge to [[_COMMUNITY_Win32 window control]]
- 1 edge to [[_COMMUNITY_Key bindings & zoom]]

## Top bridge nodes
- [[output.rs]] - degree 23, connects to 3 communities
- [[result]] - degree 6, connects to 3 communities
- [[OutShared]] - degree 16, connects to 2 communities
- [[lyrics.rs]] - degree 11, connects to 2 communities
- [[Output]] - degree 8, connects to 2 communities