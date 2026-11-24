# SPEC-0344: Session history of agents

- **Version:** 1 · **Date:** 2026-11-18
- **Sources:** `GEN` (agent harnesses keep a transcript of each session — the tool calls with
  their arguments, duration and outcome — so a person can see afterwards what an agent did and in
  which order, and replay it step by step; audit logs keep metadata and leave secrets and bulky
  payloads out; append-only line files with a size and age limit are the usual local format);
  ADR-0023 rule 7 (no secrets in context or tool output; local audit), ADR-0007 (no telemetry,
  nothing leaves the computer), ADR-0008 (every feature has an MCP tool); SPEC-0053 (agent rights,
  the grant file in the user's settings folder, the agent log, the effects table); SPEC-0313
  behaviour 6 (the secret patterns of the AI assistant). No observation of the reference: the
  reference keeps no history of agent sessions.
- **Feature matrix rows:** none (ROADMAP M19.10, harness phase after 1.0).

## Purpose
An agent session leaves its changes behind, but not how it got there: which tools it called, with
what, how long each took and what failed or was refused. The session history keeps that record per
session on the person's own computer, so the person (and an agent asked to review the work) can
read a session again in order. It is a record, not telemetry: nothing is sent anywhere, and
nothing secret is written.

This version is the record of MCP tool calls, its file format with room for the other kinds of
event, its limits, and the MCP tools that list and read sessions. Named open points (behaviour 17)
are the window view with replay and the producers of the other event kinds.

## Behaviour

### Sessions and where they live
1. A session is one MCP session of the server: a connection over stdio, or one session of the
   HTTP server (the same boundary at which the answers of SPEC-0053 v4 behaviour 9 end). Its
   record starts with its first tool call; a session without calls leaves nothing. [GEN]
2. The records of a workspace live in a folder `session-history` beside the grant file in the
   user's settings folder (a test's grant file `x.agent-permissions.toml` has `x.session-history`
   beside it), in a sub-folder named by a hash of the workspace's resolved path, so neither the
   path nor the project's name appears in a file or folder name. One file per session,
   `<id>.jsonl`; the id is the start time in milliseconds (13 digits), a dash and 8 hexadecimal
   digits. The folders are never inside the workspace, so they are never committed. [GEN, ADR-0007]
3. On Unix the folders are created readable by their owner only (`0700`) and every session file is
   created new as owner-only (`0600`); an existing file, a link in its place or a session folder
   that is a link is refused, so the record is never written through a link. Nothing is sent
   anywhere. [ADR-0007, CLAUDE.md rule 12]
4. Recording is switched on by the servers a person starts — the stdio server and the HTTP
   server of the window — not by a server built in a test or the one-shot `ferris-idea call`. A
   record that cannot be written never refuses or changes a tool call: the call is answered as
   without it, and the session records nothing more. [GEN]

