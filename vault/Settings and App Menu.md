---
tags: [tide, feature, ui]
---
# Settings and App Menu

Both copied from the official app by screenshot.

## Settings (`src/ui/settings.rs`)
Title, tabs **General / Cuenta / Acerca de**, section headings and dark
rounded cards (rows: bold title, grey description, control on the right).
- General: audio quality (radio rows, Max in gold), playback (normalize,
  sound output dropdown, WASAPI exclusive, force volume, autoplay), display
  (player background Color/Desenfoque, audio metadata, ratings, hide Uploads),
  Connect (Discord), preferences (UI zoom), legal links.
- Cuenta: profile, personal info, subscription, sign out.
- Acerca de: version, credits and licences, disclaimer.

## Hamburger menu (`src/ui/appmenu.rs`)
Archivo · Navegación · Reproducción · Ver · Ayuda, submenus on hover
(Repetir and Solución de problemas go one level deeper). Shortcuts:
Ctrl+P settings, Ctrl+H home, Ctrl+F search, Ctrl+S shuffle,
Ctrl+←/→ prev/next, Ctrl+Shift+←/→ seek, Alt+←/→ back/forward,
Ctrl +/−/0 zoom. The sidebar toggle moved to the `|←` button in the header.
