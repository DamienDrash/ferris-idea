# IDECraft as an AI-native development platform — long-term vision and target architecture

- **Status:** planned (future). Active work starts only after quality gate QG-1
  ([ADR-0015](../adr/0015-quality-gate-ai-native.md)).
- **Date:** 2026-10-31
- **Related:** [readiness and gap analysis](READINESS.md) · [future milestones](FUTURE_MILESTONES.md) ·
  ADR-0012 to ADR-0015 · [ARCHITECTURE.md](../../ARCHITECTURE.md) · ADR-0008 (MCP server) · SPEC-0053 (agent rights)

The classical IDE comes first. IDECraft's current goal is unchanged: a complete, stable and fast Rust
IDE with the relevant functions of IntelliJ IDEA Ultimate (ROADMAP.md M0–M7). This
document describes where the platform goes afterwards. It does not delay or narrow the classical work.

## 1. Product vision

IDECraft is a fully usable classical IDE and, in addition, a platform in which people and AI agents work
as equals: on one project, on several projects, alone or in teams coordinated by a human or an
authorised AI Project Owner.

### Principles

1. **Classical first, AI optional.** Every IDE function works without AI, without a model and without a
   network connection.
2. **One core for everyone.** People and agents call the same capabilities. A view and an agent never use
   two implementations of the same operation.
3. **Fully scriptable.** Agents can operate the IDE programmatically, completely, and additionally get
   machine-oriented operations (batch queries, structured diffs, dry runs) that a human does not need.
4. **Deterministic before probabilistic.** Compilers, parsers, language servers, type checkers, test runs
   and database introspection answer what they can answer reliably; models plan, explain and propose.
5. **Providers are interchangeable.** No capability depends on one vendor's model, SDK or protocol
   dialect.
6. **Local by default.** Local development never requires a cloud or LLM connection (consistent with
   ADR-0007: no telemetry, network only when the user starts something).
7. **Isolated and traceable parallel work.** Parallel processes run in separate worktrees and
   environments; every change has an author, a reason and a way back.
8. **One security model.** Rights, approvals and limits apply in the same way whether a person or an agent
   triggers an operation.
9. **Rust core.** The application core stays Rust; agents and adapters in other languages talk to it over
   protocols.
10. **Many projects, many teams.** The architecture allows several workspaces, projects and agent teams in
    one installation.

## 2. Target architecture

```
  clients     ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌───────────┐
              │ GPUI app  │ │ MCP       │ │ ACP       │ │ CLI       │ │ SDK       │
              │ (views)   │ │ adapter   │ │ adapter   │ │ (noun     │ │ (Rust     │
              │           │ │ (server)  │ │ (client)  │ │  verb)    │ │  crate)   │
              └─────┬─────┘ └─────┬─────┘ └─────┬─────┘ └─────┬─────┘ └─────┬─────┘
                    └─────────────┴──────┬──────┴─────────────┴─────────────┘
  capability layer           ┌───────────┴──────────────────────────────────────┐
  (versioned, typed)         │ code.* database.* debug.* execution.* workspace.*│
                             │ git.* data.* ui.* agent.*                        │
                             │ + permission check, budget, audit, cancellation  │
                             └───────────┬──────────────────────────────────────┘
  headless core              ┌───────────┴──────────────────────────────────────┐
                             │ workspaces · document registry · sessions ·      │
                             │ event stream · task graph · knowledge store      │
                             └───────────┬──────────────────────────────────────┘
  domain crates   core · editor · languages · database · debugger · vcs · terminal · http
```

### 2.1 Headless IDE core

**Today:** the domain crates are already free of the UI toolkit and run headless; the MCP server and the
CLI prove it. What is missing is a *process-level* core that owns live state: open documents with unsaved
changes, sessions (debug, terminal, language servers), and the event stream. Today that state lives in
GPUI entities and in the MCP server's struct ([readiness](READINESS.md) A1, A2).

