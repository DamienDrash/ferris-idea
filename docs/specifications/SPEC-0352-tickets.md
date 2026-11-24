# SPEC-0352: Tickets for people and agents — data model, storage, agent rights, search

- **Version:** 1 · **Date:** 2026-11-18
- **Sources:** `GEN` (issue trackers and task graphs: kinds, states with a transition table,
  priority, assignee, labels, comments, dependencies, links to version control; time-ordered
  identifiers; line-oriented text formats that merge well); `STD` (UUID version 7 of RFC 9562,
  Crockford Base32, JSON Lines, TOML 1.0, Git's built-in `union` merge driver); ADR-0034 (tickets,
  boards and the project cockpit, accepted with this spec), ADR-0014 rule 2 (task graph),
  ADR-0021 (envelope; amended by ADR-0034 decision 3), ADR-0023 rules 7 and 12 (untrusted content),
  ADR-0031 (project files never grant agents), ADR-0008 (an MCP tool per feature), SPEC-0053
  (agent rights, grant file, agent log, the question in the window). No observation of the
  reference: the reference's own task feature connects to external trackers and is not copied.
- **Feature matrix rows:** none (ROADMAP M20.1 and the core of M20.2, after 1.0; owner's order of
  2026-11-17).

## Purpose
People and agents need one place for the work of a project: a person writes a bug, an agent files
what it found, both comment and move the item along, and the task graph of M12.2 later works from
the same items. This spec fixes the work item (ADR-0034 decision 1) as format 1: its fields, its
states and their transition table, its files on disk, who may change what, and how items are
found. It covers the ticket model, its store, full-text search and the MCP tools; boards, the
cockpit and the window's ticket views follow in M20.3 and M20.4.

## Behaviour

### The work item
1. A work item has an `id`, a `kind`, a `title`, a `description`, a `state`, a `priority`, an
   optional `assignee`, `labels`, an optional `parent`, `depends_on`, `links`, acceptance
   `criteria`, `created`, `created_by` and its comments. [GEN, ADR-0034 decision 1]
2. **Kinds** (closed list in format 1): `task`, `bug`, `finding` (what an agent or an inspection
   found), `question`, `epic`, `milestone`, `incident`, `improvement`. A project adds labels, not
   kinds. [ADR-0034 decision 1]
3. **States** and their transition table, the same for every view and every caller:

   | From | To |
   |---|---|
   | `proposed` | `ready`, `blocked`, `cancelled` |
   | `ready` | `proposed`, `assigned`, `in_progress`, `blocked`, `cancelled` |
   | `assigned` | `ready`, `in_progress`, `blocked`, `cancelled` |
   | `in_progress` | `ready`, `assigned`, `in_review`, `blocked`, `failed`, `cancelled` |
   | `in_review` | `in_progress`, `done`, `failed`, `cancelled` |
   | `blocked` | `proposed`, `ready`, `assigned`, `in_progress`, `cancelled` |
   | `failed` | `ready`, `in_progress`, `cancelled` |
   | `done` | `ready` (reopen) |
   | `cancelled` | `proposed` (reopen) |

   A new item starts in `proposed`. Every other change of state is refused with the states it
   may go to. Guards: `assigned` needs an assignee; `done` needs every acceptance criterion met.
   [GEN, vision §3.2, ADR-0034 decision 1]
4. **Priority:** `low`, `normal` (default), `high`, `critical`. [GEN]
5. **Assignee and actors** are written `person:<name>` or `agent:<name>`; the name has 1 to 80
   characters without control characters. Every change records its actor. For a call over MCP the
   actor is `agent:<client name>` as the client named itself. [GEN]
6. **Links** have a kind and a target: `branch`, `commit` (7 to 64 hexadecimal digits),
   `pull-request`, `change-set`, `test-run`, `agent-session`, `ticket` (another item of the
   store, by id) and `attachment` (a path relative to the workspace without `..`, or an `https://`
   address). Attachments are references only: Ferris IDEA never fetches, opens or runs them for an
   agent. A target has 1 to 500 characters without control characters; at most 100 links.
   [ADR-0034 decisions 2 and 6, ROADMAP M20.2]
7. **Dependencies and parent:** `depends_on` and `parent` name items of the same store by id;
   an unknown item, the item itself and a cycle (through `depends_on`, or through `parent`) are
   refused. [GEN, ADR-0034 decision 7]
