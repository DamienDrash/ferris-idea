# ADR-0023: Ferris IDEA as an agent harness — tools, verification and orchestration, no agent loop of our own

- **Status:** accepted · 2026-11-17 by the project owner (proposed 2026-11-05) · written on the project owner's request of 2026-11-05 ("is it possible and
  sensible to build the IDE, AI-first and with MCP, into an AI harness"). It frames
  ADR-0008 and ADR-0012 to ADR-0017 as one product direction. Implementation is planned for
  M10–M12 and starts only after quality gate QG-1 (ADR-0015). Rule 7 applies now.
  **v2 · 2026-11-05:** rules 9–16 added on the project owner's request (lessons from the herdr terminal runtime,
  agents talking to each other, model choice per task, terminal access for the assistant). Rule 15
  may start before QG-1 as part of the AI Assistant (F-082).
- **Context:** An *agent harness* is everything around a language model that lets it do work:
  - the tools it can call;
  - the context it is given;
  - the loop that plans, acts and checks;
  - rights and sandboxing;
  - human approvals;
  - limits on cost and failure.

  Today's coding harnesses (Claude Code, Codex CLI, Gemini CLI, Cursor's agent) mostly act on text.
  They read files, search with `grep`, edit by string replacement and learn about mistakes from
  compiler output in a shell.

  An IDE already has what those harnesses lack:
  - a semantic model of the project (references, types, call hierarchy, safe refactorings);
  - a running verification loop (diagnostics, inspections, build, tests);
  - debugger, database and HTTP tools with structure;
  - local history to undo;
  - a trust and permission model;
  - a screen on which a person can review.

  Ferris IDEA already exposes these over MCP (ADR-0008, about 60 tools) and uses the providers' own
  command line programs for chat (ADR-0017). The architecture for a headless core (ADR-0012),
  protocol adapters (ADR-0013) and orchestration (ADR-0014) is proposed.

  The agent loop itself is where model vendors compete and improve monthly. A small project cannot
  keep up with them there, and gains nothing by trying.

## Decision

**1. Ferris IDEA is a harness without a loop of its own.** Ferris IDEA provides the parts of a harness that
depend on understanding the project:
- tools;
- context;
- verification;
- rights;
- review;
- orchestration.

It does not ship a general agent loop that competes with the vendors' agents. The loop belongs to
the agent Ferris IDEA hosts or serves (Claude Code, Codex, Gemini CLI, local models through any
ACP/MCP-capable agent). The AI Assistant chat (ADR-0017) stays a thin client of such agents.

**2. Two roles, both supported:**

| Role | Protocol | Ferris IDEA does |
|---|---|---|
| **Tool host** | MCP (ADR-0008, ADR-0013) | Any external harness calls Ferris IDEA's tools, with or without a window. |
| **Agent host** | ACP (ADR-0013) | Ferris IDEA starts the agent as a subprocess, passes it its own MCP server, answers its file and terminal requests through the capability layer, shows its plan, diffs and questions, and asks for approvals. |

The tool host comes first: it helps every agent at once and needs no new protocol.

**3. Tools are built for agents as well as people.** Every capability (ADR-0012) has:
- structured input and output with a JSON schema;
- stable error codes;
- token-lean answers: paged, filtered, with ranges instead of whole files;
- declared effects (read, write workspace, run code, network, outward action).

Prefer semantic operations over text operations:
- rename over search-and-replace;
- "references of symbol" over `grep`;
- "run affected tests" over "run all tests".

**4. Context packs instead of whole files.** A capability `context.for(target, budget)` assembles
what an agent needs for a task within a token budget:
- the symbol and its signature;
- callers and implementations;
- current diagnostics;
- related tests;
- recent changes;
- the project's conventions.

Every item names its source and version. The pack is deterministic, so two agents asking the same
question get the same context.

**5. Verification gates are part of the harness.** An agent's change set is not "done" until the gates
configured for the project have passed:
- format;
- build;
- inspections without new errors;
- affected tests;
- optional full tests.

The results go back to the agent as structured data: file, range, message and suggested fix. The
gates are deterministic Rust code (ADR-0014 rule 2), and a model cannot mark them passed.

