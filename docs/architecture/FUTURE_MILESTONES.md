# Future milestones: from classical IDE to AI-native platform

- **Status:** planned. M8 belongs to the classical phase; M10–M15 are **future** and blocked until quality
  gate QG-1 (M9, [ADR-0015](../adr/0015-quality-gate-ai-native.md)) is passed.
- **Date:** 2026-10-31 · **Tracking:** ROADMAP.md (milestones M8–M15, weight 0 so the
  current progress measure is unchanged) · **Background:** [vision](AI_NATIVE_VISION.md),
  [readiness](READINESS.md)

No dates are given: they cannot be estimated reliably before the classical IDE is complete. Effort is
given in relative sizes (S < M < L < XL) where a comparison with finished milestones supports it.

## Where the requested development areas sit

| Development area | Roadmap milestone | Phase |
|---|---|---|
| Classical IDE Completion | existing **M3–M7** (unchanged, weights unchanged) | classical, active |
| Stabilization & Hardening | **M8** | classical, after M3–M7 functions are in place |
| — release gate — | **M9 Quality Gate QG-1** | gate |
| Headless Core & Capability API | **M10** | future |
| AI-native Integration (ACP, MCP, CLI, Claude Code, Codex, Gemini CLI) | **M11** | future |
| Multi-Agent Orchestration | **M12** | future |
| Autonomous Project Organization | **M13** | future |
| Project Knowledge Graph | **M14** | future |
| Advanced Agent Intelligence | **M15** | future |
| Remote control of the local app ([plan](REMOTE_AND_MOBILE.md)) | **M16** | after M6.12, before QG-1 (PO, 2026-11-07) |
| Mobile apps for iOS and Android ([plan](REMOTE_AND_MOBILE.md)) | **M17** | future |

Dependencies: M3–M7 → M8 → M9 → M10 → M11 → M12 → M13; M14 needs M10 (and uses M12's task records when
present); M15 needs M12 and M14; M16 needs M10.3, which is built with M6.12 before QG-1; M17 needs M16 and the remote project of M6.12/M6.14.

Existing open items that touch this area stay where they are and keep their meaning:
**M6.5** (plugin system), **M6.6** (optional AI integration — scope open, see PO-STATUS question) and
**M6.8** (MCP write/diagnostic/run tools, HTTP transport for the running UI — readiness item A2 is
decided there).

---

## Community input for the AI-native phase (2026-11-05)

The project owner asked (2026-11-05) to build the AI-native milestones "by best practice and gold standard" and to
learn from what users dislike in other tools. The research is in
community feedback notes (section 3). How each expectation lands:

| Expectation from the community | Where | Acceptance (testable) |
|---|---|---|
| AI optional and off by default, one switch removes UI and network traffic | M11 (ADR-0007 stays) | with AI off a packet capture of a full session shows no AI endpoint; no AI element is drawn |
| Reviewable diffs, accept/reject per hunk; rejecting never destroys more than the agent wrote | M11.5 | property test: reject after any agent edit sequence restores the pre-agent file bytes |
| Agent changes as one transaction in Local History, undo as one step | M11.5, M12 | one undo restores all files of a task; works after restart |
| Least-privilege permissions; delete, terminal, network and production DBs need explicit grants | M10.1, M11 (SPEC-0053 rights) | protocol tests per capability effect; destructive effects never auto-granted |
| Sandbox / own worktree per agent, visible emergency stop | M12 | stop ends all agent processes within 1 s, no child survives |
| Verification instead of self-report (tests, build, inspections decide success) | M12, M15 | a task is "done" only with a green check run recorded in the task record |
| Human checkpoints at spec, review, test result, deployment — not every step | M12, M13 | configurable gates; default gates at plan and merge |
| Context control: visible fill level, add/exclude context, plan mode | M11.3, M14 | the context sent is listed before sending and can be edited |
| Free model choice, own keys, local models, cost shown before and after | M11.3 | cost estimate per task before start, actual cost after, from the provider's usage data |
| Open protocols: IDE tools via MCP, external agents via ACP | M11.1, M11.2 | conformance tests against current MCP and ACP revisions |
| MCP security: no auto-run of new servers, pinned tool descriptions, project content treated as data | M11.1 | changed tool description hash blocks the tool until re-approved |
| Parallel agents in isolated worktrees sharing indexes without re-indexing | M12, M14 | a second worktree is usable for navigation within 2 s, no full re-index |

Classical-phase themes from the same research (performance budgets, memory ceiling, incremental
indexing across worktrees, cancellable build sync, no telemetry proof) are turned into IMP entries and
M8 hardening items; they also feed the QG-1 gate report (M9.1).

---

## M8 · Stabilisation & Hardening (classical phase)

1. **Goal and scope:** turn the functionally complete classical IDE into a dependable one: quality,
   performance, architecture hygiene, test depth.
2. **Requirements:** no known data-loss bugs; performance budgets met (typing latency, cold start, memory,
   large files, large projects); flaky tests removed; documented behaviour equals implemented behaviour.
