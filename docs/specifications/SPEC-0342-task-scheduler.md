# SPEC-0342: Scheduler for agent tasks

- **Version:** 1 · **Date:** 2026-11-17
- **Sources:** `GEN` (job schedulers and work queues: one-off and recurring timers, coalescing of
  missed fires, priority queues, job dependencies, concurrency limits per resource, maintenance
  windows, per-job budgets and timeouts); ADR-0023 rules 7 (safety rules for MCP tools, untrusted
  content), 10 (every call that starts agents checked against grants and budget and recorded), 16
  (sessions survive restarts); ADR-0008 (every feature has an MCP tool); SPEC-0053 (agent rights,
  the grant file in the user's settings folder, the agent log); SPEC-0341 (the agent watchdog: same
  host, clock and settings-folder pattern). No observation of the reference: the reference has no
  scheduler for agent tasks.
- **Feature matrix rows:** none (ROADMAP M19.13, harness phase after 1.0; owner's order of
  2026-11-17).

## Purpose
A person wants agents to work while they are away: a review every night, a check when CI turns
red, a fix attempt when a merge conflict appears, a test run when a file changes. The scheduler
keeps tasks the user defined, fires them by time or by event, queues the runs by priority and
dependency, starts no more of them than the machine, the simulators and the databases can take,
keeps quiet hours, and stops every run at its budget. A scheduled run never has more rights than
the person who created the task granted, its instruction is always the user's text, and every run,
skip and refusal is written to the agent log for the session history (ROADMAP M19.10).

This version is the model without a host of its own: Ferris IDEA does not host agents yet (ROADMAP
M11.2, M11.3, M19.5). The scheduler decides over an interface that the agent host of M19.5 provides
(start a run, report its status and spending, stop it); its configuration, its state, the
file-change event source and its MCP tools exist now.

## Behaviour

### Tasks
1. A task has an `id` (1 to 64 characters of `a-z`, `0-9`, `-`, `_`), the `prompt` (the
   instruction the agent gets, written by the user), a trigger (behaviours 4 to 6), a `priority`
   (0 to 9, default 5, higher first), `after` (ids of tasks it depends on, behaviour 11),
   `resources` (names of the resources a run occupies, behaviour 12; default `machine`),
   `budget_cents` and `max_minutes` (the budget of one run, behaviour 14), `rights` (the rights a
   run may use, behaviour 16), `enabled` (default true), `created_by` and `created_at`. [GEN]
2. Bounds: a prompt of 1 to 4 000 characters without control characters other than line breaks
   and tabs; at most 100 tasks per workspace; ids unique; `after` names existing tasks, never the
   task itself, and the dependencies form no cycle; at most 8 resources per task, each name 1 to 64
   characters of `a-z`, `0-9`, `-`, `_`, `:`, `.`; `budget_cents` 1 to 10 000; `max_minutes` 1 to
   1 440. A task outside a bound is refused. [GEN]
3. The scheduler fires triggers and starts runs only while it is enabled for the workspace
   (behaviour 20); off by default. While it is off it still stops running runs at their budget
   (behaviour 14). One round (a tick) uses the scheduler's clock for every decision; nothing reads
   wall time directly, so every decision can be tested with a fake clock and a fake event source.
   [GEN]

### Triggers
4. **Once** at a time (seconds since the Unix epoch): fires in the first round at or after that
   time, then never again. A once-trigger whose time passed while Ferris IDEA was not running fires
   once in the next round. [GEN]
5. **Recurring**, either every `every` seconds (300 to 2 592 000) at `from + k × every` (`from`
   defaults to the time the task was created plus `every`), or daily at `HH:MM` in the workspace's
   time (behaviour 20, `utc_offset_minutes`), first at that time on or after the time the task was
   created. Fires that were missed while Ferris IDEA was not running, or while a round did not come,
   are coalesced into one run, never caught up one by one; the next fire time is the first one
   after now. [GEN]
6. **Event**, of a kind and an optional `filter` (1 to 256 characters): `ci_failed`, `pr_comment`,
   `merge_conflict`, `file_changed`, `ticket_status`. An event has a kind, a `subject` (branch,
   pull request, path, ticket and status) and `data`. The filter matches the subject: `*` stands
   for any characters except `/`, `**` for any characters, everything else matches itself; no
   filter matches every subject of the kind. [GEN]
7. Event sources are an interface: a source is polled once per round and returns the events since
   the last poll. This version ships the file-change source: it compares size and modification time
   of the workspace's files with the last poll (the first poll only takes the baseline), skips
   `.git`, `.ferris-idea`, `target` and `node_modules`, looks at no more than 20 000 files, and reports
   each added, changed or removed file once, with its workspace-relative path as the subject and no
   data. Sources for CI, pull request comments, merge conflicts (GitHub, ROADMAP M11/M12) and
   ticket status (ROADMAP M20) are open. [GEN]