**Target:** a headless core service, used in-process by the GUI and reachable by adapters:

- **Workspace registry:** several workspaces (folders, worktrees, projects) per process, each with its
  own settings, rights and sessions; no global workspace singleton.
- **Document registry:** path → buffer, revision, undo history, owner of the latest edit. The GUI renders
  it, agents edit it, both see the same text. Saving goes through one path (local history, external change
  check).
- **Session registry:** language server pool, debug sessions, terminals, database sessions, run
  processes — with limits and cancellation.
- **Event stream:** typed events (document changed, diagnostics, process output, task state, approval
  requested) that the GUI and agents subscribe to.
- **Modes of operation:** in-process with the GUI; `idecraft serve` without GUI for agents and CI; one
  GUI attached to a running core (later).

### 2.2 IDE Capability API

One versioned, typed interface for every IDE function. MCP tools, ACP client callbacks, CLI commands, SDK
calls and GUI actions are adapters over it.

**Domains**

| Domain | Content | Exists today as |
|---|---|---|
| `code.*` | symbols, references, hover, completion, diagnostics, code actions, rename, format, folding, syntax | `languages`, MCP `hover_symbol` … `format_file` |
| `database.*` | connections, schemas, SQL run/explain/complete, data edits, DDL, schema compare, ER, export, migrations (future) | `database`, MCP `run_sql` … `er_diagram` |
| `debug.*` | adapters, sessions, breakpoints, stack frames, variables, evaluate | `debugger`, MCP `debug_*` |
| `execution.*` | run configurations, commands, tasks, runtime environments, test reports | `core::run`, MCP `run_*`, `parse_test_report` |
| `workspace.*` | files, search, project model, settings, local history, documents | `core`, MCP `list_files`, `search_*`, `project_model` |
| `git.*` | status, diff, log, blame, branches, worktrees (future), commit, merge, remotes | `vcs`, MCP `git_*` |
| `data.*` | data exploration, profiling, transformations (future; builds on `database.*`) | — |
| `ui.*` | navigation, open file, reveal, panels, visualisations — no-ops without a GUI; the UI driver (snapshot, find, act, expect, screenshot, trace) over the semantic tree | `tool_windows`, `dialog_decisions`, `ui_tooltips`; driver planned ([ADR-0027](../adr/0027-ui-driver-and-semantic-tree.md), SPEC-0319, M6.16) |
| `agent.*` | agent sessions, tasks, messages, approvals, budgets (future) | `agent_permissions` |

**Contract of every capability**

- **Name and version:** `domain.verb` (e.g. `git.commit`), semantic version per capability; additive
  changes keep the major version; old majors stay available for a published deprecation period.
- **Typed input and output:** Rust structs with a JSON Schema 2020-12 description (the same structs
  generate MCP `inputSchema`/`outputSchema`).
- **Effects declared:** `read`, `write`, `vcs-write`, `run`, `network`, `debug`, `remote`, … — the
  permission check uses this declaration, not the caller's identity alone.
- **Preview → apply:** every write capability supports a dry run that returns exactly what would change;
  apply refuses when the target changed since the preview (the pattern IDECraft already uses).
- **Diagnostics:** results carry structured diagnostics (severity, location, code), not prose.
- **Errors:** a stable error code set (`not_found`, `permission_denied`, `conflict`, `invalid_input`,
  `unavailable`, `timeout`, `cancelled`, `internal`) plus a human message.
- **Cancellation and timeouts:** every long-running call takes a cancellation handle and a deadline;
  progress is reported on the event stream.
- **Audit:** every call with an effect is recorded with actor, capability, version, input digest, result.
- **Undo / rollback:** document edits enter the document's undo history; multi-file edits form one undo
  group; agent sessions working in their own worktree are rolled back by discarding the worktree.

Provider-neutral by construction: nothing in a capability mentions a model, a vendor or a protocol.

### 2.3 Protocols and adapters

