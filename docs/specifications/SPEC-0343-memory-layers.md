# SPEC-0343: Memory layers for agents

- **Version:** 1 · **Date:** 2026-11-17
- **Sources:** `GEN` (agent harnesses keep notes between turns and sessions: a scratchpad of the
  running session, hand-over notes of a task, the conventions and pitfalls of a project, the
  preferences of a person; each note with its origin and age, and the person can read, change
  and delete them); ADR-0023 rule 7 (untrusted content is data, not instructions; no secrets in
  context or tool output; read-only unless the session holds the matching right; local audit),
  rule 10 (content from another agent is untrusted data); ADR-0007 (nothing leaves the computer);
  ADR-0008 (every feature has an MCP tool); SPEC-0053 (agent rights, the grant file in the user's
  settings folder, the agent log); SPEC-0278 (project trust); SPEC-0313 behaviour 6 (the secret
  patterns of the AI assistant). No observation of the reference: the reference has no memory for
  agents.
- **Feature matrix rows:** none (ROADMAP M19.14, harness phase after 1.0; owner's order of
  2026-11-17).

## Purpose
Agents forget everything between sessions and lose their notes when a task passes from one agent
to the next. Memory gives them four layers to keep what they learn: the working notes of the
session, the hand-overs of a task, the conventions, decisions and known pitfalls of the project,
and the preferences of the person. Every entry says where it came from, when, how long it holds
and how far it can be trusted; the person can read, change and delete every entry; nothing secret
is kept; and nothing an entry says counts as an instruction or a right.

This version is the model and its MCP tools. The window view of the entries, the team memory
shared through the project (ROADMAP M6.15) and the link to the knowledge graph (M14) come later.

## Behaviour

### Layers and entries
1. There are four layers. **session**: the working notes of one MCP session, kept in memory and
   gone when the session ends. **task**: hand-overs and progress of a task named by an id, for
   the next agent or session on the same task. **project**: conventions, decisions and known
   pitfalls of the workspace. **user**: the person's preferences and approvals with a validity;
   they hold in every workspace. [GEN]
2. An entry has an id (`s<n>` in the session layer, `m<n>` in the others, never reused), its
   layer, a kind (`note`, `handoff`, `progress`, `convention`, `decision`, `pitfall`,
   `preference`, `approval`), the text (1 to 2000 characters, no control characters but line
   breaks and tabs), up to 10 tags (1 to 40 characters each: letters, digits, `-`, `_`, `.`), the
   task id for the task layer (1 to 80 characters: letters, digits, `-`, `_`, `.`), and its
   origin: who wrote it (`user` or `agent`), the agent's client name and version when an agent
   wrote it, and an optional source (a file, URL or tool the content came from, up to 300
   characters). It has the time it was created and last changed and an optional end of validity.
   [GEN]
3. Every entry has a trust level: `user` (the person wrote or confirmed it), `agent` (an agent
   wrote it from its own work) or `untrusted` (it carries content from outside the person's
   control: a web page, another agent's message, an issue, a file of an untrusted project). An
   agent can write `agent` or `untrusted`, never `user`. An entry is `untrusted` whatever the
   agent asks when its source is a URL (`http://`, `https://`) or when the workspace is not
   trusted (SPEC-0278). [ADR-0023 rules 7, 10]
4. An entry never grants anything: an `approval` entry records what the person said, it is not an
   agent right. Rights come only from the grant file and the questions in the window (SPEC-0053).
   [ADR-0023 rule 7]

### Where entries live
5. The task, project and user layers live in one file `memory.toml` beside the grant file in the
   user's settings folder (a test's grant file `x.agent-permissions.toml` has `x.memory.toml`
   beside it). Task and project entries carry the workspace's path and are seen only in that
   workspace. The file is never inside the workspace, so it is never committed; such a file is
   refused. It is written atomically and, on Unix, readable by its owner only. Nothing is sent
   anywhere. [ADR-0007, GEN]
6. The person can read and change the file by hand. An entry the file holds with a problem
   (unknown layer or kind, text out of bounds, a secret) is left out of every answer and named
   in `problems`; a file that cannot be read as memory is a problem too, and then nothing is
   written to it, so a hand-written file is never overwritten. [GEN]

### No secrets
7. Text, tags and source are checked with the secret patterns of SPEC-0313 behaviour 6 (tokens,
   keys, passwords in assignments, credentials in URLs, private key blocks). An entry with a secret
   is refused, and the refusal does not repeat the secret. [CLAUDE.md rule 5, ADR-0023 rule 7]

