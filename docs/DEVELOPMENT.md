# Development guide

## Requirements

- Windows 10/11, Rust stable with the MSVC toolchain.
- Network access for the first build: `lindera-ipadic` downloads the IPADIC
  dictionary in its build script. Alternatively build with
  `--no-default-features`.
- Optional: Python + Pillow to regenerate the logo (`python assets/make_logo.py`).

## Build and run

```bash
cargo build                    # debug (dependencies at opt-level 2)
cargo build --release          # thin LTO, codegen-units 4
cargo run --release -- --probe # audio / API diagnostics
```

`build.rs` generates the Windows resource (icon `assets/icon.ico` and the
version information shown in Explorer, taken from `Cargo.toml`). A release
`.exe` cannot be overwritten while Abyss is running. Close it or build into
another directory with `CARGO_TARGET_DIR=target/other`.

## Checks before a commit

```bash
cargo fmt --check        # rustfmt.toml: max_width 140
cargo clippy --all-targets
cargo test
cargo audit              # cargo install cargo-audit
```

State for 0.8.0: fmt clean, clippy without errors (remaining warnings are
dead code: unused PKCE login, old visual helpers), 26 tests passing.

### What the tests cover

| Test | Module |
| --- | --- |
| DASH timeline parsing; refusal of DRM, huge repeat counts, plain HTTP, garbage | `tidal::dash` |
| DPAPI round trip, garbage blob rejected | `secure` |
| Segment window: exact bytes and order, bounded memory, eviction, refetch of dropped segments, backpressure (fast and slow links), ≤ 4 parallel downloads, cancellation frees memory, retries, network outage, link renewal, restart | `audio::stream` (offline, with a fake fetcher) |
| Tokens never serialised in plaintext, decryptable again | `config` |
| Image disk cache prunes oldest first | `ui::images` |
| Kana / Hangul / pinyin / kanji romanization | `romanize` |

UI, network and audio output have no automated tests. They are checked by
hand with `TIDE_PAGE` screenshots and `--probe`.

## Manual testing

- `TIDE_PAGE=<page>` opens a page directly. See
  [CONFIGURATION.md](CONFIGURATION.md#environment-variables).
- Scripts that send mouse or keyboard input must run against a **separate**
  test instance, never while the user's own Abyss window is open.
- Test runs use the real `%APPDATA%\abyss\settings.json`. Back it up before
  running a newer build next to an older one, because the newer build may
  migrate the file.

## Style

- `cargo fmt`, one small module per widget or page.
- `sc()` for every pixel size; `theme::*` for colours and metrics.
- Never block the UI thread; never hold `settings.lock()` twice in one
  expression.
- Do not log tokens, signed URLs or full HTTP errors from the CDN (use
  `without_url()`).
- Do not copy code or assets from TIDAL's applications (see
  [LEGAL.md](LEGAL.md#r4--trade-dress-and-branding--medium-lawyer)).

## Commits

Conventional-style messages (`feat:`, `fix:`, `docs:`, `refactor:`,
`security:`, `perf:`). Never commit `settings.json`, tokens or crash logs.
`vault/`, `graphify-out/` and `docs/GRAPH.md` are local tools and are
git-ignored.

## Releasing (checklist)

Package (portable folder, zip and SHA-256 checksums in `dist/`):

```bash
powershell -ExecutionPolicy Bypass -File scripts/package.ps1
```

Installer (needs [Inno Setup 6](https://jrsoftware.org/isinfo.php); per-user,
no admin; uninstalling keeps `%APPDATA%\abyss`):

```bash
powershell -ExecutionPolicy Bypass -File scripts/package.ps1 -Installer
```

Builds are reproducible in their dependency set (`--locked`, `Cargo.lock`
committed). Binaries are not code-signed.

1. Checks above, plus `cargo audit`.
2. Ship with the binary: `LICENSE`, `assets/fonts/Inter-LICENSE.txt`,
   `assets/licenses/IPADIC-COPYING.txt` (if `kanji-romaji` is on), and the
   third-party notices described in [DEPENDENCIES.md](DEPENDENCIES.md).
3. Resolve the open items in [LEGAL.md](LEGAL.md) before any public binary.

## Abyss phase A validation

Use `node scripts/check-i18n.cjs` for offline catalog and fixed-text checks.
Use `node scripts/generate-brand.cjs` to regenerate the shared SVG, PNG and ICO.
The Rust package remains `tide` for lockfile continuity; the binary target and
Windows metadata are Abyss. Repository links keep the existing RaunaCO/TIDE URL.
Legacy diagnostic TIDE environment variables and DPAPI entropy are compatibility
identifiers. They are deliberately retained, including those in `src/audio/`.
During phase A, do not execute tests or modify audio sources. Format with
`cargo fmt -- --config skip_children=true`, then run rustfmt with
`--edition 2021 --config skip_children=true` on non-audio modules and `cargo build`.
This prevents the formatter from traversing the protected audio module.
