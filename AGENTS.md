# Agent instructions

Binding for every AI agent working in this repository. `CLAUDE.md` is a link to this file.

1. Read `docs/legal/RESEARCH_BOUNDARIES.md` before any task. It overrides convenience. When
   unsure: don't do it, say so in the pull request or in an issue, and continue with independent
   work.
2. Never decompile, unpack or inspect the internals of the reference IDE; never touch licence or
   trial data. Open sources from `JetBrains/intellij-community` may be ported: follow
   `docs/legal/APACHE_PORTING.md` (header, `docs/legal/PROVENANCE.md`, crate licence).
3. Roles: observation, specification (`docs/specifications/`), implementation (`crates/`,
   `tests/`). The records of observing the reference are kept in the project's working repository
   and are not published here. One role per commit, prefix `obs:` / `spec:` / `impl:`. For
   proprietary features the implementation reads only specifications; open sources may be read and
   ported.
4. The roadmap for readers is ROADMAP.md; the project's internal tracking is not part
   of this repository. Claim progress only with evidence (test count, commit, file).
5. No credentials in files, commits, logs or answers.
6. Checks before every commit: `scripts/check-pr.sh --changed-only` (format and lints for the whole
   workspace, tests of the crates the change touches); `scripts/check-docs.sh` for documentation
   alone. The public CI (workflow) runs format, lints, the docs check and
   the workspace tests on every pull request.
7. Releases, publication and the repository's settings are the project owner's decisions. An agent
   never signs in to the reference vendor's account or services.
8. No licence checks, trials or telemetry in the product (ADR-0007). New features get an MCP tool
   (ADR-0008). Look and feel per ADR-0009.
9. Weaknesses found in the reference (slow, wasteful, unsafe, error-prone, not automatable) are
   never copied 1:1: add an `IMP-nnnn` entry to `docs/specifications/IMPROVEMENTS.md` and state the
   improvement as a deviation in the specification.
10. Public-facing text (README, docs, changelog, templates, GitHub metadata) follows
    `.claude/rules/branding-and-documentation.md` and `docs/design/`. The brand rules never apply to
    the desktop application UI.
11. Every pull request with a user-visible change adds its lines as `changelog.d/<PR number>.md`
    (see `changelog.d/README.md`), not in `CHANGELOG.md` (format:
    `docs/design/CONTENT_STYLE_GUIDE.md` §7).
12. Security first: wherever a design, default or dependency choice affects security, take the more
    secure option; a weaker option needs the project owner's explicit decision. Remote paths follow
    ADR-0025.
