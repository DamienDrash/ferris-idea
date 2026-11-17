# ADR-0034: Tickets, boards and the project cockpit — one work item for people, agents and the task graph

- **Status:** proposed · **Version:** 1 · **Date:** 2026-11-17 · written on the Project Owner's
  request of 2026-11-17 (ROADMAP M20, item M20.1): a cockpit per project with the relevant KPIs, a
  ticket system for agents and users, and project-management boards to choose from (Kanban, Scrum,
  Gantt, Lean, classic to-do lists with Gantt and others). Rule 12 ("security first") applies
  throughout; every weaker option is named as the Project Owner's decision. Implementation is
  after 1.0 (M20) and after quality gate QG-1 (ADR-0015). The spec of M20.1 follows this ADR once
  it is accepted.
- **Related:** [ADR-0007](0007-no-restrictions.md) (no telemetry),
  [ADR-0008](0008-mcp-server.md) (MCP server),
  [ADR-0012](0012-headless-core-capability-api.md) (capability layer),
  [ADR-0014](0014-orchestration-principles.md) (task graph, persisted records),
  [ADR-0021](0021-idecraft-file-formats.md) (file formats; amended in one point, decision 3),
  [ADR-0023](0023-idecraft-as-agent-harness.md) rules 7 and 12 (untrusted content, messages
  between agents), [ADR-0025](0025-network-security-for-remote-connections.md) (remote paths),
  [ADR-0031](0031-project-files-never-grant-agents.md) (project files never grant),
  ADR-0033 (internal vault for agents, proposed at the same time),
  [SPEC-0053](../specifications/SPEC-0053-agent-permissions.md) (agent rights),
  [SPEC-0323](../specifications/SPEC-0323-project-room.md) (project room, M6.15),
  [AI-native vision](../architecture/AI_NATIVE_VISION.md) §3.2 (task graph), ROADMAP M12.2, M19.10,
  M19.13, M20.2–M20.6

## Context

Three things the Project Owner asked for share one subject, the work to be done in a project:

- **Tickets** that people and agents both create, read, comment on and move along.
- **Boards** to choose from per project: Kanban, Scrum, Scrumban, Gantt, Lean, a to-do list with a
  Gantt chart, an Eisenhower matrix and a roadmap timeline.
- **A cockpit** per project with KPIs on code, build, delivery, flow, agents and databases.

IDECraft already plans a fourth thing about the same subject: the persistent task graph of M12.2
(ADR-0014 rule 2, vision §3.2). It holds tasks with a hierarchy, dependencies, priority, owner,
assignee, acceptance criteria and a fixed state transition table with guards, and it is the record
agents work from. If tickets were a separate store, every task would exist twice, and the two copies
would drift: an agent would close a task the person still sees as open on the board.

How tools in this space store work items, from their public documentation and general knowledge
(`DOC`, `GEN`):

- **Hosted trackers** (GitHub Issues and Projects, GitLab Issues, Jira, Linear) keep items in the
  vendor's database. Boards, tables and roadmaps are views over the same items with extra fields
  (GitHub Projects: board, table and roadmap layouts over one item set; Linear: cycles for
  iterations). They need an account and the network, and the history is the vendor's.
- **Trackers in the repository** keep items next to the code: git-bug stores them as Git objects on
  their own refs, Fossil has tickets in its repository database, and several small tools keep one
  Markdown file per task in a folder. Items travel with clones and work offline; what is committed
  is as public as the repository.
- **IDE task integrations** of established IDEs mostly connect to an external tracker and keep only a
  local list of "current tasks" with a link to a branch or changelist; the tracker stays the source.

Constraints from earlier decisions:

- Agents get no rights from project files (ADR-0031). A ticket in a cloned repository must not make an
  agent do anything.
- Text from the project or from outside is untrusted for agents (ADR-0023 rule 7). Ticket text is
  exactly that: anyone who can open an issue on a public tracker, or commit to the repository, writes
  it.
- Nothing leaves the machine unasked, and nothing is measured for us (ADR-0007). Every KPI is computed
  on the machine from data the user already has.
- Every feature has an MCP tool (ADR-0008); every changing effect has a right and a row in the effect
  table (ADR-0031 decision 6).

## Decision

### 1. One work item, one source of truth

