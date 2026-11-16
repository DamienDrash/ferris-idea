# ADR-0009: Look and feel close to the reference

- **Status:** accepted · 2026-10-30 · amended 2026-11-04 by [ADR-0018](0018-brand-palette-and-icon-in-the-app.md):
  the colours of the default theme and the app icon follow the IDECraft brand; layout, sizes and
  behaviour stay as decided here
- **Context:** The product should look and feel as close as possible to the
  reference. The New UI themes (`expUI`, Islands), Darcula/Light, the icon set
  (`platform/icons`, incl. `expui`) and Jewel (`platform/jewel`, a standalone
  implementation of the New UI look) are Apache-2.0; JetBrains Mono and Inter
  are OFL-1.1.
- **Decision:** `crates/ui` loads the open theme JSON files and New UI SVG icons
  (recorded in PROVENANCE.md) and takes metrics (spacing, radii, font sizes)
  from Jewel. Default keymap is a port of the open default keymaps (`$default.xml`,
  `Mac OS X 10.5+.xml`). Layout follows the reference: tool-window stripes,
  main toolbar, editor tabs, status bar. Areas whose assets are proprietary
  (database explorer, JS/TS, Spring) get own icons in the same style.
- **Not taken:** product name, logos, app icon, splash, any proprietary icon.
- **Amendment (2026-11-04, ADR-0018):** the default theme is `IDECraft Dark`, own colours from the
  brand tokens over `Islands Dark`; the reference's themes stay bundled for comparison. The app
  icon is IDECraft's own (brand package v1.1.5) and is set at start-up.
- **Consequences:** F-018 (compatible keymap) becomes permitted. Visual
  closeness increases UWG risk only on publication; own name and app icon
  remain distinguishing.