**6. Change sets are the unit of work.** An agent's edits form a change set. Each change set:
- can be previewed as a diff;
- can be applied in part, per file or hunk;
- can be reverted through local history;
- is tied to the task, agent, model and cost that produced it.

By default, agents with write rights work in their own worktree (ADR-0014 rule 3). Integration goes
through the merge queue.

**7. Safety rules that apply now to every new MCP tool:**
- **Untrusted content:** tool results that carry project or external content (file text, web pages,
  database rows, issue text) are marked as untrusted data in their schema description. Agents must
  not treat them as instructions.
- **No secrets:** secrets never enter context or tool output. Passwords stay in the keychain;
  `.env`-like values are masked.
- **Approval for outward actions:** pushing, publishing, sending, deleting outside the workspace and
  writing to production databases need a human approval. A grant to do this is never implied by a
  role.
- **Read-only first:** every tool works read-only unless the session holds the matching right
  (`agent-permissions.toml`, IMP-0293 pattern).
- **Local audit:** an audit record of every tool call (who, what, effect, result) is kept on the
  machine and never sent anywhere (ADR-0007).

**8. A harness benchmark.** A fixed task suite runs against the reference projects
(`tests/reference-projects/`). Examples:
- fix a failing test;
- rename across modules;
- add an endpoint with a test;
- fix an SQL query against the reference database.

Each task is run by each supported agent twice: with Ferris IDEA's tools, and with only shell and file
access. The measurements are:
- success;
- turns and tokens;
- wall time;
- gate failures;
- reverted change sets.

This turns "better than text-based harnesses" into a number, and it guards against regressions when
tools change. The results stay local; publishing them is GO-4.

## v2 additions (2026-11-05)

The project owner asked to take the lessons from herdr into the plan. herdr is an Apache-2.0 terminal runtime
that hosts coding agents in panes and lets one agent direct others through a CLI, a socket API and a
skill. He also asked for three more things:
- agents that talk to each other;
- a weaker or stronger model per task, changeable later;
- the assistant chat seeing terminal input and output.

**9. Visible state per agent.** Every hosted agent has one of these states:
- working;
- waiting for approval (blocked);
- waiting for input;
- idle;
- done;
- failed.

The state comes from the protocol (ACP session updates, MCP task status). Only for terminal agents
does it come from hooks or, last, from recognised screen text (rule 11).

The states roll up from agent to task to project:
- in the sidebar;
- in the task board;
- in the status bar.

"Blocked" and "waiting for input" raise a notification, so a person sees at once where they are
needed. Unknown screens count as *unknown*, never as idle. This improves on herdr, where unfamiliar
approval prompts look idle.

**10. Any agent can conduct.** An `ferris-idea` agent skill, plus matching MCP tools and CLI commands,
lets one agent direct others:
- `agent.start(task, model)`;
- `agent.prompt`;
- `agent.read` (output and state);
- `agent.wait(state)`;
- `agent.stop`;
- `agent.list`.

The conductor can be any agent, from any provider. The deterministic task graph (ADR-0014 rule 2)
stays the source of truth underneath:
- Every call is checked against the conductor's grants: how many agents, which models, what budget.
- Every call is recorded.
- Starting agents needs an explicit grant (ADR-0014 rule 5).

**11. Terminal agents as a fallback.** Agents without ACP run in Ferris IDEA terminal tabs:
- With state detection from their lifecycle hooks where they offer them, otherwise from shipped
  screen manifests.
- Under stricter defaults: their own worktree, no network grant, and an approval for outward actions
  (ADR-0013 rule 4).
- Detection manifests ship with Ferris IDEA. They are updated only when the user asks, never fetched in
  the background (ADR-0007).

**12. Agents talk to each other.** Agents exchange messages through the persisted message store
(ADR-0014 rule 4), never through each other's context. Messages are:
- direct, from one agent to another;
- to a task's team;
- to the project.

The structured kinds are:
- question and answer;
- request for review;
- handoff;
- blocker;
- finding;
- status.

Every message is visible to people in the session view, carries sender, model and task, and can be
answered by a person in place of an agent. Content from another agent is untrusted data for the
receiver, just like a web page (rule 7). An agent cannot raise its rights by asking another agent to
act for it: the receiver's own grants apply.

