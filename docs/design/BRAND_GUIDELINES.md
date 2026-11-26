# Ferris IDEA brand guidelines

**Status:** v4 · 2026-11-22 (new name Ferris IDEA, §1, §2 and §4; stage stable in the descriptions; symbol, logo, app icon and social preview to be redrawn) · v3 2026-11-14 (brand package v1.1.5; positioning as IDE AI harness and source-available wording, §1 and §3) · applies to the repository, documentation and any future project website.
The desktop application has its own rules: [APPLICATION_UI.md](APPLICATION_UI.md).

## 1. Identity

| Item | Value |
|---|---|
| Product name | **Ferris IDEA** (formerly IDECraft, renamed on 2026-11-22) |
| Technical short name | `ferris-idea` (program, crates `ferris-idea-*`, config folder `~/.config/ferris-idea`, project folder `.ferris-idea/`, environment prefix `FERRIS_IDEA_`, bundle identifier `dev.ferrisidea.FerrisIDEA`); in use from rebranding step 2, until then the technical names are still `ferris-idea` (§2) |
| Repositories | public `ferris-idea` (`github.com/DamienDrash/ferris-idea`), development `ferris-idea-dev` (private) |
| Primary tagline | **Your IDE. Your database. Your model.** (chosen by the project owner on 2026-11-16 from four proposals; it replaces "The IDE for developers." and the proposal of 2026-11-14) |
| Tagline line two | A Rust-native IDE with a built-in SQL client, where you and the AI you choose build software together. ("The AI you choose": a local model or any endpoint through the AI Assistant's OpenAI-compatible setting, or an agent such as a CLI tool over MCP; see License term) |
| Secondary description | Rust-native. Rights granted per workspace. You keep the window. (short descriptor for places without room for line two; the README head uses line two) |
| Category term | **IDE AI harness**, always introduced as "what we call" or "our term": the project coined it (definition in the README section "What an Agent Can Do with It") |
| GitHub description | Ferris IDEA: an open-source, Rust-native IDE for code and databases whose operations are MCP tools for AI agents. Stable. (set by the project owner's coordinator on 2026-11-22; "source-available" replaced by "open-source" on 2026-11-25, ADR-0038) |
| GitHub topics | `rust`, `ide`, `code-editor`, `database-tools`, `sql`, `developer-tools`, `mcp`, `ai-agents`, `open-source`, `stable` (`open-source` replaced `source-available` on 2026-11-25, ADR-0038). The topic `alpha` was replaced by `stable` on 2026-11-22 (stage stable since v1.0.0, CONTENT_STYLE_GUIDE §5). The earlier topic `clean-room` is dropped: the bare word promises more than the record shows (the roles are separated by commit and document type and taken by AI agents, not by separated teams) |
| License term | **open source**, Apache License 2.0 (`Apache-2.0`) since 2026-11-25 (ADR-0038; before: source-available under PolyForm Shield 1.0.0). Never "source-available" for the current version; "free" never stands alone (it can mean price or freedom) |

Brand package v1.1.5 changes symbol, logo, app icon and colours; name, tagline and descriptions stay (confirmed by the product owner on 2026-11-04: the package wording "Deterministic Rust IDE & AI Agent Platform" is not used until `main` demonstrates it). On 2026-11-06 the product owner dropped "experimental" from the descriptions and topics, since the project stage is alpha (CONTENT_STYLE_GUIDE §5); per-feature status still uses the terms in §4.

On 2026-11-14 the project owner ordered that Ferris IDEA is presented openly as an **IDE AI harness**
and that the license becomes source-available (replaced by Apache-2.0 on 2026-11-25, ADR-0038). The harness wording is bound to what `main`
demonstrates: tools over MCP, rights per workspace and database permissions per connection, both
granted by the user outside the project, dry run where a tool has one, project trust, a local log
of every changing call, two transports. The words "platform", "agent platform" and
"deterministic" stay unused, and everything beyond that list (hosting agents, orchestration,
benchmark, change sets) is written as Planned.

Character: developer-focused, technically precise, open about its sources and its gaps, minimal, functional,
independent, transparent about its maturity stage (stable, see CONTENT_STYLE_GUIDE §5).

## 2. Naming

Rename note (2026-11-22): the project owner renamed the product from IDECraft to Ferris IDEA. Step 1 <!-- rebrand:keep -->
changed the name everywhere it is shown (documentation, application texts, metadata, repository
URLs) with a mechanical rewrite; step 2 changes the technical names and takes over the
existing `~/.config/ferris-idea` and `.ferris-idea/` folders once. The released sections of
`CHANGELOG.md` keep the old name. The coordinator recommended a trademark search before
publication (see [TRADEMARKS.md](../../TRADEMARKS.md)). <!-- rebrand:keep -->

- Always `Ferris IDEA`: two words, capital F, "IDEA" in capitals, one space, no hyphen. Not
  "FerrisIDEA", "Ferris Idea", "Ferris-IDEA", "ferris IDEA". "Ferris" is the Rust mascot (CC0),
  "IDEA" alludes to IntelliJ IDEA; the name is never written so that it suggests a JetBrains
  product (§3, §5).
- Lowercase `ferris-idea` only where the medium requires it (repository, URLs, package names).
- Code identifiers: after rebranding step 2 they use `ferris-idea` (crates `ferris-idea-*`, binary
  `ferris-idea`, workspace settings in `.ferris-idea/`, MCP server name `ferris-idea`, title
  `Ferris IDEA`). Until step 2 is merged the code still uses `ferris-idea` (crates `ferris-idea-*`, binary
  `ferris-idea`, `.ferris-idea/`, MCP server name `ferris-idea`); text that names such an identifier writes
  it as it is today.
- Feature names are plain descriptions ("SQL script splitting", "MCP server"), not sub-brands.

## 3. Positioning

Never "clone", "replacement", "replace", "1:1", "compatible with" or "drop-in" for the relation
to the reference; the measure is a "feature-presence score", not a statement that Ferris IDEA can
stand in for the reference. "Clean-room" is used only qualified: "follows clean-room principles;
the roles are separated by commit and document type and taken by AI agents, not by separated
teams".

Ferris IDEA is an independent research project that rebuilds the functional scope of a full
polyglot IDE with integrated database tools in Rust, and offers that IDE's own operations to AI
agents as tools under rights the user grants (an IDE AI harness). We describe the reference products
(IntelliJ IDEA Ultimate, DataGrip) only to explain *what* functionality is targeted.

Allowed: "Ferris IDEA targets the functional scope of IntelliJ IDEA Ultimate, including database tooling."
Not allowed: "an IntelliJ replacement", "the open-source IntelliJ", "IntelliJ in Rust", "by ArtCraft".

Harness wording. Allowed: "312 of its operations are tools an agent calls over MCP" (a measured
count, given once), "every feature we build gets a tool" (a rule), "rights per workspace",
"a part of the tools previews by default", "any MCP client; tested with Claude Code", and, since
ADR-0031 and SPEC-0053 v8 are on `main`: "every tool that changes something checks a right",
"a project file never grants an agent anything", "every changing call leaves a line in the local
log". Each of the three is said together with its limits from SPEC-0053 behaviour 14 (tests read
the source; formatters, linters and language servers start under trust alone; the log holds no
contents). Not allowed: "every
capability is a tool", "where a preview is possible", "nothing runs without a right",
"everything is logged", "audit trail", "tamper-proof", "safe", "secure",
"quantum-safe", "audited", "works with any agent", "better for agents than a shell", tool counts
next to those of other products. Remote paths: say what is enforced and where the code is, with
the sentence that no independent audit exists.

## 4. Symbol and assets

**To be redrawn (2026-11-22).** The symbol, wordmark, horizontal logo, app icon, avatar and social
preview below belong to the old name: the bracket is a **C** around an **I**, the wordmark reads
"IDECraft". They stay in use, unchanged and under their current file names, until the new <!-- rebrand:keep -->
artwork for Ferris IDEA replaces them in a separate change; do not draw substitutes, and do not
rename the asset files before then. The table describes the current files. <!-- rebrand:keep -->

Source: brand package **IDECraft Brand Identity v1.1.5** (2026-11-03, <!-- rebrand:keep --> release status *conditional*,
see the open items below). The SVG masters in [`assets/`](assets/) are byte-identical copies of the
package files; never edit their paths.

Symbol: an outer bracket open to the right (**C**, *Craft*) around a centred rectangular core
(**I**, *IDE*). The bracket stands for the compiler and system boundary, the core for the active
cursor at the centre of the work. Square 32-unit grid, straight edges only, no curves.

| Asset | File | Spec | Use |
|---|---|---|---|
| Status of every row below | to be redrawn for Ferris IDEA | | |
| Symbol, dark surfaces | [`assets/idecraft-symbol.svg`](assets/idecraft-symbol.svg) | 32×32 grid, bracket `M28 4H4V28H28V23H10V9H28V4Z` in `foreground-primary`, core `M15 12H19V20H15V12Z` in `accent-primary`, transparent | 24 px and larger |
| Symbol on its own background | [`assets/idecraft-symbol-dark.svg`](assets/idecraft-symbol-dark.svg) | 32×32 on `background-primary`: the 28×19 crab (one rectangle per run of the grid, absolute path data) in `foreground-primary`, the cursor in `accent-primary`; generated by `scripts/brand-assets.py` (2026-11-23), the startup animation's background | where the host surface is unknown |
| Symbol, light surfaces | [`assets/idecraft-symbol-light.svg`](assets/idecraft-symbol-light.svg) | same geometry, bracket `background-primary`, core `accent-mark-light` | 24 px and larger |
| Symbol, one colour | [`assets/idecraft-symbol-monochrome.svg`](assets/idecraft-symbol-monochrome.svg) | bracket and core in `foreground-primary` | dark surfaces only |
| Micro symbol 16 px | [`assets/idecraft-symbol-micro.svg`](assets/idecraft-symbol-micro.svg) | 32 grid snapped to 2-unit steps (2 px strokes at 16 px) | 16 px only, never as app icon |
| Micro symbol 20 px | [`assets/idecraft-symbol-micro-20.svg`](assets/idecraft-symbol-micro-20.svg) | native 20×20 grid, 2 px strokes | 20 px only |
| Wordmark | [`assets/idecraft-wordmark.svg`](assets/idecraft-wordmark.svg) | "Ferris IDEA" as outlined paths (absolute, no transforms), viewBox 76.5×32, cap height 10 from y 11 to the baseline at 21, 2 units on either side; "Ferris" in `foreground-primary`, "IDEA" in `accent-primary`; generated by `scripts/brand-assets.py` (2026-11-23) | dark surfaces |
| Horizontal logo | [`assets/idecraft-horizontal.svg`](assets/idecraft-horizontal.svg) | crab symbol + wordmark, viewBox 109×32, the wordmark's paths in a group translated by 32.5, 4.5 units between the crab and the wordmark (the proportions of the logo); generated by `scripts/brand-assets.py` (2026-11-23), the startup animation draws from it | dark surfaces |
| App icon | [`assets/idecraft-app-icon.svg`](assets/idecraft-app-icon.svg) | 512×512, rounded square (rx 100) in `background-primary` with a 4-unit `background-secondary` stroke, symbol ×9 | the desktop application only |
| Square avatar | [`assets/idecraft-avatar.svg`](assets/idecraft-avatar.svg), [`.png`](assets/idecraft-avatar.png) | 512×512, `background-primary`, symbol at 50 % width (safe for circular crops) | GitHub organisation/profile image |
| Social preview | [`assets/idecraft-social-preview.svg`](assets/idecraft-social-preview.svg), [`.png`](assets/idecraft-social-preview.png) | 1280×640, self-contained (2026-11-22): background `background-primary` with a 40-unit grid in `border` at 35 %, the tagline in three lines (76 px bold, the third in the accent), below it "Stable · Rust-native · Built-in SQL client" in the mono font, the crab at 16 px per cell over a faint editor frame on the right, "Ferris IDEA" at the bottom left; generated by `scripts/brand-assets.py` | GitHub social preview |
| Repository banner | — | not used; the README starts with the symbol and a text H1 | — |
| Documentation illustrations | — | functional diagrams only (DESIGN_SYSTEM.md, §6); no decorative artwork | — |

Platform app icons (macOS `.icns`, Windows `.ico`, Linux hicolor PNGs and desktop entry) built from
the app icon live in [`packaging/icons/`](../../packaging/icons/README.md).

Rules
- Pick the master by size: 16 px micro, 20 px micro-20, 24 px and larger the primary symbol.
- Clear space: at least 8 units of the 32 grid (a quarter of the symbol) around the visible
  outline, for the symbol and the horizontal logo alike. The 4.5 units between the crab and the
  wordmark are internal to the lockup. The host page provides the clear space.
- The wordmark and the horizontal logo exist only for dark surfaces. On light pages place them on
  a `background-primary` panel; do not invent a light colour variant.
- Do not recolour beyond the token pairs, redraw the wordmark from a font, add gradients, shadows
  or outlines, rotate, or centre the core differently. The core sits deliberately right of centre.
- The PNG files are rendered from the SVGs; regenerate them when an SVG changes.
- Symbol, wordmark, horizontal logo, app icon, avatar and social preview were made when the
  product was called IDECraft.
  They are not covered by the code licence: all rights in them are reserved, and
  [TRADEMARKS.md](../../TRADEMARKS.md) says how they may be used. The project owner had released
  them on 2026-11-07 under the project's licence of that time and withdrew that release on
  2026-11-16, before the project was published.

Wordmark provenance: the current wordmark shows the old name IDECraft, created by the project owner;
the name Ferris IDEA was chosen by the project owner on 2026-11-22. The wordmark was drawn as
outlined SVG paths during the AI-assisted brand work; no source typeface or glyph template is
documented, so it is never attributed to a font. Its use follows
[TRADEMARKS.md](../../TRADEMARKS.md), like the other brand assets.

Open items of package v1.1.5 (tracked in the status file)
- Native checks of the app icon in macOS Finder/Dock, Windows Explorer/taskbar and a Linux desktop
  are not verified yet.

## 5. Trademark and attribution boundaries

- Never use JetBrains, IntelliJ, DataGrip, ArtCraft, Crafting Apps or storytold logos, icons,
  artwork, colours schemes as brand elements, or screenshots.
- Name those products only in plain text and only to describe the functional reference or the
  presentation inspiration.
- Open-source material from `JetBrains/intellij-community` used *in the application* follows
  [`docs/legal/APACHE_PORTING.md`](../legal/APACHE_PORTING.md); it is never part of the Ferris IDEA brand.

## 6. Independence notice

Every public entry point (README, website, release notes) carries this text or a shorter form
that keeps both sentences:

> Ferris IDEA is an independent research project. It is not affiliated with, endorsed by, or
> sponsored by JetBrains or the ArtCraft team. JetBrains, IntelliJ IDEA and DataGrip are
> trademarks of JetBrains s.r.o.; ArtCraft is a trademark of the ArtCraft Team.
