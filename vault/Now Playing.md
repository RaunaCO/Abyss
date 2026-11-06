---
tags: [tide, feature, ui]
---
# Now Playing

Full-window view opened from the player bar (or `toggle_now_playing`). Code:
`src/ui/now_playing.rs`, background in `src/ui/fx.rs` and `src/ui/images.rs`.

## Rotating background (copied from the official app)
Found in TIDAL's own bundle (`rotator` component, cached web app):
- The cover, **blurred 28 px and saturated 1.3**, scaled to about **2× the
  window's larger side** (`40vmax × 4 × 1.26`), turns about its centre.
- **Playing:** 360° per 60 s. **Pause:** coasts +30° over 1.5 s (ease-out
  cubic) and stops. **Dragging the progress bar:** angle follows it
  (180° across the track). New track: cross-fade 0.65 s.
- Scrim on top: black gradient from 0.34 (top) to 0.76 (bottom).

GPUI cannot rotate images, so TIDE pre-renders **180 frames** (one every 2°,
192 px, blur σ 7.5 on a 128 px source, light dithering against banding) and
cross-fades the two neighbours of the current angle (`fx::rotating_backdrop`).
State machine: `fx::BgSpin` (Stopped / Spinning / Coasting / Scrub).
On top: two slow light sweeps in the cover's palette (28 s cycle).
The player bar shows the same turning cover as a strip under a veil.

Setting "Fondo del reproductor": Desenfoque (above) or Color (flat colour).

## Lyrics
- 32 px semibold, inactive lines at 55 % opacity, fade 0.46 s with
  `cubic-bezier(.4,0,.2,1)` (all from TIDAL's CSS).
- Smooth follow: eases the scroll so the current line sits at ~32 % height,
  measured from the laid-out lines. Manual scroll stops following;
  **Sincronizar** or clicking a line resumes.
- Glow on the current line (two soft shadows in a lightened cover colour).
- Providers: TIDAL, then LRCLIB. Offset buttons ±0.5 s.

## Tabs and queue
Letra / Créditos / Similares, plus **Cola** as a toggle that docks the queue
beside any tab (lyrics and queue together).

Related: [[UI System]], [[Decision Log]].