There is one record type, the **work item**. A ticket, a task of the task graph, an epic, a
milestone and a checklist entry of a to-do list are all work items with a different `kind`. The task
graph of M12.2 is not a second store: it is the engine that validates and applies changes to work
items. Boards are views over work items (decision 7), the cockpit reads them (decision 8).

What is stored where:

| Data | Where | Why |
|---|---|---|
| **Planning state** — title, description, kind, state, priority, assignee, labels, parent, dependencies, estimate, dates, iteration, rank, acceptance criteria, links | the work item's files (decision 2) | people read, review and merge it |
| **History** — every change with actor, time, old and new value | the work item's event log beside it (decision 3) | KPIs (cycle time, flow) and conflict resolution need it |
| **Run state** — which agent session works on it, attempts, retries, leases, gate runs, cost | the engine's local state (ADR-0021 class "state"), keyed by the work item's id | machine-written, per machine, changes every second |

Run state never holds planning fields. When a run ends, its result reaches the work item as an event
("gates passed on change set X", "moved to `in_review` by agent session Y") and as a link to the
session record of M19.10.

**Kinds:** `task`, `bug`, `finding` (what an agent or an inspection found), `question`, `epic`,
`milestone`, `incident` (needed for two DORA keys, decision 8), `improvement` (Kaizen board). The list
is closed in format 1; a project can add labels, not kinds.

**States** are the fixed table of vision §3.2, the same for every board:
`proposed → ready → assigned → in_progress → in_review → done`, plus `blocked`, `failed`,
`cancelled`. Every transition has a guard in Rust code; `done` needs every acceptance criterion met
and, for work done by an agent, a review by another identity (ADR-0014 rule 2). A board column is a
named group of states, optionally split by a project-defined `stage` value inside one state (for
example "in_progress / coding", "in_progress / testing"). Columns therefore never invent a state the
engine does not know, and a WIP limit or a cycle time means the same on every board.

### 2. Storage: plain text files, in one of three places with the same format

A work item is a folder of text files:

```
<store>/items/<id>/item.toml        planning state, ADR-0021 envelope kind = "work-item"
<store>/items/<id>/description.md   the description, Markdown
<store>/items/<id>/comments/<comment-id>.md   one file per comment, small TOML front matter
<store>/items/<id>/events.jsonl     history, decision 3
<store>/iterations/<id>.toml        sprints and cycles
<store>/boards/<name>.toml          shared board definitions, decision 7
```

`item.toml` is written with a fixed key order and one field per line, so Git merges two changes to
different fields of the same item without a conflict. Lists (labels, dependencies) are written one
entry per line, sorted. A new comment is a new file and never conflicts. Attachments are files in the
item's folder, at most 10 MiB each, and never executed or opened by IDECraft beyond a preview of
images and text.

The same layout lives in one of three **stores**, chosen per project:

| Store | Where | Sharing | History | Offline |
|---|---|---|---|---|
| **Private** (default) | the user's data folder, one folder per project, kept as its own small Git repository | only this user and their agents on this machine | Git log of the store | yes |
| **Repository** | `.idecraft/work/` in the project, under the project's version control | everyone who can read the repository | Git log of the project; items follow branches | yes |
| **Project room** | the server-owned room of M6.15 (SPEC-0323) | the room's roles | the room's history | no (cached read-only copy) |

Why the private store is the default (rule 12): what is committed to a repository is as public as the
repository. A bug ticket can name a security hole, a customer or an internal host before anyone thinks
about it, and repositories become public later. IDECraft cannot
reliably know whether a remote is public. The private store loses nothing on one machine: it has
history, works offline and needs no server.

Choosing the repository store is an explicit act per project. The question names the remote(s) and
says that every item will be readable by everyone who can read the repository and stays in its
history. A work item can carry `visibility = "private"`; such an item is kept in the private store
even when the project uses the repository store, and the board shows both. Before an item is written
to the repository or room store, IDECraft scans its text for secret patterns (the scanner of
ADR-0023 rule 7) and refuses to write a match; a person can mark a false match, an agent cannot.

Moving items between stores (private to repository, repository to room) is a command that shows the
items first and needs the `ticket-manage` right for agents (decision 5).

A local database is only an index: a cache in the user's cache folder for full-text search, board
queries and KPIs, rebuilt from the files at any time and never the source of truth.

### 3. History and merges

