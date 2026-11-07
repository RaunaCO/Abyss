# Legal notes and risk register

> **This is not legal advice.** It is an engineering assessment written by the
> project to make the risks of TIDE visible. Disclaimers in this repository do
> not create rights, do not bind TIDAL and do not protect anyone from a claim.
> Items marked **[lawyer]** need review by a qualified lawyer in the
> jurisdictions where TIDE is developed and distributed; items marked
> **[TIDAL]** can only be settled by TIDAL in writing.

Last reviewed: 2026-11-07.

## What TIDE is

An independent desktop program that signs a user into **their own** TIDAL
account and plays what that account is entitled to stream. It is not
affiliated with, endorsed by or sponsored by TIDAL Music AS or its owners.
"TIDAL" is a trademark of its owner and is used only to say which service TIDE
talks to.

## What TIDE does not do (verified in the code)

- It does not decrypt, unwrap or work around DRM. Encrypted streams are
  refused: a JSON manifest whose `encryptionType` is not `NONE`
  (`src/tidal/mod.rs`, `stream_at`) and any DASH manifest that contains a
  `ContentProtection` element (`src/tidal/dash.rs`) produce an error.
- It does not store audio on disk. Streams are held in memory while playing
  and dropped afterwards (`src/audio/stream.rs`). There is no download or
  export feature.
- It does not unlock content or quality beyond what TIDAL's servers return for
  the signed-in account and the OAuth client in use.
- It sends no telemetry. See [PRIVACY.md](PRIVACY.md) for every host it
  contacts.

## Risk register

Severity reflects likely consequences for the project and its users if the
risk materialises, not a legal conclusion.

### R1 — OAuth clients belonging to TIDAL's own apps — **Critical** [lawyer] [TIDAL]

**Where:** `src/config.rs` (`DEVICE_CLIENT_ID`, `DEVICE_CLIENT_SECRET`),
`src/tidal/mod.rs` (`PKCE_CLIENT_ID`, `PKCE_CLIENT_SECRET`, `PKCE_REDIRECT`,
`appMode=android`).

**Evidence:** TIDE has no client credentials issued to it. It signs in with:

- the client used by TIDAL's TV / limited-input apps (device-code flow), and
- the client of TIDAL's Android app (PKCE flow with the Android redirect URI
  and `appMode=android`). This flow has no entry point in the current UI
  (`start_web_login` is never called), but the code and credentials are still
  compiled in, and sessions created with it earlier keep working because the
  client ID and secret are stored with the session in `settings.json`.

Those values circulate publicly, but being public does not make their use
authorised. To TIDAL's servers the sessions look like requests from TIDAL's
own apps.

**Why it matters:** Presenting as another (first-party) client is the part of
TIDE most likely to be treated as unauthorised access or a breach of contract.
Search results for TIDAL's Content Guidelines list reverse engineering and
misrepresenting affiliation among prohibited activities, and the Terms allow
TIDAL to suspend or terminate accounts for breaches (TIDAL's legal pages
refused automated access during this review, so the wording has to be checked
by hand). Consequences range from blocked credentials (TIDE stops working for
everyone at once), through suspended user accounts, to takedown requests
against the repository. The Android client also yields lossless FLAC for
CD-quality tracks, while the TV client gets AAC for them. A higher quality
tier obtained by impersonating a client is more sensitive than ordinary use.

**Recommendation:** Before any public binary release, either obtain client
credentials for TIDE from TIDAL, or document clearly that each user must
supply their own client configuration. At a minimum, remove the unused
Android/PKCE login path (pending owner decision, see the final report).

### R2 — Private API endpoints and a spoofed client version — **High** [lawyer] [TIDAL]

**Where:** `src/tidal/mod.rs` (all `api.tidal.com/v1` and `/v2/home/feed`
calls). The home feed sends `x-tidal-client-version: 2.47.0`, the version
string of TIDAL's desktop app; without it the endpoint answers `400`.