### Untrusted event data
8. The task's instruction is always its `prompt`. An event never changes it, is never inserted into
   it, and can neither create nor change a task. The event that queued a run is handed to the host
   beside the prompt as untrusted data — kind, subject and data, each cut to 4 000 characters —
   which the host must give the agent marked as untrusted content (ADR-0023 rule 7). The agent log
   records the event's kind and subject, cut to 256 characters, never its data. [ADR-0023 rule 7]

### Queue
9. A trigger that fires queues one run of the task. A task has at most one queued run: a further
   fire while one is queued is coalesced into it and logged as `coalesced`. The queue holds at most
   `max_queue` runs (behaviour 20); a fire beyond that is skipped and logged. [GEN]
10. Each round the queued runs are considered in order of priority (higher first), then the time
    they were queued, then the task id. A run that cannot start yet stays queued; a later run whose
    dependencies and resources allow it may start before it. [GEN]
11. A queued run of a task with `after` waits while a run of any of those tasks is queued or
    running. When none is, it may start only if the last finished run of each of them succeeded;
    if one failed, was stopped or skipped, or never ran, the run is skipped with that reason.
    [GEN]
12. Concurrency limits are hard caps: at most `max_parallel` runs (behaviour 20) run at once, and
    at most `limits[name]` runs that occupy a resource `name`. A resource without a configured
    limit has the limit 1. A run starts only when every resource it occupies is below its limit.
    [GEN]
13. Quiet hours (`from` and `to` as `HH:MM` in the workspace's time; `to` before `from` spans
    midnight): no run starts during them. Triggers still fire and queue; the queued runs start when
    the quiet hours end. Running runs are not stopped by quiet hours. [GEN]

### Runs and budgets
14. Every run has a hard budget: `budget_cents` and `max_minutes` of its task. The host gets both
    when the run starts. Each round the scheduler asks the host for every running run's status and
    spending; it stops the run when its time is up, when its spending reaches the budget, or when
    the host reports no spending (it cannot be checked, so it counts as reached). [GEN]
