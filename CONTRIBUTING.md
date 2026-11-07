# Contributing

1. Fork and branch from `main`.
2. Follow [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).
3. Keep PRs focused; describe the change and attach a screenshot for UI work.
4. Never commit tokens, `settings.json` or `crash.log`.
5. Run `cargo fmt --check`, `cargo clippy --all-targets`, `cargo test` and
   `cargo audit` before opening a PR.
6. Do not add code or assets taken from TIDAL's applications, and do not add
   features that save streams or work around DRM (see docs/LEGAL.md).
7. Report security problems privately (GitHub security advisories), see
   docs/SECURITY.md.