**Evidence:** These endpoints are undocumented. TIDAL's public developer
platform describes its own catalogue API. According to a secondary source
(not confirmed against TIDAL's pages), full-length playback for third parties
goes through TIDAL's Player SDK.

**Change made:** the request no longer claims a subscription tier
(`x-tidal-authenticated-subscription: HIFI_PLUS` was hard-coded). The feed was
verified to load without it.

**Recommendation:** Ask TIDAL whether a desktop client may use these endpoints,
or move to the official developer API where it covers the feature.

### R3 — Terms of Service exposure of users — **High** [lawyer]

TIDAL's Terms (as surfaced by search during this review) limit the service to
personal, non-commercial use and let TIDAL suspend accounts for breaches. Users
of TIDE take that risk with their own accounts. The README and the first-run
experience should say so plainly. TIDE must never be sold or bundled with a
paid offering.

### R4 — Trade dress and branding — **Medium** [lawyer]

- The UI deliberately mirrors the layout, spacing and motion of TIDAL's
  desktop app (the project's goal was "identical"). Copying the overall look
  and feel can raise trade-dress / unfair-competition questions even when no
  asset is copied.
- No TIDAL logo, font or image ships in the repository. The icons are
  Material Design paths (Apache-2.0) per `src/ui/icons.rs`. The TIDE logo is an
  original design (`assets/make_logo.py`). Cover art is loaded at run time
  from TIDAL's image CDN and is not redistributed.
- Animation timings and blur parameters were measured from TIDAL's desktop
  app and re-implemented in Rust (`src/ui/tilt.rs`, `src/ui/fx.rs`). No TIDAL
  code is included, but the project should not copy further material from
  TIDAL's application files. Their content guidelines name reverse engineering
  explicitly.
- The name "TIDE" is close to "TIDAL" in an identical market. A trademark
  search and clearance before any public launch is recommended.
- The Discord button reads "Escuchar en TIDAL" and links to
  `tidal.com/browse/track/<id>`. This is descriptive use, but any use of the
  TIDAL name should stay factual.

### R5 — Third-party lyrics — **Medium** [lawyer]

Lyrics are copyrighted works. When TIDAL has none, TIDE asks:

| Provider | Endpoint | Terms status |
| --- | --- | --- |
| LRCLIB | `lrclib.net/api` | Public API; its documentation asks clients to identify themselves (done, see the User-Agent) and to honour rate limits. No licence for the lyrics data is stated. |
| NetEase Cloud Music | `music.163.com/api/...` | Undocumented private API. No terms for third-party use were found. |
| lyrics.ovh | `api.lyrics.ovh/v1` | No terms found. |

TIDE displays lyrics only while a track plays and does not store or export
them. Whether showing lyrics obtained this way is lawful depends on the
providers' licences, which are unclear. The lookups can now be turned off in
Settings ("Letras de otros proveedores"). The safest configuration for a
public release is TIDAL-only lyrics, which TIDAL licenses (e.g. from
Musixmatch).

### R6 — Discord Rich Presence — **Low**

TIDE uses its own Discord application ID (`DISCORD_APP_ID`), not Spotify's or
TIDAL's. The application name shown is "TIDE". Discord's developer terms apply
to that application. It discloses listening activity, see
[PRIVACY.md](PRIVACY.md).

### R7 — Open-source licences — **Low (one gap fixed)**

TIDE is MIT-licensed. All 565 crates compiled for Windows use permissive or
weak-copyleft licences. There is no GPL/AGPL code. Obligations that apply to
**binary** releases are in [DEPENDENCIES.md](DEPENDENCIES.md). In short:

- **IPADIC dictionary** (embedded in the binary by `lindera`): its licence
  requires every copy to carry the NAIST copyright notice and the ICOT
  "no warranty" terms. The text was missing; it is now in
  `assets/licenses/IPADIC-COPYING.txt` and must ship with binaries.
- **MPL-2.0** (`symphonia*`, `option-ext`): binary distributors must tell
  recipients where the MPL-covered source can be obtained.
- **Apache-2.0** crates (e.g. `gpui`, `cpal`): include their licence and any
  `NOTICE` files with binaries.
- **Inter** font: SIL OFL 1.1, licence in `assets/fonts/Inter-LICENSE.txt`.
- `kakasi` was deliberately not used because it is GPL-3.0.

### R8 — Build-time download — **Informational**

`lindera-ipadic` downloads the dictionary while building, from
`https://Lindera.dev/...`, and checks it against an MD5 hash. Building
therefore needs network access and trusts that host. Use
`--no-default-features` to build without it.

## Questions for a lawyer

1. In the target jurisdictions, does signing in with another vendor's OAuth
   client credentials (R1) risk unauthorised-access or anti-circumvention
   claims, or is it a contract (ToS) matter only?
2. Does the TIDAL Terms clause on personal use, together with the Content
   Guidelines, make distributing TIDE itself actionable against the developer,
   or only against users?
3. Trade dress: how close may the UI be to TIDAL's desktop app?
4. Trademark: is "TIDE" clear to use for a TIDAL-compatible player?
5. Lyrics: may lyrics from LRCLIB / NetEase / lyrics.ovh be displayed in a
   distributed app?
6. What disclaimer and notice wording fits the distribution channel (GitHub
   releases, package managers)?

## Questions for TIDAL

1. Can TIDE get its own client ID for desktop playback, and under what terms?
2. Is full-length playback outside the Player SDK allowed for a desktop app?
3. Are `api.tidal.com/v1` and `/v2/home/feed` permitted for third-party clients?
4. What branding rules apply to the name "TIDAL" in a compatible client?
