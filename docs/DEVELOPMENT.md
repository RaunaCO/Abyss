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

CI (`.github/workflows/ci.yml`) runs these on `windows-latest` for every push
and pull request to `main`, in this order. Run the same locally:

```bash
cargo fmt --check                  # rustfmt.toml: max_width 140
cargo clippy --all-targets --locked
cargo test --locked
cargo audit                        # cargo install cargo-audit
```

A change passes when: fmt reports nothing; clippy has no errors (warnings
are allowed, there is known dead code: unused PKCE login, old visual helpers;
do not add new ones); every test passes; audit reports no vulnerabilities.
`--locked` fails if `Cargo.lock` is out of date: update it in the same
change. Dependabot (`.github/dependabot.yml`) opens weekly update PRs for
Cargo and GitHub Actions; actions are pinned by commit SHA.

State for 1.1.0: 94 tests passing.

### What the tests cover

| Test | Module |
| --- | --- |
| DASH timeline parsing; refusal of DRM (adaptation set or representation), huge repeat counts, more than `MAX_SEGMENTS` (limit inclusive), missing representation, segment template or timeline, plain HTTP, malformed XML | `tidal::dash` |
| DPAPI round trip (also non-ASCII); empty, garbage, tampered and truncated blobs rejected | `secure` |
| Segment window: exact bytes and order, bounded memory, eviction, refetch of dropped segments, backpressure (fast and slow links), ≤ 4 parallel downloads, cancellation frees memory, retries, network outage, link renewal, restart | `audio::stream` (offline, with a fake fetcher) |
| Tokens never serialised in plaintext, decryptable again (also non-ASCII); plaintext tokens from older files rewritten as `protected_tokens`; defaults for an empty file, unknown fields ignored, optional fields round trip | `config` |
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

Last.fm needs the application key and secret registered for Abyss. They are not
in the repository: set `ABYSS_LASTFM_API_KEY` and `ABYSS_LASTFM_API_SECRET` in the
environment before packaging (the script warns if they are missing). A build
without them works, but *Connect* says Last.fm is not available.

Builds are reproducible in their dependency set (`--locked`, `Cargo.lock`
committed). Binaries are not code-signed.

### Automatic release (GitHub Actions)

The usual way to release is the `Release` workflow, not the commands above:

1. A pull request bumps the version (`Cargo.toml`, `scripts/abyss.iss`, README,
   `Cargo.lock`) and renames `## [Unreleased]` in `CHANGELOG.md` to
   `## [X.Y.Z] - date`. Merge it once `ci` is green.
2. Wait for `ci` to pass on the merge commit on `main`, then push the tag:
   `git tag vX.Y.Z <commit>` and `git push origin vX.Y.Z` (`vX.Y.Z-rcN` makes a
   pre-release).
3. The workflow refuses to run unless the tagged commit is on `main`, `ci`
   passed on it and the tag matches the version in `Cargo.toml`. It then builds
   on a clean runner with the Last.fm credentials from the repository secrets
   `ABYSS_LASTFM_API_KEY` and `ABYSS_LASTFM_API_SECRET`, and creates a **draft**
   release with the installer, the zip, `SHA256SUMS.txt`, notes (CHANGELOG
   section, checksums, SmartScreen and unofficial-client notes) and a build
   provenance attestation (`gh attestation verify <file> --repo RaunaCO/Abyss`).
4. Review the draft, install it and publish it by hand.

*Actions > Release > Run workflow* on `main` is a dry run: it builds and keeps the
files as run artifacts for 7 days and never creates a release. Preview the notes
locally with `scripts/release-notes.ps1`.

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