**13. Model per task, changeable.** Each task carries a model tier:
- **light:** fast and cheap, for search, summaries, tests and lint;
- **standard;**
- **strong:** for planning, hard bugs, architecture and review.

It also carries a concrete agent and model, chosen when the task starts. A person can change the
model or tier of a waiting or paused task at any time; a conductor can do so within its grants. A
running agent is not switched mid-turn: the change takes effect at the next turn, or by handing the
task off with a summary to a new agent on the other model.

Escalation is a rule, not a guess. After *n* failed gate runs (rule 5), or on a blocker of a named
kind, the task is offered one tier up. Every switch is logged with its reason and cost.

Ferris IDEA suggests a tier from the task kind. It never switches without the person's or the
conductor's confirmed rule.

**14. Shared deterministic processes.** Gate watchers run once per worktree, or once for the merge
queue, and every agent reads their results:
- build;
- affected tests;
- inspections;
- format.

Agents do not each start their own runs. This saves tokens, CPU and time, and gives all agents the
same verdict.

**15. The assistant sees what the person sees, with consent.** The AI Assistant chat and hosted
agents can be given:
- terminal tabs: the last *n* lines, the selection, or the output of one command with its exit code;
- run and test output;
- debugger state;
- problems;
- the Git diff.

They come in through "Attach to chat" and through MCP tools (`terminal.read`, `run.output`, and so
on).

Rules for this:
- **Visible attachment:** an attachment shows up as a chip in the chat, like attached code
  (SPEC-0138).
- **Secrets masked:** known secret patterns are masked before anything leaves the IDE.
- **Opt-in per tab:** a terminal tab is never read without the person's choice. The choice can be
  "this tab for this chat".
- **Commands need approval:** proposing a command is allowed, but running it in the person's
  terminal needs an approval per command, or a grant for a named command pattern in the project.
  Its output then comes back to the chat.

The read-only part and "Explain with AI" on terminal output belong to the classical AI Assistant
(F-082). They may be built before QG-1.

**16. Sessions survive.** Hosted agent sessions, their layout, state and message history persist
across Ferris IDEA restarts. Agents that support resuming (ACP `session/load`, CLI resume flags) are
resumed. Others are shown as ended, with their history.

Remote hosts are reached through the user's existing SSH configuration. Ferris IDEA stores no keys or
passwords for them.

## Consequences
- **Product:**
  - The differentiator is concrete and measurable (rule 8).
  - We ride on the vendors' progress instead of competing with it.
  - Agents that speak only MCP and agents that speak ACP are both first-class.
- **Engineering:** Most of the work is already planned:
  - capability layer and adapters (M10, M11);
  - orchestration (M12).

  New items for the future milestones are context packs (rule 4), structured gate results (rule 5),
  change sets (rule 6) and the benchmark (rule 8).
- **Now, before QG-1:** only rule 7 applies, and it shapes the MCP tools we add during the parity
  work. No ACP hosting, context packs or benchmark code before the gate.
- **Risks:**
  - The protocols still change; adapters keep that change local (ADR-0013).
  - Prompt injection through project content is the main security risk. Rules 7 and 3 limit what an
    injected instruction can reach.
  - Vendors may build their own IDE integrations that bypass ours; the MCP tool host still serves
    them.

## Alternatives considered
- **Our own agent loop with direct model APIs:** we would duplicate vendor work, lag behind every model
  release, and take on API key and billing handling. Rejected. A thin, replaceable chat client stays
  (ADR-0017).
- **Only an MCP server, no hosting:** this is cheapest, but gives up review, approvals, worktree
  isolation and orchestration inside the IDE, which is where an IDE adds most. Rejected as the end
  state; it is the first step.
- **Building on a third-party harness framework:** this would tie the core to one vendor's runtime and
  licence. Rejected. Adapters over our own capability layer stay vendor-neutral.

## References
- ADR-0007 No restrictions
- ADR-0008 MCP server
- ADR-0012 Headless core
- ADR-0013 Protocol adapters
- ADR-0014 Orchestration
- ADR-0015 QG-1
- ADR-0017 Provider CLIs
- [AI-native vision](../architecture/AI_NATIVE_VISION.md)
- [Future milestones](../architecture/FUTURE_MILESTONES.md)