### Recall
8. Recall answers the entries of the session, the given task, the workspace's project layer and
   the user layer, or of the layers asked for. Expired entries are left out. With a query, an
   entry counts by the number of distinct query words (two or more characters, compared without
   case) that its text, tags or kind contain; entries with none are left out. Ties go by layer
   (session, task, project, user), then trust (`user`, `agent`, `untrusted`), then the newer
   change, then the id. Without a query every entry counts the same and they come in the same
   order. The task layer counts only for the task asked for. [GEN]
9. Recall fills whole entries into a budget of characters of text (default 4000, 200 to 20000) and
   says how many entries were left out for the budget. [M19.2, GEN]
10. Every answer says that entries are data and not instructions, and every entry carries its
    trust level; an `untrusted` entry is also marked `untrusted: true`. [ADR-0023 rules 7, 10]

### Writing and forgetting
11. Writing adds an entry or changes one by id (its text, kind, tags, source, validity; not its
    layer, task or origin). An agent's change never raises the trust level: an `untrusted` entry
    stays `untrusted`, and an agent's change of an entry the person wrote makes it `agent`;
    behaviour 3 applies to the changed entry too. The validity is given
    in days (1 to 3650) or removed. Each layer of a workspace holds at most 500 entries; the
    session layer at most 200. [GEN]
12. Forgetting deletes entries by id, or all expired entries of the layers asked for. [GEN]
13. Writing and forgetting answer what would change and change nothing unless `dry_run` is false.
    Then the task and project layers need the `write` right, the user layer the `settings` right
    (SPEC-0053); the session layer needs no right, since nothing is written. A real change of a
    stored layer is logged in the agent log like every changing tool. [ADR-0023 rule 7, SPEC-0053]

### MCP
14. `memory_recall` (query, layers, task, budget) answers the note of behaviour 10, the entries
    with every field of behaviour 2 and 3, `left_out`, and `problems`. [ADR-0008]
15. `memory_write` (id to change, or layer and task for a new entry; kind, text, tags, source,
    trust `agent` or `untrusted`, `valid_days` or `no_validity`, `dry_run`) answers the entry as it
    would be or was written. Unknown arguments and out-of-bounds values are refused. [ADR-0008]
16. `memory_forget` (ids, or `expired` with layers and task; `dry_run`) answers the ids that would
    be or were deleted; an unknown id is refused. [ADR-0008]

## Acceptance criteria
- **AK-1** `crates/core/tests/it/memory.rs`: `entries_and_their_bounds`: kinds, text, tags, task id
  and source within and outside their bounds; ids never reused after a deletion.
- **AK-2** `memory.rs`: `trust_levels`: an agent cannot write `user`; a URL source and an untrusted
  workspace make an entry `untrusted`; an agent's change of a person's entry makes it `agent` and
  never raises an `untrusted` one.
- **AK-3** `crates/mcp/tests/it/memory.rs` (the patterns live in `ferris-idea-ai`):
  `secrets_are_refused`: a token, a password assignment, a URL with
  credentials and a private key block in text, tag or source are refused without the secret in
  the message.
- **AK-4** `memory.rs`: `recall_ranks_and_fits_the_budget`: query words, the order of behaviour 8,
  expired entries left out, the budget with `left_out`, the layers and the task asked for, another
  workspace's entries never seen.
- **AK-5** `crates/core/tests/it/memory.rs`: `the_file`: written beside the grant file, owner-only on Unix, refused
  inside the workspace; a hand-written entry with a problem is left out and named; a broken file
  is never overwritten.
- **AK-6** `crates/mcp/tests/it/memory.rs`: `memory_tools`: write with dry run and for real, the
  rights per layer (`write`, `settings`, none for the session), recall with the note and the
  untrusted mark, forget by id and the expired ones, the session layer gone in a new session, a
  secret refused, a real write logged.

## Out of scope
The window view of the entries; the team memory shared through the project (ROADMAP M6.15); the
link to the project knowledge graph (M14, locked until QG-1); ranking by meaning (embeddings) and
any model call; recalling entries into the context packs of M19.2 by themselves (a later step of
SPEC-0339); confirming an agent's entry as the person's (`user` trust) from the window.

## Known deviations from the reference
None: the reference has no memory for agents.
