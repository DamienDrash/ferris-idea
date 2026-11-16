# ADR-0004: Clean-room process and traceability

- **Status:** accepted · 2026-10-30 · amended by ADR-0006 (open sources may be ported; clean room applies to proprietary parts)
- **Context:** The project reimplements functionality of a proprietary IDE.
  Copyright protects expression, not functionality (§ 69a Abs. 2 UrhG; CJEU
  C-406/10). The process must make independence verifiable.
- **Decision:** Three separated artefact types — observations (`OBS-*`,
  `docs/research/observations/`), neutral specifications (`SPEC-*`,
  `docs/specifications/`), implementation (`crates/`, `tests/`). Every spec
  statement carries a source type (`GEN`, `STD`, `OSS`, `DOC`, `OBS`). Code
  references specs, tests reference acceptance criteria. One agent may hold
  several roles; commits are prefixed `obs:`, `spec:`, `impl:` and never mix.
- **Consequences:** `docs/specifications/TRACEABILITY.md` links spec → code →
  tests → feature matrix rows. Reviews check for foreign identifiers or text.
- **Detail:** `docs/legal/RESEARCH_BOUNDARIES.md`.
