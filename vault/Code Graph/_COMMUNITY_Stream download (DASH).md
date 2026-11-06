---
type: community
members: 38
---

# Stream download (DASH)

**Members:** 38 nodes

## Members
- [[dot-append()]] - code - src/audio/stream.rs
- [[dot-begin()]] - code - src/audio/stream.rs
- [[dot-byte_len()]] - code - src/audio/stream.rs
- [[dot-can_jump()]] - code - src/audio/stream.rs
- [[dot-cancel()]] - code - src/audio/stream.rs
- [[dot-downloaded()]] - code - src/audio/stream.rs
- [[dot-finish()_1]] - code - src/audio/stream.rs
- [[dot-is_cancelled()]] - code - src/audio/stream.rs
- [[dot-is_seekable()]] - code - src/audio/stream.rs
- [[dot-live()]] - code - src/audio/stream.rs
- [[dot-new()_9]] - code - src/audio/stream.rs
- [[dot-read()]] - code - src/audio/stream.rs
- [[dot-restart_at()]] - code - src/audio/stream.rs
- [[dot-seek()_3]] - code - src/audio/stream.rs
- [[dot-segment_for()]] - code - src/audio/stream.rs
- [[dot-total_len()]] - code - src/audio/stream.rs
- [[AtomicBool_1]] - code
- [[AtomicUsize]] - code
- [[Buffer]] - code - src/audio/stream.rs
- [[Client_3]] - code
- [[Condvar]] - code
- [[MediaSource]] - code
- [[Mutex_3]] - code
- [[PARALLEL_SEGMENTS]] - code - src/audio/stream.rs
- [[Read]] - code
- [[Reader]] - code - src/audio/stream.rs
- [[Seek]] - code - src/audio/mod.rs
- [[SeekFrom]] - code
- [[Segments]] - code - src/tidal/models.rs
- [[Single]] - code - src/tidal/models.rs
- [[State]] - code - src/audio/stream.rs
- [[StreamSource]] - code - src/tidal/models.rs
- [[fetch()]] - code - src/audio/stream.rs
- [[get_bytes()]] - code - src/audio/stream.rs
- [[io]] - concept
- [[segment_worker()]] - code - src/audio/stream.rs
- [[start()]] - code - src/audio/stream.rs
- [[stream.rs]] - code - src/audio/stream.rs

## Live Query (requires Dataview plugin)

```dataview
TABLE source_file, type FROM #community/Stream_download_DASH
SORT file.name ASC
```

## Connections to other communities
- 6 edges to [[_COMMUNITY_Player engine API]]
- 4 edges to [[_COMMUNITY_Decoder & resampler]]
- 1 edge to [[_COMMUNITY_Player commands]]
- 1 edge to [[_COMMUNITY_Engine tracks]]
- 1 edge to [[_COMMUNITY_Decode worker]]
- 1 edge to [[_COMMUNITY_TIDAL models]]
- 1 edge to [[_COMMUNITY_Discord presence]]
- 1 edge to [[_COMMUNITY_Motion & easing]]

## Top bridge nodes
- [[Buffer]] - degree 31, connects to 3 communities
- [[StreamSource]] - degree 8, connects to 3 communities
- [[stream.rs]] - degree 21, connects to 2 communities
- [[Reader]] - degree 11, connects to 1 community
- [[Read]] - degree 3, connects to 1 community