3. **Dependencies:** M3–M7 functions at least at parity level 3.
4. **Architecture prerequisites:** readiness A1–A3 decided; one save path for documents.
5. **Deliverables:** performance test suite with budgets in CI; soak test (long editing session, many
   files); fuzzing of parsers and protocol decoders; long-lived language server pool shared by GUI and
   MCP (readiness B4, done in M8.3); `ARCHITECTURE.md` brought in line with the code; open IMP items closed or scheduled.
6. **Acceptance criteria:** the measurable conditions 3–5 of QG-1 hold.
7. **Risks / open decisions:** reference values for cold start and memory need fresh observations
   (OBS-0001 is an idle measurement); budget values are a PO decision.
8. **Effort:** L (comparable to several M6 items together).
9. **Non-goals:** new features beyond the feature matrix; any AI functionality.

## M9 · Quality Gate QG-1

1. **Goal:** a measurable release decision that the classical IDE is done enough to build on.
2. **Requirements:** the seven criteria of ADR-0015 (parity ≥ 85 %, weight 4–5 rows at level ≥ 3, ≥ 50 %
   of weight verified, tests and 20 clean CI runs, performance budgets, no blockers, readiness A1–A3,
   GO-5).
3. **Dependencies:** M8.
4. **Prerequisites:** `scripts/parity.sh` up to date with the feature matrix.
5. **Deliverables:** a gate report (numbers from `parity.sh`, CI run ids, performance results) linked from
   ROADMAP; GO-5 recorded in PO-STATUS.
6. **Acceptance:** all criteria met with evidence; GO-5 given.
7. **Risks / open decisions:** thresholds (PO); whether parts of M10 (pure refactoring without AI) may
   start earlier.
8. **Effort:** S (measurement and report).
9. **Non-goals:** relaxing the criteria to start sooner.

## M10 · Headless Core & Capability API (future)

1. **Goal:** one programmatic interface to every IDE function, used by the GUI and all adapters
   (ADR-0012).
2. **Requirements:** capability registry with `domain.verb` names in the domains `code`, `database`,
   `debug`, `execution`, `workspace`, `git`, `data`, `ui`, `agent`; typed versioned schemas (JSON Schema
   2020-12); declared effects; preview → apply; structured diagnostics and error codes; cancellation,
   deadlines, progress; audit with actor; multi-root workspaces; document, session and workspace registries;
   event stream.
3. **Dependencies:** M9.
4. **Prerequisites:** domain-first rule followed since ADR-0012; document registry from M6.8.
5. **Deliverables:** capability crate(s); existing MCP tools (167 on 2026-11-03) moved behind capabilities with unchanged
   behaviour; `ferris-idea serve`; event stream; permission model extended to all declared effects;
   compatibility tests (old MCP tool names keep working for a deprecation period).
6. **Acceptance:** every MCP tool and every GUI action that changes state goes through a capability;
   protocol tests unchanged and green; GUI and an MCP client edit the same open document and see each
   other's changes; cancelling a long call stops it within a defined time.
7. **Risks / open decisions:** in-process vs. separate core process for the GUI; event stream transport;
   storage for sessions; size of the move (100+ operations).
8. **Effort:** XL.
9. **Non-goals:** AI providers, agent hosting, orchestration.

## M11 · AI-native Integration (future)

1. **Goal:** agents of different providers work in Ferris IDEA through standard protocols (ADR-0013).
2. **Requirements:** MCP adapter on the current revision (stateless, Streamable HTTP, OAuth 2.1 for
   remote, Tasks extension); ACP client hosting agents as subprocesses; agent adapter records per agent
   type (Claude Code via ACP adapter, Codex CLI via ACP adapter, Gemini CLI native, other ACP agents, local
   models); CLI noun-verb commands; SDK crate; secrets for provider keys in the OS keychain; Hybrid Mode
   prerequisites (agent edits attributed and offered as previews); per-agent state with roll-up and
   notifications, terminal agents as fallback with shipped detection manifests, sessions that survive a
   restart and remote hosts over the user's SSH configuration (ADR-0023 v2 rules 9, 11, 16).
3. **Dependencies:** M10.
4. **Prerequisites:** capability registry, permission model with effects, event stream.
5. **Deliverables:** ACP client; agent adapter layer; MCP over Streamable HTTP; CLI; SDK (published only
   with GO-4); agent session view (minimal UI for Hybrid Mode); documentation per supported agent with its
   limits.
6. **Acceptance:** each named agent completes a scripted task (edit, run tests, report) in a test project
   through its connection type, with permission requests answered by Ferris IDEA's policy; Ferris IDEA works
   unchanged with no agent configured and no network.
7. **Risks / open decisions:** protocol churn (MCP revised 2026-07, ACP v2 in draft); adapters maintained
   outside Ferris IDEA (Claude and Codex ACP adapters); Gemini CLI replaced by Antigravity CLI for some users;
   provider terms of use; which adapters to ship vs. document.