`events.jsonl` holds one JSON object per line: id, time (UTC), actor, field, old value, new value,
and for agents the session id and model. The project's `.gitattributes` entry for the repository
store uses Git's built-in `union` merge for these files, which keeps the lines of both sides; IDECraft
sorts by time on reading. No merge driver with our own code is needed, and none runs from project
configuration (ADR-0031).

This amends ADR-0021 in one point: a project file of kind `work-item-events` is JSON Lines, because
appended lines merge with Git's own `union` driver and TOML arrays do not.

When a merge leaves `item.toml` with a conflict anyway (the same field changed on two branches),
IDECraft offers a field-wise resolution from the two event logs: for each conflicting field the later
event wins and the other value is kept as a visible "conflict resolved" event, so nothing is lost
silently. A person confirms; an agent with `vcs-write` may apply it, and the result is a normal commit.

On loading, the engine replays the event log against the transition table. A state that no legal
sequence of transitions reaches (for example an item edited by hand from `proposed` to `done`) is
shown as **unverified**: the board marks it, the cockpit leaves it out of flow KPIs, and agents see
`state_verified: false`. Hand edits stay possible for people; they just do not count as guarded
transitions.

### 4. Identifiers that do not collide

- The **id** of a work item is a UUID version 7 (time-ordered, 74 random bits), created on the machine
  that creates the item. It names the item's folder, so two branches or two machines never create the
  same folder.
- The **key** people say and type is a project prefix and the first characters of the id's random
  part in Crockford Base32: `IC-7K3QDX`. Six characters by default; IDECraft lengthens the key shown
  for an item when two items in the visible stores share a prefix, as Git does with short commit
  hashes. Keys are never reused.
- **Sequential numbers** (`#123`) are only aliases, given by an authority that sees every item: the
  project room, or a synced external tracker (decision 10). Without such an authority there are no
  numbers, because two branches would hand out the same one.
- References in commit messages, branch names and comments use the key (`IC-7K3QDX`); IDECraft links
  them in the log, the editor and the board.

### 5. Rights of agents

Building on SPEC-0053 (granted per workspace in the user's own grant file, never by the project,
ADR-0031), three new rights:

| Right | Allows an agent to |
|---|---|
| `ticket-write` | create work items (kinds `task`, `bug`, `finding`, `question`), comment, edit the text of items it created, add links, assign itself, and move items along the transition table except into `done` and `cancelled` |
| `ticket-triage` | change priority, assignee (to anyone), labels, estimate, dates, iteration, rank and parent; create `epic` and `milestone` items |
| `ticket-manage` | move items into `cancelled`, move items between stores, edit board definitions and iterations |

Rules:

- **Reading needs no right.** Listing, reading and querying items, boards and KPIs are reading tools,
  with one exception: items marked `visibility = "private"` are listed to an agent with their key and
  title only, unless its session holds the right `secret-read` (SPEC-0053 behaviour 10).
- **`done` is never set by an agent alone.** An agent moves its work to `in_review`; `done` needs the
  guard (criteria met, review by another identity) and, by default, a person's confirmation in the
  window. The Project Owner may allow a named reviewer agent to confirm (open question 2).
- **Nothing is deleted by an agent.** Agents cancel; deleting an item folder is a person's action, and
  in the repository store it stays in Git history anyway.
- **No bypass through file tools.** The file tools (`create_file`, `copy_file`, `rename_file`, …) refuse
  paths inside a ticket store and name the ticket tools, so the guards and rights above cannot be
  skipped with the right `write`. An agent with `run` can still change files with its own programs; as
  for every other file, the engine then sees an unverified state (decision 3), never a guarded one.
- **Limits:** at most 50 items created per agent session and hour by default, so a looping agent
  cannot flood a board; reaching the limit is a blocker shown to the person.
- Every changing ticket tool has a row in the effect table of ADR-0031 decision 6 and writes the line
  of the agent log.

### 6. Ticket text is untrusted content for agents

- Every text field of a work item — title, description, comments, labels, stage names, attachment
  names, text imported from external trackers — is untrusted data in the tool schemas (ADR-0023
  rule 7), whoever wrote it. The tool result says who wrote it (person, agent session, imported from
  which tracker) and in which store it lives.
- **The instruction is the assignment, not the text.** An agent works on an item because a person, or
  the engine under a grant the person set up, assigned it. The text describes the work; it can never
  widen rights, pick a model, raise a budget, start agents, run commands or switch on a scheduler
  trigger. There is no field in format 1 that names a command, a script, a model or a grant.
