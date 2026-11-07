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
- Audio: `tide --probe` reports time to first audio and seek latency.

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

| Build | `tide.exe` |
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
| Audio buffer | Whole track in RAM (hundreds of MB for long 24/192 tracks) | Needs a bounded, seekable window in `stream::Reader`. Risky for gapless and seeking without listening tests. |
| Memory cache of artwork | Unbounded for the session (`Images::map`); resampled copies capped at 4000 | Evicting would need usage tracking. Typical sessions stay well under the private-memory figures above. |
| Disk cache | Trimmed at start-up to 512 MB by modification time (not last access) | Reads do not update modification times. Good enough for a cache. |
| `parking_lot` mutex on `Settings` | Taken from UI and background threads | Short critical sections. Holding it twice in one expression deadlocks (fixed earlier). |
