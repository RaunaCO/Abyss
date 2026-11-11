# Changelog

All notable changes to Abyss. Versions follow [semantic versioning](https://semver.org/);
before 1.0.0 every minor version may change behaviour.

Release plan: **0.8.0** feature complete → **0.9.0** beta (real-world testing,
fixes only) → **1.0.0** stable. Public distribution is on hold until the legal
questions in [docs/LEGAL.md](docs/LEGAL.md) are resolved.

## [1.0.1]

- The welcome guide shows only once: it counts as seen when it first appears, so
  closing the window before the last step no longer brings it back.
- New Windows setup installer (per user, no administrator rights, keeps your data
  on uninstall) next to the portable zip.
- README landing page with performance goals and measurements.

## [1.0.0]

First stable release. Free and open source (MIT).

- Includes everything in 0.10.x: Abyss branding, six languages, guided
  introduction, local files in the Library with tag editing, several lyrics
  providers, cleaner fullscreen player and shared motion.
- Documentation reviewed for the release: README, LEGAL (status for 1.0.0),
  ROADMAP, third-party notices.
- Known limits: unsigned binary; the unused Android/PKCE login code is still
  compiled in (LEGAL.md R1); Discord Rich Presence needs an application ID.

## [0.10.1] - 2026-11-11, for owner testing

- Debugging and optimisation pass: removed unused code (pulse and ripple helpers, cover zoom, window drag helper) and applied the clippy fixes outside the audio engine.
- `crash.log` keeps the previous report as `crash.old.log` and replaces URL query strings and bearer tokens with `[redacted]`.
- `THIRD_PARTY_NOTICES.md` lists the components of the build and ships in the package.

- Fullscreen Now Playing has a larger artwork column, clear title and artist
  hierarchy, and centered progress, transport, volume and utility controls below
  the artwork. The bottom player bar is hidden. Queue, Similar, Credits and
  Lyrics share a detail panel; narrow layouts stack with scroll access.
  Artwork lighting, transitions and Windows reduced animation remain supported.
- Removed controller navigation, its dedicated shortcuts and associated copy.
  The welcome guide now has four steps and remains repeatable from Settings.
- Both queue views center short lists as a block without scrolling. Longer
  queues center the current track on opening, track changes and panel resizing,
  with enough space to center the first and last tracks.

- Local tracks, albums and tag-derived artists share the Library, sorting,
  filters, search, pages and applicable context menus. Local management lives
  in Settings; login opens the local Library without a service session.
- Removed the unused secondary music-provider remnants from code, welcome,
  translations, dependencies and documentation. TIDAL sharing has one URL path.
  Cargo.lock was refreshed offline; existing user files and credentials stay on disk.
- Validation: formatted the edited Rust modules without traversing audio,
  built with `cargo build --offline` after each step and checked six catalogs.
  Reused the existing checksum-verified IPADIC archive after the lock refresh.
  No tests, installs, commits, pushes or user-data deletion. Live UI review remains pending.

## [0.10.0] - 2026-11-11, for owner testing

- Phase C item 6: one-time four-step welcome (introduction, service, language,
  local library), repeatable in Settings > Appearance. TIDAL and local-only
  starts are remembered without deleting saved tokens. Import uses the existing
  file/folder picker and preserves originals.
  Completion or Skip persists the marker; replaying the guide keeps it complete.
  All new copy is in en, es, pt, fr, de and ja; language changes reuse Settings.
  Offline users can open Settings, repeat the guide and access their local library.
- Phase C item 2: shared Abyss mark presentation and bundled OFL Inter hierarchy.
  All content pages share the page entrance; floating menus, search, information
  and dialogs use the same motion tokens as sidebar, queue, lyrics and player
  interactions. One 80/140/220/300 ms rhythm, common easing and 35 ms stagger
  capped at eight cards. Queue meters now use the paced ambient scheduler.
  Windows reduced animation snaps interactions, scrolling, artwork fades and
  page/panel entrances and freezes decorative loops and artwork lighting.
  No raster transforms or extra ambient image generation were introduced.

- Phase B item 3: Settings has a dedicated local-files section for file/folder
  imports, watched folders, the imported list and removal without deleting music.
  The sidebar list and top import button link to Settings. The per-file editor
  writes title, artist, album artist, album, genre, year, track/disc and PNG/JPEG
  artwork into MP3, FLAC and M4A/MP4 using Lofty 0.25.5. WAV edits stay in the
  index. Read-only/in-use files are rejected before writes. A validated sibling
  temporary is atomically published only after a one-time full-original backup;
  Restore original uses the same safeguards. Index, queued metadata and content
  addressed covers refresh after save, including covers on tracks without albums.
- Lofty, lofty_attr and ogg_pager sources are pinned with their licences under
  vendor/lofty-rs because the build shell cannot reach crates.io. Remaining
  dependencies were already cached. No software was installed.
- Phase B item 4: local embedded USLT/Vorbis/MP4 lyrics and sibling LRC files now
  work in the sidebar and fullscreen. LRCLIB, TIDAL, NetEase and lyrics.ovh use
  a documented script-aware order, synchronized-over-plain selection and a total
  eight-second provider budget. External-provider preferences cover local music
  too. Missing lyrics are remembered for 15 minutes with an explicit retry.
  Both views share interpolated emphasis and scrolling, a 150 ms active-line
  lead and the Windows reduced-animation preference. LRC handles repeated
  timestamps, offsets and UTF-8/UTF-16 files. See docs/LYRICS.md.
- All new interface copy is externalized in en, es, pt, fr, de and ja. Phase B
  validation uses cargo fmt, cargo build --offline and the catalog checker.
  src/audio/ was preserved. No tests, commits, pushes or user-data deletion.
  Pending: real-file editing/restoration and locking checks, live lyric-provider
  coverage, and visual/synchronization review in the six languages.

- Reconciled both partial localization implementations into one embedded lookup
  API, with runtime labels instead of translated constants. Removed duplicate
  module declarations and duplicate extraction keys.
- Complete English, Spanish, Portuguese, French, German and Japanese catalogs.
  Settings selects and saves the interface language, with Windows UI language
  as the default. Date order, formatted messages and input placeholders follow
  the selected language. The offline fixed-text check also validates catalog
  keys and placeholders. See docs/LOCALIZATION.md.
- Rebranded the window, login, sidebar, Settings, menu, media controls, user
  agent and Windows resources to Abyss. Added an original grayscale sound-wave
  vortex in SVG, PNG and multi-size ICO, with a dependency-free generator.
- The binary target is Abyss.exe. Cargo's package name remains tide for lockfile
  continuity. Packaging produces Abyss portable archives and an optional Inno
  Setup installer. The installer keeps the upgrade AppId, selects the new Abyss
  program directory and preserves configuration on uninstall.
- First startup copies the complete legacy tide configuration to abyss using a
  staging folder, then publishes the completed copy. Existing Abyss data wins.
  Originals, backups and failed staging copies are preserved. Failures stop
  startup before any store loads. Legacy DPAPI entropy stays compatible.
  Copied local artwork references resolve under the new root; music-file
  paths remain unchanged.
  ABYSS_CONFIG_DIR and ABYSS_CACHE_DIR override the new paths; legacy TIDE
  overrides remain supported and bypass migration.
- Discord activity has no bundled application ID until the owner supplies a
  verified Abyss application ID. It stays inactive without one; Settings has
  the ID field and explains the requirement. No remote application was renamed.
- Retained the unofficial-client notice, non-affiliation with TIDAL and the
  unresolved legal questions. Repository URLs and compatibility identifiers
  retain their existing names.
- Validation: cargo fmt and cargo build after each phase A item; six-language
  catalog check; package script syntax; Windows executable identity and logo
  inspection. Audio sources were preserved byte for byte. No tests, installs,
  commits, pushes or user-data deletions were performed.
- Pending owner validation: native-speaker and visual review, migration on a
  real legacy profile and installer upgrade/uninstall. Inno Setup is not
  available here, so no installer was built or executed. Existing dead-code
  warnings and upstream future-compatibility warnings remain.

## [0.9.9] - for owner testing

- The lyrics sidebar shows romanization below the original. Fullscreen lyrics
  show the romanized line first, with the original in smaller type.
- Drag the left edge to resize the queue and lyrics sidebar. Abyss saves the
  width. Previous tracks stay dimmed in both queue views and can be played
  again by clicking them. Playback advances if preloading runs late.
- Opening either queue view centers the current track, which has extra space
  around it. The sidebar queue also shows a small animated playback indicator.

- Replay and ratings use a local database with atomic writes and a one-time
  import from the original JSON files. Replay saves in the background and
  retries failed writes.

- Home pairs a cover-led feature with vertical listening history, compact
  recommendations and open artwork shelves. The columns stack in narrow windows.
- The matte sidebar expands over the page, keeping the content and search in place.
- Settings has separate sections for audio, appearance, library and lyrics,
  privacy, account and credits, with flat groups and a shared heading treatment.
- Library has a dedicated control strip. Catalogue pages use responsive
  artwork headers, while Replay groups listening metrics into a summary.
- The player separates the transport, track details and utility controls.
  Queue, lyrics, full screen, search, menus and dialogs use the same matte finish.
- Icon hover and press transitions respond faster. Ambient animation draws at
  a lower rate and the page behind fullscreen playback is suspended.
- The artwork caches hold fewer bytes. Fullscreen backgrounds blend between
  48 smaller samples per cover.

## [0.9.8] - unreleased (local library, glass; for owner testing)

### Local files
- Imported files go straight into the Library: Tracks lists them with your
  favourites (newest first) and Albums groups them into albums with their
  embedded artwork, track / disc numbers and year. Local albums open like any
  album (play, queue, remove from the Library; no service actions).
- Drag audio files or folders onto the window to import them. Added folders
  are remembered and scanned again for new files at start-up.
- Without a service session, Library tracks and albums still show local music.

### Liquid glass
- The reflection is now a soft light centred on the pointer, with a weaker
  counter-reflection on the opposite side and a directional sheen that grows
  towards the edges. It follows the pointer anywhere, including the middle of
  large panels (before, the light was aimed from the centre and flipped there).

## [0.9.7-c] - unreleased (panel spacing and alignment, for owner testing)

- One spacing scale for every rounded panel: content starts 20 px from the
  edge (14 px on narrow panels) and headings sit 14 px above their content.
  Before, each panel used its own padding (9 to 26 px). Applies to Home
  shelves, the album / artist / playlist header, the biography, every Replay
  card, Settings groups and their rows, Local files, the
  inline album panel and the playlist dialogs.
- Shelf arrows line up with the panel's right edge and the title's height.
- Replay: the most played track's artwork starts at the panel's padding
  like everything else, instead of floating in the middle.
- Grids fill the content column exactly, so the last column ends on the same
  edge as the controls and panels above it (it was a few pixels short).
- Settings' quality options use the same corner radius as the other rows;
  dialogs use the same radius and padding as the info panel.
- Pages start at the same height (album and Settings pages were 2–4 px off),
  and the space under "Nueva playlist" matches the other control rows.

## [0.9.7-b] - unreleased (interface polish, for owner testing)

- Long titles, artists and albums end in "…" when they do not fit (cards,
  track rows, menus, the player bar); before, they were cut off mid-letter.
- Liquid glass on the surfaces that were still flat: Library category,
  sort, view and search controls; filter fields; sort chips; the inline
  album panel and its tip; the search top hit and recent searches; Settings
  tabs and buttons; For You shortcuts; the scroll letter bubble; the dragged
  track; the Now Playing tabs (one capsule); and the app menu's reflection.
- The app menu is solid again, so the page never shows through its labels;
  checked options (Shuffle, Repeat) keep their label aligned with the rest and
  show the check after it.
- Quick search results keep their full row height (they were squeezed
  together when the list was long).
- Every page shares one content width, so the left edge no longer moves
  between Home, Library, albums, playlists and Replay; Settings uses the
  same centred column, a little narrower.
- Replay: the four stat cards line up (labels at the same height) and small
  daily rates read "menos de 0.1 al día" instead of 0.0.

## [0.9.7] - unreleased (glass widgets and local music)

- Rounded widgets, floating menus, search and playback controls use Liquid Glass-inspired reflections that follow the pointer, brighten on press and fade out.
- Import individual audio files or a music folder from the top bar or Archivos locales. Abyss reads the original MP3, FLAC, WAV and AAC-in-M4A/MP4 files and stores a local index.
- Local tracks use the existing playback queue and audio output, with or without a TIDAL session. Their metadata is kept out of TIDAL requests, Replay and Discord activity.

## [0.9.6-c] - unreleased (rounded content widgets, for owner testing)

- Replay keeps its layout and separates its metrics, ranking, chart and collections into rounded widgets.
- Home uses the same widgets for featured content, shelves and recently played items.
- Settings reorganizes General, Account and About into clearly labeled groups with controls that wrap as space narrows.
- Artist and album pages share a cover and action header, with distinct sections for tracks, discography and related content.

## [0.9.6-b] - unreleased (interface refinement, for owner testing)

- Replay uses the page background and keeps its dividers, with wider content, centred metrics and readable chart labels.
- Track rows and library lists have more room, adapt to narrow panes and keep action buttons aligned when hovered.
- The top bar matches the page background and adds a persistent light/dark theme switch. Search gets its own row when space is tight.

## [0.9.6] - unreleased (interface fixes, for owner testing)

- Dedicated full-screen player with a button in the playback bar and Esc to return.
- Lyrics open in the normal view's side panel, alongside the existing queue workflow.
- Replay groups the most played track, listening totals, expandable ranking, activity chart, artists and albums in a responsive dashboard.
- Window chrome, menus and Settings share one neutral grey, with clearer separators throughout lists and panels.
- The playback bar shows album · artist and a subtle, slowly changing wash from the cover art.

## [0.9.5] - unreleased (Replay, context menus and track presentation, for owner testing)

### Replay
- A **dashboard** in three columns: General Stats (plays, listening time,
  tracks, days with plays), Top Artists (carousel); Most Played Track and
  Also on Repeat; Plays Over Time, Plays by Genre and Top Albums (carousel).
  Periods: **This Year** (plays per month), **Last 30 Days** and **This Month**
  (per day), **All Time** (days, months or years depending on the span).
- Actions: **Play Top Tracks** (up to 50, in order), **Generate Playlist**
  (the new-playlist dialog filled with them, a name and a description; it is
  created on TIDAL only when you confirm) and **Share Image** (a PNG of the
  dashboard in Pictures\Abyss; the path is copied).
- All figures come from the local history; Plays by Genre says that TIDAL does
  not provide genres instead of inventing them.

### Context menus
- Tracks, albums and artists get a new right-click menu: artwork, title and
  details at the top, three main actions (Play, Favorite / Follow, Share),
  navigation (View Album / View Artist, Song / Album / Artist Info), an
  expandable **Add to playlist** with each playlist's cover (scrolls when
  long; albums add all their tracks) and a red **Remove** at the end:
  from the current playlist, or from the collection / unfollow. Every Remove
  asks for confirmation first. Albums and artists also open it from cards and
  Library rows; Escape or a click outside closes it.

