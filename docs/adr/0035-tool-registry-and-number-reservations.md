# ADR-0035: A tool module registers itself; specification numbers are reserved before use

- **Status:** accepted · **Version:** 1 · **Date:** 2026-11-18 · written on the Project Owner's
  request of 2026-11-18: parallel pull requests that add MCP tools conflict in the same shared
  places, and agents that pick "the next free" specification number collide.
- **Related:** [ADR-0008](0008-mcp-server.md) (MCP server),
  [ADR-0030](0030-ci-strategy.md) (checks), [ADR-0031](0031-project-files-never-grant-agents.md)
  (every changing effect has a right, held by a test),
  [SPEC-0053](../specifications/SPEC-0053-agent-permissions.md) (the effects table),
  [SPEC-0220](../specifications/SPEC-0220-document-registry.md) (the open-files lists)

## Context

A new MCP tool had to be named, by hand, in up to eight shared places: the sum of routers in
`all_tools()`, the effects table in `crates/mcp/src/effects.rs`, the open-files lists in
`crates/mcp/src/open_files.rs`, the list of handler sources in `crates/mcp/tests/it/effects.rs`, the
full list of tool names in `crates/mcp/tests/it/protocol.rs`, the tool count in the README and rows
at the end of `docs/specifications/TRACEABILITY.md`. On 2026-11-18 thirteen open pull requests each
added tools this way, and each merge made the others conflict. Specification numbers collided too:
two branches wrote `SPEC-0350` for different topics.

The lists exist for a reason: a tool without a row in the effects table, a changing tool without a
right, and a file-changing tool in no open-files list must fail a test (ADR-0031). That safety has to
stay; only the place where a tool's entries are written moves.

## Decision

1. **Routers are found by a build script.** `crates/mcp/build.rs` reads every source under
   `crates/mcp/src/` and writes `tool_registry.rs` into the build folder: every
   `#[tool_router(router = NAME, vis = "pub(crate)")]` (and the crate root's `#[tool_router]`), sorted
   by name. `IdeServer::all_tools()` joins them. A router of a module without `vis` or a router name
   used twice stops the build with a message that says what to write. A test fails when a tool name
   is in two routers, because joining them would drop one silently.
   - Chosen over `inventory` and `linkme`: both rely on the linker keeping registrations in an
     otherwise unreferenced object file, which depends on the platform and the link step, and both
     are new dependencies. The build script needs no dependency, works the same on every platform,
     and its output is sorted, so the server's start does not depend on link or file system order.
2. **A module classifies its own tools.** Next to its router, a module declares
   `pub(crate) const TOOL_EFFECTS: &[ToolEffect]` (rows written with `effects::tool` and the check
   constants of `effects.rs`) and, when its tools change files, `pub(crate) const
   THROUGH_BUFFER_TOOLS: &[&str]` or `pub(crate) const OWN_FILES_TOOLS: &[(&str, &str)]`. The build
   script finds them like the routers; `effects::TOOLS`, `open_files::THROUGH_BUFFER` and
   `open_files::OWN_FILES` join them once, sorted. The tools of `lib.rs` keep their rows in
   `effects.rs` and `open_files.rs`. `REFUSES_UNSAVED` stays one list in `open_files.rs`: it only
   shrinks. The ratchets are unchanged: every tool needs a row, a duplicate row fails, every check a
   row names must be made in the handler's code.
3. **Tests derive their lists.** The ratchet over the handlers' code reads every source with an
   `impl IdeServer` block (plus the window interface and the dialogs list) instead of a fixed list.
   The protocol test compares `tools/list` with the effects table and checks a sample of tools per
   area, not every name.
4. **The README states no exact tool count that each pull request must change.** Its first screen
   says "more than 300", held by the protocol test; the table of tools by area keeps its counts with
   the date they were taken (CONTENT_STYLE_GUIDE §3), refreshed by hand when the README is revised.
5. **Traceability rows are written as fragments.** A pull request adds its rows as
   `docs/specifications/traceability.d/<PR number or branch>.md`; `scripts/traceability.sh check`
   (run by `scripts/check-docs.sh`) checks them and `scripts/traceability.sh assemble` appends them to
   `TRACEABILITY.md` at the daily version cut, as `scripts/changelog.sh` does for the changelog.
6. **Specification and ADR numbers are reserved before use.** `docs/specifications/RESERVED.md` has
   one row per number: number, topic, branch, date. An agent reserves a number by merging a pull
   request that only adds its row (or the coordinator assigns it and adds the row);
   `scripts/spec-number.sh next` prints the next free number. `scripts/spec-number.sh check`, run by
   `scripts/check-pr.sh`, fails when a branch adds a `SPEC-NNNN` or `ADR-NNNN` file whose number is
   already used by another file on `origin/main` or reserved there for another branch.

## Consequences

- A pull request that adds a tool module changes its own files, one `mod` line in `lib.rs`, one in
  `tests/it/main.rs`, and its own fragments. Two such pull requests no longer conflict.
- What a tool is allowed to do is now written next to its code; the tests still read the joined
  table, so a reviewer finds every row by searching for the tool's name.
- A build script runs on every change of `crates/mcp/src/`; it reads the sources in well under a
  second. `scripts/check-pr.sh` tests the whole workspace when a build script changes, as before.
- A reservation is one more small pull request; it is merged without waiting for anything else.