- Triggers of the scheduler (M19.13) on ticket events are configured in the user's settings, not in
  the store, and run with the grants of that configuration.
- Links in ticket text are never fetched by IDECraft on its own; an agent that wants to open one needs
  its usual rights for the network.
- Items an agent creates are untrusted for every other agent too (ADR-0023 rule 12).
- No secrets in tickets: the scanner of decision 2 runs on every write, in every store.

### 7. Boards are views

A board is a saved view: a filter (store, kinds, labels, iteration, assignee), a layout, and the
layout's settings. Board definitions shared with the team live in the store's `boards/` folder; a
person's own boards and settings live in the user's settings folder. Switching the layout never
changes items.

| Board | Fields it needs | Computed |
|---|---|---|
| **Kanban** | state, `stage`, `rank`; per column a WIP limit; optional swimlane field (assignee, priority, epic, label) | WIP per column, blocked items, cumulative flow diagram from the event log |
| **Scrum** | iteration (start, end, goal), estimate (points or hours, per project), `rank` for the backlog | burndown and burnup per iteration, velocity over the last iterations, carry-over |
| **Scrumban** | Kanban fields plus an optional iteration and a WIP limit on the "ready" queue | as Kanban, plus planned against done per iteration |
| **Gantt** | start, end or duration, dependencies with type (finish-to-start, start-to-start, finish-to-finish, start-to-finish) and lag, milestones (`kind = milestone`), a working calendar, optional baseline | critical path by the critical path method on the dependency graph (longest path, earliest and latest start, slack), drift against the baseline; a cycle in dependencies is refused at entry |
| **Lean** | the event log (time per state and stage), `blocked` time, value stream stages = the board's columns | lead time, cycle time, flow efficiency, waiting time per stage; Kaizen board = items of kind `improvement` |
| **To-do list with Gantt** | `rank`, parent (for nesting), optional start and end | list for items without dates, bars for items with dates, in one view |
| **Eisenhower matrix** | priority (important or not, from priority ≥ high by default) and urgency (explicit `urgent` flag, or due within a configurable number of days) | the four quadrants |
| **Roadmap timeline** | epics and milestones with target dates or quarters | progress per epic from the state of its children |

`rank` is a fractional index (a string between its neighbours), so reordering one item changes only
that item and two people reordering on two branches rarely conflict. A baseline is a stored snapshot
of start and end dates, kept as its own file in the store.

### 8. The cockpit: KPIs computed on the machine

The cockpit is a set of tiles, chosen per project. Every tile shows its value, its trend over a chosen
period, its **definition** and its **data sources**, and says "not enough data" instead of a number it
cannot support. Thresholds are set by the user and raise a hint in the window, never a message outside
it. Nothing is sent anywhere (ADR-0007); data from hosted services (CI, pull requests, deployments) is
fetched only when the user connects that service and only on refresh or a schedule the user set.

**Delivery (the four DORA keys).** A *deployment* comes from one source the project configures: a tag
pattern (default `v*`), a named run configuration, or the deployment records of a connected CI or
hosting service. A *failure* is an item of kind `incident`, or a `bug` linked to the deployment that
caused it, or a rollback (a deployment of an older version).

| KPI | Definition |
|---|---|
| Deployment frequency | deployments to the production environment per week, with the share of weeks that had at least one |
| Lead time for changes | median time from a commit's author time to the first deployment that contains it |
| Change failure rate | deployments followed by a failure, divided by all deployments in the period |
| Failed deployment recovery time | median time from the start of a failure (incident created, or failed deployment) to the next deployment or the incident moving to `done` |
| Open pull requests and age | count and median age of open pull requests or merge requests (connected service only) |

**Flow (from the event log).**

| KPI | Definition |
|---|---|
| Lead time | median time from an item's creation to `done` |
| Cycle time | median time from an item's first entry into `in_progress` to `done` |
| Throughput | items reaching `done` per week |
| WIP | items in `assigned`, `in_progress` and `in_review` now, with the age of each |
| Blocked | items in `blocked` now and total blocked time in the period |
| Flow efficiency | time in active states divided by cycle time; active = `in_progress` minus stages marked "waiting" and minus `blocked` |

Unverified items (decision 3) are left out and counted separately.

**Code and build.**

