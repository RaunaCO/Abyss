---
type: community
members: 31
---

# Decoder & resampler

**Members:** 31 nodes

## Members
- [[dot-finish()]] - code - src/audio/decode.rs
- [[dot-new()_2]] - code - src/audio/decode.rs
- [[dot-next_chunk()]] - code - src/audio/decode.rs
- [[dot-open()]] - code - src/audio/decode.rs
- [[dot-process()]] - code - src/audio/decode.rs
- [[dot-seek()]] - code - src/audio/decode.rs
- [[dot-trim()]] - code - src/audio/decode.rs
- [[Decoder]] - code - src/audio/decode.rs
- [[FormatReader]] - code
- [[Resampler2]] - code - src/audio/decode.rs
- [[SampleBuffer]] - code
- [[SincFixedIn]] - code
- [[codec_type_null]] - concept
- [[decode.rs]] - code - src/audio/decode.rs
- [[decoder_as]] - concept
- [[decoderoptions]] - concept
- [[do_seek()]] - code - src/audio/mod.rs
- [[error_as_symerror]] - concept
- [[formatoptions]] - concept
- [[hint]] - concept
- [[interleave()]] - code - src/audio/decode.rs
- [[mediasourcestream]] - concept
- [[mediasourcestreamoptions]] - concept
- [[metadataoptions]] - concept
- [[resampler]] - concept
- [[seekmode]] - concept
- [[seekto_1]] - concept
- [[sincinterpolationparameters]] - concept
- [[sincinterpolationtype]] - concept
- [[time]] - concept
- [[windowfunction]] - concept

## Live Query (requires Dataview plugin)

```dataview
TABLE source_file, type FROM #community/Decoder__resampler
SORT file.name ASC
```

## Connections to other communities
- 4 edges to [[_COMMUNITY_Player engine API]]
- 4 edges to [[_COMMUNITY_Stream download (DASH)]]
- 2 edges to [[_COMMUNITY_Audio output (cpal  WASAPI)]]
- 1 edge to [[_COMMUNITY_Engine tracks]]
- 1 edge to [[_COMMUNITY_TIDAL client & auth]]
- 1 edge to [[_COMMUNITY_Decode worker]]
- 1 edge to [[_COMMUNITY_Community 36]]

## Top bridge nodes
- [[decode.rs]] - degree 28, connects to 4 communities
- [[do_seek()]] - degree 5, connects to 4 communities
- [[Resampler2]] - degree 8, connects to 2 communities
- [[Decoder]] - degree 8, connects to 1 community
- [[dot-open()]] - degree 2, connects to 1 community