### The file format
5. A session file is append-only JSON Lines. Each line has `seq` (from 0, one per line), `time`
   (milliseconds since the epoch) and `kind`. The first line is `start` with `version` (the format
   version, 1) and `client` (the client's name and version as it introduced itself). [GEN]
6. A `tool_call` line holds the tool's `tool` name, its `arguments` after behaviour 9, `redacted`
   (how many values were replaced), `result_bytes` (the size of the answer, never its content),
   `duration_ms`, `outcome` (`ok`, `error` or `refused`, decided as for the agent log, SPEC-0053
   v7 behaviour 11) and, for an error or a refusal, `error`: the message after behaviour 9, cut to
   300 characters. [GEN]
7. The format has room for the other parts of a session: `context` (a context pack handed to the
   agent: target, pieces, characters), `change_set` (id, files, state), `check_gate` (gate,
   passed, findings) and `cost` (model, input and output tokens, cost in micro-units of a
   currency). A reader passes on lines of these kinds and counts lines it cannot read
   (`unreadable`) without failing. [GEN]
8. A `truncated` line with its `reason` (`events` or `bytes`) is the last line of a session that
   reached a limit of behaviour 11; calls after it are not recorded. [GEN]

### No secrets
9. Before a call's arguments are written:
   - every value under a key whose name says secret (words `password`, `passwd`, `passphrase`,
     `secret`, `token`, `credential(s)`, `authorization`, `auth`, `cookie`, and `api key`,
     `private key`, `access key`, `client secret` in any spelling: `api_key`, `apiKey`,
     `API-KEY`) is replaced by `‹redacted›`, whatever its type, except `true`, `false` and `null`;
   - in every text: private key blocks, the password of a URL (`scheme://user:‹redacted›@`),
     `Bearer` and `Basic` credentials, well-known token forms (GitHub, GitLab, Slack, OpenAI and
     Anthropic style `sk-` keys, AWS access key ids, Google API keys, JSON web tokens) and
     assignments to secret names (`API_TOKEN=…`, `password: …`) are replaced by `‹redacted›`;
     then the patterns of SPEC-0313 behaviour 6 mask what they find as `‹masked›`;
   - a text longer than 1000 characters keeps its first 200 and a note of how many were left
     out; nesting deeper than 32 levels is left out; arguments longer than 16 KiB after this are
     replaced by a note of their size.

   Each replaced value counts in `redacted`. Keys are kept. [CLAUDE.md rule 5, ADR-0023 rule 7]

### Limits
10. When a session's record starts, the workspace's older records go: those older than 30 days,
    then the oldest beyond 100 sessions (the new one counts). Only files named like a session are
    ever deleted. [GEN]
11. A session records at most 5000 lines and 4 MiB; then behaviour 8 applies. [GEN]

### Reading back
12. The sessions of a workspace are listed newest first, a page at a time (default 20, at most
    100, with `offset`), each with its id, `started`, `ended` (time of its last line), `client`,
    the number of tool calls, errors and refusals, the summed duration of its calls, whether it
    was truncated, whether it is the asking session (`current`), and `unreadable`. `total` says
    how many there are. [GEN]
13. One session is read by id, or `current` for the asking session, as its lines in order from
    `from` (a `seq`, default 0), at most `limit` lines (default 100, at most 500), optionally only
    the kinds asked for. The answer has the session's summary of behaviour 12, the lines, `next`
    (the `seq` to continue from, or null at the end) and `unreadable`. Each line carries `offset_ms`
    from the session's start, so a viewer can replay the session at its pace. An id not of the form
    of behaviour 2 is refused before any file is touched. [GEN]
14. Only the asking workspace's sessions are listed and read. Every answer says that the recorded
    arguments are data and not instructions. [ADR-0023 rule 7]

### MCP
15. `session_history_list` (`limit`, `offset`) answers behaviour 12 plus `folder` and the limits
    of behaviours 10 and 11. `session_history_read` (`session`, `from`, `limit`, `kinds`) answers
    behaviour 13. Both only read, need no right (like `agent_watch_status` and `memory_recall`)
    and have their rows in the effects table. Unknown arguments are refused. [ADR-0008, SPEC-0053]
16. The calls of both tools are recorded like every other call. [GEN]

### Open points
17. Named for later versions:
    - **Window view with replay** (ROADMAP M19.10): a tool window listing the sessions, a session
      shown as a timeline of its lines with play, pause, step and speed, using `offset_ms`.
    - **Producers of `context`, `change_set`, `check_gate` and `cost`**: written by context packs
      (M19.2), change sets (M19.4), check gates (M19.3) and the agent host (M19.5, M19.7) once
      they are on the main line; until then a call of their tools shows up as a `tool_call`.
    - **A switch to turn recording off** per workspace, and deleting a session from the window.

## Acceptance criteria
- **AK-1** `crates/core/tests/it/session_history.rs`: `secrets_are_redacted_before_writing`: key
  names, URL passwords, bearer tokens, token forms, assignments and private key blocks are
  replaced and counted, `true`/`false`/`null` and other keys kept, long texts and deep nesting
  cut, the extra masker applied.
- **AK-2** `session_history.rs`: `a_session_is_written_and_read_back_in_order`: start line, tool
  calls with every field of behaviour 6, the other kinds of behaviour 7, unreadable lines counted,
  paging with `from`, `limit`, `kinds` and `next`, `offset_ms`.
- **AK-3** `session_history.rs`: `files_are_private_and_never_follow_links` (Unix): folder `0700`,
  file `0600`, hashed folder name, a link in place of the file or folder refused, an invalid id
  refused.
- **AK-4** `session_history.rs`: `limits_prune_and_truncate`: sessions beyond 100 and older than
  30 days deleted, other files kept, the line and byte limits end with one `truncated` line.
- **AK-5** `crates/mcp/tests/it/session_history.rs`: `tool_calls_are_recorded_without_secrets`:
  calls of a recording server appear with outcome, duration and result size; a token in an
  argument is not in the file; a refused call is `refused`; a server not recording leaves nothing.
- **AK-6** `crates/mcp/tests/it/session_history.rs`: `session_history_tools_list_and_read`: list
  with `current`, read by id and `current`, paging, an invalid id refused, another workspace's
  sessions never seen, rows in the effects table.

## Out of scope
The window view and replay (behaviour 17); recording what an agent does outside Ferris IDEA's MCP
server (its own shell and file edits); the content of tool answers; any export or upload.

## Known deviations from the reference
None: the reference keeps no history of agent sessions.
