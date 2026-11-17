# Content style guide

**Status:** v3 · 2026-11-14 · for README, docs, release notes, issue and PR templates, website.

## 1. Language

- Public documentation is **English** (US spelling: "color", "license" as noun and verb).
- Exceptions, kept as they are and labelled: the legal analysis under German law
  (`docs/legal/`, German, with an English summary in `docs/legal/README.md`) and the internal
  tracking files `ROADMAP.md` / status file (German format required by the project's tracking tool).
  The roadmap for readers outside the project is `docs/public/ROADMAP.md`, in English.

## 2. Voice

- Short, declarative headings: "Databases, Built In", "Built in Rust", "Build from Source".
  Title Case for the README's H2 and H3 headings, sentence case for docs headings.
- Active verbs, present tense, second person: "Split a script into statements."
- Concrete over abstract: name the command, the file, the number.
- Product first: what you can do, then how it works, then why.
- No marketing superlatives. Banned: "blazing", "world's fastest", "revolutionary", "seamless",
  "perfect replacement", "100 % compatible", "production-ready" (until the criteria in §5 hold).
- No emoji in headings or body text.

Good: "Explore schemas, write SQL, and work with data." / "One Workspace. Every Language."
Bad: "The world's fastest IDE." / "A perfect IntelliJ replacement."

## 3. Claims

- Every capability claim must be true for the current `main` branch and backed by a test, a
  command the reader can run, or a linked document.
- Numbers carry their kind: *measured* (with method link) or *estimated*.
- Comparisons to the reference products describe targets, never achievements.
- A number names the commit or date it was taken at, in the sentence or in the caption below it.
- License wording: IDECraft is **source-available**. Never "open source", "free software",
  "FOSS" or "OSI-approved"; "free" never stands alone. A summary of the license says that it is
  a summary and that the license text decides.
- Agent and security wording names the mechanism and its limit, not an adjective: "rights per
  workspace", "loopback only, token per start", "no sandbox for started programs". The allowed
  and banned phrases are in BRAND_GUIDELINES §3.
- Parity is a "feature-presence score": the project's own weighted checklist of how much of the
  reference's functional scope exists. Never a quality claim, never "replace". It is stated once
  in the README's status section, not on the first screen; the first screen uses the count of
  rows compared side by side.
- Each number is stated once on a page, with its kind and date, and referred to elsewhere.
- A rule is written as a rule ("the rule is: every feature has a specification"), not as a
  verified fact about every case.
- Statements only the project owner can confirm (who reviewed what, who he is) are not written
  as facts by agents; the text says what the repository records.

## 4. Feature status terminology

Use exactly these words, in this order of maturity:

| Status | Meaning | Evidence required |
|---|---|---|
| **Works** | implemented, tested, usable from the CLI or app today | tests in CI, command in docs |
| **Experimental** | implemented but incomplete or unstable; may change | at least one test |
| **In progress** | code exists on a branch or partially on `main`, not usable yet | link to PR or task |
| **Planned** | specified or on the roadmap, no code | roadmap or spec link |
| **Not planned** | deliberately out of scope | reason |

Never mix states in one sentence ("supports Java" when only file detection works). Say
"Java: file detection works; language server integration is planned."

## 5. Project maturity

Current stage: **beta** (since v0.20.0, 2026-11-17; alpha since v0.2.0, 2026-10-31). The stage is a constant in
`crates/core/src/release.rs`, shown by `idecraft --version` and the About dialog, and written as
the `Stage:` line of every version in `CHANGELOG.md`.

| Stage | Criteria (all must hold) | Reached |
|---|---|---|
| **pre-alpha** | code exists; no desktop window that edits files | v0.1.0 (2026-10-30) |
| **alpha** | the desktop UI opens, edits and saves files for the priority languages (Python, SQL, JavaScript, TypeScript, Java; SPEC-0004), backed by the window self-test and a hand test | v0.2.0 (2026-10-31): editor view and workbench window (#65, #66), saving with local history (#78), highlighting for all priority languages (#27), window self-test 32/32, hand-test rounds 1 and 2 passed |
| **beta** | daily use for one priority workflow is possible, shown by: (1) every hand test of that workflow in the current round of the hand-test log passed in an agent run (computer use or Remote Robot, marked as agent run; the product owner delegated beta testing to the agent on 2026-11-06), with no failed hand test or open defect in `docs/testing/` left for it; (2) the window self-test (`scripts/ci-window.sh`) green on the commit that is tagged; (3) the workflow's feature-matrix rows at parity stage 3 or higher in `docs/research/PARITY.md`; (4) the agent records the switch with this evidence in the status file and the `Stage:` line of the version | v0.20.0 (2026-11-17), for both priority workflows: all hand tests of (a) and (b) in round 5 passed in agent runs, window self-test 1,065/0 on `v0.19.0`, rows of both workflows at stage 3 or higher (CHANGELOG v0.20.0) |
| **1.0** | beta criteria for every priority workflow, a published release (GO-4) | not reached |

Priority workflows: (a) a project in a priority language: open, edit, navigate, run or debug, and
commit with Git; (b) SQL work against PostgreSQL or MySQL: connect, browse the schema, run
queries and edit data. "Production-ready" is not used before a 1.0
release. A stage changes only in a version whose changelog section names the evidence.

## 6. Technical writing rules

- One sentence per idea; paragraphs of at most three sentences in the README.
- Code, commands, paths, crate names in backticks; commands copy-pasteable and tested.
- Link instead of repeating: each fact has one home (see [docs/README.md](../README.md)).
- Dates in ISO format (2026-10-31).
- Trademarks: plain text, descriptive use only (BRAND_GUIDELINES.md, §5).

## 7. Release notes

Format in [`CHANGELOG.md`](../../CHANGELOG.md): one section per version, newest first, headed
`## IDECraft vX.Y.Z — YYYY-MM-DD`, followed by a line `Stage: <stage>` (§5) and the groups
*Added*, *Changed*, *Fixed*, *Removed* and *Security* (a change that protects data or closes a
weakness), each line a user-visible change in the active voice ending
with its pull request numbers, e.g. `(#123)`. Internal work (observations, specifications only,
status records, CI internals) is left out.

Versioning:

- [Semantic Versioning](https://semver.org/); `0.x` until 1.0. The only source of the version is
  `[workspace.package] version` in `Cargo.toml`; every crate, `idecraft --version`, the About
  dialog, problem reports and the MCP `serverInfo.version` read it.
- Every pull request with a user-visible change adds its lines as a fragment
  `changelog.d/<PR number>.md` in the format above, under its groups (`### Added` …), each line
  ending with that number; it does not edit `CHANGELOG.md`, so pull requests do not conflict.
  [`changelog.d/README.md`](../../changelog.d/README.md) has the rules, including the name a
  branch uses before its pull request exists, and `scripts/changelog.sh check` checks them.
- At the end of each working day with fragments or entries under `## Unreleased`, cut a minor
  version: first `scripts/changelog.sh assemble` moves the fragments under `## Unreleased`
  (newest pull request first in each group) and deletes them; then raise the
  version in `Cargo.toml` (and `Cargo.lock`), rename `## Unreleased` to
  `## IDECraft vX.Y.Z — YYYY-MM-DD` with its `Stage:` line, start a new empty `## Unreleased`,
  and after the merge put an annotated tag `vX.Y.Z` on that merge commit and push it. The date is
  the merge date on `main` in Europe/Berlin.
- A patch version (`vX.Y.Z+1`) is only for a fix to an already tagged version made outside the
  daily cut.
- `cargo test` checks that the newest changelog version equals the `Cargo.toml` version and that
  its stage equals the stage constant (`crates/core/src/release.rs`).

GitHub releases use the title `IDECraft vX.Y.Z` and the same text. No GitHub release exists yet;
publishing needs the product owner's approval (GO-4).
