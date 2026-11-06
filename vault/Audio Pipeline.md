---
tags: [tide, tech, audio]
---
# Audio Pipeline

`Buffer/Reader → symphonia → rubato sinc → ReplayGain → rtrb → cpal | WASAPI exclusive`

## Facts learned
- Device-code login (`fX2JxdmntZWK0ixT`) returns **FLAC only for Hi-Res**; 16-bit
  tracks arrive as AAC. Accepted limitation: see [[Decision Log]].
- Hi-Res arrives as DASH MPD → parsed in `tidal/dash.rs`.
- Resampler delay must be trimmed for sample-accurate starts.
- A release exe can't be rebuilt while the app is running.

## Verify
`tide --probe <id>`: prints codec, rate, depth and runs a decode/seek test
(see [[Testing Workflow]]).

Related: [[Architecture]], [[TIDAL API Notes]].
