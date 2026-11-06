---
type: community
members: 18
---

# Queue & playback

**Members:** 18 nodes

## Members
- [[dot-add_to_queue()]] - code - src/ui/mod.rs
- [[dot-autoplay_more()]] - code - src/ui/mod.rs
- [[dot-new_uid()]] - code - src/ui/mod.rs
- [[dot-next()]] - code - src/ui/mod.rs
- [[dot-next_index()]] - code - src/ui/mod.rs
- [[dot-notify_toast()]] - code - src/ui/mod.rs
- [[dot-play_index()]] - code - src/ui/mod.rs
- [[dot-play_shuffled()]] - code - src/ui/mod.rs
- [[dot-play_tracks()]] - code - src/ui/mod.rs
- [[dot-queue_next()]] - code - src/ui/mod.rs
- [[dot-skip_after_error()]] - code - src/ui/mod.rs
- [[dot-start_radio()]] - code - src/ui/mod.rs
- [[dot-toggle_favorite_album()]] - code - src/ui/mod.rs
- [[dot-wants_lyrics()]] - code - src/ui/now_playing.rs
- [[NowPlaying]] - code - src/ui/mod.rs
- [[Track_9]] - code
- [[now_secs()]] - code - src/ui/mod.rs
- [[shuffle()]] - code - src/ui/mod.rs

## Live Query (requires Dataview plugin)

```dataview
TABLE source_file, type FROM #community/Queue__playback
SORT file.name ASC
```

## Connections to other communities
- 18 edges to [[_COMMUNITY_AppView core]]
- 15 edges to [[_COMMUNITY_Community 34]]
- 9 edges to [[_COMMUNITY_Async loading]]
- 9 edges to [[_COMMUNITY_Community 30]]
- 3 edges to [[_COMMUNITY_Key bindings & zoom]]
- 1 edge to [[_COMMUNITY_Pages & overlays (ui)]]
- 1 edge to [[_COMMUNITY_Catalogue items]]
- 1 edge to [[_COMMUNITY_Engine tracks]]
- 1 edge to [[_COMMUNITY_Community 48]]
- 1 edge to [[_COMMUNITY_Now Playing]]
- 1 edge to [[_COMMUNITY_Playlist editing]]

## Top bridge nodes
- [[dot-notify_toast()]] - degree 12, connects to 5 communities
- [[dot-play_index()]] - degree 15, connects to 4 communities
- [[dot-queue_next()]] - degree 6, connects to 4 communities
- [[dot-toggle_favorite_album()]] - degree 5, connects to 4 communities
- [[dot-play_tracks()]] - degree 10, connects to 3 communities