# Interface localization

Abyss embeds complete catalogs for English (`en`), Spanish (`es`), Portuguese
(`pt`), French (`fr`), German (`de`) and Japanese (`ja`) in `src/i18n/`.
Existing translations were retained and reconciled. Portuguese, French,
German and Japanese use machine translations reviewed for terminology,
placeholders and the new product name. Native-speaker wording and visual
layout review remain release checks.

## Phase D: language selection and review

The initial six languages are a bounded maintenance scope, not a ranking of
markets or a claim about user demand. English is the source and fallback;
Spanish supports the project's working language. Portuguese, French and German
extend coverage with Latin-script interfaces, while Japanese also exercises
non-Latin text, compact labels and a different date order. Further languages
should have a complete catalog and a sustainable review path before inclusion.
Portuguese wording follows Brazilian usage; Spanish avoids country-specific
terms where practical. Regional variants currently share their base catalog.

Phase D compared all entries in each of the six catalogs against English,
checked identical key sets and brace placeholders (including names, order
independence and repeated occurrences), and reviewed wording across navigation,
playback, library, playlists, queue, lyrics, Settings, local metadata and
onboarding. Damaged question-mark substitutions were rewritten in UTF-8.
Spanish uses Biblioteca, playlist, cola, Letras and Ajustes; each other language
uses its corresponding catalog terminology consistently. Product and provider
names remain unchanged. Button labels use short action verbs where practical;
warnings retain the information needed to act safely.

The offline fixed-text checker was run and extended to flag damaged text and
named placeholders. This is a catalog and source review, not native-speaker
certification or a visual fit check. Native-speaker review and real GPUI layout
checks at narrow widths and different scales remain release work. No tests
were run in phase D; validation uses the checker, cargo fmt and cargo build.

`i18n::t("meaningful.key")` is the single lookup API. It borrows immutable
catalog text for the life of the process, so it also works for GPUI APIs
requiring a static reference. Resolve translations during rendering or in
functions, never in a `const` initializer. `i18n::text(key, &[values])` formats
numbered slots using the same lookup. It substitutes once and never interprets
braces in user-provided values. Translators can reorder numbered slots.

The default is the Windows UI language, obtained from `GetUserDefaultUILanguage`,
with English for unsupported languages. On other systems the locale environment
is used. Settings > Appearance > Language persists an override in `language`;
System restores the default. Changing language invalidates rendered content and
Replay labels and updates input placeholders without changing entered text.
Service metadata, lyrics, user playlist names and server-provided content remain
as supplied. Protocol values and persisted sort keys remain canonical.

Run the requested offline check with the existing Node.js runtime:

```powershell
node scripts/check-i18n.cjs
```

The check validates all catalog keys, missing values, placeholder parity and
literal translation references. Its Rust string lexer recognizes comments,
escaped quotes and raw strings. It rejects unreviewed UI text, including literal
children and input placeholders. `scripts/i18n-technical.json` lists only
reviewed protocol identifiers, font names, whitespace, filename templates and
source metadata used for matching. Add visible copy to all six catalogs rather
than exempting it. Diagnostic logs, developer probes, test fixtures, native
operating-system messages and the protected `src/audio/` tree are outside the
fixed-UI-text scan. UI error summaries and application-level service errors are
localized; raw operating-system or provider details may retain their language.

No tests or application sessions were run during phase A. Catalog checks and
development builds do not validate real playback, installer upgrades, migration
on a user's machine or text layout at every window size.
