# ADR-0006: Use of IntelliJ Platform sources under Apache-2.0

- **Status:** accepted · 2026-10-30 (after the project owner's legal consultation) · amended by [ADR-0032](0032-source-available-licence.md) on 2026-11-14: a crate with ported code is `LicenseRef-PolyForm-Shield-1.0.0 AND Apache-2.0`, not `Apache-2.0` only; the ported files stay under Apache-2.0 · since [ADR-0038](0038-open-source-licence-apache-2-0.md) (2026-11-25) the whole project is Apache-2.0, so such a crate is `Apache-2.0` again
- **Context:** Large parts of the reference platform are open source in
  `JetBrains/intellij-community` under Apache-2.0: platform, Java, Kotlin,
  Python Community, Git, terminal, the data `grid`, the MCP server plugin,
  themes, icons, keymaps and Jewel. Porting them to Rust is a derivative work
  (§ 69c Nr. 2 UrhG), which Apache-2.0 permits under the conditions of its
  section 4. Proprietary parts (Database Tools, JS/TS, Spring, …) are not in
  the repository.
- **Decision:** Open parts may be ported or reused. Each use is recorded in
  `docs/legal/PROVENANCE.md`; ported Rust files carry an SPDX/attribution
  header; crates containing ported code are licensed `Apache-2.0`; `NOTICE`
  carries the upstream notice. Proprietary parts stay clean-room (ADR-0004).
  Reference commit: `d4018fb57c367e7b5d895bd8a0c84c382efd0e17`.
- **Consequences:** Faster, more faithful behaviour and look for open areas.
  Supersedes the "no adoption" rule in ADR-0004 for open sources only. JVM
  plugins cannot be loaded; reuse means porting logic. Procedure:
  `docs/legal/APACHE_PORTING.md`.
