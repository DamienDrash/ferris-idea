# SPEC-0341: Heartbeat for agents (watchdog)

- **Version:** 1 · **Date:** 2026-11-17
- **Sources:** `GEN` (watchdogs and supervisors of long-running processes: progress timeouts,
  restart limits with back-off, escalation to a person instead of restarting forever); ADR-0023
  rule 9 (visible state per agent, "unknown" never idle), rule 7 (safety rules for MCP tools),
  rule 16 (sessions survive restarts, resuming through ACP `session/load` or CLI resume flags);
  ADR-0008 (every feature has an MCP tool); SPEC-0053 (agent rights, the grant file in the user's
  settings folder, the agent log). No observation of the reference: the reference has no agent
  watchdog.
- **Feature matrix rows:** none (ROADMAP M19.12, harness phase after 1.0; owner's order of
  2026-11-17).

## Purpose
An agent that Ferris IDEA hosts can hang (it works but makes no progress), fall asleep (it went idle
while its task is open) or die (its process is gone). A watchdog notices this, wakes the agent with
a prompt the user wrote, or resumes its saved session after the process is gone, and stops after a
few attempts or when the budget is spent and tells the person instead of trying forever. An agent
that waits for an approval or an answer, or whose state Ferris IDEA cannot tell, is never nudged.

This version is the model without a host of its own: Ferris IDEA does not host agents yet (ROADMAP
M11.2, M11.3, M19.5). The watchdog decides over an interface that the agent host of M19.5 provides
(state, progress, process, saved session, waking, resuming, notices); its configuration, its own
state and its MCP tools exist now.

## Behaviour

### What the watchdog sees
1. Per watched agent the host reports an observation: its state (`working`,
   `waiting_for_approval`, `waiting_for_input`, `idle`, `done`, `failed`, `unknown`; ADR-0023 rule
   9), whether its process is `alive`, `gone` or not known, a progress counter that grows with
   every sign of progress (output, tool calls, edits; the host chooses, the watchdog only compares)
   or none, and the money spent so far in cents or none. [ADR-0023, GEN]
2. Progress is a growing counter or a change of state. Time since the last progress is measured
   with the watchdog's clock; nothing reads wall time directly, so the decisions can be tested with
   a fake clock. [GEN]

### Decisions per round
3. The watchdog acts only while it is enabled for the workspace (behaviour 13); off by default.
   One round observes every agent the host lists once, in the order of their ids, and takes at most
   one action per agent. [GEN]
4. `done` and `failed` end the watch of the agent: nothing is done for it until its state changes.
   [GEN]
5. `waiting_for_approval` and `waiting_for_input` need the person: the agent is never woken and
   never resumed in these states, because a wake prompt must not answer an approval or a question
   in the person's place. [ADR-0023 rule 9, GEN]
6. `unknown` never counts as idle or hung: the agent is never woken in it. When the state has been
   `unknown` without progress for `unknown_after` seconds, the person gets one notice; the notice is
   not repeated until the state changes. [ADR-0023 rule 9]
7. A process that is `gone` while the state is neither `done` nor `failed` means the agent died.
   When resuming is on and the host has a saved session for it marked resumable, the watchdog
   resumes that session; otherwise it escalates (behaviour 10). A process whose liveness is not
   known is never taken as gone. [ADR-0023 rule 16, GEN]
8. A `working` agent without progress for `stall_after` seconds is hung, an `idle` one without
   progress for `idle_after` seconds is asleep; both are woken with the wake prompt. After a wake or
   a resume the watchdog waits `grace` seconds before it judges the agent again. [GEN]
9. Before every wake or resume the limits are checked; any one that is reached escalates instead:
   - `max_attempts` wakes and resumes in a row without progress in between;
   - `max_actions` wakes and resumes of the agent in all;
   - the budget: with `budget_cents` set, spending at or above it, or spending the host does not
     report (it cannot be checked, so it counts as reached);
   - no wake prompt configured (only for a wake; resuming needs no prompt).
   Progress (behaviour 2) sets the attempts in a row back to 0; the count of all actions and the
   spending are never set back. [GEN]
10. Escalating marks the agent `escalated` with its reason and sends the person one notice. From
    then on the watchdog takes no action for the agent until the person acknowledges the
    escalation; acknowledging sets the attempts in a row back to 0 and the clock of progress to
    now, but neither the count of all actions nor the budget. So the number of actions per agent
    stays bounded by `max_actions` whatever happens. A failed wake or resume escalates with the
    host's reason. [GEN]

### Security
11. The wake prompt is the text the user wrote in the watchdog's configuration in their own
    settings folder (behaviour 13). It is never taken from an agent's output, from the workspace or
    from an MCP call, and it is the only text the watchdog ever sends to an agent. [ADR-0023 rule
    7, GEN]
12. Resuming never widens an agent's rights: the resumed session gets the rights its saved session
    had, less every right the workspace no longer grants now (the intersection of both). A session
    saved without rights is resumed without rights. [ADR-0023 rule 7, SPEC-0053]
13. Every wake, resume, escalation, notice and acknowledgement writes one line to the agent log
    `.ferris-idea/agent-log.jsonl` (SPEC-0053 behaviour 5) with `time`, `tool` (`agent_watch`),
    `agent`, `action`, `reason` and, as they apply, `attempt`, `actions`, `prompt_chars` (the
    length of the prompt, never its text) and `rights`. A wake or a resume is taken only after its
    line was written: when the log cannot be written, nothing is sent and the agent is escalated.
    [SPEC-0053, ADR-0023 rule 7]