8. **Effort:** L.
9. **Non-goals:** multi-agent coordination, own model hosting, model fine-tuning.

## M12 · Multi-Agent Orchestration (future)

1. **Goal:** several agents and people work in parallel on one project under a Project Owner (ADR-0014).
2. **Requirements:** identities, roles, capability profiles and grants as separate models; persistent task
   graph with dependencies, priorities, deadlines, acceptance criteria, blockers, escalations, retries,
   recovery, handoffs, artefacts and decision log; deterministic state transitions with guards; structured
   agent messages; worktree per session; isolated runtime environments and test databases; conflict
   detection; merge queue that tests the merged state; reviews (deterministic checks, then semantic);
   rollback; budgets and emergency stop; any agent can conduct through the `ferris-idea` skill, MCP and CLI
   (`agent.start/prompt/read/wait/stop`) under grants; agent-to-agent messages (direct, team, project)
   visible to people; model tier per task (light, standard, strong), changeable later, with rule-based
   escalation; shared gate watchers per worktree (ADR-0023 v2 rules 10, 12–14).
3. **Dependencies:** M11.
4. **Prerequisites:** capability layer with effects and audit; `git.worktree.*` capabilities.
5. **Deliverables:** task graph engine; scheduler; message store; worktree and environment manager; merge
   queue; Agent Mode views (task board, sessions, reviews, budgets); budget service.
6. **Acceptance:** a scripted scenario with at least three agents of two providers delivers a feature with
   parallel tasks; an injected semantic conflict between two green changes is caught by the merge queue;
   a crash mid-run resumes from the persisted state; a model cannot change a task state or grant except
   through validated requests (negative tests).
7. **Risks / open decisions:** storage engine for the graph (embedded database vs. files in the
   repository); container runtime optional or required; review policy defaults; cost accounting across
   providers.
8. **Effort:** XL.
9. **Non-goals:** cross-project teams, switching models without a confirmed rule (Ferris IDEA suggests a
   tier, a person or conductor decides).

## M13 · Autonomous Project Organization (future)

1. **Goal:** hierarchical and cross-project teams with bounded delegation.
2. **Requirements:** team leads with delegated subgraphs; agents serving several projects with per-project
   grants; cross-project tasks and dependencies; shared components as projects; hierarchy depth, agent
   count and budget limits split on delegation; AI Project Owner possible, with people as developers or
   reviewers.
3. **Dependencies:** M12.
4. **Prerequisites:** multi-workspace core (M10); task graph (M12).
5. **Deliverables:** delegation model and limits; cross-project task references; organisation views.
6. **Acceptance:** recursive agent creation beyond the limits is refused (tests); a delegated budget can
   never exceed its parent's remainder; an AI Project Owner cannot grant itself rights.
7. **Risks / open decisions:** accountability when an AI holds the Project Owner role; human approval
   points that remain mandatory.
8. **Effort:** L.
9. **Non-goals:** organisation-wide administration, billing.

## M14 · Project Knowledge Graph (future)

1. **Goal:** a persistent, incrementally updated graph of the project's facts for people and agents.
2. **Requirements:** nodes and edges for symbols, dependencies, schemas, queries/ETL, Git history, tests,
   runtime diagnostics, decisions, tasks; provenance, version, validity, confidence on every edge;
   model statements stored only as hypotheses; invalidation when sources change; query capability
   (`workspace.knowledge.*`).
3. **Dependencies:** M10; uses M12 task records when present.
4. **Prerequisites:** capability layer and event stream (for incremental updates).
5. **Deliverables:** graph store; importers from existing analysers (project model, LSP symbols, schema
   introspection, Git log, test reports); query capability and MCP exposure.
6. **Acceptance:** after a commit the graph reflects the change incrementally; every fact answers "where
   from, which version, how sure"; a hypothesis never appears as a verified fact (tests).
7. **Risks / open decisions:** storage engine and size on large repositories; privacy of runtime data;
   which embeddings, if any (optional, local).
8. **Effort:** L.
9. **Non-goals:** a general-purpose search engine; cloud indexing.

## M15 · Advanced Agent Intelligence (future)

1. **Goal:** better assignment, quality and cost through measured data.
2. **Requirements:** agent capability registry with measured run times, success rates per task kind,
   quality metrics, costs and availability; routing suggestions; running several agents on one task and
   comparing results; cost optimisation within budgets.
3. **Dependencies:** M12, M14.
4. **Prerequisites:** task history with outcomes; budget service.
5. **Deliverables:** registry; routing suggestions with explanation; comparison runs; reports.
6. **Acceptance:** suggestions are reproducible from recorded data; a human or the granted policy decides;
   comparison runs stay within budget.
7. **Risks / open decisions:** metric validity on small samples; vendor terms on benchmarking.
8. **Effort:** M–L.
9. **Non-goals:** training or fine-tuning models.
