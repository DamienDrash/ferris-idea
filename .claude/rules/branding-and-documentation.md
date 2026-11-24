---
paths:
  - "README.md"
  - "CHANGELOG.md"
  - "changelog.d/**"
  - "CONTRIBUTING.md"
  - "ARCHITECTURE.md"
  - "docs/**"
  - ".github/**"
---

# Branding and documentation

Applies to every public-facing change: README, docs, changelog and its fragments (`changelog.d/`), release notes, issue/PR
templates, GitHub metadata, website.

1. Follow `docs/design/` — BRAND_GUIDELINES.md (name `Ferris IDEA`, tagline, assets, independence
   notice), CONTENT_STYLE_GUIDE.md (English, voice, status words), DESIGN_SYSTEM.md (tokens,
   typography, screenshots, accessibility), README_STRUCTURE.md (section order, checklist).
2. Claims must match current `main`. Use only the status words Works / Experimental /
   In progress / Planned / Not planned. Never write planned features in present tense.
3. Screenshots and demos only from the real application, with a caption naming the commit or
   command. No mockups presented as product.
4. No JetBrains or ArtCraft logos, icons, artwork, screenshots or copied text; name those
   products only descriptively. Keep the independence notice in the README.
5. At most three static factual badges (stage, license, verified platform; DESIGN_SYSTEM §8). No
   fake CI, coverage, download or release links. Ferris IDEA is source-available: never write
   "open source".
6. Two design domains: these rules do not apply to the desktop application UI, which follows
   `docs/design/APPLICATION_UI.md`, ADR-0009 and ADR-0018. Exception (ADR-0018, 2026-11-04): the
   application's colour palette (`Ferris IDEA Dark`/`Ferris IDEA Light`) and its app icon come from the
   brand tokens and assets in `docs/design/`; keep them in step when the tokens change. Do not
   change application code in a docs task.
7. `ROADMAP.md` in this repository is the roadmap for readers, in English. The project's internal
   tracking files are German and are not part of this repository.
8. Run `scripts/check-docs.sh` before committing documentation changes.
