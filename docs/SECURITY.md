# Security

How Abyss handles credentials, network traffic and local data, what has been
checked, and what is still open. Report vulnerabilities privately through
GitHub's security advisories for `RaunaCO/Abyss` rather than public issues.

## Threat model

| Asset | Threat | Status |
| --- | --- | --- |
| TIDAL refresh / access tokens | Read from disk by another user, a backup, a synced folder or a stolen drive | Mitigated on Windows (DPAPI) |
| Same tokens | Read by malware running as the same Windows user | **Not mitigated.** DPAPI decrypts for any process of that user. No desktop app can fully prevent this. |
| Tokens and signed stream URLs | Leaked through logs or crash reports | Mitigated (see Logging) |
| Network traffic | Interception / tampering | TLS (rustls) for every request; stream URLs must be `https://` |
| Process | Malformed server data (manifests, JSON, images) causing crashes or memory exhaustion | Partially mitigated (see Input validation) |
| Listening activity | Disclosure to Discord or lyrics providers | User-controllable, see [PRIVACY.md](PRIVACY.md) |

## Authentication

- **Flows:** OAuth 2.0 device authorisation (`start_device_login` /
  `poll_device_login`). A PKCE flow (`start_pkce_login` /
  `finish_pkce_login`) exists in `src/tidal/mod.rs` but nothing in the UI
  calls it. Both use client credentials that belong to TIDAL's own apps; see
  [LEGAL.md](LEGAL.md#r1--oauth-clients-belonging-to-tidals-own-apps--critical-lawyer-tidal).
- **PKCE:** S256 challenge from a 32-byte random verifier (`rand::thread_rng`,
  a CSPRNG).
- **Refresh:** access tokens are refreshed 60 s before expiry and once after a
  `401`. A refresh lock (`Tidal::refresh_lock`) makes concurrent requests
  share a single refresh. Before this fix, five parallel requests at start-up
  caused five refreshes.
- **Scopes:** `r_usr w_usr w_sub` (read/write user library and playlists).
- **Logout:** `Tidal::sign_out` deletes tokens, user ID, name and picture
  from memory and `settings.json`. Tokens are **not** revoked on TIDAL's side,
  because no revocation endpoint is used. Revoke sessions from your TIDAL
  account page if a device is lost.

## Token storage

`src/secure.rs`, `src/config.rs`

- On Windows, `access_token` and `refresh_token` are serialised together and
  encrypted with `CryptProtectData` (current-user scope, UI forbidden, an
  application entropy string). The result is stored base64-encoded as
  `protected_tokens` in `settings.json`. Plaintext fields are never written
  when protection succeeds.
- Files from older versions that contain plaintext tokens are rewritten
  encrypted the first time they are loaded.
- If the blob cannot be decrypted (other user, other machine, damaged file),
  the session is dropped and the user signs in again. A warning is logged
  without the content.
- On non-Windows builds there is no protection backend yet: tokens stay in
  plaintext as before. A keyring backend is future work.
- Credential Manager was considered. It gives the same protection against
  local malware as DPAPI, which it uses internally. The tokens would also need
  splitting (2.5 KB blob limit), and the settings file would no longer be
  self-contained. DPAPI on the existing file is the smaller change.
- `settings.json` lives in `%APPDATA%\abyss`, which inherits the user-only ACL
  of the profile. Abyss does not change ACLs.

The OAuth `client_id` / `client_secret` are stored in plaintext. They are
TIDAL's public app credentials, not user secrets.

## Network

- One `reqwest` blocking client with **rustls** (no OpenSSL) and certificate
  validation on. Connect timeout 10 s, request timeout 30 s, and longer
  per-request timeouts for audio (60–120 s).
- User-Agent: `Abyss/<version> (https://github.com/RaunaCO/TIDE)`. LRCLIB asks
  clients to identify themselves. The User-Agent contains nothing about the
  user.
- Stream URLs from manifests are rejected unless they are `https://`.
- Hosts contacted: see [PRIVACY.md](PRIVACY.md#network-destinations).

## Input validation

- **DASH manifests** (`src/tidal/dash.rs`): XML parsed with `roxmltree`
  (no DTD / entity expansion). The number of segments is capped at 20 000,
  because a hostile `r="999999999999"` would otherwise try to allocate
  terabytes. An invalid `timescale` and non-HTTPS templates are rejected, and
  so is any `ContentProtection`. Unit tests cover each case.
- **JSON** from TIDAL is read with `serde_json::Value` lookups that default on
  missing fields. Errors become user-visible messages (for example a `400`
  shows "No se pudo cargar: TIDAL 400…" instead of crashing).
- **Images**: decoded by the `image` crate, which is memory-safe and has
  allocation limits. Downloads that declare more than 32 MB are refused.
- **Discord IPC**: frames larger than 1 MB now drop the connection. Before,
  the pipe was left out of sync.

## Logging and crash reports

- `env_logger`, default filter `Abyss=info,tide=info`, written to stderr. Release builds
  are GUI-subsystem apps, so nothing is visible unless stderr is redirected.
- Tokens are never logged. Download errors are logged with
  `reqwest::Error::without_url()` because TIDAL's CDN URLs carry signatures.
- Verified (2026-11-07): a debug build with `RUST_LOG=debug`, run with an
  expired token so a refresh happened, browsing albums. The output contained
  neither token, nor the words `Bearer`/`access_token`/`refresh_token`, nor
  signed-URL query parameters.
- Panics are written to `%APPDATA%\abyss\crash.log` (message + backtrace),
  overwriting the previous one. Panic messages in Abyss do not contain tokens.
  The backtrace contains local paths. Review it before attaching it to an
  issue.

## Dependencies

`cargo audit` (RustSec database, 2026-11-07): **0 vulnerabilities**,
18 "unmaintained" warnings. All of them are transitive, through `gpui` (font
shaping: `rustybuzz`, `ttf-parser`, …), through `souvlaki` (`zbus`/`zvariant`,
Linux-only and not compiled on Windows) or through `async-std`/`instant`/`paste`.
None is directly actionable in Abyss without upstream releases.

Run `cargo install cargo-audit && cargo audit` before each release.

## Known gaps

- Tokens are not revoked on logout.
- No protection against same-user malware (inherent to desktop apps).
- No code signing of release binaries yet. Users cannot verify where a
  downloaded `.exe` came from.
- `crash.log` is never rotated or cleaned.
- Non-Windows token storage is plaintext.
