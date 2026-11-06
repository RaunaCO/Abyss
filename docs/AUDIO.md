# Audio pipeline

```
HTTP / DASH segments -> Buffer/Reader -> symphonia -> (rubato sinc) -> gain -> rtrb ring -> cpal | WASAPI exclusive
```

| Stage | Notes |
| --- | --- |
| Source | Progressive download with a seekable `Reader`; DASH MPD parsed in `tidal/dash.rs` |
| Decode | symphonia: FLAC, MP4/AAC |
| Resample | rubato sinc (`Resampler2`), delay trimmed so tracks start sample-accurate; only when the device rate differs |
| Normalize | ReplayGain from stream info (optional) |
| Output | cpal shared mode, or WASAPI exclusive for bit-perfect output |
| Transport | Lock-free `rtrb` ring buffer, gapless queue, sample-accurate seek |

## Quality reality check
With the device-code login TIDAL serves **FLAC only for Hi-Res tracks**;
CD-quality (16-bit/44.1) tracks come as AAC. This is a limitation of that
login type, not of the player.

## Exclusive mode
Enabled in Settings. The device is opened at the track's native format when
possible; otherwise the sinc resampler converts to a rate the device accepts.