### Song Info
- Cover, primary and featured artists, producers (TIDAL credits), year, disc
  and track; and technical data **read from the stream itself**: format and
  bit depth, sample rate and channels (FLAC STREAMINFO or the MP4 sample
  entry), duration (exact from the sample count when available). Bitrate and
  file size are shown only when they can be known (single-file streams); the
  320 / 96 kbps of TIDAL's AAC tiers are labelled as nominal; segmented (DASH)
  streams say so. Album Info and Artist Info (TIDAL biography) use the same
  panel.

### Interface
- Track lists: each track has a discreet outline and a little space between
  rows; hover and the playing track are outlined more clearly.
- Top bar: Settings button left of Upload; the line under the bar is thicker
  and in the frame's colour.

## [0.9.4-e] - unreleased (window frame and navigation, for owner testing)

### Window frame
- **A frame around the whole UI, made of the sidebar.** The sidebar has the
  frame's colour and continues as a thin margin around two rounded panels:
  the page (top bar and content) and, at the bottom across the full width,
  the **player bar**. The sidebar runs from the top of the window down to the
  player bar (never beside or over it).
- **Unfolding the sidebar resizes the page** beside it (it no longer slides
  over the content); grids and lists adapt as it widens and shrink back when
  it folds. Whether the pointer is over it is decided from the pointer's
  position on every move, so it stays open while it widens under a still
  pointer.
