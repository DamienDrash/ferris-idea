# ADR-0002: License MIT OR Apache-2.0

- **Status:** superseded by [ADR-0032](0032-source-available-licence.md) on 2026-11-14 (PolyForm Shield 1.0.0, source-available) · was accepted on 2026-10-30 (GO-3). Nothing was distributed under this licence. The text below is kept as the record of the earlier decision.
- **Amendment (ADR-0006):** crates containing code ported from Apache-2.0 sources are licensed `Apache-2.0` only.
- **Context:** The project wants an open-source license once published. The Rust
  ecosystem convention is dual MIT/Apache-2.0 (also used by ArtCraft). All
  planned dependencies are compatible with it (see `deny.toml`).
- **Options:** MIT only (simplest, no patent grant) · Apache-2.0 only (patent
  grant, explicit trademark clause) · **MIT OR Apache-2.0** (users choose; widest
  compatibility, including GPLv2 projects via MIT) · copyleft (GPL/MPL; would
  constrain reuse and is not needed for a research project).
- **Decision:** MIT OR Apache-2.0. Apache-2.0 section 6 states that no trademark
  rights are granted, which fits the clean-room stance.
- **Consequences:** Contributions are accepted under the same dual license
  (stated in `CONTRIBUTING.md`). Before publication every file is checked for
  third-party content (`docs/legal/RESEARCH_BOUNDARIES.md`, section 4).
