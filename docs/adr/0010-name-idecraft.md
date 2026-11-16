# ADR-0010: Project name IDECraft and separate brand domain

- **Status:** accepted · 2026-10-31 · supersedes the naming part of ADR-0001
- **Context:** The project owner set the official name **IDECraft**, repository `idecraft`, tagline "The IDE
  for developers.", and asked for repository presentation in the style of the ArtCraft project
  family while the application keeps IntelliJ IDEA Ultimate as its interaction reference.
- **Decision:**
  - Product `IDECraft`, repository `DamienDrash/idecraft` (renamed on GitHub 2026-10-31; old URLs redirect).
  - Two design domains: brand (README, docs, website) per `docs/design/`; application UI per
    `docs/design/APPLICATION_UI.md` and ADR-0009. They never share visual rules — except the
    colour palette and the app icon, which the application takes from the brand since 2026-11-04
    ([ADR-0018](0018-brand-palette-and-icon-in-the-app.md)).
  - Code identifiers follow: crates `idecraft-*`, binary `idecraft`, settings directory
    `.idecraft/`, MCP server name `idecraft` (renamed 2026-10-31 in a separate change).
- **Consequences:** The "…Craft" pattern is ArtCraft's family signature; every public surface
  carries an independence notice (BRAND_GUIDELINES.md §6, legal analysis §7 addendum). A
  trademark register check for "IDECraft" is still open (ROADMAP M0.5).