- The two rows at the top (caption with menu, name and window buttons; then
  sidebar toggle, back / forward, search and avatar) are replaced by one
  52 px **top bar** inside the page panel, separated from the page by a thin
  line: **breadcrumb** on the left (Browse › Library, For You › Replay,
  Playlists › name...; parents can be clicked; on narrow windows only the
  current page), the **search** centred and always visible (it narrows with
  the window; results open centred under it), and on the right an **Upload**
  icon (reserved for local files; shown, not usable, with a tooltip saying so)
  and the **window controls** in one grey container (minimise, maximise /
  restore, close; close turns red on hover). Empty space in the bar moves the
  window; double-click maximises.
- Back / forward buttons removed: history stays on Alt + ← / →, Backspace /
  Shift + Backspace and the mouse's side buttons.
- The app menu (File, Navigation, Playback...) opens from the Abyss logo at the
  top of the sidebar.
- Now Playing covers the page panel; the sidebar (frame) stays beside it.

### Sidebar
- Faster: opens in 0.16 s; folds 0.15 s after the pointer leaves, in 0.18 s.

## [0.9.4-c] - unreleased (visual polish, for owner testing)

### Sidebar
- **Always a rail that unfolds on hover.** Wider rail (76 px) with centred
  icons, short dividers for the sections and a tile for the account; when the
  pointer reaches it (or comes within a few pixels), it unfolds to 280 px
  **over** the page with a soft shadow, so the content never moves or
  re-lays out; labels fade in as the panel widens. It folds back 0.35 s after
  the pointer leaves (no flicker when crossing the edge). The collapse button
  and the saved collapsed mode are gone (`sidebar_collapsed` is ignored).