8. **Bounds:** title 1 to 200 characters on one line; description up to 64 KiB; comment 1 byte to
   64 KiB; at most 20 labels of 1 to 40 characters without white space or control characters; at
   most 50 criteria of 1 to 500 characters. Values out of bounds are refused with the field's
   name. [GEN]

### Identifiers
9. The `id` is a UUID version 7 (48 bits of milliseconds, 74 random bits) written in lowercase
   with hyphens; it names the item's folder, so two machines or two branches never create the same
   folder. A folder whose name is not such an id is reported as a problem and skipped. [STD,
   ADR-0034 decision 4]
10. The **key** people type is the store's prefix, a hyphen and the first characters of the id's
    random bits in Crockford Base32: `IC-7K3QDX`. Six characters by default; a key is lengthened
    while two items of the store share it. A tool accepts the id, the key with or without prefix
    in any case, or any longer unique start of the Base32 part; an ambiguous start is refused with
    the candidates. The prefix is 1 to 8 letters or digits, kept in the store's `store.toml`
    (default `T`). Sequential numbers do not exist in format 1. [ADR-0034 decision 4]

### Files on disk
11. A store is a folder:

    ```
    <store>/store.toml                         envelope kind "work-store": prefix
    <store>/items/<id>/item.toml                envelope kind "work-item": planning state
    <store>/items/<id>/description.md           the description
    <store>/items/<id>/comments/<comment-id>.md one file per comment: TOML front matter between
                                                `+++` lines (author, created), then the text
    <store>/items/<id>/events.jsonl             the history, one JSON object per line
    ```

    `item.toml` starts with the ADR-0021 envelope (`[ferris-idea]`, `kind = "work-item"`,
    `format = 1`) and is written deterministically: fixed key order, one field per line, lists one
    entry per line and sorted (labels, dependencies, links), absent optional fields left out, and
    no field that changes on every write. Two changes of different fields of one item on two
    branches therefore merge without a conflict, and a new comment is a new file. Comment ids are
    UUIDs version 7, so comment files sort by time. Times are UTC in RFC 3339 (`2026-11-18T09:30:00Z`).
    A reader refuses a wrong `kind` and a newer `format` with the ADR-0021 message. [ADR-0034
    decision 2, ADR-0021]
12. `events.jsonl` gets one line per change: `id`, `time`, `actor`, `field`, `old`, `new`, and
    `approved_by` when a person confirmed it (behaviour 20). Creating an item writes a `created`
    line with its state. The repository store's `.gitattributes` entry
    `events.jsonl merge=union` lets Git keep the lines of both sides; Ferris IDEA writes it when it
    creates the repository store and never registers a merge driver of its own. Lines are sorted
    by time on reading; a line that is not valid JSON is reported and skipped. [ADR-0034 decision 3]
13. **Verified state:** on reading, the state changes of the event log are replayed from the
    `created` line against the table of behaviour 3. When a step is not in the table, a step's
    `old` is not the state reached so far, or the end is not the state in `item.toml`, the item
    is shown as `state_verified: false` (for example after an edit by hand or a merge). People may
    edit the files; such edits stay visible as unverified. [ADR-0034 decision 3]
14. Files are written through a temporary file and a rename, so a reader never sees half a file.
    [GEN]

### Where the store lives
15. Two stores in format 1, chosen per project in the user's settings folder, never by a project
    file (ADR-0031):
    - **private** (default): `tickets/<16 hex digits of the SHA-256 of the workspace path>/`
      beside the user's grant file; only this user and their agents on this machine see it;
    - **repository**: `.ferris-idea/work/` in the project, under the project's version control;
      everyone who can read the repository reads every item, also in its history.

    The choice is kept in `tickets.toml` beside the grant file (`[[workspace]]` with `path` and
    `store`). A choice file inside the workspace is ignored. Choosing the repository store is a
    person's act: over MCP it needs the `settings` right and the person's confirmation in the
    window for that call, and the question says that every item becomes readable for everyone who
    can read the repository and stays in its history. Switching moves no items. [ADR-0034
    decision 2, rule 12]
16. A repository store found in a project whose user has not chosen it is not used; the tools say
    that it exists. [ADR-0031]
