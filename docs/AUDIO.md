# Audio pipeline

```mermaid
flowchart LR
    M[playbackinfo manifest] -->|DASH MPD or JSON| SRC
    SRC[Downloader<br/>stream.rs<br/>4 parallel segment fetches] --> BUF[(SegmentWindow<br/>init + 2 behind + 15 ahead)]
    BUF --> R[Reader<br/>blocking, seekable]
    R --> DEC[symphonia<br/>FLAC / MP4-AAC]
    DEC -->|f32| RS{device rate ==<br/>track rate?}
    RS -- no --> RUB[rubato sinc<br/>256 taps, BH2 window]
    RS -- yes --> G
    RUB --> G[gain: volume × ReplayGain]
    G --> RING[rtrb lock-free ring<br/>~0.3 s]
    RING --> EQ{EQ chain<br/>does anything?}
    EQ -- no --> OUT
    EQ -- yes --> DSP[biquad chain<br/>dsp.rs, f64] --> OUT{output}
    OUT --> SH[cpal shared mode]
    OUT --> EX[WASAPI exclusive]
```

| Stage | Code | Notes |
| --- | --- | --- |
| Resolve | `tidal::stream` / `stream_at` | Tries `HI_RES_LOSSLESS → LOSSLESS → HIGH` for *Max*, fewer for lower tiers. Encrypted streams are refused. |
| Download | `audio/stream.rs` | DASH (every quality TIDAL serves this client, AAC included): init segment, then media segments over at most 4 parallel connections, appended in order into a **`SegmentWindow`**: the init segment is kept, segments the decoder has passed are dropped (2 kept behind), and downloads wait while they are 15 segments (~60 s) ahead, 30 (~2 min) once a segment arrives slower than twice real time. Network errors are retried for about 2 minutes (delay growing to ~5 s) while the player shows buffering; 4xx server errors other than expiry fail after 3 attempts. 401/403/410 (expired signed links, ~1 h) renew the manifest for the same quality and continue at the same segment. A segment needed again after it was dropped is fetched back. Single progressive files are kept whole, allocated once from Content-Length. Nothing is written to disk. |
| Decode | `audio/decode.rs` | symphonia (`flac`, `isomp4`, `aac`, `mp3`, `wav`, `pcm`). Samples become interleaved `f32`. |
| Seek | `audio::do_seek` | For DASH, the download restarts at the target segment, so seeking far ahead does not wait for the bytes in between. Otherwise symphonia seeks inside the buffer. `--probe` measured a seek to 150 s at 91 ms (debug build, 2026-11-07). |
| Resample | `decode::Resampler2` | rubato `SincFixedIn` (sinc length 256, oversampling 256, cubic interpolation, Blackman-Harris² window). Its delay is trimmed so tracks start sample-accurately. Used **only** when the output rate differs from the track rate. |
| Gain | `OutShared::gain` | Volume curve × ReplayGain (`trackReplayGain`, limited by `trackPeakAmplitude`). Both are forced to 1.0 in "exclusive + fixed volume". |
| Transport | `output::ring` | `rtrb` single-producer/single-consumer ring, ~0.3 s at the output rate. The real-time callback only pops samples and multiplies by the gain. |
| Equaliser | `audio/dsp.rs`, `output::fill` | Optional chain of parametric filters, applied in the output callback just after the samples leave the ring and before the volume. See below. |
| Output | `output.rs` | cpal shared mode (F32 / I16 / I32 device formats) or WASAPI exclusive (`wasapi` crate) at the track's own rate and bit depth when the device accepts it. |

## Parametric equaliser

Settings > Equalizer holds a chain of up to 32 filters, like Equalizer APO:
Peaking, Low Shelf, High Shelf, High Pass and Low Pass, each with frequency,
gain and Q, plus a global pre-amp. Filters can be added, removed, reordered,
switched off one by one and edited by number or (gain) with a slider.

- **Where.** Inside the real-time output callback (`output::fill`), so it works
  the same in shared mode and in WASAPI exclusive, and a change is heard at once
  (the 0.3 s ring is not in the way).
- **Maths.** RBJ Audio EQ Cookbook biquads (High / Low Pass are second order, Q
  defaults to Butterworth 0.707; stack two for 24 dB per octave). State and
  coefficients are `f64`, the pre-amp comes first and the value stays `f64`
  through every section; the only step down to `f32` is the last one. Nothing is
  clipped inside the equaliser: the volume comes after it, and a float device
  takes the samples as they are (above 1.0 too, like Equalizer APO), while
  integer outputs limit last. Versions up to 1.2.0 clipped at full scale
  *before* the volume, which distorted boosted material even at low volume.
- **Pre-amp.** *Automatic pre-amp* (on by default) sets it to the negative of
  the loudest point of either channel's response, from 1 Hz to Nyquist with the
  maxima refined, every time the chain changes. The equaliser page shows the
  loudest sample that really reaches the device (`Peak -x dBFS`) and flags
  anything over full scale.
- **Channels.** Each filter works on Stereo, Left or Right (like
  `Channel: L R` in Equalizer APO). There is one chain per channel; a channel
  with no filters and no pre-amp is not touched even while the other is being
  filtered (bit for bit). The pre-amp applies to both.
