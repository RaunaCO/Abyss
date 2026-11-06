# Code graph

The repository ships a code graph built with
[graphify](https://pypi.org/project/graphifyy/) (`pip install graphifyy`).
Code is parsed with tree-sitter, no LLM or API key needed.

| Output | What |
| --- | --- |
| `graphify-out/graph.html` | Interactive graph, open in a browser |
| `graphify-out/GRAPH_REPORT.md` | Hubs, communities, cycles, surprising links |
| `graphify-out/graph.json` | Raw graph for queries |
| `vault/Code Graph/` | One Obsidian note per symbol + `graph.canvas` |

## Update after changing code
```bash
python -m graphify update .
python -m graphify export obsidian --graph graphify-out/graph.json --labels graphify-out/.graphify_labels.json --dir "vault/Code Graph"
python -m graphify export html --graph graphify-out/graph.json --labels graphify-out/.graphify_labels.json
```

## Query
```bash
python -m graphify query "where is the Now Playing background drawn?"
python -m graphify explain "Buffer"
python -m graphify path "AppView" "Engine"
python -m graphify god-nodes
```
Community names are kept in `graphify-out/.graphify_labels.json`.
