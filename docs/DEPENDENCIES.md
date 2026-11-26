# Dependencies and licences

Abyss's own code is MIT ([LICENSE](../LICENSE)). The table lists the direct
dependencies. The summary below covers the normal dependencies of the Iced
binary for `x86_64-pc-windows-msvc`, from `cargo tree` and `cargo metadata`,
2026-11-22 (436 crates).

## Direct dependencies

| Crate | Purpose | Licence |
| --- | --- | --- |
| `iced` =0.14.0 (feature `ui-iced`, default) | UI framework | MIT |
| `wgpu` =27.0.1 (feature `ui-iced`) | Lyrics blur and backdrop shaders | MIT OR Apache-2.0 |
| `bytemuck` (feature `ui-iced`) | Vertex and uniform data | Zlib OR Apache-2.0 OR MIT |
| `rfd` 0.15 (feature `ui-iced`) | File and folder pickers | MIT |
| `reqwest` 0.12 (rustls, gzip, blocking, json) | HTTP | MIT OR Apache-2.0 |
| `serde`, `serde_json` | Settings and API JSON | MIT OR Apache-2.0 |
| `rusqlite` with bundled SQLite | Local Replay history and ratings | MIT (Rust bindings), public domain (SQLite) |
| `roxmltree` | DASH manifests | MIT OR Apache-2.0 |
| `symphonia` 0.5 (flac, isomp4, aac, mp3, wav, pcm) | Decoding | **MPL-2.0** |
| `lofty` 0.25.5 (vendored, with `lofty_attr` and `ogg_pager`) | Local metadata and embedded lyrics | MIT OR Apache-2.0 |
| `tempfile` | Transactional sibling files and one-time backups | MIT OR Apache-2.0 |
| `rubato` | Sinc resampling | MIT |
| `cpal` | Shared-mode audio output | Apache-2.0 |
| `wasapi` (Windows) | Exclusive-mode output | MIT |
| `windows-sys` (Windows) | DPAPI | MIT OR Apache-2.0 |
| `rtrb` | Lock-free ring buffer | MIT OR Apache-2.0 |
| `image` (jpeg, png) | Artwork decoding / resampling | MIT OR Apache-2.0 |
| `souvlaki` | Windows media controls (SMTC) | MIT |
| `lindera` + embedded IPADIC (feature `kanji-romaji`) | Japanese readings | MIT (code); dictionary under the **IPADIC licence** |
| `pinyin` | Chinese romanization | MIT |
| `parking_lot`, `anyhow`, `log`, `env_logger`, `base64`, `sha2`, `rand`, `dirs`, `open`, `unicode-segmentation`, `raw-window-handle` | Utilities | MIT and/or Apache-2.0 |
| `embed-resource` (build) | Windows icon resource | MIT |

## Licences across the graph

`base64` is required by TIDAL manifests, PKCE and encrypted token storage.
Tokio remains transitive through HTTP and UI dependencies; it is not a direct
application dependency. The lockfile was refreshed offline for 0.10.1.

| Licence | Crates |
| --- | --- |
| MIT and/or Apache-2.0 (various spellings) | 244 |
| MIT only | 95 |
| Apache-2.0 only (incl. `cpal`) | 7 |
| Unicode-3.0 | 18 (+2 combined) |
| MPL-2.0 (`symphonia*`, `option-ext`) | 14 |
| BSD-3-Clause / BSD-2-Clause only | 13 |
| BSL-1.0 / Zlib / ISC / CC0 and other permissive combinations | 41 |
| `ring` (Apache-2.0 AND ISC), `webpki-roots` (CDLA-Permissive-2.0) | 2 |

`self_cell` offers Apache-2.0 OR GPL-2.0-only and is used under Apache-2.0.
No other GPL, LGPL or AGPL crate is compiled. `kakasi` (GPL-3.0) was evaluated and
rejected for romaji.

Regenerate with:

```bash
cargo tree --locked -e normal --prefix none --target x86_64-pc-windows-msvc --format "{p}"
cargo metadata --format-version 1 --filter-platform x86_64-pc-windows-msvc
```

## Bundled assets

| Asset | Licence |
| --- | --- |
| `assets/fonts/Inter-*.ttf` | SIL Open Font License 1.1 (`assets/fonts/Inter-LICENSE.txt`) |
| IPADIC dictionary (inside the binary) | NAIST / ICOT terms (`assets/licenses/IPADIC-COPYING.txt`) |
| Icons (`src/art.rs`) | Original Abyss icon family (MIT, with the project) since 0.9.1 |
| `assets/logo.png`, `assets/icon.ico` | Original Abyss artwork (MIT, with the project) |

## Obligations when distributing binaries

1. **IPADIC**: include `assets/licenses/IPADIC-COPYING.txt`. Its licence
   requires the copyright notice and the "no warranty" paragraphs to
   accompany every copy.
2. **MPL-2.0** (`symphonia`): say where the source of the MPL-covered files
   can be obtained (e.g. a link to crates.io / the upstream repository in the
   notices).
3. **Apache-2.0**: include the licence text and any upstream `NOTICE` files.
4. **MIT / BSD / ISC / Zlib / Unicode / CDLA**: include their copyright and
   licence notices.
5. **Inter**: include the OFL text. Do not sell the font on its own.

A generated `THIRD_PARTY_NOTICES` file (e.g. with `cargo about`) covering items
2–4 does not exist yet. Create one before publishing binaries.

## Supply chain

- `cargo audit` (2026-11-07): 0 vulnerabilities, 18 unmaintained warnings,
  all transitive. See [SECURITY.md](SECURITY.md#dependencies).
- `Cargo.lock` is committed. Builds are reproducible in their dependency set.
- The IPADIC build script downloads from `Lindera.dev` over HTTPS and checks an
  MD5 hash.
- `souvlaki` is built with `use_zbus`. That only matters on Linux; the
  `zbus`/`zvariant` advisories do not apply to Windows builds.


See its `PATCH.md` for the small host-process safety patch and regression test.