| KPI | Definition |
|---|---|
| Build health | share of green builds on the main branch over the last 20 runs (local runs, the server CI the project configured, or a connected CI service), and time since the last green run |
| Test health | tests passed, failed, skipped in the last run; flaky tests = tests that passed and failed on the same commit in the period; median duration |
| Coverage | line coverage from the newest coverage report the project produces (LCOV, Cobertura, JaCoCo), with the report's date |
| Inspections | problems by severity on the main branch from IDECraft's own inspections, new since the period start |
| Dependency vulnerabilities | known advisories by severity for the versions in the project's lockfiles, matched against a local copy of an advisory database (OSV format, which RustSec, GitHub and others publish); the copy's date is shown and the copy is refreshed only on the user's request or schedule |

**Agents (from session records of M19.10 and the work items).**

| KPI | Definition |
|---|---|
| Sessions | agent sessions started, by agent and model |
| Agent success rate | items an agent moved to `in_review` whose change set later reached `done` and was not reverted within 14 days, divided by items assigned to agents and finished (done, failed or cancelled) in the period |
| Gate failures | gate runs failed per change set (ADR-0023 rule 5) |
| Cost and budget | spend per period, per item and per agent from the usage the provider program reports; for subscriptions without a per-call price, tokens and the subscription name, never an invented amount |
| Waiting approvals | approvals waiting now and their age |

**Databases.** Connections in use and slow queries from IDECraft's own query history (threshold set
per connection).

### 9. MCP tools and command line

Every function above has an MCP tool (ADR-0008) and an `idecraft` command; the window uses the same
capabilities (ADR-0012). Names are provisional for the spec:

- reading: `ticket_query` (filter, full text, paged), `ticket_get`, `ticket_history`, `board_list`,
  `board_view` (columns with item keys and titles, paged), `gantt_schedule` (dates, slack, critical
  path), `cockpit_kpis` (values with definition, sources, period and "not enough data"),
  `kpi_series`, `iteration_list`;
- changing, each with a dry run by default and the right of decision 5: `ticket_create`,
  `ticket_update`, `ticket_comment`, `ticket_transition` (refused with the guard's reason when the
  table does not allow it), `ticket_link`, `ticket_move_store`, `board_save`, `iteration_save`,
  `baseline_save`;
- sync: `tracker_sync` (decision 10).

Results carry item text as untrusted data with its author and store, never the whole store at once,
and are paged.

### 10. Optional sync with external trackers

- **Only official APIs:** GitHub (REST and GraphQL for Issues and Projects), GitLab (REST), Jira
  (REST, Cloud and Data Center), Linear (GraphQL). No scraping, no unofficial endpoints.
- **Credentials** come from the vault of ADR-0033 as a handle, or from the system keychain until the
  vault exists; never from a project file. The connection asks for the narrowest token scope the
  service offers (issues of the named repository or project), and IDECraft says which scope it
  needs.
- **Import first.** The first version imports and refreshes (tracker to IDECraft). Writing back is a
  second, per-tracker opt-in. Every write to an external tracker is an outward action: for agents it
  needs the right `remote` plus `ticket-write` and a person's approval per call (ADR-0023 rule 7).
- **Mapping** of fields and states is a per-connection table with defaults; the external id and number
  are stored as aliases of the item (decision 4). A conflict (both sides changed the same field since
  the last sync) keeps the newer value and records the other as a visible event.
- **No webhooks:** IDECraft opens no port for a tracker (ADR-0025 decision 5). Refresh is on request or
  on the user's schedule.
- Imported text has the trust level "external" (decision 6). Items marked `visibility = "private"` are
  never written to an external tracker.
- IDECraft is fully usable without any external tracker.

## Consequences

- **One model to build, once.** M12.2 and M20.2 are the same code: the transition table and guards are
  written once, and both the board and the orchestration engine call them. The ticket part (M20.2)
  can ship before the full multi-agent engine; the engine later adds scheduling and run state on top
  without moving any data.
- **Works for one person offline** from day one (private store), for a team through the repository or
  the room, without a second system.
- **Reviewable and mergeable:** items are text in a diff; reordering and commenting rarely conflict;
  real conflicts resolve field by field from the event logs.
- **Privacy has a cost in convenience:** a new project keeps its items private, and sharing through
  the repository is one deliberate step with a warning. Teams that want items in the repository have
  to choose it once.
- **Hand edits are allowed but marked:** people can edit item files in any editor; edits that skip
  the guards show as unverified and do not feed flow KPIs. This keeps the files honest text without
  making the guards decorative.