- Selection and hover surfaces grow with the panel (a rounded square on the
  rail, the full row unfolded). More padding and taller rows.

### Contrast
- Menus, context menus, the sort menu, search menu, dialogs, popovers and
  tooltips share one floating style: a solid surface a step above the page,
  a visible edge and a moderate shadow (in the light theme they were white
  on white before).
- Compact controls (Library bar, title-bar search and navigation) have a
  slightly lighter surface and a subtle edge; settings cards are a touch
  lighter; unselected quality options show a ring.
- Secondary and dim text slightly more legible in both themes.

### Fixed
- Eased values started while drawing (side panel, segmented controls,
  sidebar) could stop until the next mouse event; they now keep requesting
  frames until they settle.

## [0.9.4-b] - unreleased (For You: Replay and Mixes & novedades, for owner testing)

### Sidebar
- "Your Collection" is replaced by **For You**: **Replay** and **Mixes &
  novedades**. Tracks, Albums and Artists are reached only through Library's
  category switch.

### Replay (new)
- Personal listening statistics: listening time, plays (30 s or half the
  track), distinct tracks and artists, average per listening day; listening
  per day or month and by hour of day; most played tracks, artists and
  albums; recent activity. Periods: 7 days, 30 days, this year, all; the
  dates covered are shown.