### Configuration and persistence
14. The configuration lives in the user's settings folder beside the grant file of SPEC-0053
    (`agent-watch.toml`), one `[[workspace]]` table per workspace with `path` and the keys
    `enabled` (default false), `wake_prompt` (none), `stall_after` (600), `idle_after` (300),
    `grace` (300), `unknown_after` (900), all in seconds, `max_attempts` (3), `max_actions` (10),
    `budget_cents` (none) and `resume` (true). Paths are compared after resolving links. A file
    inside the workspace is ignored and reported, so a cloned project cannot configure the
    watchdog; unknown keys or values outside the bounds of behaviour 15 make the file a problem and
    leave the watchdog off. [SPEC-0053, GEN]
15. Bounds: every time from 60 to 86 400 seconds, `max_attempts` from 1 to 10, `max_actions` from 1
    to 50, a wake prompt of 1 to 2 000 characters without control characters other than line breaks
    and tabs. [GEN]
16. The watchdog's own state per agent — phase (`watching`, `needs_person`, `unknown`, `woken`,
    `resumed`, `escalated`, `ended`), the last state and progress seen, the time of the last
    progress, the attempts in a row, all actions, the time of the last action, the escalation
    reason and whether the unknown notice was sent — is saved after every round that changed it in
    `agent-watch-state.json` beside the configuration, per workspace, written through a temporary
    file and a rename. It is read back when the watchdog starts, so a restart of Ferris IDEA neither
    forgets an escalation nor sets the limits back. A state file that cannot be read starts empty
    and is reported. [ADR-0023 rule 16, GEN]

### MCP tools
17. `agent_watch_status` (reads, needs no right) answers the configuration of the workspace (with
    `wake_prompt_set` and the prompt's length, not the prompt), the files consulted, their
    problems, the watched agents with their saved state, and `host`, which says that no agent host
    is attached yet until M19.5 provides one. [ADR-0008]
18. `agent_watch_configure` changes the configuration of the workspace: `enabled`, the times, the
    limits, `budget_cents`, `resume`, and `acknowledge`, a list of agent ids whose escalation the
    person acknowledges. It has a dry run (`dry_run`, default true) that answers the configuration
    it would write. `dry_run: false` needs the `settings` right (it changes the user's own
    settings, SPEC-0053 v8) and is logged. It never sets the wake prompt: an argument
    `wake_prompt`, like any unknown argument, is refused; the prompt it keeps is the one in the
    file. Values outside the bounds of behaviour 15 are refused with the bound. Unknown agent ids
    in `acknowledge` are refused. [ADR-0008, ADR-0023 rule 7]

## Acceptance criteria
- **AK-1** Given a fake host and a fake clock, when a working agent makes no progress for
  `stall_after` and an idle one for `idle_after`, then each is woken once with the configured
  prompt, judged again only after `grace`, and progress sets its attempts back (behaviours 2, 3, 8).
- **AK-2** Given agents that wait for approval, wait for input, are `unknown`, `done` or `failed`,
  when any time passes, then none is woken; an `unknown` one gets exactly one notice after
  `unknown_after` (behaviours 4, 5, 6).
- **AK-3** Given an agent whose process is gone, when it has a resumable saved session, then it is
  resumed with the intersection of its saved rights and the workspace's current grants; without a
  session, with resuming off or when the resume fails, it is escalated; an unknown liveness is
  never taken as gone (behaviours 7, 12).
- **AK-4** Given an agent that never makes progress, when rounds go on, then after `max_attempts`
  actions it is escalated with one notice and nothing more is sent; after acknowledging, the
  attempts start again but `max_actions` and the budget still stop it; a budget reached, spending
  not reported with a budget set, and a missing wake prompt escalate instead of waking (behaviours
  9, 10).
- **AK-5** Given a log that cannot be written, when a wake is due, then nothing is sent; every
  wake, resume, escalation, notice and acknowledgement leaves one line in the agent log with the
  prompt's length but not its text (behaviour 13).
- **AK-6** Given a configuration file, when it is read, then defaults, bounds, unknown keys, a file
  inside the workspace and other workspaces' tables follow behaviours 14 and 15; given saved state,
  when a new watchdog starts from it, then escalations and counts are kept (behaviour 16).
- **AK-7** Given an MCP client, when it calls `agent_watch_status`, then it gets the configuration
  without the prompt's text and the saved agents; `agent_watch_configure` answers a dry run by
  default, refuses `dry_run: false` without the `settings` right, refuses `wake_prompt`, values
  out of bounds and unknown agents, and with the right writes the file, keeps the prompt and
  acknowledges an escalation (behaviours 17, 18).

## Out of scope
- Hosting agents (ACP client, terminal agents with state detection): ROADMAP M11.2, M11.3, M19.5.
  Until then no agent is watched in a running Ferris IDEA; the model, its configuration, its state
  and its tools are ready for the host to plug in.
- The timer that runs the rounds in the window or a headless server, and notices in the window;
  they come with the host.
- Changing the wake prompt from the window's settings; until then the user edits the file.
- Per-agent configuration (one configuration per workspace in this version).

## Known deviations from the reference
The reference has no agent watchdog; nothing is copied. Deviations from common practice, on
purpose:
- **D1** No endless restart loop: every agent's actions are bounded by `max_actions`, which neither
  an acknowledgement nor a restart of Ferris IDEA sets back (behaviours 10, 16).
- **D2** Unknown is not idle (ADR-0023 rule 9), and an agent waiting for a person is never nudged,
  so a wake prompt cannot answer an approval prompt (behaviours 5, 6).
- **D3** An agent cannot write its own wake prompt: the configuration lives in the user's settings
  folder, a project's files cannot configure the watchdog, and the prompt is never set over MCP
  (behaviours 11, 14, 18).
