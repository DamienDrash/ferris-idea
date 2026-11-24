# ADR-0018: Brand palette and app icon in the desktop application

- **Status:** accepted · 2026-11-04 · supersedes ADR-0009 and ADR-0010 in part (colours and app
  icon of the running application)
- **Context:** Until now the desktop application took every colour from the reference's open
  themes (ADR-0009) and the brand (`docs/design/`) applied only to the repository, the docs and
  the packaged icons (ADR-0010, `APPLICATION_UI.md`). IDECraft is often started as a bare binary
  (`ferris-idea ui <folder>`), so macOS showed a generic Dock icon, and the windows looked like the
  reference's product. On 2026-11-04 the project owner decided to bring brand identity v1.1.5 into the
  running application with the words "Icon und Markenfarben".
- **Decision:**
  - **Icon:** the packaged brand icon (`packaging/icons/macos/FerrisIDEA.icns`) is embedded in the
    binary and set as the application icon at start-up on macOS. On Linux the windows carry the
    application id `ferris-idea`, which `ferris-idea.desktop` names as `StartupWMClass`; GPUI 0.2 has no
    call to set a window icon. The embedded `Info.plist` names `FerrisIDEA.icns` (`CFBundleIconFile`)
    for a future app bundle.
  - **Colours:** two own themes, `Ferris IDEA Dark` and `Ferris IDEA Light`, take the brand tokens of
    `DESIGN_SYSTEM.md` (Nord palette): neutrals for window, islands and popups, foreground, the
    accent for selection, focus, primary buttons, links, the project avatar and the selected tab,
    and the status colours. They are own files whose parents are `Islands Dark` and `Islands
    Light`; they override colours only. `Ferris IDEA Dark` is the default for new installs; a theme
    the user already chose is kept.
  - **Unchanged:** layout, sizes, metrics, icons inside the windows, keymaps and behaviour stay
    those of the reference (ADR-0009). `Islands Dark`, `Islands Light`, `Dark` and `Light` stay
    bundled and selectable, so the windows can still be compared with IntelliJ side by side.
- **Reason:** a running Ferris IDEA should be recognisable as Ferris IDEA (Dock, window colours), not as
  a copy of the reference's product. Colours are the part of the look that carries a brand without
  changing how the IDE is used. It also lowers the UWG risk noted in ADR-0009.
- **Consequences for 1:1 parity:** parity is measured for layout, sizes and behaviour, which stay
  IntelliJ-like. Colour comparisons with the reference (hand test 10, OBS-0007) are made with
  `Islands Dark` chosen. Text colours of the brand themes meet WCAG AA (4.5 : 1), checked by a
  unit test (`SPEC-0060` v6). Brand rule 6 (`.claude/rules/branding-and-documentation.md`) and
  `docs/design/APPLICATION_UI.md` now let the palette and the icon cross into the application;
  typography, voice and the rest of the brand domain still do not.
