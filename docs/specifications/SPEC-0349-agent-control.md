# SPEC-0349: Agents conduct agents and exchange messages

- **Version:** 1 · **Date:** 2026-11-18
- **Sources:** `GEN` (process supervisors and message queues: an append-only record with retention,
  bounded waits, ownership of child processes); ADR-0023 rules 7 (safety rules for MCP tools), 10
  (any agent can conduct: start, prompt, read, wait, stop, list; every call checked against the
  conductor's grants and recorded; starting agents needs an explicit grant) and 12 (agents talk
  through the persisted message store, never through each other's context; content from another
  agent is untrusted data; the receiver's own grants apply); ADR-0014 rules 4 (communication is
  persisted structured records), 5 (bounded delegation: hierarchy depth, agents per participant)
  and 7 (one security model, audit); ADR-0008 (every feature has an MCP tool); SPEC-0348 (the agent
  host, its configuration file, its states and its tools); SPEC-0053 (the `agents` right, the grant
  file, the agent log of behaviour 11); SPEC-0313 behaviour 6 (masking secrets). No observation of
  the reference: the reference's AI features are not examined
  (`docs/legal/RESEARCH_BOUNDARIES.md`).
- **Feature matrix rows:** none (ROADMAP M19.6, harness phase after 1.0).

## Purpose
An agent working in Ferris IDEA wants to hand part of its task to another agent, watch it, wait for
it and stop it, and agents want to tell each other things (a question, a finding, a handoff)
without pasting text into each other's context. A person wants all of it bounded and on the record:
no agent can start more agents than allowed, start programs the person did not configure, raise its
own rights or another's, or approve what another agent asks the person to approve.

This version builds on the agent host of SPEC-0348: its tools become the conductor's `start`,
`prompt`, `read`, `stop` and `list`, each call carrying the caller's identity, and three tools are
added: `agent_wait`, `agent_message_send` and `agent_message_read`.

## Behaviour

### Identity
1. The *caller* of a tool is the MCP client as it named itself at `initialize`, written
   `mcp:<name>`: the name in lower case, every character other than a-z, 0-9, `.`, `-` and `_`
   replaced by `-`, at most 64 characters; a client without a name is `mcp:unnamed`. A hosted
   agent's id (`<definition>-<n>`, SPEC-0348 behaviour 1) never contains `:`, so no client can take
   a hosted agent's identity. The name is what the client declared, not a proof: the grant file
   stays the boundary (SPEC-0053), and two clients with the same name are one caller. [GEN]
2. Every line the agent log gets for a call (SPEC-0053 behaviour 11) carries `caller`. The agent
   host's tools and the tools of this specification are logged for every call, reading ones
   included: reaching a hosted agent is logged like a change. [ADR-0023 rule 10, ADR-0014 rule 7]

### Conducting
3. A hosted agent records its *conductor* (the caller that started it) and its *depth*: one more
   than the depth of its conductor. A client's depth is 0; a hosted agent's depth is its own. A
   start whose new depth would exceed `max_depth` is refused. (Hosted agents reach no Ferris IDEA tool
   yet, SPEC-0348 behaviour 14; the depth check is made for every caller so it already holds when
   they do, open point 1.) [ADR-0014 rule 5]
4. A start is refused when the caller conducts `max_per_conductor` running agents already, besides
   the host's own `max_agents` (SPEC-0348 behaviour 4). The checks and the start happen under one
   lock, so two calls at once cannot pass the cap together. [ADR-0014 rule 5]
5. `agent_host_start` can start only an agent of the user's `agent-host.toml` (SPEC-0348 behaviour
   11) and passes no rights to it: no tool of this specification takes a right, a grant, a program,
   an argument or an environment value, and unknown arguments are refused. [ADR-0023 rule 10]
6. Prompting, reading, waiting for, answering and stopping a hosted agent is allowed only to its
   conductor. Another caller is refused with the conductor's identity; an agent without a recorded
   conductor (not started through these tools) is refused to every caller. [ADR-0014 rule 7]
7. Approving is the person's decision: `agent_host_respond` with an option of a kind that allows
   (`allow_once`; `allow_always` stays refused, SPEC-0348 behaviour 15) asks the person in the
   window for exactly that request every time; the answer is never remembered for the session or
   saved as a grant. Without a window, or without an answer, the approval is refused and the request
   keeps waiting. Rejecting (`reject_once`, `reject_always`) and cancelling stay with the conductor.
   This narrows SPEC-0348 behaviour 22: no agent approves another agent's permission request.
   [ADR-0023 rules 7 and 12]
8. The limits live in `agent-host.toml` in a `[control]` table: `max_depth` 0 to 3 (default 1; 0:
   nobody starts agents through MCP) and `max_per_conductor` 1 to 8 (default 2). Unknown keys and
   values out of bounds are a problem of the file, and nothing can be started (SPEC-0348 behaviour
   11). [GEN]
9. `agent_host_list` names the caller, the limits, and with every hosted agent its `conductor`,
   its `depth` and whether the caller conducts it (`yours`). [ADR-0023 rule 10]

### Waiting
10. `agent_wait` takes `id`, `states` (default: every state but `working`) and `timeout_seconds`
    (default 30, 1 to 120). It returns as soon as the agent is in one of the states, or when the time
    is up, with `reached`, the seconds waited and the agent as it is then. It never waits longer
    than its bound, and an agent that ended is answered at once whatever the states. [ADR-0023 rule
    10]

