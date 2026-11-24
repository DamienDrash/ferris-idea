# Architecture readiness for the AI-native platform

- **Date:** 2026-10-31 · **Basis:** `main` at `fd163b0` (after PR #73), code read crate by crate
- **Updated:** 2026-11-15 (QG-1 gate report measured again on `main` at `c92f957c`: rows "MCP server" and
  "Open-buffer state", the state of A2 tool by tool, the tool count of A3) · 2026-11-14 (QG-1 gate report, [QG-1-REPORT.md](../quality/QG-1-REPORT.md): rows "MCP server", "Agent rights", "Audit log", "Open-buffer state" and the state of A1–A3 re-checked against `main` at `40ccdff4`; afterwards A1 decided, A2 implemented, #750) · 2026-11-07 (row "UI driver and semantic tree", ADR-0027) · 2026-11-03 (M8.4) · section 1 re-checked against `main` at `3f77f8d`; the gap analysis
  (section 2) keeps its original wording, with the state of each A item noted under it
- **Target:** [AI-native vision](AI_NATIVE_VISION.md) · **Milestones:** [future milestones](FUTURE_MILESTONES.md)
- **Scope:** planning only. Nothing here changes code or the current priorities in ROADMAP.md.

This document compares the code as it is with the target architecture and sorts the gaps:

- **A – consider now:** decisions whose later reversal would be expensive. "Consider" means: keep them in
  mind while building the classical IDE; it does not mean refactoring now.
- **B – prepare later:** extensions that fit onto the architecture after the classical IDE is done.
- **C – future:** no technical effect today.

## 1. What exists today

Status words: **implemented** (code and tests), **partial**, **stub** (crate with no logic), **planned**
(only in docs), **absent**.

| Area | Status | Where | Notes |
|---|---|---|---|
| Domain crates free of the UI toolkit | implemented | all crates except `workbench` and `app` | `ui` holds themes, icons, keymaps and the tool window layout and is toolkit-free despite its name; `workbench` and `mcp` do not depend on each other ([crate diagram](../../ARCHITECTURE.md#crate-dependencies)) |
| Headless binary | implemented | `crates/app/src/main.rs` | `open`, `mcp`, `ui`, `edit`, `container-exec` and spike commands; hand-written argument matching |
| MCP server | implemented | `crates/mcp/src/lib.rs` (316 tools in the effects table `crates/mcp/src/effects.rs`, 2026-11-15), `crates/mcp/src/http.rs` | stdio (one process per client) and Streamable HTTP inside the window process (`SPEC-0221`); tool bodies still live inside the MCP crate |
| MCP for the running UI | implemented | `crates/app/src/mcp_http.rs`, `SPEC-0221` | loopback only, token per start, one route per open project; an agent and a human share the open project and its unsaved text |
| Agent rights | implemented | `core::permissions` (`run`, `http`, `debug`, `remote`, `ssh`, `trust`, `write`, `vcs-write`, `quit`, `terminal-read`, `ports`, `ui`, `secret-read`, `settings`, `recent-projects`), database permissions held by the user (`SPEC-0012` v3, ADR-0031), `mcp::effects`, `mcp::window` | grants in the user config folder, never in the project (IMP-0028, ADR-0031); since 2026-11-14 (#724) every tool that changes something checks a right, recorded per tool in `mcp::effects::TOOLS` and enforced by the ratchet `crates/mcp/tests/it/effects.rs` (two named exceptions: `ui_driver_stop`, `local_history`); over HTTP the window asks for a missing right (IMP-0007) |
| Audit log | partial | `.ferris-idea/agent-log.jsonl` | since 2026-11-14 (#724) one line for every call of a tool that changes something or uses a database, from the effects table; no actor yet (A5) |
| Secrets | implemented | `database::secret` (OS keychain) | database passwords and AI provider keys (`ai` uses the same store); profiles refuse passwords in URLs |
| Preview → apply | implemented | git, LSP edits, formatting, SQL rename, data edits, terminal commands | consistent `dry_run` default |
| Shared action ids | implemented | `editor::perform(state, action)` | one string-keyed dispatcher for keymap, menus and MCP (`editor_action`) |
| Local history | implemented | `core::LocalHistory` | written by window saves, MCP restore/format tools and refactorings (`SPEC-0193`) |
| Workspace model | implemented, single root per server | `core::Workspace` | one root per MCP server; the window process serves several projects, each with its own server |
| Open-buffer state | implemented | `editor::DocumentRegistry` (`SPEC-0220` v5) | the window publishes unsaved text; MCP `read_file`/`open_documents` read it; of the 86 tools that change files, 28 edit open files through the buffer (one undo step), 13 refuse unsaved ones and 45 write Ferris IDEA's own or new files to the disk (`mcp::open_files`, 2026-11-15) |
| Event bus / observable state | absent | — | GPUI `notify` inside the window; `core::watch` polls files and sends snapshots over a channel |
| Cancellation | partial | `core::run`, `core::watch`, Search Everywhere, long database and copy operations in the window | stop flags where added since ADR-0012; most MCP calls still end by timeouts only |
| Structured errors | absent | — | MCP errors carry text messages, no stable codes |
| Capability versioning | absent | — | only the server version; tools are not versioned |
| Language servers | implemented | `languages::pool` (`SPEC-0090`) | long-lived pool, one server per root and command, shared by the window and MCP in one process (B4 done for the classical phase) |
| Debugger | implemented | `debugger`, Debug window | DAP launch and attach, conditions evaluated by Ferris IDEA when the adapter cannot (`SPEC-0178`) |
| UI driver and semantic tree | planned | [ADR-0027](../adr/0027-ui-driver-and-semantic-tree.md), [SPEC-0319](../specifications/SPEC-0319-ui-driver.md), ROADMAP M6.16 | today the self-test clicks by painted bounds of named marks (`hit_targets`) or calls view methods directly; the driver gives `ui.*` its first capabilities (AccessKit tree, locators by role and name, auto-waiting, events, right `ui`, off by default for external clients); the shared tree with screen readers waits for the toolkit decision of ADR-0019 |
| Plugins | implemented | `crates/plugins` | `wasmi` modules without host functions (ADR-0016) |
| AI integration | partial | `crates/ai` | AI Assistant with a user-chosen OpenAI-compatible endpoint or the provider's CLI, off by default (SPEC-0138, ADR-0017); agent integration (ACP, M11) gated by QG-1 |
| Undo across files | partial | `languages::undo_group`, editor undo groups (`SPEC-0193`) | one undo for a refactoring across files; no rollback of an agent session (B10) |
| Refactoring crate | stub | `crates/refactoring` | refactorings live in `languages` (LSP), `editor` (undo groups) and `database` (SQL) |
| Task graph, roles, orchestration, knowledge graph, budgets | absent | — | — |

Documentation drift found during the analysis (fixed after the PO's approval in a separate change, 2026-10-31):

- `ARCHITECTURE.md` rule 1 says only `ui` and `app` know the UI toolkit; in fact `workbench` and `app` do,
  and `ui` does not.
- The ROADMAP header still says there is no window; the workbench window exists since PR #65.
- `crates/refactoring` is listed as owning refactorings; they are in `languages` and `database`.

## 2. Gap analysis

### A – consider now

These are cheap to respect while building features and expensive to retrofit. None of them requires a
refactoring now; each is a rule for new code plus one recorded decision.

**A1. Domain operations must live outside the MCP crate and outside the views.**
- *Today:* the 73 MCP tool bodies are methods of `IdeServer` in `crates/mcp/src/lib.rs`; some workbench
  views call domain code directly (`Repo::discover` in `commit_view.rs` and `workbench_view.rs`) or do it
  themselves (file save as a plain `std::fs::write` in `editor_view.rs`).
- *Risk:* every new feature adds a third copy of the same operation (view, MCP tool, later ACP/CLI). A
  capability layer added at the end has to untangle 100+ tools and views at once.
- *Later effect:* inconsistent behaviour between human and agent paths (e.g. GUI save skips local history
  and the "changed on disk" check that MCP tools have).
- *Recommendation:* new behaviour goes into a domain crate as a plain function or method with typed input
  and output; the MCP tool and the view only adapt it. Move existing tool bodies opportunistically when
  they are touched (ADR-0012).
- *Effort / migration risk:* low per feature; moderate if postponed (estimated as a dedicated milestone).
- *Decision needed now:* **yes, as a rule** (ADR-0012, proposed then). No code change now.
- *State 2026-11-14:* **decided** — ADR-0012 accepted (coordinator under the Project Owner's general
  authority of 2026-11-13): rules 1–4 bind new code; rule 5 (capability layer, `ferris-idea serve` as the
  core for all callers) stays planned for M10.

**A2. One document service for open buffers.**
- *Today:* the text of an open, unsaved file exists only inside a GPUI entity.
- *Risk:* agents and humans cannot work on the same open file: an agent writes to disk while the editor
  holds a different version; ADR-0008's promise "write tools edit through the editor model so undo works"
  cannot be kept.
- *Recommendation:* when the GUI starts serving MCP (M6.8), route buffer access through a headless
  document registry (path → `EditorState` + revision) owned outside `workbench`; the GPUI view renders it.
  This is the first piece of the headless core.
- *Effort:* medium; touches `workbench` editor handling. *Decision:* **taken 2026-11-02 — yes**, the
  document registry comes before M6.8's HTTP transport.
- *State 2026-11-03:* reading done (`SPEC-0220`); MCP write tools do not edit through the buffer yet.
- *State 2026-11-14:* **implemented** (`SPEC-0220` v2). Writing: an MCP tool that changes an open file
  hands the window an edit (`Window::edit_document`, `DocumentEdit` at the revision it read); the editor
  applies it as one undo step, saves it when the buffer was saved and leaves it unsaved otherwise.
  Through the buffer: `eslint_file`, `fix_inspection`, `generate_code`, `optimize_imports`,
  `prettier_file`, `restore_local_revision`, `suppress_inspection`, `surround_with`,
  `update_copyright` (and the Prettier, ruff and black paths of `format_file`). Tools computing from
  the disk (31: language-server edits and refactorings, file operations, Git, SQL renames, resource
  bundles, exports) refuse a file open with unsaved changes. The ratchet
  `open_files::every_file_changing_tool_edits_open_files_through_the_buffer_or_refuses` keeps every
  tool with a `files` effect in exactly one list (`crates/mcp/src/open_files.rs`); the refusing list
  may only shrink. Left: refusing tools through the buffer (language-server edits against the shown
  text, refactorings as one undo group in the editors), the other reading tools on open buffers.
- *State 2026-11-15* (`SPEC-0220` v5, `main` at `c92f957c`, [QG-1-REPORT.md](../quality/QG-1-REPORT.md)
  section 6): of 316 tools, 86 change files. **Through the buffer (28):** the nine above plus the
  language-server edits and refactorings (`apply_code_action`, `rename_symbol`, `change_signature`,
  `delegate_methods`, `extract_interface`, `extract_superclass`, `override_methods`, `pull_members_up`,
  `safe_delete`, `undo_refactoring`, `move_file`, the LSP path of `format_file`) and
  `compare_files`, `file_properties`, `git_line_change`, `git_resolve_conflict`, `git_rollback`,
  `rename_sql_object`, `resource_bundle` (v5); parts of six still refuse an unsaved file (encodings,
  a rollback that deletes an added file, taking a whole conflict side, the moved file itself, an undo
  group that renamed files, a read-only target). **Refuses unsaved (13):** `delete_file`,
  `rename_file`, the eight Git operations of the working tree, `er_diagram`, `export_file`,
  `export_store_result`; under a saved editor they write the disk, which the editor takes as one
  undo step. **Own files (45):** write the disk by design — Ferris IDEA's data and settings in
  `.ferris-idea/` and `.idea/` (22), new files only (7), the user's settings folder (2), SSH hosts and
  rooms (10; `remote_write` does not look at a remote project's open documents), the window's own
  steps and record (4). Still reading the disk: search, diagnostics, symbols. Pending: #830
  (`replace_text_in_file` through the buffer, `create_file` with a text).

**A3. Writes need a right, not only a dry-run default.**
- *Today:* `git_commit`, `git_merge`, `apply_code_action`, `rename_symbol`, `format_file`,
  `restore_local_revision` change files or the repository when called with `dry_run=false`; no grant is
  checked.
- *Risk:* the permission model is the one thing that must hold for humans and agents alike (principle 8);
  adding a `write` right after agents rely on unrestricted writes is a breaking change.
- *Recommendation:* add `write` (workspace files) and `vcs-write` (commit, merge, switch) to
  `AgentPermissions` in the classical phase — it is a small change inside the existing SPEC-0053 model and
  also an IntelliJ-parity improvement over per-command dialogs.
- *Effort:* small. *Decision:* **taken 2026-11-02 — yes**, `write` and `vcs-write` in the classical
  phase.
- *State 2026-11-04:* implemented (`SPEC-0053` v4): `write` for workspace files, `vcs-write` for commit, branch, switch, merge, stash, cherry-pick, rebase, resolve, pull and push; checked once in the step that writes; clients of the window's HTTP server are asked in the window.
- *State 2026-11-14:* complete for every changing tool (#724, `SPEC-0053` v8): `crates/mcp/src/effects.rs` names the change and the checks of each of the 312 tools, and `effects::every_tool_with_a_changing_effect_checks_a_right` fails for a changing tool without a check or a row that claims a check its code does not make (passed on macOS 2026-11-14). Known limits needing the Project Owner: `SPEC-0053` v8 behaviour 14.
- *State 2026-11-15:* 316 tools in the table; the ratchet passed in the server CI run of `main` at `c92f957c`.

**A4. Stable error codes and cancellation in new long-running operations.**
- *Today:* text errors; no cancellation, only timeouts.
- *Risk:* both are part of every capability signature; retrofitting means changing every call site.
- *Recommendation:* new long-running domain functions take a cancellation handle (a plain
  `Arc<AtomicBool>` or similar, no new dependency) and return a domain error enum with a stable code.
  Existing code stays until it is moved.
- *Effort:* small per feature. *Decision needed now:* rule only (ADR-0012).

**A5. Identity on every recorded action.**
- *Today:* the agent log records what ran, not who asked (human, which agent, which session).
- *Risk:* audit, budgets and multi-agent review all key on the actor; logs without actor are useless for
  that later.
- *Recommendation:* when log entries are next extended, add an `actor` field (`human`, or the MCP client's
  name and a session id from the MCP `initialize` handshake).
- *Effort:* small. *Decision needed now:* no; recommended with the next change of the log.

**A6. Single-root assumptions stay local.**
- *Today:* `Workspace { root }`, one per server, is the right model for the classical IDE.
- *Risk:* global singletons ("the workspace") would block multi-workspace and worktree-per-agent setups.
- *Recommendation:* keep passing the workspace explicitly, as today; no global state. No change needed.

### B – prepare later (after the quality gate)

| ID | Gap | Fits onto | Milestone |
|---|---|---|---|
| B1 | Capability trait, registry, versioned schemas, generated MCP tool list | A1, rmcp argument structs | M10 |
| B2 | Headless core service (`ferris-idea serve`) hosting workspaces, documents, sessions; GUI as a client of it or in-process | A2 | M10 |
| B3 | Event stream (document changed, diagnostics, task state) for agents and UI | GPUI notify today; `core::watch` | M10 |
| B4 | Long-lived language server pool shared by GUI and agents | `languages::lsp` | M8 (classical) / M10 |
| B5 | Multi-root workspaces and several projects per process | `core::Workspace` | M10 |
| B6 | ACP agent client, CLI noun-verb commands, SDK crate | capability layer | M11 |
| B7 | Worktree-per-session execution, isolated run environments, test databases | `vcs`, `core::run`, `database` | M12 |
| B8 | Unified permission model: identities, roles, grants, approvals, budgets | `core::permissions`, database permissions | M10/M12 |
| B9 | Secret store beyond database passwords (API keys for providers) | `database::secret` (keychain) | M11 |
| B10 | Undo across files and rollback of an agent's session | local history, preview/apply, Git worktrees | M12 |

### C – future (no technical effect today)

Agent routing and success statistics, cost optimisation, comparing several agents on one task,
autonomous project organisation and hierarchical teams, cross-project dependencies, the project knowledge
graph, Agent Mode UI. They depend on B and need no preparation in current code.

## 3. Critical risks

1. **Two owners of document state** (A2). Highest risk: it decides whether "humans and agents use the same
   core" is possible at all. Mitigation: decide the owner before M6.8's UI transport.
2. **Capability logic bound to one protocol** (A1). The MCP crate is becoming the de-facto core. Mitigation:
   ADR-0012 rule now, extraction milestone later.
3. **Writes without rights** (A3). Mitigated: `write`/`vcs-write` rights since 2026-11-04 (`SPEC-0053` v4).
4. **Semantic merge conflicts between parallel agents.** Two green changes can be incompatible after the
   merge. No code risk today; mitigated in the design by a merge queue that re-tests the merged state
   (vision §4.3).
5. **Protocol churn.** MCP and ACP revise often; binding domain code to an SDK's types would spread every
   revision through the code base. Mitigation: adapters only at the edge (ADR-0013).

## 4. Effect on the current plan

None of the A items adds a milestone to the classical phase or changes its order. A1, A4, A5 and A6 are
rules for new code; A2 and A3 are decisions to make at the point where the classical roadmap touches them
anyway (M6.8 and the next MCP write tool).
