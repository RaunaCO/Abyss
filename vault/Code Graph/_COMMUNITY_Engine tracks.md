---
type: community
members: 7
---

# Engine tracks

**Members:** 7 nodes

## Members
- [[dot-detail()]] - code - src/tidal/models.rs
- [[dot-make_track()]] - code - src/audio/mod.rs
- [[dot-norm_gain()]] - code - src/audio/mod.rs
- [[dot-tier()]] - code - src/tidal/models.rs
- [[StreamInfo]] - code - src/tidal/models.rs
- [[Track_1]] - code - src/audio/mod.rs
- [[spawn_worker()]] - code - src/audio/mod.rs

## Live Query (requires Dataview plugin)

```dataview
TABLE source_file, type FROM #community/Engine_tracks
SORT file.name ASC
```

## Connections to other communities
- 6 edges to [[_COMMUNITY_Player engine API]]
- 5 edges to [[_COMMUNITY_Community 36]]
- 2 edges to [[_COMMUNITY_Community 4]]
- 1 edge to [[_COMMUNITY_Decoder & resampler]]
- 1 edge to [[_COMMUNITY_AppView core]]
- 1 edge to [[_COMMUNITY_Queue & playback]]
- 1 edge to [[_COMMUNITY_Player commands]]
- 1 edge to [[_COMMUNITY_Stream download (DASH)]]
- 1 edge to [[_COMMUNITY_TIDAL models]]
- 1 edge to [[_COMMUNITY_Decode worker]]

## Top bridge nodes
- [[StreamInfo]] - degree 14, connects to 7 communities
- [[Track_1]] - degree 7, connects to 3 communities
- [[spawn_worker()]] - degree 6, connects to 2 communities
- [[dot-make_track()]] - degree 6, connects to 1 community
- [[dot-norm_gain()]] - degree 3, connects to 1 community