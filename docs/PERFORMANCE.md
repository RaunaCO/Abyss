# Performance

Measurements, how they were taken, and what was changed. Numbers come from
one machine (Windows 11, AMD Radeon RX 6800 XT) and are indicative, not
benchmarks to compare across machines.

## How to measure

- `TIDE_PERF=1`: logs frames per second, average and maximum render time each
  second, plus which animation keeps frames coming.
- Start-up / CPU / memory: launch the release build on a cover-heavy page
  (`TIDE_PAGE=albums`, covers already in the disk cache), wait 15 s, then read
  the process's total CPU time and private memory (PowerShell:
  `Start-Process -PassThru`, then `TotalProcessorTime` and
  `PrivateMemorySize64`). Back up `settings.json` first (see
  [DEVELOPMENT.md](DEVELOPMENT.md#manual-testing)). Discard the first run of a
  freshly built `.exe`, because antivirus scanning inflates it.
- Audio: `Abyss --probe` reports time to first audio and seek latency.

## 0.9.9

Windows release builds, isolated profiles, the same local 24-bit / 192 kHz WAV
with artwork and the animated fullscreen background. Sampled once per second
for 20 s after an 8 s warm-up.

| Measure | 0.9.8 | 0.9.9 |
| --- | ---: | ---: |
| CPU, percent of one logical core | 14.5% | 4.8% |
| Peak resident working set | 158.3 MB | 132.5 MB |
| Peak private bytes | 220.2 MB | 175.7 MB |

Fullscreen skips the covered page. The animation scheduler targets 30 fps for
ambient motion and up to 120 fps for interactive motion. Two sets of rotating artwork samples now
cost 6 MiB of pixel data instead of about 51 MiB. The decoded and encoded
cover caches target 64 and 24 MiB respectively. The 800 MB RAM target was met in
this scenario. Streaming, very large libraries and long sessions need separate
measurements.

## 0.9.3 (2026-11-08)

Same method (20 s warm-up, 25 s measured): idle Home 0.4 % of one core and
**0 frames/s**; Now Playing paused 27.2 % at 57 frames/s (0.9.2: 29.5 %).
The new animations (sync spin, cover viewer, queue column, Now Playing
slide) only ask for frames while they move. Frame scheduling now happens at
the end of each render, so animations in the bar, panels and overlays never
stall after one frame.

## 0.9.2 vs 0.9.1 (2026-11-07)

Same method, 20 s warm-up and 30 s measured, release builds on the same
isolated configuration (0.9.1 = the installed build).

| State | 0.9.1 | 0.9.2 |
| --- | --- | --- |
| Idle (Home) | 4.1 % · 119 frames/s | **1.4 % · 0 frames/s** |
| Now Playing, paused | 26.8 % · 57 frames/s | 29.5 % · 55 frames/s |
| Now Playing, playing | 35.9 % · 77 frames/s | 30.7 % · 79 frames/s |
| Lyrics, playing | 43.5 % · 71 frames/s | 47.0 % · 68 frames/s |

Now Playing figures are within run-to-run noise (a few points). The panel
glides, cross-fades, side-panel slide and loading dots only ask for frames
while they move; idle Home draws nothing.

## CPU and frame pacing in 0.9.0 (2026-11-07)

Release build, isolated configuration, 200 Hz monitor, steady state (35 s
warm-up, 40 s measured), CPU as a share of one core, frames from `TIDE_PERF`.

| State | 0.8.0 | 0.9.0-beta.1 |
| --- | --- | --- |
| Now Playing, playing | 71 % · 200 frames/s | **30 %** · ~60 frames/s |
| Now Playing, paused | ~75 % · ~200 frames/s | **23 %** · 59 frames/s |
| Lyrics, playing | 82 % · 198 frames/s | **28 %** · 57 frames/s |
| Playing, Now Playing closed | - | **21 %** · 61 frames/s |
| Idle (Home) | 0 frames when nothing moves | unchanged |

Interactive motion keeps the full refresh rate (bursts at ~200 frames/s while
scrolling or hovering). The cap only applies when the only things moving are
ambient (`motion::AMBIENT`).

## Memory in 0.8.0 (2026-11-07)

Release build, isolated configuration (`TIDE_CONFIG_DIR`), Now Playing open,
silent playback, sampled every second. **PWS** = Private Working Set (private
RAM actually resident, the product target), **WS** = Working Set, **PB** =
Private Bytes (committed). All values are peaks in MB. Abyss runs as a single
process (no child processes).

| Scenario | 0.1.0 PWS / WS / PB | 0.8.0 PWS / WS / PB |
| --- | --- | --- |
| Idle (Home) | 120 / 165 / 230 | **76** / 133 / 167 |
| 24/96, 13 min + 19 min queued | 860 / 915 / 1282 | **117** / 170 / 226 |
| 24/192, 6 tracks continuous (14 min) | not measured | **190** / 245 / 306 |
| 24/192 transitions (logged) | - | **179** / 233 / 295 |
| AAC (quality Low) | - | **94** / 148 / 203 |
| Browsing with heavy scrolling (318 covers) | - | **127** / 173 / 270 |
| Same, image budget forced to 24 MB (28 eviction rounds) | - | **93** / 139 / 205 |

- Target: < 800 MB PWS (preferably < 650 MB) with 24/192 - met with a wide
  margin. Memory no longer depends on track length or sample rate.
- After each gapless transition PWS returns to its previous level (about 135 MB
  in the 24/192 run): no growth across tracks.
- Encoded-image eviction (32 MB budget) was not reached in testing (9 MB held
  after 318 covers); the decoded-image path was exercised with a forced budget.
- Real segment sizes match the manifest's `bandwidth` (ratio 1.00–1.01;
  24/192 ≈ 40 MB/min, largest segment 2.75 MB); a full 24/192 window is about
  50 MB.

## Results (2026-11-07)

### Image derivations made lazy

Before, every cover that loaded was decoded **four times**: the original,
plus a blurred PNG, a colour palette and a dominant colour. The last three are
only used for the playing track, page headers and one settings preview. Now
they are computed on first request (`images::derive`).

Release build, `TIDE_PAGE=albums`, 15 s after launch:

| | Window shown | CPU time | Private memory |
| --- | --- | --- | --- |
| Before (5 runs) | 189–336 ms | 2.39–3.03 s | 255–275 MB |
| After (2 warm runs) | 191–197 ms | 1.53–1.98 s | 251–253 MB |

That is about 35 % less CPU while a page of artwork loads. Memory barely
changes because the derived images were small. Startup to the first window
is unchanged; it is dominated by GPU device creation and font loading.

### Crisp images

Artwork is resampled (Lanczos3) on a background thread to the exact
device-pixel size it is drawn at, cached per size, and handed to GPUI already
decoded (`RenderImage`), so swapping it in never blanks a frame. This costs a
little CPU once per (image, size) but removes the aliasing of GPU
minification. Large images are rounded up to 32 px steps so a window resize
reuses copies.

### Token refresh

With an expired token, start-up used to issue five parallel refreshes to
`auth.tidal.com`. It now issues one (`refresh_lock`).

### Rendering

Previously measured with `TIDE_PERF`: render under 1 ms per frame on typical
pages. When idle the UI stays near one frame per second, because frames are
only requested while an animation or playback position is moving.

### Audio

From `--probe` (debug build): decoder opened in 310 ms, first audio after
385 ms, seek to 150 s in 91 ms for a 24-bit DASH stream.

## Binary size

| Build | `Abyss.exe` |
| --- | --- |
| release, default features | 71.1 MiB |
| release, `--no-default-features` | 25.4 MiB |

About 46 MiB of the default binary is the embedded IPADIC dictionary
(`dict.words` alone is 32 MB). It sits in the read-only data section and is
paged in only when Japanese lyrics are romanized, so it costs disk and
download size, not RAM. Debug info goes to a separate `.pdb` on MSVC and does
not inflate the `.exe`.

## Known costs not addressed

| Area | Cost | Why not changed now |
| --- | --- | --- |
| Disk cache | Trimmed at start-up to 512 MB by modification time (not last access) | Reads do not update modification times. Good enough for a cache. |
| `parking_lot` mutex on `Settings` | Taken from UI and background threads | Short critical sections. Holding it twice in one expression deadlocks (fixed earlier). |