17. Before an item, a comment or a change is written, its text (title, description, comment,
    labels, criteria, link targets) is scanned for secrets with the masking rules of the AI
    context (`ferris_idea_ai::context::mask_secrets`); a match is refused, in every store, naming the
    field but not the value. [ADR-0034 decisions 2 and 6]

### Rights of agents
18. A new agent right `ticket-write` (granted per workspace in the user's grant file, SPEC-0053;
    never by the project) lets an agent create items, comment, edit, link, assign and move items
    along the transition table. Reading — listing, showing, searching — needs no right. [ADR-0034
    decision 5]
19. Some changes by an agent need, on top of `ticket-write`, a person's confirmation in the
    Ferris IDEA window for that one call: moving an item to `done` or `cancelled`, setting or changing
    the priority (other than `normal` on a new item), creating an item of a kind other than
    `task`, `bug`, `finding` and `question`, assigning anyone but the agent itself, and changing
    the title or description of an item someone else created. The question names the item's key,
    what would change and the client; it offers only Deny and Allow Once, and nothing is
    remembered. Without a window, without an answer within two minutes or on Deny the call is
    refused and nothing is written. [ADR-0034 decision 5 and open question 3, rule 12]
20. The confirmed change is recorded with `approved_by` = `person:window` in the event log. The
    model refuses such a change without that confirmation, whoever calls it, so the rule does not
    depend on the MCP layer. [vision §3.2: the engine validates]
21. At most 50 items are created per agent and hour (counted from `created_by` and `created` in
    the store); the 51st is refused with the time the limit frees up. [ADR-0034 decision 5]
22. Agents never delete items; there is no delete tool. Every changing call is a dry run unless
    `dry_run: false`, has a row in the effect table and writes the line of the agent log
    (SPEC-0053 behaviour 11). [ADR-0034 decisions 5 and 9, ADR-0031 decision 6]

### Ticket text is data
23. Every text field of an item — title, description, comments, labels, criteria, link targets —
    is untrusted data, whoever wrote it. Every tool result carries `untrusted_data` with that
    sentence, and each item and comment says who wrote it (`created_by`, `author`) and in which
    store it lives. The text never widens rights, starts agents or picks a model; format 1 has no
    field that names a command, a script, a model or a grant. [ADR-0023 rule 7, ADR-0034 decision 6]

### Search
24. Full-text search splits the query at white space into terms, compared ignoring case; an item
    matches when every term occurs in its key, kind, title, description, labels or comments. Hits
    are ranked by occurrences (title three times), then newest first, and carry a snippet of up to
    160 characters around the first hit. The same filters as listing apply (behaviour 25). The
    search reads the files; an index is a later cache (ADR-0034 decision 2). [GEN]
25. Listing filters by `kind`, `state` (several), `open` (not `done`, `cancelled`, `failed`),
    `assignee`, `label` and `priority`, sorted newest first, paged by `limit` (default 50, at most
    200) and `offset`, with the total. [GEN]

### MCP tools
26. `ticket_list` and `ticket_search` (behaviours 24, 25) answer per item the key, id, kind,
    title, state, `state_verified`, priority, assignee, labels, created, created_by and the number
    of comments, plus the store (kind and folder), problems found while reading and
    `untrusted_data`. Without a store folder the answer is empty, not an error. [ADR-0008]
27. `ticket_show` answers one item with its description, criteria, links, the key, title and
    state of each dependency, its children (items whose parent it is), its comments in time order
    and its last 100 events. [ADR-0008]
28. `ticket_create` (kind, title, description, priority, labels, assignee, parent, depends_on,
    links, criteria), `ticket_comment` (key, text) and `ticket_update` (key; title, description,
    state, priority, assignee or `""` to clear it, add/remove labels, parent or `""`, add/remove
    dependencies, add/remove links, criteria) answer what they would write with `dry_run`
    defaulting to true, including `approval_needed` (the reasons of behaviour 19); `dry_run:
    false` needs `ticket-write` and, when `approval_needed` is not empty, the person's
    confirmation. [ADR-0008, ADR-0034 decision 9]
29. `ticket_store` answers the store in use, both stores' folders, whether a repository store
    exists in the project, and with `store` changes the choice (behaviour 15); `dry_run` defaults
    to true. [ADR-0034 decision 2]