All adapters sit on the capability layer; none implements domain logic ([ADR-0013](../adr/0013-protocol-adapters.md)).
State of the specifications checked on 2026-10-31:

| Adapter | Direction | Specification state | Plan and limits |
|---|---|---|---|
| **MCP server** | agents call IDECraft | latest revision `2026-07-28`: stateless (no `initialize`, `server/discover`, no protocol sessions), stdio and Streamable HTTP, OAuth 2.1 for HTTP, tools with `outputSchema`/`structuredContent`, progress and cancellation in core, **Tasks moved to the extension `io.modelcontextprotocol/tasks`**, **Roots and Sampling deprecated**. Official Rust SDK `rmcp` supports it. ([spec](https://modelcontextprotocol.io/specification/latest), [changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog)) | Exists (stdio and Streamable HTTP on localhost for the running window, 167 tools on `rmcp`, 2026-11-03). Next: tool list generated from the capability registry; long operations via the Tasks extension; workspace passed as an explicit parameter (Roots is deprecated); no Sampling dependency. |
| **ACP client** (Agent Client Protocol) | IDECraft hosts coding agents | stable protocol version `1` (v2 draft), JSON-RPC over stdio with the agent as subprocess; client side answers `fs/*`, `terminal/*`, `session/request_permission`; agents receive MCP servers in `session/new`. Rust crates `agent-client-protocol`, `agent-client-protocol-schema`. ([intro](https://agentclientprotocol.com/overview/introduction), [protocol](https://agentclientprotocol.com/protocol/overview)) | IDECraft acts as ACP *client*: file and terminal callbacks map to `workspace.*` and `execution.*` (with the same permission checks); IDECraft passes its own MCP server to the agent in `session/new`, so the agent gets the full capability set. |
| **CLI** | scripts, CI, humans in a shell | own | `idecraft <domain> <verb> --json` over the capability layer, plus `idecraft serve`. Today's hand-written commands remain. |
| **SDK** | external Rust integrations | own | a versioned crate exposing the capability types and a client; published only with GO-4. |
| **Remote** | remote agents, remote workspaces | MCP Streamable HTTP + OAuth 2.1; ACP remote transport is work in progress upstream | opt-in only (ADR-0008: local by default); TLS and authentication mandatory; no remote access without explicit grant. |
| **Agent-to-agent** | between agents | A2A 1.0 (Linux Foundation; agent card, task, message, artifact) | not needed inside IDECraft (IDECraft's own task graph and message log coordinate); optional bridge for external agent systems later. Note the name clash: IBM's *Agent Communication Protocol* merged into A2A in 2025 and is unrelated to the Agent *Client* Protocol. |

**Agents named in the brief** (state 2026-10-31, to be re-checked when M11 starts):

| Agent | Machine interface | ACP | Notes |
|---|---|---|---|
| Claude Code | `claude -p` with JSON/stream-JSON output and JSON schema; Agent SDK (Python/TypeScript); MCP client | via adapter (`claude-agent-acp`, built on the Agent SDK) | background commands of the adapter keep running after a cancel (upstream limitation) |
| OpenAI Codex CLI | `codex exec --json`, `--output-schema`; `codex app-server` (experimental) | via adapter (`codex-acp`, TypeScript) | `codex mcp-server` was removed upstream |
| Gemini CLI | `-p`, JSON output; MCP | native (`gemini --acp`) | Google replaced Gemini CLI by Antigravity CLI for unpaid tiers (2026-06); Antigravity has no native ACP as far as known (unverified) |
| Other ACP agents, local models, own agents | ACP or MCP-capable runners | per agent | local models through a local runner that speaks ACP or through an own agent built on the SDK |

Consequence: IDECraft cannot assume that every agent speaks every protocol. The **agent adapter layer**
records per agent which protocol is used (native ACP, ACP adapter, headless CLI with JSON output) and
which features are missing (e.g. no permission callback → the agent runs only in an isolated worktree
without network rights).

## 3. Multi-agent collaboration and orchestration

### 3.1 Identities, roles, capabilities and rights — four separate things

| Concept | Meaning | Example |
|---|---|---|
| **Identity** | who acts: a person or an agent instance (agent type, provider, model, instance id) | `human:alice`, `agent:claude-code#3` |
| **Role** | what the participant is responsible for in a project | Project Owner, Architect, Developer, Reviewer |
| **Capability profile** | what an agent can technically do (from the registry, §4) | supports ACP, 200k context, can run tests |
| **Grant** | what the participant is allowed to do in a workspace | `write`, `run`, `database:read-data` |

Roles are data, not code: Project Owner, Product Manager, Architect / Tech Lead, Developer, Database
Engineer, QA Engineer, Reviewer, Integrator, Researcher, and project-specific roles. Any identity — human or
agent — can hold any role. A role implies *default* grants that a human with authority confirms; it never
implies unrestricted system rights. In particular, **the Project Owner role does not include system
administration, secret access or budget changes** unless granted separately.

### 3.2 Task graph and delegation

A persistent task graph, implemented deterministically in Rust:

- tasks and subtasks (hierarchy), explicit dependencies (DAG; cycles rejected), parallelisable siblings;
- priority, optional deadline, owner (role) and assignee (identity);
- acceptance criteria as checkable items (tests to pass, files touched, reviewer approval);
- states with a fixed transition table: `proposed → ready → assigned → in_progress → in_review → done`,
  plus `blocked`, `failed`, `cancelled`; each transition has a guard (e.g. `done` needs all criteria met
  and a review by an identity other than the assignee);
- blockers and escalations (to the owner role, then the Project Owner, then a human);
- retries with a limit and backoff, recovery after a crash from the persisted state;
- structured handoffs and artefacts (diff, branch, test report, decision record) attached to the task;
- an append-only decision log.

Models plan and propose (split a task, choose an assignee, write criteria). The engine validates and
applies — **a model can never set a state, skip a guard or grant a right**; it can only submit requests the
engine checks like any other caller's.

### 3.3 Parallel development

- **One worktree per agent session** (Git worktrees from `vcs`), branch per task; shared read-only project
  knowledge.
- **Separate runtime environments:** own ports, own temporary directories, optional containers (only when
  installed; no new runtime dependency), **isolated test databases** per session (schema or database per
  session, created from a template).
- **Conflict detection early:** overlapping files and symbols between running tasks are reported before
  they become merge conflicts; the scheduler can serialise tasks that touch the same area.
- **Merge queue:** changes are integrated one at a time; each candidate is rebased on the current target
  and the **merged state is tested again** — two individually green changes can be incompatible together.
- **Reviews:** deterministic checks first (build, tests, lints, formatting, schema diff), then semantic
  review by another identity (human or agent) against the acceptance criteria.
- **Rollback:** discard a worktree; revert an integrated change with a recorded reason; database changes
  only through migrations that have a down path or a snapshot.

### 3.4 Agent-to-agent communication

Messages are records in the project store, not chat inside one model's context window: `task_assignment`,
`status_update`, `question` / `answer`, `blocker`, `review_request`, `change_request`, `handoff`,
`decision_record`, `completion_report`. Each has sender, recipients, task, references (files, commits,
artefacts) and a schema. Agents receive them through the capability layer (`agent.messages.*`) and the
event stream; any agent can be replaced mid-task because the history lives outside it.

Protocols: inside IDECraft, the own message store plus MCP/ACP for tool access is enough. A2A becomes
relevant only for exchanging tasks with external agent systems; its task and artefact model maps onto the
task graph.

### 3.5 Hierarchical and cross-project teams

- A Project Owner coordinates teams; team leads receive delegated subgraphs.
- An agent may serve several projects; rights are always per project and workspace.
- Cross-project tasks and dependencies reference tasks in other projects; shared libraries are projects
  of their own.
- **Delegation budgets:** maximum hierarchy depth, maximum number of agents a participant may start, and a
  cost budget that is split, never multiplied, when delegated. An agent cannot create agents unless its
  grant allows it, and never beyond the depth and count limits — uncontrolled recursive spawning is
  impossible by construction.

## 4. Agent capability registry

Per agent type and instance: provider and model, protocols (ACP native / adapter / headless CLI / MCP),
available tools, context limit, working modes (interactive, autonomous, plan-only), cost model, measured
run times, success rates per task kind, quality metrics (review outcomes, test pass rate, rework),
grants, availability. Measured values come from the task graph's history, never from the agent's own
claims.

The orchestration engine *suggests* assignees from the registry; a human or the Project Owner decides
unless an automatic policy was granted. The registry supports running several independent agents on the
same task and comparing their results (tests, review, diff size) before one is integrated.

## 5. Project knowledge graph

A persistent, incrementally updated graph linking code symbols and references, module and package
dependencies, database schemas and relations, SQL queries and ETL pipelines, Git history, tests and
results, runtime diagnostics, architecture decisions, and agent tasks and outcomes.

Every edge carries **origin** (which analyser, test or participant), **version** (commit, schema version),
**validity** (since/until) and **confidence**. Facts from deterministic analysers are marked as such;
statements from a model are stored only as *hypotheses* with their source until a deterministic check or a
human confirms them. Deleting or invalidating a source invalidates what depends on it.

The graph is built from what IDECraft already computes (project model, LSP symbols, schema introspection,
Git log, test reports); it adds no new analyser of its own in the first step.

## 6. Security and cost control

One model for humans and agents, extending SPEC-0053:

- least privilege; grants per workspace and project, stored outside the project (IMP-0028);
- effects declared per capability; approval-required operations (push, merge to protected branches,
  schema changes, data changes in non-test databases, running outside a sandbox);
- read-only and write grants per domain, including file writes and VCS writes ([readiness](READINESS.md) A3);
- protected database connections (the existing per-connection AI permission levels);
- secrets only in the OS keychain or an external vault, never in project files, logs or model context;
- sandboxed execution for agent processes where the OS allows it; resource limits (time, memory,
  processes, disk);
- audit log of every effect with actor and session (extension of `.idecraft/agent-log.jsonl`);
- rate limits per agent and capability;
- **central budget management:** token and cost budgets per agent, task, project and time window; a hard
  stop when a budget is exhausted; maximum parallel agent sessions per project and in total;
- **emergency stop:** one action cancels all agent sessions of a project (or all), revokes their temporary
  grants and leaves worktrees intact for inspection.

## 7. Working modes of the future UI

All three modes are views over the same core state (workspaces, documents, sessions, tasks); switching
mode never stops running processes.

- **Developer Mode:** today's classical IDE — editor, database tools, debugger, project tree, VCS. No agent
  surface unless the user opens one.
- **Hybrid Mode:** person and agent on the same project at the same time. Agents analyse, prepare changes
  in their own worktree or as previews, and offer them for acceptance (diff review, per-hunk accept,
  run tests first). Edits by agents in shared documents are attributed and undoable.
- **Agent Mode:** tasks, teams, delegation, worktrees, reviews, merge queue and budgets in the foreground;
  the editor opens on demand.

Technical prerequisites: document registry and event stream (§2.1), attribution of edits, task graph
(§3.2), worktree management (§3.3). UX details are designed when those exist; only the concepts are fixed
here.

## 8. What this vision does not do

- It does not change the order or priority of M0–M7.
- It does not introduce AI features, agents, orchestrators or UI before QG-1.
- It does not add runtime dependencies; libraries for the adapters (e.g. ACP crates) are evaluated under
  `deny.toml` when their milestone starts.
- It does not examine the AI features of the reference IDE (excluded in ROADMAP "Bewusst nicht im Umfang").
