# Ferris IDEA roadmap

**As of 2026-11-16 (main `4223536e`) · v0.20.0 · stage beta · quality gate QG-1 recorded as reached.** This is the
roadmap for readers outside the project.
It is written from the project's internal tracking and carries no dates for future work: the
project does not promise delivery dates. Items use the status words of the
[content style guide](docs/design/CONTENT_STYLE_GUIDE.md): Works, Experimental, In progress,
Planned, Not planned. For a milestone, *Done* means its tasks are ticked with evidence.

Where the project stands in numbers: [README, Development Status](README.md#development-status). What each
version changed: [CHANGELOG.md](CHANGELOG.md).

## How progress is measured

Progress is measured as **parity**, a feature-presence score: the project's own weighted checklist
of how much of the reference's functional scope exists in Ferris IDEA. The [feature matrix](docs/research/FEATURE_MATRIX.md) has 57 rows. Each row has a weight from 1 to 5
and a stage from 0 to 4:

| Stage | Meaning | Share counted |
|---|---|---|
| 0 | open | 0 |
| 1 | part of the logic, as a library or an MCP tool | 0.10 |
| 2 | logic largely complete and tested, but without a complete window part | 0.30 |
| 3 | usable by its user: in the window for a person, over MCP for an agent | 0.70 |
| 4 | compared with the reference side by side, with the comparison documented | 1.00 |

Parity is the weighted sum of the shares. It measures feature presence by reasoned judgment. It
does not measure quality, effort or completeness inside a row, and the rows are coarse. The
method and every row's evidence are in [PARITY.md](docs/research/PARITY.md) (German).

| Group | Rows | Parity on 2026-11-16 |
|---|---|---|
| IDE core | 18 | 94.0 % |
| Languages | 11 | 96.6 % |
| Database tools | 11 | 87.1 % |
| Advanced functions | 17 | 87.7 % |
| **All** | **57** | **91.8 %** |
| Verified side by side (stage 4), share of the weight | | 75.4 % |

<sub>Measured with <code>scripts/parity.sh</code>.</sub>

## Milestones

The numbers are the project's internal milestone numbers; they are kept so that documents and
changelog entries that name them can be followed. Milestone states are a second vocabulary beside
the feature status words: Done, In progress, Planned, Not reached.

| # | Milestone | State |
|---|---|---|
| M0 | Legal analysis and research rules, in the first commit together with the first code | Done |
| M1 | Repository, architecture, decision records | Done |
| M2 | Reference environment for observation and comparison | Done |
| M3 | IDE core: workspace, editor, search, window, settings, keymap | In progress (94.0 %) |
| M4 | Language support: code insight, Python, JavaScript and TypeScript, Java and Kotlin, others, frameworks | In progress (96.6 %) |
| M5 | Database tools | In progress (87.1 %) |
| M6 | Advanced functions: debugger, Git, run and test, HTTP client, terminal, containers, remote, plugins, AI Assistant | In progress (87.7 %) |
| M7 | Feature parity, verified against the reference | In progress (75.4 % of the weight verified) |
| M8 | Stabilization: performance budgets, fuzz and soak tests, project trust, file formats | Done; hand tests of parts of it open, accessibility In progress |
| M9 | Quality gate QG-1 | Done: recorded as reached on 2026-11-16, with named limits ([gate report](docs/quality/QG-1-REPORT.md)) |
| M10–M15 | AI-native phase | Planned, after the database tools |
| M16 | Remote control of the local app from paired devices | In progress |
| M17 | Mobile apps for iOS and Android | In progress |
| M18 | Web IDE in the browser | Planned; its WebSocket endpoint is In progress |
| M19 | Harness MVP: tool selection for agents, context packs, verification gates, change sets, agent host with visible agent states, local models, desktop sandbox, session history, benchmark, a heartbeat that wakes stalled agents, a task scheduler, memory layers, an encrypted vault from which agents use credentials without ever reading them | Planned, after 1.0 |
| M20 | Project cockpit with KPIs per project, tickets for people and agents, boards to choose from (Kanban, Scrum, Scrumban, Gantt, Lean, to-do lists with Gantt) | Planned, after 1.0 |

## Done: the quality gate

Quality gate QG-1 ([ADR-0015](docs/adr/0015-quality-gate-ai-native.md)) decides whether the classical
IDE is complete enough to start the AI-native phase. The [gate report](docs/quality/QG-1-REPORT.md)
records it as reached on 2026-11-16, each criterion with its limits:

| Criterion | Result |
|---|---|
| Parity of at least 85 %, every row of weight 4 or 5 at stage 3 or higher | 90.2 % at the gate measurement; all 20 heavy rows at stage 4. Limit: 12 rows were rated stage 4 by the coordinating agent on 2026-11-14 and 2026-11-15; without them 84.1 % |
| At least 50 % of the weight verified side by side | 73.1 % at the gate measurement; without the same ratings 52.6 % |
| Tests green on macOS and Linux, 20 clean CI runs of `main` in a row | met: 20 of 20 on 2026-11-15, the macOS run with every headless performance budget green on 2026-11-16. Limit: the window self-test ran once per system |
| Performance budgets | met in the macOS window run. Limit: a true cold start was never measured |
| No blocking defect, no known data-loss bug | met. Limit: one unexplained window defect stays open |
| Architecture readiness | met, with the writing paths that still go to the disk named |
| The project owner's go | given in advance on 2026-11-05 and 2026-11-14, conditional on the other criteria |

## Now: the database tools

Work in this phase, all **In progress**:

- The database group: script splitting, ER diagrams, database refactorings, schema comparison,
  query plan and the newer dialects are at stage 3; they need a side-by-side comparison with a live
  connection to reach stage 4. The defects a first comparison without a connection found in
  Ferris IDEA are being fixed.
- The open round of hand tests, and the recorded failures.
- Keymap: 56 of the 419 actions of the open macOS keymap are missing.
- Window proof for what is implemented without one: remote debugging, containers and pods as
  project hosts, hosts without SSH, paired devices.
- The window part of project rooms (working together on one project); the server half exists.
- UI driver: pointer and semantic actions, screenshots, annotations for every view. Today external
  clients can press keys, and two tool windows are annotated.
- Accessibility: a tree for screen readers ([ADR-0019](docs/adr/0019-accessibility-path-for-gpui.md)).

## Next: the harness, inside the current phase

**Planned.** These close gaps in what the agent side already offers and need no new architecture.

1. **Decide the open points of the rights model.** The project's own formatter and linter and the
   language servers start under project trust alone; commit hooks run under `vcs-write`
   (SPEC-0053, behaviour 14).
2. **Untrusted-content marking on every tool** that returns project, terminal, database or web
   content. Today 2 tool descriptions tell the agent not to follow instructions in the result, and
   2 more mark plugin-supplied names as untrusted data.
3. **A dry run for the writing tools that lack one.**
4. **A harness benchmark.** A fixed task suite on the sample projects, each agent run twice (with
   Ferris IDEA's tools, and with shell and files only), measuring success, turns, tokens, time and
   reverted changes. Until it exists, the project makes no claim that its tools make agents better.

## Later: the AI-native phase

**Planned, after the database tools.** QG-1 opens this phase. The direction is described in
[ADR-0023](docs/adr/0023-ferris-idea-as-agent-harness.md) (accepted on 2026-11-17) and
[the vision](docs/architecture/AI_NATIVE_VISION.md). The rule stays that Ferris IDEA ships no agent
loop and no model of its own.

| Milestone | Content |
|---|---|
| M10 Headless core and capability layer | every tool and every window action through one registry with versioned schemas, declared effects, error codes and cancellation ([ADR-0012](docs/adr/0012-headless-core-capability-api.md)) |
| M11 Hosting agents | Ferris IDEA starts an agent as a subprocess over the Agent Client Protocol, shows its plan, diffs and questions, and answers its permission requests by policy ([ADR-0013](docs/adr/0013-protocol-adapters.md)) |
| Change sets and verification gates | an agent's edits as one reviewable, partly applicable, revertible unit; "done" only when format, build, inspections and affected tests pass, checked by code and not by the model |
| Context packs | symbol, callers, diagnostics, related tests and recent changes within a token budget |
| Tighter containment | per-command allow lists, policy files a team can review, a sandbox or worktree per agent session |
| M12 Several agents | orchestration under grants and budgets ([ADR-0014](docs/adr/0014-orchestration-principles.md)) |
| M13–M15 | project organisation by agents, a project knowledge graph, further agent functions |

## Remote, mobile, web

Plan: [REMOTE_AND_MOBILE.md](docs/architecture/REMOTE_AND_MOBILE.md). Network rules for every path:
[ADR-0025](docs/adr/0025-network-security-for-remote-connections.md).

| Item | State |
|---|---|
| Remote projects over SSH with `ferris-idea serve` on the host | Experimental |
| Remote debugging; containers, pods, WSL and hosts without SSH as project hosts | In progress |
| Paired devices: pairing, rights per device, device log, revoke | In progress |
| Remote access to projects, runs, terminals and agent tasks from a paired device | Planned |
| Mobile apps: pair, browse, read a file (simulator and emulator) | In progress |
| Mobile apps: editing, search, run, debug, terminal, Git; a local mode | Planned |
| WebSocket endpoint over TLS 1.3 in `ferris-idea serve` | In progress; never run in a real browser |
| Web client, workspaces from `devcontainer.json`, self-hosting | Planned |

## Platforms and distribution

| Item | State |
|---|---|
| macOS (Apple Silicon) desktop window | Experimental |
| Linux desktop window: the window self-test passed once on a virtual X display | In progress |
| Linux desktop window: verification on a real desktop | Planned |
| Windows desktop window | Planned |
| Signed and notarized macOS build, installers, updates | Planned; needs the owner's publication approval |
| Published releases | Planned; none exist |

## Not planned

- Loading the reference platform's JVM plugins. Plugins are WebAssembly modules.
- Trial periods, license checks or telemetry in the product ([ADR-0007](docs/adr/0007-no-restrictions.md)).
- Rebuilding the reference's AI assistant. Its open-source MCP server plugin served as a template
  for the first tool groups, and its MCP database tools were observed
  ([PROVENANCE.md](docs/legal/PROVENANCE.md), OBS-0004).
- The design, icons and texts of the reference's proprietary parts.

## How this file is kept

It is rewritten from the internal tracking when a version is cut. If this file and the code
disagree, the code and [CHANGELOG.md](CHANGELOG.md) are right; please open an issue.