- Built only from a **local history** that Abyss now records while it plays
  (`history.json`; seconds actually heard, pauses excluded; never sent
  anywhere). TIDAL does not give this app the account's full history, so the
  page says from which date data exists and that earlier listening cannot be
  counted. TIDAL's own monthly "My Most Listened" lists are shown below when
  its Home feed has them.

### Mixes & novedades (new)
- One page with the user's mixes and radios, **Staff Picks** and **Uploads**
  (each under a small source label), using the same feeds as before. The
  Staff Picks and Uploads tabs are gone from Home; "Ocultar Uploads" now
  applies to this page.

## [0.9.4] - unreleased (sidebar and Library redesign, for owner testing)

### Sidebar
- Three zones: **identity** at the top (Abyss's mark in a rounded square and
  the name), a **middle** that takes the free height and scrolls, and an
  **account panel** pinned to the bottom, always visible above the player bar
  (which is unchanged). A barely visible vertical gradient sets the sidebar
  apart from the page in both themes.
- **Browse**: Home, Library, Recent. **Your Collection**: Tracks, Albums,
  Artists (they open Library on that category). **Playlists** with "+" (new
  playlist) on the same row, then "All playlists" and each playlist with a
  square thumbnail and its name (truncated). Section titles are labels, not
  buttons.
- The selected row fades its lifted background in and out; hover brightens
  progressively. In Your Collection the category shown in Library is marked
  by its text only, so there is one highlight.
- **Account panel**: TIDAL profile picture (or a neutral avatar), name and
  e-mail as TIDAL reports them (or the account ID when there is no separate
  e-mail), Settings and Sign out. Sign out now asks for confirmation.
- The window's caption bar no longer repeats "Abyss" (the identity is in the
  sidebar). Explore and "Mixes y radios" are reachable from the app menu
  (Navegación) since they are no longer in the sidebar.
- Narrow windows (under 900 logical px, e.g. small window at 150 % zoom)
  show the sidebar as the icon rail automatically; the saved preference is
  not changed.
- **Recent**: a page with the albums played in Abyss (the local history that
  already fed "Escuchado recientemente").

### Library
- Tracks, Albums and Artists in **one view**: "Library" with the active
  category beside it, then one control row: the **category switch** on the
  left; on the right the **sort selector** (the criterion in words, e.g.
  "Alfabético", "Fecha de incorporación", with its options for that category
  under it), a **list / grid** switch and a compact **search field**. All
  controls share one height; segmented controls are one surface with the
  active option lifted inside.
