# Design

Two separate domains. Never mix them.

**Brand domain** — README, documentation, social images, future website. Presentation inspired by the ArtCraft project family.

- [BRAND_GUIDELINES.md](BRAND_GUIDELINES.md) — name, tagline, symbol, assets, trademark boundaries, independence notice
- [DESIGN_SYSTEM.md](DESIGN_SYSTEM.md) — colour tokens, typography, layout, screenshots, diagrams, accessibility ([tokens.json](tokens.json))
- [CONTENT_STYLE_GUIDE.md](CONTENT_STYLE_GUIDE.md) — voice, claims, feature status terms, release notes
- [README_STRUCTURE.md](README_STRUCTURE.md) — section order and merge checklist
- ArtCraft reference analysis — verified reference observations with sources
- [assets/](assets/) — symbol masters, wordmark, horizontal logo, app icon, avatar, social preview (brand package v1.1.5)

**Application domain** — the desktop IDE. Interaction reference: IntelliJ IDEA Ultimate. Since
ADR-0018 its default theme and app icon use the brand palette and icon; layout and behaviour stay
IntelliJ-like.

- [APPLICATION_UI.md](APPLICATION_UI.md)
