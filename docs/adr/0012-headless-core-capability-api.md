# ADR-0012: Headless core and one capability layer for all callers

- **Status:** accepted · 2026-11-14 · decided by the coordinator under the Project Owner's general authority
  of 2026-11-13 (PO-STATUS); proposed 2026-10-31. Rules 1–4 apply now (rule 4: `SPEC-0220`, reading since
  v1, agent writes through the open buffer since v2); rule 5, the capability layer with `ferris-idea serve`
  as the core service for all callers, stays planned for milestone M10 (after quality gate QG-1,
  [ADR-0015](0015-quality-gate-ai-native.md); the remote backend that `ferris-idea serve` already is since
  M6.12 does not change that)
- **Context:** The domain crates are already free of the UI toolkit. But the operations people and agents
  use are implemented at the edges: 73 MCP tool bodies live inside `crates/mcp/src/lib.rs`, some views call
  domain code directly or do the work themselves (file save in `workbench/src/editor_view.rs`), and the
  state of open, unsaved documents exists only in GPUI entities. Each new feature risks a third copy of the
  same operation when ACP, a CLI or an SDK are added. Details: [readiness](../architecture/READINESS.md) A1, A2, A4.
- **Decision:**
  1. **Domain first.** New behaviour is a function or method in a domain crate with typed input and output.
     MCP tools and views only translate arguments and render results. Existing tool bodies move when they
     are touched for other reasons.
  2. **New long-running operations** take a cancellation handle and a deadline and return a domain error
     enum with stable codes. No new dependency is required for this.
  3. **No global workspace state.** Workspaces stay explicit parameters (as today).
  4. **Document registry before the UI serves agents.** Before the running GUI exposes MCP (ROADMAP
     M6.8), open documents get one owner outside `workbench` (path → buffer, revision, undo), which the
     GPUI view renders and agents edit. The decision on its exact shape is part of M6.8.
  5. **Later (M10):** a capability layer — `domain.verb` names (`code`, `database`, `debug`, `execution`,
     `workspace`, `git`, `data`, `ui`, `agent`), versioned typed schemas, declared effects, preview → apply,
     structured diagnostics, audit, cancellation — and a headless core service (`ferris-idea serve`) that
     hosts workspaces, documents, sessions and an event stream.
- **Consequences:** No refactoring now. New code is slightly more structured (a domain function plus a thin
  adapter). The capability layer can later be generated from what exists instead of rewritten. The MCP
  crate stays an adapter and does not grow into the core.
- **Alternatives considered:** (a) keep MCP as the core API — rejected: binds every caller to one
  protocol's revisions; (b) build the capability layer now — rejected: delays the classical IDE, which has
  priority.