15. At most `max_runs_per_day` runs start per workspace and day (in the workspace's time); a run
    beyond that is skipped and logged. [GEN]
16. A run never has more rights than its creator granted: a task may only be created with rights
    the creator holds (behaviour 22), and a run gets the task's rights less every right the
    workspace no longer grants when it starts (the intersection of both), the same rights a
    directly started agent of the workspace would get. A task without rights runs without rights.
    [ADR-0023 rule 7, SPEC-0053]
17. A run that the host fails to start is `failed` with the host's reason. A run whose status the
    host cannot tell is stopped, because its spending cannot be checked (behaviour 14). A queued run
    of a task that was removed or disabled is skipped; a running one is stopped. Budget and rights
    of a run are fixed when it starts; changing the task later does not change a running run.
    [GEN]

### Agent log and persistence
18. Every queueing, coalescing, start, end (`succeeded`, `failed`, `stopped`), skip and refusal
    writes one line to the agent log `.ferris-idea/agent-log.jsonl` (SPEC-0053 behaviour 5) with
    `time`, `tool` (`scheduler`), `task`, `run`, `action`, `reason` and, as they apply,
    `priority`, `rights`, `budget_cents`, `max_minutes`, `spent_cents`, `trigger`, `event_kind`,
    `event_subject` and `prompt_chars` (the prompt's length, never its text). A run is started only
    after its line was written: when the log cannot be written, nothing starts and the run stays
    queued. [SPEC-0053, ADR-0023 rule 10]
19. The scheduler's state per workspace — per task the next fire time, the last fire and the last
    outcome, the queue, the running runs and the last 200 finished runs with trigger, event kind
    and subject, rights, times, spending and reason, and the count of runs of the day — is saved
    after every round that changed it in `agent-schedule-state.json` beside the configuration,
    written through a temporary file and a rename. It is read back when the scheduler starts, so a
    restart of Ferris IDEA neither loses a queued run nor resets the day's count; runs that were
    running are asked for again. A state file that cannot be read starts empty and is reported.
    [ADR-0023 rule 16, GEN]
20. The configuration lives in the user's settings folder beside the grant file of SPEC-0053
    (`agent-schedule.toml`), one `[[workspace]]` table per workspace with `path`, `enabled`
    (default false), `utc_offset_minutes` (−720 to 840, default 0), `quiet_hours` (none),
    `max_parallel` (1 to 8, default 1), `max_queue` (1 to 200, default 50), `max_runs_per_day` (1 to
    500, default 24), `limits` (resource name → 1 to 16) and its `[[workspace.task]]` tables. Paths
    are compared after resolving links. A file inside the workspace is ignored and reported, so a
    cloned project cannot schedule tasks; unknown keys or values outside the bounds make the file a
    problem and leave the scheduler off. [SPEC-0053, GEN]

### MCP tools
21. `schedule_list` (reads, needs no right) answers the configuration of the workspace with its
    tasks (with their prompts, which the user wrote), the files consulted and their problems, the
    queue with why each run waits, the running runs, the last finished runs (`limit`, default 20,
    at most 200), the bounds, and `host`, which says that no agent host is attached yet until
    M19.5 provides one. [ADR-0008]
22. `schedule_add` adds or replaces a task (`replace: true` for an existing id). `schedule_remove`
    removes one; it is refused while other tasks depend on it, naming them. `schedule_configure`
    changes `enabled`, `utc_offset_minutes`, `quiet_hours` (or `no_quiet_hours`), `max_parallel`,
    `max_queue`, `max_runs_per_day` and `limits`. All three have a dry run (`dry_run`, default
    true) that answers what they would write. `dry_run: false` needs the `settings` right (it
    changes the user's own settings, SPEC-0053 v8) and is logged. `schedule_add` with
    `dry_run: false` also needs every right it names in `rights` to be granted by the workspace's
    grant file at that moment; a right granted only once for a call, or not at all, is refused with
    its name. Values outside the bounds of behaviours 2, 5, 6 and 20 are refused with the bound.
    [ADR-0008, ADR-0023 rules 7 and 10]

## Acceptance criteria
- **AK-1** Given a fake clock, when rounds pass a once-trigger, an interval and a daily time, then
  each fires once when due, the once-trigger never again, and missed fires are coalesced into one
  run with the next fire after now (behaviours 3, 4, 5).
- **AK-2** Given a fake event source, when events of several kinds and subjects arrive, then only
  tasks whose kind and filter match are queued, a second event coalesces, and the run carries the
  event as untrusted data while the prompt stays the task's text; the file-change source reports
  added, changed and removed files after its baseline and skips `.git` (behaviours 6, 7, 8, 9).
- **AK-3** Given queued runs of different priority, dependencies and resources, when rounds go on,
  then they start by priority, a dependent run waits for its dependency and is skipped when it
  failed, and neither `max_parallel` nor a resource limit (default 1) is ever passed (behaviours
  10, 11, 12).
- **AK-4** Given quiet hours spanning midnight, when a trigger fires in them, then the run is
  queued but starts only after they end (behaviour 13).
- **AK-5** Given running runs, when the time is up, the spending reaches the budget or is not
  reported, then the run is stopped; the day's cap and the queue's cap skip further runs; a removed
  task's runs are skipped or stopped (behaviours 14, 15, 17).
- **AK-6** Given a task whose rights the workspace no longer fully grants, when it starts, then it
  gets the intersection; every queueing, start, end, skip and refusal leaves one line in the agent
  log with the prompt's length but not its text or the event's data, and a log that cannot be
  written starts nothing (behaviours 16, 18).
- **AK-7** Given a configuration file, when it is read, then defaults, bounds, cycles, unknown
  dependencies, unknown keys, a file inside the workspace and other workspaces' tables follow
  behaviours 1, 2 and 20; given saved state, when a new scheduler starts from it, then the queue,
  the running runs and the day's count are kept (behaviour 19).
- **AK-8** Given an MCP client, when it calls `schedule_list`, then it gets the tasks, the queue
  and the history; `schedule_add`, `schedule_remove` and `schedule_configure` answer a dry run by
  default, refuse `dry_run: false` without the `settings` right, `schedule_add` refuses a right the
  grant file does not hold, values out of bounds and cycles, `schedule_remove` refuses a task
  others depend on, and with the rights they write the file (behaviours 21, 22).

## Out of scope
- Hosting agents (ACP client, terminal agents): ROADMAP M11.2, M11.3, M19.5. Until then no run
  starts in a running Ferris IDEA; the model, its configuration, its state and its tools are ready
  for the host to plug in.
- The timer that runs the rounds in the window or a headless server, and the scheduler's view in
  the window; they come with the host.
- Event sources for CI results, pull request comments and merge conflicts (GitHub, ROADMAP
  M11/M12) and for ticket status (ROADMAP M20); the interface is defined.
- Named time zones with daylight saving time; this version uses a fixed `utc_offset_minutes`.
- Showing runs in the session history view (ROADMAP M19.10); the records are kept for it.
- A model or agent per task (ROADMAP M19.7); the host's default agent runs every task.

## Known deviations from the reference
The reference has no scheduler for agent tasks; nothing is copied. Deviations from common practice,
on purpose:
- **D1** Event data never becomes an instruction: it is handed over as untrusted data beside the
  user's prompt, and a project's files cannot create tasks (behaviours 8, 20).
- **D2** Every limit is a hard cap that no setting can turn off: budget per run (unreported
  spending stops the run), runs per day, queue length, concurrency per resource with a default of
  1 (behaviours 9, 12, 14, 15).
- **D3** Missed fires are coalesced, so a machine that slept for a week does not start a week of
  runs (behaviour 5).
