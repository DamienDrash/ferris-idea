# SPEC-0348: Agent host over ACP

- **Version:** 2 · **Date:** 2026-11-23 (v1 2026-11-18)
- **Changes in v2:** proposed edits of a permission request with their diff (behaviour 24), the
  mark of a request that repeats a refused one (behaviour 25), what a turn changed in the
  workspace and undoing it (behaviours 26–29), MCP `agent_host_changes` (behaviour 30); from
  OBS-0121 gaps A9, A10 and IMP-0547 to IMP-0549.
- **Sources:** `GEN` (Agent Client Protocol version 1, the open protocol for an editor that hosts a
  coding agent as a subprocess: JSON-RPC 2.0 over stdio, one message per line; `initialize`,
  `session/new`, `session/prompt` with its stop reasons, `session/update`,
  `session/request_permission`, `session/cancel`; process supervisors: cleaned environments,
  graceful stop then kill); ADR-0013 rule 3 (Ferris IDEA is an ACP client that hosts agents);
  ADR-0023 rules 2 (agent host role), 7 (safety rules for MCP tools), 9 (visible state per agent,
  notification, unknown never idle); ADR-0017 (credentials stay with the provider's program);
  ADR-0008 (every feature has an MCP tool); SPEC-0053 (agent rights, the grant file in the user's
  settings folder, the agent log); SPEC-0341 (the watchdog reads the same seven states). v2:
  `OBS` OBS-0121 (the reference's agent sessions beside Ferris IDEA's host: approval cards with the
  diff of an edit, the list of a turn's changed files with undo), IMP-0547 to IMP-0549; ACP's
  `diff` content of a tool call (`path`, `oldText`, `newText`) [`GEN`].
- **Feature matrix rows:** none (ROADMAP M19.5, harness phase after 1.0).

## Purpose
A person wants to run the coding agent of their choice inside Ferris IDEA and see at a glance what
each agent is doing: working, blocked on an approval, waiting for them, idle, done or failed. The
agent host starts an agent the user configured as a child process, speaks the Agent Client
Protocol with it, derives its state from the protocol alone, raises a notice when an agent needs
the person, and lets a person (or an agent the person granted the right) prompt it, read its
output, answer its permission requests and stop it.

This version is the headless core and its MCP tools. The window's view of the agents, terminal
agents as a fallback (ADR-0023 rule 11), and conducting agents from other agents (M19.6) build on
it.

## Behaviour

### Lifecycle
1. An agent is defined in the host's configuration (behaviour 11) by an id, a program and its
   arguments. Started agents are *hosted agents* with their own id `<definition>-<n>`. [GEN]
2. The child's stdin and stdout carry the protocol: one JSON-RPC 2.0 message per line, UTF-8. Its
   stderr is read line by line into the output (behaviour 9) and never parsed. [GEN]
3. The host speaks ACP version 1. It sends `initialize` with `protocolVersion: 1`, the client
   capabilities `fs.readTextFile: false`, `fs.writeTextFile: false`, `terminal: false` and its
   name and version. An answer with another version, or an error, fails the agent. Then it sends
   `session/new` with the workspace as `cwd` and no MCP servers; an error, or an answer without a
   session id, fails the agent with the reason and the hint that signing in happens in the agent's
   own program. [GEN, ADR-0017]
4. Starting is refused when `max_agents` agents run already (behaviour 12), when the program does
   not exist, or when the plan's working folder is not the host's workspace. [GEN]
5. Stopping answers every waiting permission request with `cancelled`, sends `session/cancel` when
   a turn is running, closes the child's stdin, waits up to 3 seconds for the process to end and
   kills it otherwise. A stopped agent ends `done`. When the host goes away (the MCP server or the
   window ends), every agent still running is killed: no agent outlives its host. [GEN]
6. A prompt is sent as `session/prompt` with one text block, only while the agent is
   `waiting_for_input` or `idle`; in any other state it is refused with the state. A prompt has 1
   to 20 000 characters and no control characters but line breaks and tabs. [GEN]

### State
7. The state is derived from what the protocol and the process said, in this order (the first
   that holds wins): [ADR-0023 rule 9]
   - the process ended: `failed` when the protocol had failed, `done` when the host stopped it or
     its exit code was 0, `failed` otherwise (with the exit code or "ended by a signal");
   - the protocol failed (behaviours 3, 10): `failed`;
   - a stop is in progress: `working`;
   - the agent wrote something the host cannot read (behaviour 10), or closed its output while its
     process runs: `unknown`;
   - a permission request waits: `waiting_for_approval`;
   - the session is not open yet: `working` (starting);
   - a turn is running: `working`;
   - the last turn ended with `max_tokens`, `max_turn_requests` or `refusal`, or the prompt was
     answered with an error: `waiting_for_input`, with the reason;
   - no prompt was answered yet: `waiting_for_input`;
   - otherwise (the last turn ended with `end_turn` or `cancelled`): `idle`.
   A stop reason the host does not know makes the state `unknown`, never `idle`. Every state
   carries a `detail` in words and the time it was entered.
8. Entering `waiting_for_approval`, `waiting_for_input`, `failed` or `unknown` raises one notice
   for the person: it is kept with the agent (the newest 20) and recorded in the workspace's
   notifications (SPEC-0061 behaviour 64), `failed` as an error. The notice names the agent and
   the state; it never repeats agent text other than the detail. [ADR-0023 rule 9]
9. The output keeps the newest 500 entries per agent, each with a growing number, the time, a
   kind (`prompt`, `message`, `thought`, `tool_call`, `plan`, `permission`, `answer`, `refused`,
   `stderr`, `state`, `host`) and at most 8 000 characters of text. Streamed message and thought
   chunks join the newest entry of their kind. `session/update` kinds the host does not show still
   count as progress. A progress counter grows with every message from the agent, for the
   watchdog (SPEC-0341 behaviour 1). [GEN]
10. A line that is no JSON-RPC 2.0 message, a message with neither method nor id, an answer to a
    request the host never sent, and a line longer than 4 MiB (dropped) make the state `unknown`
    until the next readable message. A failed protocol (behaviour 3) or a failed write to the
    child ends the process. [GEN, ADR-0023 rule 9]

### Security
11. The configuration lives in the user's settings folder beside the grant file of SPEC-0053
    (`agent-host.toml`): `max_agents` and `[[agent]]` tables with `id`, optional `name`, `command`
    and optional `args` and `env`. A file inside the workspace is ignored and reported, so a cloned
    project can never make Ferris IDEA start a program. A malformed file, unknown keys or values out
    of bounds are a problem and nothing can be started. No tool call can name a program, an
    argument or an environment value; it can only name a configured agent. [SPEC-0053, GEN]
12. Bounds: `max_agents` 1 to 8 (default 2); at most 32 agents; an id of 1 to 64 characters of
    a-z, 0-9, `-`, `_`, unique; `command` an absolute path; at most 64 arguments of at most 4 096
    characters without NUL; at most 32 extra environment names. [GEN]
13. The child's environment is cleared and gets only the names `PATH`, `HOME`, `USER`, `LOGNAME`,
    `SHELL`, `LANG`, `LC_ALL`, `LC_CTYPE`, `LC_MESSAGES`, `TERM`, `TMPDIR`, `TZ` and the agent's
    extra names, each with Ferris IDEA's own value when set. A name that looks like a credential
    (a part `TOKEN`, `SECRET`, `PASSWORD`, `PASSWD`, `PASS`, `KEY`, `APIKEY`, `CREDENTIAL(S)`,
    `AUTH`, `COOKIE`, `PRIVATE`, or `TOKEN`, `SECRET`, `PASSWORD`, `CREDENTIAL`, `APIKEY` anywhere)
    is refused in the configuration and never passed: the agent's own program keeps its
    credentials (ADR-0017); handing one over through the vault is open point 3. Answers name the
    environment passed, never its values. [ADR-0017, ADR-0023 rule 7]
14. The agent starts in the workspace (resolved) and its session is opened there. The host offers
    no file system, no terminal and no MCP server to the agent: every request from the agent other
    than `session/request_permission` is answered with error -32601 "method not found" and shown
    as `refused`. So a hosted agent cannot reach Ferris IDEA's tools, and cannot answer its own
    permission requests through them. [ADR-0013 rule 3, ADR-0023 rule 7]
15. A permission request is kept as waiting with the host's request number, the tool call's title
    and kind, and the options (id, name, kind) and makes the state `waiting_for_approval`. The host
    never answers one on its own, never on a timer and never by default: only an explicit answer
    (behaviour 22) or a stop (behaviour 5, `cancelled`) ends it. An option of kind `allow_always`
    is never chosen: the host gives no standing approval. A request without options is answered
    `cancelled` at once. [ADR-0023 rule 7]
16. Hooks of the shared agent configuration (SPEC-0346, M19.8) are never run by the host, and no
    project file is read to configure an agent. [ADR-0023 rule 7]
17. Everything an agent writes — messages, thoughts, tool call titles, plans, permission titles
    and option names, stderr, error messages — is untrusted data. Over MCP it is returned with
    secrets masked (SPEC-0313 behaviour 6) and described as data, not instructions. [ADR-0023 rule 7]

### MCP tools
18. A new right `agents` ("start the agents you configured for Ferris IDEA's agent host, send them
    prompts, read their output, answer their permission requests and stop them") is granted in the
    grant file like the others; in the window a missing right is asked for (SPEC-0053 v4
    behaviour 7). [SPEC-0053]
19. `agent_host_list` (reads, needs no right) answers the configuration file, its problems,
    `max_agents`, the configured agents (no environment values), the default environment names,
    the hosted agents with state, detail, session, turns, the number of waiting approvals and the
    exit, and the notices. `agent_host_start` takes `agent` and has a dry run (`dry_run`, default
    true) that answers the plan — program, arguments, working folder, environment names — what is
    offered, and the sandbox (none yet). `dry_run: false` needs `agents`, starts the agent, waits up
    to `wait_seconds` (default 10, at most 120) for the session and answers the hosted agent.
    [ADR-0008]
20. `agent_host_prompt` takes `id`, `text` and `wait_seconds` (default 0) and needs `agents`. [ADR-0008]
21. `agent_host_read` takes `id`, `since`, `limit` (default 50, at most 500), `wait_seconds` and
    `until` (states; default any but `working`) and needs `agents`. It answers the agent with its
    waiting permission requests, the entries, `more` and `next_since`. [ADR-0008]
22. `agent_host_respond` takes `id`, `request` and either `option_id` or `cancel: true`, needs
    `agents` and refuses an `allow_always` option. [ADR-0008, ADR-0023 rule 7]
23. `agent_host_stop` takes `id`, needs `agents` and answers the final state. Every call of the
    changing tools is written to the agent log (SPEC-0053 behaviour 5) through the effects table.
    [ADR-0008, SPEC-0053]

### Proposed edits and repeated requests (v2)
24. A permission request keeps, besides title, kind and options, the edits its tool call proposes:
    every `content` item of type `diff` with its `path`, `oldText` (absent for a new file) and
    `newText`, at most 8 per request, each text cut at 64 Ki characters with a mark. The output
    entry of the request names the edited files. They are the agent's own words about the edit,
    untrusted, shown so the person can judge it before allowing it; what really changed comes from
    behaviour 27. [GEN, OBS-0121 R7]
25. When the person refused a request in the running turn (an option of kind `reject_*`, or
    cancelling it), a later request of the same turn and the same kind whose last command (the part
    after the last `&&`, `;` or `|`, spaces joined) or whose edited files equal those of the refused
    one carries the refused request's number (`similar_refused`), and its output entry says "you
    refused a similar request in this turn". It is still asked like any request: the mark informs,
    it decides nothing. A new prompt clears the refusals. [IMP-0547]

### What a turn changed (v2)
26. Before a prompt is sent, the host records the workspace: every regular file below it that the
    workspace's ignore rules (`.gitignore`, `.git/info/exclude`, `.ignore`) do not exclude, without
    the folders `.git` and `.ferris-idea`, symbolic links not followed; for each its size, modification
    time and, for files of at most 512 KiB while the record stays within 64 MiB, its content. At
    most 20 000 files; beyond, the record is marked `truncated`. [GEN]
27. When the turn ends (the answer to the prompt, or the process ended during the turn), the host
    records the workspace again and compares, then leaves `working` ("recording what the turn
    changed" is its detail meanwhile). The turn's changes list every file created, modified (other
    content; by size and time when a content was not recorded) or deleted, by path, with the lines
    added and removed for text, `binary` for other content or content not recorded, and
    `restorable` when its content before and after was recorded. The list comes from the file
    system, not from the agent: files the agent's commands created or changed count, a refused edit
    does not, and changes anyone else made in the workspace during the turn count too. An output
    entry of kind `changes` sums it up. The newest 8 turns per agent are kept. [IMP-0548,
    IMP-0549]
28. Undo puts back the newest turn not undone yet, only between turns, only when named: every file
    of the turn returns to its content before (created files are deleted, deleted files written
    again, folders created as needed), all or nothing. It is refused, naming the files, when a file
    was not recorded or changed again after the turn (its content no longer equals the content
    after the turn). An undone turn is marked `undone`, and an output entry says so. [IMP-0548]
29. The diff of a turn is a unified diff per text file with three lines of context; binary or
    unrecorded files are named only; at most 200 KiB per answer, cut with a mark. [GEN]
30. `agent_host_changes` takes `id`, `turn` (default the newest kept), `diff` (default false),
    `undo` (default false) and `dry_run` (default true) and needs `agents`; it answers the turn's
    prompt (first 80 characters), files, `truncated`, `undone` and the kept turns, the diff when
    asked, and with `undo` what would be put back or the blockers. `undo` with `dry_run: false`
    needs `write` as well and is written to the agent log. Texts are masked like the output
    (behaviour 17). [ADR-0008, SPEC-0053]
31. The window's view of agents (SPEC-0357) reads the same changes and undoes through the same
    host. [ADR-0023 rule 9]

## Acceptance criteria
- **AK-1** Given a fake ACP agent (a shell script, no model, no network), when it is started, then
  `initialize` and `session/new` are spoken, the agent is `waiting_for_input` with an open session
  and protocol version 1; a prompt makes it `working` and its `end_turn` `idle`, with the message in
  the output; a stop ends it `done` (behaviours 1–3, 5–7, 9).
- **AK-2** Given a prompt that makes the agent ask a permission, then the agent is
  `waiting_for_approval` with the request's title and options, stays so without an answer, raises a
  notice, refuses `allow_always` and unknown options, and an `allow_once` answer reaches the agent
  and the turn ends `idle`; stopping instead cancels the request and ends `done` (behaviours 5, 8,
  15).
- **AK-3** Given the stop reason `max_tokens`, a line that is no JSON-RPC, a file system request
  and a crash, then the agent is `waiting_for_input` with the reason, `unknown` (a prompt is
  refused), the request is refused with "method not found", and the crash is `failed` with its exit
  code; a start beyond `max_agents` is refused (behaviours 4, 7, 10, 14).
- **AK-4** Given the configuration, then a file inside the workspace is ignored, unknown keys and
  values out of bounds are problems, credential-like environment names are refused, and the plan
  passes only allowlisted names and the variables of the test process do not reach the child
  (behaviours 11–13).
- **AK-5** Given an MCP client, then `agent_host_list` shows the configuration and the states;
  `agent_host_start` answers a dry run by default and refuses `dry_run: false` without `agents`;
  with `agents` an agent is started, prompted, read (masked, with `next_since`), its permission
  request answered, and stopped; `allow_always` is refused; and every tool has its row in the
  effects table (behaviours 17–23).

- **AK-6** Given a fake agent that writes one file and creates another through a command, then the
  turn's changes list both (created, one binary), the output sums them up, Undo of another turn is
  refused, Undo of the turn deletes both and marks it undone, a second Undo is refused; a file
  changed again after a turn blocks Undo and nothing is changed; an unchanged workspace and
  ignored files give no change (behaviours 26–29).
- **AK-7** Given an edit request with a `diff` content, then the request carries path, old and new
  text; refusing it and the agent asking again for the same file marks the second request with the
  first one's number; the refused turn has no change. Over MCP `agent_host_changes` answers the
  same, the diff, a dry run of Undo, refuses Undo without `write` and undoes with it (behaviours
  24, 25, 30).

## Out of scope
- The window's view: agents with their state in the sidebar, the task board and the status bar,
  and the approval dialog (ADR-0023 rule 9); the notices already reach the notifications.
- Terminal agents with state detection from hooks or screen manifests (ADR-0023 rule 11, M11.3).
- `session/load` and resuming after a restart (ADR-0023 rule 16; the watchdog of SPEC-0341 calls
  it through the host later), `authenticate`, session modes and slash commands.
- Conducting other agents with grants and budgets (M19.6), models and costs per agent (M19.7).

## Open points (integration)
1. **Sandbox (SPEC-0347, M19.9).** The host builds a `SpawnPlan` (program, arguments, working
   folder, whole environment) and starts exactly that. The sandbox wraps the plan before the start
   (for example a Seatbelt or Landlock launcher in front of the program, and its proxy in the
   environment). Until then the agent runs with the user's rights, and every start answer says so.
2. **Shared agent configuration (SPEC-0346, M19.8).** Project rules and skills are not passed to
   hosted agents yet; the agent reads its own. Hooks are never run by the host (behaviour 16).
3. **Credentials (M19.15).** An agent that needs a key gets it from its own program's store. The
   vault will hand a credential to a started process by reference; until then no credential is
   passed (behaviour 13).
4. **Ferris IDEA's tools for hosted agents.** Passing Ferris IDEA's own MCP server in `session/new`
   needs a session with the agent's own grants (M19.1, M19.6); until then `mcpServers` is empty.
5. **Watchdog (SPEC-0341, M19.12).** The state, the progress counter, the process and the stop are
   the observation the watchdog asks for; waking goes through `prompt` with the user's wake text.
6. **Scheduler (SPEC-0342, M19.13).** A scheduled run starts a configured agent and prompts it with
   the task's text; the scheduler's `host` still says none until the two are joined.

## Known deviations from the reference
The reference's AI features are not examined; nothing is copied. Deviations from common practice,
on purpose:
- **D1** No standing approvals: `allow_always` is never chosen, and nothing is approved by default
  or by timeout (behaviour 15).
- **D2** A cleaned environment by default: only an allowlist reaches the agent, and credential-like
  names cannot be added (behaviour 13).
- **D3** `unknown` is a state of its own and never counts as idle, so an unreadable agent is shown
  as needing a look instead of as finished (behaviours 7, 10).
- **D4** The reference lists only the files the agent says it edited; a class file its command
  created was neither listed nor restored, and its first undo found no turn (OBS-0121 R5a–R5e).
  Ferris IDEA compares the workspace itself and undoes every file of the turn, or none, naming
  what blocks it (behaviours 26–28, IMP-0548).
- **D5** The reference counted a denied edit as "edited 1 file" (OBS-0121 R7); here the change
  list comes from the file system (behaviour 27, IMP-0549).
- **D6** After a "No" the reference showed the agent's next, equivalent request like a new one
  (OBS-0121 R5d); here it carries the mark of the refusal (behaviour 25, IMP-0547).
- **D4** The programs come only from the user's settings folder, never from the project or a tool
  call (behaviour 11).
