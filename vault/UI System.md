---
tags: [tide, ui]
---
# UI System

- Metrics in `ui/theme.rs`: rail 64, sidebar 220, player 68, caption 30,
  header 46, content max 1120, card 176.
- Zoom: `sc(v)`; steps 50-200 %; Ctrl +/−/0, Ctrl+wheel.
- Navigation: history with back/forward (mouse buttons, Alt+arrows).
- Motion: `fx.rs`, `motion.rs`, `smooth.rs`.
- Method: screenshot the real TIDAL and TIDE side by side, then adjust.
  Close the real app afterwards; never inject input into other windows.

Open gaps: [[Roadmap]]. Tests: [[Testing Workflow]]. Back to [[Home]].
