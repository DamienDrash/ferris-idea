# Desktop application UI — separate design domain

**Status:** v2 · 2026-11-04 (v2: brand palette and app icon in the application, ADR-0018)

The IDECraft **desktop application** and the IDECraft **brand** are two design domains. Never mix
them — with one exception since 2026-11-04: the application takes the brand's **colour palette**
and **app icon** ([ADR-0018](../adr/0018-brand-palette-and-icon-in-the-app.md)).

| | Brand domain | Application domain |
|---|---|---|
| Applies to | README, docs, social images, website, release notes | the running IDE: windows, editor, tool windows, dialogs |
| Reference | ArtCraft presentation conventions (analysis) | IntelliJ IDEA Ultimate interaction and behaviour |
| Rules | [BRAND_GUIDELINES.md](BRAND_GUIDELINES.md), [DESIGN_SYSTEM.md](DESIGN_SYSTEM.md), [CONTENT_STYLE_GUIDE.md](CONTENT_STYLE_GUIDE.md) | this file, ADR-0009, ADR-0018, legal boundaries |
| Colours | `tokens.json` brand tokens | default theme `IDECraft Dark`/`IDECraft Light` built from the same tokens; the reference's themes stay selectable (see below) |
| Fonts | Inter, JetBrains Mono per DESIGN_SYSTEM.md | system UI font and JetBrains Mono in the editor (SPEC-0060) |

## Application reference

The application follows IntelliJ IDEA Ultimate for functional behaviour and interaction patterns:
project and workspace navigation, editor tabs, tool windows, database explorer, SQL consoles,
run and debug configurations, version control, search and navigation, keyboard-driven
workflows, settings and plugin management.

Decisions and limits:
- [ADR-0009](../adr/0009-look-and-feel.md): look and feel close to the reference using the
  **open** themes, icons, keymaps and Jewel metrics from `JetBrains/intellij-community`
  (Apache-2.0), recorded in [PROVENANCE.md](../legal/PROVENANCE.md).
- [RESEARCH_BOUNDARIES.md](../legal/RESEARCH_BOUNDARIES.md): no proprietary JetBrains assets,
  fonts, icons, source code or protected design material; proprietary areas get own icons in the
  same style.
- The application's own name, app icon and about screen use the IDECraft brand. The app icon
  master is [`assets/idecraft-app-icon.svg`](assets/idecraft-app-icon.svg); platform files are in
  [`packaging/icons/`](../../packaging/icons/README.md). The binary embeds the macOS icon and sets
  it at start-up, so a bare `idecraft ui` shows it in the Dock; Linux windows carry the
  application id `idecraft` for `idecraft.desktop`.
- [ADR-0018](../adr/0018-brand-palette-and-icon-in-the-app.md): the default theme `IDECraft Dark`
  (and its counterpart `IDECraft Light`) takes the brand tokens of
  [DESIGN_SYSTEM.md](DESIGN_SYSTEM.md): neutrals, foreground, accent for selection, focus, primary
  buttons, links and the project avatar, status colours. Only colours change; layout, sizes,
  metrics, in-window icons and behaviour stay IntelliJ-like. `Islands Dark`, `Islands Light`,
  `Dark` and `Light` stay selectable for side-by-side comparison with the reference. Spec:
  `SPEC-0060` v6 (contrast figures there).

## Current state

The desktop UI runs on GPUI ([ADR-0003](../adr/0003-ui-toolkit.md)); its specifications live in
`docs/specifications/` like every other feature. This file defines the boundary between the
domains.