- **Many small files:** a large project with thousands of items has thousands of folders. The index
  (decision 2) keeps queries fast; Git handles this size well.
- **KPIs depend on configuration:** the DORA keys need a deployment source and incidents; without them
  the tiles say "not enough data" rather than guessing.
- **New rights** `ticket-write`, `ticket-triage`, `ticket-manage` in SPEC-0053, rows in the effect
  table, and file tools that refuse ticket store paths.
- **ADR-0021 amended:** kind `work-item-events` is JSON Lines in a project folder.
- **Risks:** prompt injection through ticket text is the main risk, as for all project content; decisions
  6 and 5 limit what an injected instruction reaches. Advisory data and CI data are only as fresh as
  the user's last refresh, and the tiles show their date.

## Alternatives considered

- **Separate ticket store beside the task graph.** Two sources of truth that drift; every change would
  need a sync between our own two stores. Rejected.
- **A local database (SQLite) as the source of truth.** Fast and transactional, but binary: no review
  in a diff, no merge across branches, sharing only through a server. Kept as a rebuildable index
  only.
- **The repository as the default store.** Simplest for teams, but leaks items of every project whose
  repository is or becomes public. Rejected as the default under rule 12; available per project.
- **A dedicated Git ref or orphan branch (as git-bug does).** Keeps items out of the code history and
  shares them with pushes, but most hosting services and tools do not show such refs, items no longer
  follow feature branches, and pushes of extra refs surprise users. Rejected for format 1; it may
  return as a fourth store if the private store proves too isolated (open question 7).
- **One file for all items, or one file per board.** Every concurrent change would conflict. Rejected.
- **Sequential numbers as the primary key.** Collide across branches and machines without a central
  authority. Kept as aliases given by an authority only.
- **An external tracker as the source of truth.** Needs an account and the network, puts work items
  and agent findings on a third party's servers, and cannot carry the guarded transitions. Rejected;
  sync is optional.
- **A merge driver of our own registered from the project.** Would run IDECraft code from a project
  configuration on every merge (ADR-0031). Git's built-in `union` driver plus field-wise resolution in
  the window does the job. Rejected.
- **Board-specific states** (each board defines its own columns as states). WIP, cycle time and the
  guards would mean something different on each board, and agents would face states the engine does
  not know. Rejected; columns group the fixed states and stages.

## Open questions for the Project Owner (defaults apply until answered)

1. **Default store for a new project.** Default: the private store; the repository store is chosen
   per project after a warning. Alternative: the repository store by default for repositories
   without a remote.
2. **May an agent confirm `done`?** Default: no; `done` needs the guard and a person's confirmation in
   the window. Alternative: a named reviewer agent (another identity than the assignee) may confirm
   when the user grants it per workspace.
3. **Priority, assignee and closing by agents.** Default: priority and assignee need `ticket-triage`,
   cancelling needs `ticket-manage`; neither is granted by default, and once granted they work
   without a question per call. Alternative: a question per call even with the right.
4. **Key format.** Default: project prefix plus six Base32 characters of the random id
   (`IC-7K3QDX`), sequential numbers only as aliases from the room or a tracker. Alternative:
   per-machine counters with a machine prefix (`IC-M1-42`).
5. **Data from hosted services in the cockpit** (CI runs, pull requests, deployments). Default: off;
   only local data until the user connects a service. Alternative: suggest connecting the remote's
   hosting service when the project is opened.
6. **Advisory database for dependency vulnerabilities.** Default: downloaded only when the user asks
   (and on a schedule the user sets), from the official OSV export, stored in the user's cache folder.
   Alternative: bundled with each IDECraft release, refreshed on request.
7. **A Git ref as a fourth store** for people who want items shared through Git but outside the code
   history. Default: not in format 1.
8. **Write-back to external trackers.** Default: import and refresh only in the first version;
   writing back per tracker as a later opt-in, with a person's approval for every agent write.
   Alternative: two-way sync from the start for GitHub Issues.
9. **Amending ADR-0021** with the JSON Lines event log in a project folder. Default: yes, as in
   decision 3.
10. **Order of building.** Default: the work item model, the private store, Kanban, the to-do list and
    the flow KPIs first (they need no external data), then Scrum and Gantt, then the repository and
    room stores, then the cockpit's build, delivery and agent tiles, then tracker sync.