## Acceptance criteria
- **AK-1** Given a new store, when an item is created, updated and commented, then `item.toml`
  has the envelope, fixed key order, sorted one-per-line lists and no changing timestamp field,
  the comment is its own file, every change is one event line, and reading gives back the same
  item (behaviours 1, 11, 12, 14).
- **AK-2** Given an item, when its state is changed along and against the table, then legal steps
  are kept, illegal ones are refused naming the allowed states, `assigned` without assignee and
  `done` with an unmet criterion are refused; an `item.toml` edited by hand to a state the events
  do not reach reads as unverified (behaviours 3, 13).
- **AK-3** Given values out of bounds, unknown or cyclic dependencies and parents, links of a
  wrong form, then each is refused naming the field (behaviours 6, 7, 8).
- **AK-4** Given several items, when keys are computed and resolved, then ids are UUID version 7,
  keys have the prefix and six Base32 characters, lengthen on a shared start, and resolve from id,
  key, lower case and a longer start, while an ambiguous start is refused (behaviours 9, 10).
- **AK-5** Given two branches that each change another field of one item and add a comment, when
  their `item.toml` texts are merged line by line and their event logs by union, then the result
  has both changes without conflict and reads as verified (behaviours 11, 12, 13).
- **AK-6** Given an agent actor, when it closes an item, cancels one, changes a priority, creates an
  epic, assigns someone else or edits another's title without confirmation, then the model
  refuses; with a person's confirmation it writes and records `approved_by`; the 51st item within
  an hour is refused (behaviours 19, 20, 21).
- **AK-7** Given items with text, when searching with one or several terms in any case, then only
  items that contain every term are found, the title ranks higher, filters and paging hold
  (behaviours 24, 25).
- **AK-8** Given the choice file, when no choice is made, the private store beside the grant file
  is used; a repository choice uses `.ferris-idea/work/` and writes its `.gitattributes`; a choice
  file inside the workspace is ignored (behaviours 12, 15, 16).
- **AK-9** Given an MCP client, when it calls the ticket tools, then reading needs no right and
  marks text as untrusted data; changing tools answer dry runs, refuse `dry_run: false` without
  `ticket-write`, refuse a secret in the text, refuse a change that needs confirmation without a
  window or on Deny, write it on Allow Once, and `ticket_store` needs `settings` and the
  confirmation to choose the repository store (behaviours 15, 17, 18, 19, 22, 23, 26–29).

## Out of scope
- Boards (Kanban, Scrum, Scrumban, Gantt, Lean, to-do list with Gantt, Eisenhower, roadmap),
  stages, ranks, iterations, estimates and dates: ROADMAP M20.3. Format 1 is extended by them
  without changing the fields above.
- The cockpit and its KPIs: ROADMAP M20.4. The event log is kept for them.
- The ticket views of the window (list, detail, editing, confirming a field-wise merge): with
  M20.3. Until then a person edits the files or confirms agents' changes in the question.
- The project room store (ADR-0034 decision 2, M6.15), the private store as its own Git
  repository, moving items between stores, items with `visibility = "private"`, and marking a
  false secret match: next versions.
- Standing rights `ticket-triage` and `ticket-manage` that skip the question of behaviour 19
  (ADR-0034 open question 3, alternative): a Project Owner decision.
- File tools refusing paths inside a store (ADR-0034 decision 5): until then an agent with `write`
  can change the files, and the result reads as unverified (behaviour 13).
- A search index, external tracker sync (M20.5), reports and export (M20.6), the scheduler's
  `ticket_status` event source (SPEC-0342), links to costs of agent sessions (M19.10).
- A JSON Schema of the kinds `work-item` and `work-store` under `docs/formats/`.

## Known deviations from the reference
The reference keeps tasks in external trackers and a local list of current tasks; nothing of it is
copied. Deviations from common trackers, on purpose:
- **D1** The store is private by default; sharing through the repository is a person's deliberate
  choice (behaviour 15).
- **D2** Ticket text is untrusted data for agents, and an agent cannot close, cancel or
  reprioritise without a person's confirmation for that call (behaviours 19, 23).
- **D3** No sequential numbers: keys from random ids never collide across branches and machines
  (behaviour 10).