- Switching category replaces the content in place (no history entry, no
  reload); each category keeps its own search for the session and its own
  sort; list or grid is remembered per category. Empty collections, loading
  and searches without results have their own messages.
- Album and artist **list views** (cover, name, artist, year, tracks,
  length; picture and name). Tracks can also be shown as a grid.
- Sort labels renamed: "Alfabético", "Fecha de incorporación", "Año de
  lanzamiento" (and short forms in narrow layouts).
- Track tables drop BPM, then date added, then rating, then album as the
  list gets narrower, so titles keep room at large zoom.

### Fixed
- Panel scrolling (sidebar, queue, lyrics...) could stop after the first
  frame of a wheel glide when nothing else was animating.
- Confirmation dialogs (sign out, delete playlist) did not close with Escape.

## [0.9.3] - unreleased (fixes and interface refinement, for owner testing)

### Fixed
- **Playing a track from a list started the wrong one** (often two rows
  further down). Unavailable tracks were removed from the list before the
  chosen position was applied, so every unavailable track above the chosen
  one shifted it by one row (your favourites had two such pairs). The chosen
  track is now mapped to its place in the playable queue; picking an
  unavailable track shows a message instead of playing another. Applies to
  Pistas, albums, playlists, mixes, search, artist pages and the album
  dropdown (double click and the row's play button).
  Shuffle now always starts on a playable track.
- **Now Playing no longer shows the page through it while it slides.** It
  was drawn semi-transparent during the animation and the player bar dropped
  its background as soon as it started. It is now one opaque sheet of
  constant height that slides as a whole (nothing inside resizes), and the bar
  keeps its own surface until the sheet is in place, then blends into it.
  The large cover is decoded ahead of time, so it is there from the first
  frame of the slide.
- Animations started by the player bar, side panels and overlays could stop
  after one frame when nothing else was moving (the next frame was scheduled
  before those parts were built). Frames are now scheduled at the very end of
  each render.

### Interface
- **Sync icon** redrawn as two arrows chasing around a circle. Pressing
  "Sincronizar" turns it smoothly (eased start, steady speed, each angle drawn
  as vectors) while the lyrics glide back; the button fades out when they
  arrive (at least 0.5 s), and nothing keeps running afterwards.
- **Queue in Now Playing** opens and closes with one eased movement: the
  queue slides in from the right and fades in while the cover column makes
  room; the lyrics re-wrap once instead of on every frame.
- **Album dropdown**: a real triangular tip, centred on the album that opened
  it (using the grid's actual card width), drawn as one surface with the panel
  (its rim continues the border). The album title and artist brighten and
  underline gradually on hover; artist and album links in track lists do the
  same (slightly quieter at rest).
- **Full-screen cover**: click the cover in an album dropdown to see it
  centred at the best resolution (1280 px, resampled once to the screen's
  pixels; the cached smaller copy is shown until it arrives). Short fade and
  zoom; Escape or a click outside closes it; clicks on the image do nothing.
- **Accent colour** (Ajustes → Pantalla → Color de acento): eight ready-made
  colours, a hue strip for any other, the value as hex and "Restablecer".
  Saved in `accent`. All accent uses (active controls, current track,
  indicators, selection, quality badge) follow it; on the light theme it is
  darkened as needed to stay readable on white. Abyss never reads the Windows
  accent, and it now sets its own neutral window border so the Windows
  option "show accent colour on title bars and window borders" does not draw
  the system colour around it.

## [0.9.2] - unreleased (interaction and visual polish, for owner testing)

Interface only; audio, streaming, lyrics sync and features unchanged.

### Interaction
- **Progress and volume sliders keep dragging** while the button is held, even
  when the pointer leaves the bar, goes over Now Playing or leaves the window
  (mouse capture plus a capture-phase listener). Larger hit area on the
  progress line.
- **Smooth wheel scrolling in every panel**: lyrics (Now Playing and side
  panel), queue, search menu, credits, similar tracks and the sidebar's
  playlists now glide like the main page (same distance and easing). A new
  notch adds to the glide, the opposite direction cancels it, and nothing runs
  once it settles. Touchpads keep their native scrolling. Scrolling lyrics by
  hand still pauses the follow and shows "Sincronizar".
- Queue / lyrics side panel slides in and out from the right edge with a fade
  (0.30 s in, 0.24 s out) instead of appearing at once; its content keeps its
  width while it moves.

### Visual
- **No more cut-off text above the transport controls**: lyric lines fade out
  as they approach the top and bottom of their area (they used to be clipped
  half-visible just above play/pause).
- **New loading state**: three softly breathing dots and "Cargando", centred
  in the page, shown only after 120 ms (fast loads never flash it) and faded
  in; the content fades in when it arrives. Replaces the plain text line.
- **Cover is now perfectly still.** The tilt/drift is gone; only a soft light
  follows the pointer over the Now Playing cover (brighter under the pointer,
  slightly darker on the far side), limited to the cover's rounded shape.
- **Track changes without a black flash**: the new cover fades in over the old
  one (Now Playing, the player bar and both backgrounds), colours move to the
  new palette gradually, and nothing fades from black. Covers of the next two
  tracks are decoded ahead of time, so a skip usually changes the cover within
  ~60 ms (0.18 s fade when cached, 0.42 s after a download). The audio skip is
  never delayed by this.

### Light theme
- **Ajustes → Pantalla → Tema: Oscuro / Claro**, saved in `light_theme`
  (default dark). The light palette is designed on its own (not an inversion):
  off-white background, white cards and menus, near-black text, a deeper teal
  accent and darker gold so they stay readable on white. Hover washes,
  separators, switches, buttons, menus, tooltips, scrollbars, the loading
  state and the player bar (with a hairline border) follow the theme.
- Now Playing deliberately keeps its dark, immersive look over the artwork in
  both themes, and the player bar matches it while Now Playing is open.

## [0.9.1] - unreleased (UI refinement, for owner testing)

Interface quality only; audio, streaming and features unchanged.

### Icons and components
- **New icon family**, drawn for Abyss (replaces the Material Design paths): a
  24 px grid, one 1.7 stroke with round caps and joins, geometric shapes;
  solid glyphs are filled and stroked so their corners are soft; hairline
  window controls. New "sync" glyph.
- Settings cards: row highlights (hover and the selected quality) sit 4 px
  inside the card with their own rounded corners. Before, the selected row's
  highlight showed square corners outside the card's rounded edge.
- Player bar adapts to narrow windows and large zoom: the sides give way and
  the rating stars hide below 1150 px, instead of overlapping the transport
  controls (seen at 150 % zoom).
- Explore's featured strip scrolls sideways instead of cutting its last item.

### Interaction
- One response per control: icon buttons, the sidebar and the player-bar
  toggles sink 7 % while pressed and ease back (paint-only scale, so it is
  smooth); the extra "pop" spring that stacked on top of it is gone. The
  spring remains only for play/pause and ratings, at half the amplitude.
- Player-bar toggles get gradual hover instead of an instant change.
- Card hover depth reduced to an almost imperceptible lift.
- Slider knobs have a fixed size and fade in (they grew pixel by pixel from
  nothing before) and are centred without negative margins.

### Lyrics
- Manual timing correction removed (±0.5 s buttons, offset display and its
  state). Lyrics stay synced to playback; following and manual scrolling stay.
- **Floating "Sincronizar"** over the lyrics (Now Playing and the side
  panel): not part of the layout, fades in when you scroll away and brings
  the current line back with a smooth glide. The side panel now glides too
  instead of jumping.
- The lyrics column no longer uses a negative margin for the glow's room.

### Cover
- **Rebuilt the Now Playing cover tilt.** The 64 × 64 grid of masked image
  pieces is gone. GPUI 0.2 cannot transform image sprites, so instead of a
  fake perspective the cover is one continuous surface: a single image at its
  exact device-pixel size with GPU-smoothed rounded corners, a subtle drift
  and foreshortening toward the pointer (never enlarged), light and shade
  gradients and a shadow that all share the same rounded rectangle. No seams,
  no clipped glare, same drawing at rest and tilted.

### Development
- `TIDE_ICON_SHEET`, `TIDE_LYRICS_UNFOLLOW`, `TIDE_SHOW_KNOBS` (screenshots).

## [0.9.0-beta.1] - unreleased (beta, for owner testing)

Faster and steadier; no new features.

### Performance
- **Frame pacing.** Slow background motion (Now Playing light sweep, rotating
  cover, progress, equaliser bars) is drawn at most 60 times a second;
  interactions (scrolling, hover, presses, panels, lyrics scrolling, cover
  tilt) still run at the monitor's full refresh rate. GPUI's repeating
  animations, which redraw as fast as the GPU allows for as long as they are
  on screen, were replaced by clock-driven ones that go through this pacing.
  Measured on a 200 Hz monitor (share of one CPU core, steady state):
  Now Playing playing 71 % → 30 %, Now Playing paused ~75 % → 23 %, lyrics
  82 % → 28 %, playing with Now Playing closed 21 % (was drawing ~200 frames
  a second). Idle stays at 0 frames.
- `TIDE_PERF` now logs why frames were drawn and how they were paced.

### Stability
- Network errors are retried for about 2 minutes (delay growing to ~5 s)
  instead of six attempts (~10 s); the player shows buffering meanwhile.
  Server errors (4xx other than expired links) still fail after 3 attempts.

### Development
- `TIDE_DEMO_NP_CLOSED=1` keeps Now Playing closed in demo playback.

## [0.8.0] - unreleased (release candidate for owner review)

### Audio
- **Bounded streaming.** DASH tracks are no longer held whole in memory. Each
  segment is stored on its own; the init segment is kept, two segments are kept
  behind the decoder, and downloads pause 15 segments (about 60 s) ahead.
  Memory per track no longer depends on its length or sample rate.
- **Expired links are renewed.** TIDAL's signed segment links expire after
  about an hour; a 401/403/410 now fetches a fresh manifest for the same
  quality and continues at the same segment (pauses longer than an hour, very
  long tracks, late seeks).
- **Going back past the window** (e.g. a demuxer re-reading) fetches the
  dropped segment again instead of failing.
- **Unstable networks:** six attempts per segment with growing back-off (rides
  out drops of about 10 s), and the window doubles to about 2 minutes when
  segments arrive slower than twice real time.
- Progressive (non-DASH) downloads are allocated once from the
  Content-Length instead of growing by doubling.
- Decoding, resampling, gain and WASAPI output are unchanged; bytes reach the
  decoder exactly as served (verified by tests and by identical decode results
  against 0.1.0 on 24/192 and 24/96 streams).

### Memory
- Images are always handed to GPUI already decoded, so GPUI's never-evicting
  asset cache is not used. Decoded covers (96 MB) and encoded originals
  (32 MB) are byte-limited LRU caches; images not drawn in the last frames are
  dropped and their GPU textures released.
- The rotating Now Playing background is generated directly as textures (no
  PNG round trip) and only the current and previous covers are kept.
- Cover colour, palette and blur are computed on demand.
- Image disk cache trimmed to 512 MB.

### Interface
- Covers fade in the first time they appear.
- Failed page loads show a **Reintentar** button.
- Crisp covers at every size (Lanczos resampling to device pixels).
- Quick search menu with a draggable scrollbar.

### Security and privacy
- Session tokens encrypted at rest with Windows DPAPI (plaintext settings from
  older versions are migrated on first start).
- One token refresh at a time; signed URLs kept out of logs; DASH manifests
  validated (segment limit, HTTPS only, DRM refused).
- Setting to turn off third-party lyrics lookups.

### Release and development
- Version information embedded in `Abyss.exe` (from `Cargo.toml`).
- `scripts/package.ps1` builds the portable package and SHA-256 checksums;
  `scripts/abyss.iss` builds a per-user installer with Inno Setup 6.
- `TIDE_CONFIG_DIR` / `TIDE_CACHE_DIR` run Abyss on separate data (tests).
- `--probe` can invalidate links on purpose (`TIDE_PROBE_EXPIRE=1`) to test
  renewal against the real CDN.

### Measured (release build, isolated configuration)
- Private Working Set peaks: idle 76 MB, 24/96 long tracks 117 MB (860 MB in
  0.1.0), 24/192 continuous with gapless transitions 190 MB, AAC 94 MB,
  heavy browsing 127 MB. Clean shutdown in every run, no errors.
- Expired-link renewal verified against TIDAL's CDN (deliberately invalidated
  links on a 24/192 stream; decoded output identical).

### Known issues
- Gapless transitions work in testing but have not been verified by ear or
  with a capture; bit-perfect output has not been verified with a digital
  loopback.
- The session uses OAuth clients of TIDAL's own apps (see LEGAL.md R1).

## [0.1.0]

First development versions (not released): playback engine with Hi-Res DASH,
library, playlists, lyrics, Now Playing, Discord presence, ratings, settings.

