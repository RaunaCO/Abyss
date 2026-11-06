---
tags: [tide, tech, graph]
---
# Code Map

The code graph is built with **graphify** (no LLM needed for code): every
module, type, function and call becomes a note under `Code Graph/`, grouped in
communities (`_COMMUNITY_*` notes). Open `Code Graph/graph.canvas` for the
visual map, or `../graphify-out/graph.html` in a browser.

## Regenerate after code changes
```bash
python -m graphify update .
python -m graphify export obsidian --graph graphify-out/graph.json --labels graphify-out/.graphify_labels.json --dir "vault/Code Graph"
python -m graphify export html --graph graphify-out/graph.json --labels graphify-out/.graphify_labels.json
```
`.graphifyignore` keeps `target/`, the generated notes and the fonts out.
Community names live in `graphify-out/.graphify_labels.json`; clustering can
renumber communities after big refactors, so check the names then.

## Ask the graph
```bash
python -m graphify query "how does seeking reach the decoder?"
python -m graphify explain "AppView"
python -m graphify path "AppView" "Buffer"
python -m graphify god-nodes
```

## Main communities
| Area | Community note |
| --- | --- |
| App state | [[_COMMUNITY_AppView core]] · [[_COMMUNITY_Async loading]] · [[_COMMUNITY_Key bindings & zoom]] |
| Pages | [[_COMMUNITY_Pages & overlays (ui)]] · [[_COMMUNITY_Page blocks]] · [[_COMMUNITY_Navigation pages]] |
| Now Playing | [[_COMMUNITY_Now Playing]] · [[_COMMUNITY_Rotating background]] · [[_COMMUNITY_Visual effects]] |
| Chrome | [[_COMMUNITY_Title bar]] · [[_COMMUNITY_Sidebar]] · [[_COMMUNITY_App menu]] · [[_COMMUNITY_Settings page]] |
| Audio | [[_COMMUNITY_Player engine API]] · [[_COMMUNITY_Decode worker]] · [[_COMMUNITY_Decoder & resampler]] · [[_COMMUNITY_Stream download (DASH)]] · [[_COMMUNITY_Audio output (cpal  WASAPI)]] |
| TIDAL | [[_COMMUNITY_TIDAL client & auth]] · [[_COMMUNITY_TIDAL models]] · [[_COMMUNITY_Catalogue items]] |
| System | [[_COMMUNITY_Win32 window control]] · [[_COMMUNITY_System media controls]] · [[_COMMUNITY_Discord presence]] |

## Hubs (most connected)
`AppView` (the whole UI state), `sc()` (zoom-aware pixels), `Tidal` (API
client), `Icon`, `TextInput`, `Block`, `Buffer`, `Item`, `Page`, `Engine`.

## Known cycles
`input.rs` ↔ `ui/mod.rs`, and `audio/mod.rs` ↔ `stream.rs` ↔ `decode.rs`:
harmless (same crate), listed by graphify's report.

Back to [[Home]].