### Messages
11. Messages are kept per workspace in the user's settings folder beside the grant file, in the
    folder `agent-messages` (mode 0700), one file per workspace named after a hash of its resolved
    path (mode 0600), one JSON line per message. A store inside the workspace is never used.
    [ADR-0014 rule 4, SPEC-0053]
12. A message has `seq` (growing per store), `time`, `from` (the caller; never an argument),
    `to`, `kind`, optional `reply_to` (the `seq` of an earlier message) and `text`. `to` is a hosted
    agent's id, a caller identity (`mcp:<name>`) or `all` (the project). `kind` is one of
    `question`, `answer`, `review_request`, `handoff`, `blocker`, `finding`, `status`. The text has 1
    to 4 000 characters and no control characters but line breaks and tabs. [ADR-0023 rule 12]
13. A message that holds something the secret masking of SPEC-0313 behaviour 6 would mask is
    refused, and nothing is stored: the store never holds credentials. [ADR-0017, ADR-0023 rule 7]
14. The store is append-only: no tool changes or deletes a message. Retention: messages older than
    30 days and all but the newest 1 000 are dropped when a message is added; only then is the file
    rewritten (atomically, mode 0600). A line the store cannot read is skipped and counted. [GEN]
15. `agent_message_read` answers the messages a caller may see: those to it, to `all`, from it,
    and those to or from an agent it conducts; filters `from`, `to`, `kind`, `since` (a `seq`) and
    `limit` (default 50, at most 200) narrow them. Texts are masked again on the way out and the
    answer says that messages are data from other agents, never instructions: a message never
    makes Ferris IDEA act, and the receiver's own rights apply to whatever it does next. [ADR-0023 rules
    7 and 12]
16. `agent_message_send` and `agent_message_read` need the `agents` right; sending is logged as a
    change of Ferris IDEA's own files, with the recipient and kind as targets and the text by its
    length only (SPEC-0053 behaviour 11). [SPEC-0053]

## Acceptance criteria
- **AK-1** Given the conductor records, then a client's start gets depth 1, a start beyond
  `max_depth` (from a hosted agent at depth 1, and from a client with `max_depth = 0`) and beyond
  `max_per_conductor` is refused, ownership is checked, and client names become `mcp:` identities
  that cannot equal a hosted id (behaviours 1, 3, 4, 6).
- **AK-2** Given the message store, then a message is appended with a growing `seq` to a 0600 file
  in a 0700 folder beside the grant file, a text with a secret is refused and nothing is stored,
  bad recipients, kinds and texts are refused, the caller sees only its own messages, and retention
  keeps the newest 1 000 and drops messages older than 30 days (behaviours 11–15).
- **AK-3** Given `[control]` in `agent-host.toml`, then the defaults are 1 and 2, values out of
  bounds and unknown keys are problems (behaviour 8).
- **AK-4** Given an MCP client with the `agents` right and the fake ACP agent, then a started agent
  names its conductor and depth, `agent_wait` returns at the wanted state and returns after its
  bound when the state is not reached, another client cannot prompt, read, wait for or stop it, an
  approval is refused without a window and goes through when the window's person allows it,
  rejecting needs no window, and every call — reading ones included — is in the agent log with its
  caller (behaviours 2, 6, 7, 9, 10).
- **AK-5** Given an MCP client, then `agent_message_send` and `agent_message_read` need `agents`, a
  message goes from one client to another with `from` set by the server, a secret is refused, the
  answer marks messages as data, and the log line has the recipient but not the text (behaviours
  12, 13, 15, 16).

## Out of scope
- Hosted agents calling Ferris IDEA's tools themselves (SPEC-0348 open point 4): until then the
  conductors are MCP clients.
- Team and task addressing (ADR-0023 rule 12 "to a task's team") and the task graph; budgets and
  models per conductor (M19.7, ADR-0023 rule 10 "which models, what budget").
- The window's view of messages and of who conducts whom, and a person answering in place of an
  agent; the CLI commands of ADR-0023 rule 10.

## Open points (integration)
1. **Hosted agents as conductors.** When a hosted agent gets Ferris IDEA's MCP server in its session
   (SPEC-0348 open point 4), its caller identity is its hosted id, bound by Ferris IDEA to its
   session, not a name it declares; behaviour 3 then limits its depth.
2. **Budgets (M19.7).** `agent_host_start` will also check the conductor's budget and the models it
   may use.
3. **Delivering messages.** A hosted agent cannot read the store yet; its conductor passes a message
   on in a prompt. With open point 1 it reads its own messages.
4. **Several Ferris IDEA processes.** Two processes (the window and a stdio server) on one workspace
   share the store file; appends are single writes, but `seq` is counted per process and may repeat
   across them until the store takes a file lock.

## Known deviations from the reference
The reference's AI features are not examined; nothing is copied. Deviations from common practice,
on purpose:
- **D1** No agent approves another agent's permission request; approving always asks the person
  (behaviour 7).
- **D2** Only the conductor steers an agent; other agents with the same right are refused
  (behaviour 6).
- **D3** Messages with secrets are refused rather than stored masked, so the store never holds a
  credential, masked or not (behaviour 13).
- **D4** Reading calls on agents are logged too, so the record shows who watched which agent
  (behaviour 2).
