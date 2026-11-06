---
type: community
members: 8
---

# Decode worker

**Members:** 8 nodes

## Members
- [[Chunk]] - code - src/audio/mod.rs
- [[DecMsg]] - code - src/audio/mod.rs
- [[End]] - code - src/audio/mod.rs
- [[Error_1]] - code - src/audio/mod.rs
- [[Format]] - code - src/audio/mod.rs
- [[Seeked]] - code - src/audio/mod.rs
- [[SyncSender]] - code
- [[decode_loop()]] - code - src/audio/mod.rs

## Live Query (requires Dataview plugin)

```dataview
TABLE source_file, type FROM #community/Decode_worker
SORT file.name ASC
```

## Connections to other communities
- 5 edges to [[_COMMUNITY_Player engine API]]
- 2 edges to [[_COMMUNITY_Community 36]]
- 1 edge to [[_COMMUNITY_Stream download (DASH)]]
- 1 edge to [[_COMMUNITY_Text input]]
- 1 edge to [[_COMMUNITY_Decoder & resampler]]
- 1 edge to [[_COMMUNITY_Engine tracks]]

## Top bridge nodes
- [[decode_loop()]] - degree 10, connects to 5 communities
- [[DecMsg]] - degree 8, connects to 1 community
- [[Chunk]] - degree 3, connects to 1 community
- [[End]] - degree 2, connects to 1 community
- [[SyncSender]] - degree 2, connects to 1 community