- **Channel gains and meters.** Each channel has its own gain (-12 to +12 dB) on
  top of the pre-amp (`pre x gain` per channel, same 10 ms slide). The audio is
  only left alone when no filter does anything and the pre-amp and both gains are 0 dB.
  The output callback also keeps the loudest sample of each channel (atomic maxima,
  read-only) for the level meters of the equaliser page.
- **Dither.** Only for a 16-bit target (WASAPI exclusive or a 16-bit shared
  device) and only when the signal was changed on the way (equaliser active or
  volume below full): triangular dither of one LSB, so quiet detail is not
  lost to rounding. 24- and 32-bit outputs and the untouched path are not
  dithered. Integer conversion rounds to nearest with scale `2^(bits-1)`.
- **Bit-perfect when idle.** If the equaliser is off, the chain is empty, every
  filter is disabled or is a 0 dB peaking / shelf, and the pre-amp is 0 dB,
  `Dsp::process` returns without touching a sample: the bytes that reach the
  device are the same as without this feature. High / Low Pass always count as
  processing. The Settings page says which of the two states applies.
- **No glitches.** Switching on or off is a 10 ms dry / wet blend; every other
  change slides the coefficients over 10 ms; a seek or track change clears the
  filter memory.
- **UI to audio thread, without locks.** The UI publishes the chain into
  `EqShared`, a seqlock over atomic words that lives in `OutShared`. The callback
  reads the newest consistent copy; if a write is in progress it keeps the
  previous one and tries again on the next callback. It never waits, locks or
  allocates, and moving a slider cannot stall WASAPI.

What is tested (`cargo test`): filter responses against sine waves generated in
memory and against the analytic response; coefficients and impulse responses
against values from a separate implementation (plain Python, doubles); the
definitions' properties (shelf corner at half the gain, Butterworth -3 dB);
noise and distortion more than 120 dB below a tone through boosts and cuts;
that a boost is not clipped inside the equaliser and that the automatic pre-amp
leaves no overs for a full-scale tone; bit-for-bit equality when idle (also
through a real ring and `fill`) and for a channel with no filters; absence of
jumps while switching, changing or moving filters between channels; the seqlock
under a hammering writer; the integer quantiser, the dither statistics and the
peak meter. What is **not** verified is the device: like the rest of the
bit-perfect claim, it needs a digital loopback capture.

## Gapless

The engine (`audio/mod.rs`) starts the next track's download and decoder
before the current one ends. When the current decoder runs out, the next
track's samples go into the same ring, with no gap and no reopening of the
device as long as the sample rate stays the same. In exclusive mode a change
of rate means the device must be reopened at the new rate. The engine waits
for the ring to drain first, so there is a short pause between tracks of
different rates, as in any bit-perfect player.

## Bit-perfect output

"Exclusive mode" plus "fixed volume" (`force_volume`) in Settings:

- the device is opened in WASAPI exclusive mode at the track's sample rate
  and bit depth (`open_exclusive`);
- the gain is exactly 1.0 (no volume, no ReplayGain);
- no resampling happens when the device accepted the native rate;
- `write_sample` converts the `f32` samples back to the device's integer
  format. 16- and 24-bit PCM values are exactly representable in `f32`, so
  the round trip reproduces them unchanged.

This is the design intent. It has **not** been verified with a digital
loopback capture: that is a recommended test before claiming bit-perfect
output publicly. If the device refuses the format, Abyss falls back to
resampling or to shared mode and shows the actual format in the player's
quality label.

## Quality and login type

What TIDAL returns depends on the OAuth client of the session (see
[LEGAL.md](LEGAL.md#r1--oauth-clients-belonging-to-tidals-own-apps--critical-lawyer-tidal)):

- the TV device-code client gets FLAC for hi-res tracks only. CD-quality
  tracks arrive as AAC;
- sessions created with the Android client get FLAC for every track.

## Known limitations

- **Network:** about 60 s (2 min on slow links) is buffered ahead and a
  segment is retried for about 2 minutes, so an outage of roughly 3 minutes
  stops the track with an error. The 0.1 design survived longer outages
  because the whole track was downloaded in the first minute.
- **Gapless** is implemented (next track queued when the current one has
  finished downloading, i.e. ~60 s before its end) and works in testing, but
  has not been verified by ear or with a sample-accurate capture.
- Output backends other than Windows (cpal on Linux/macOS compiles, untested).
- No crossfade (planned).

## Checking the pipeline

```bash
cargo run --release -- --probe            # first hi-res favourite
cargo run --release -- --probe 121047221  # a track ID
cargo run --release -- --probe "artist title"
```

It prints the stream tier, codec and segment count, opens the decoder, decodes
20 s, seeks to 150 s and runs the 48 kHz resampler. Example (2026-11-07):

```
stream: MAX 24-BIT 44.1 kHz FLAC "116 DASH segments"
decoder: 44100 Hz, Some(24) bit, 2 ch, opened in 310 ms
decoded 20.1 s, peak 0.273, first audio after 385 ms
seek to 150 s -> 150.00 s in 91 ms
resampler to 48 kHz produced 222632 frames
```
