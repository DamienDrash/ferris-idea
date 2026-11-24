# SPEC-0345: Agent benchmark (regression test of the harness)

- **Version:** 1 · **Date:** 2026-11-18
- **Sources:** `GEN` (agent benchmarks give an agent a task in a small project, check the result
  with tests instead of judging it, and report success, duration, tool calls and cost per agent
  and model; a recorded run replayed without a model keeps such a suite deterministic and free of
  credentials); ADR-0023 (Ferris IDEA as agent harness: MCP tools are the agent's hands, rule 7 safety
  rules); ADR-0008 (every feature has an MCP tool); SPEC-0053 (agent rights); SPEC-0278 (project
  trust); SPEC-0281 behaviour 5 (one-shot tool calls over an in-memory pipe). No observation of
  the reference: the reference has no agent benchmark.
- **Feature matrix rows:** none (ROADMAP M19.11, harness phase after 1.0; basis of M15; owner's
  order of 2026-11-18).

## Purpose
Ferris IDEA's MCP tools are what agents work with. A change to a tool can break agents without a test
noticing: an argument renamed, an answer shaped differently, a right asked for that was not
before. The benchmark holds tasks of its own, each a small project with a goal and deterministic
checks, and runs them through the real MCP server. In CI a run replays a recorded tool script, so
no model is called and no credentials are needed; the same tasks and report serve to compare
agents and models later.

## Behaviour

### Tasks
1. A task is a folder with `task.toml`, a `project/` folder (the starting state) and a
   `scripts/` folder of recorded runs. The repository keeps its tasks in
   `tests/agent-benchmark/<id>/`. `task.toml` has `id` (the folder's name; letters, digits, `-`),
   `title`, `prompt` (what an agent is told), `grants` (the agent rights of a run: any of `write`,
   `run`, `vcs-write`; default none), `max_tool_calls` (1 to 500, default 50), `max_seconds` (1 to
   3600, default 300) and one or more `[[check]]` tables. Unknown keys and values out of bounds are
   refused with the file named. [GEN]
2. A check is one of:
   - `file` with `contains` or `absent` (text the file must or must not hold) or `equals` (a file of
     the task folder the result must equal byte for byte); a missing file fails `contains` and
     `equals` and passes `absent`;
   - `tool` with `args` (a table), `pointer` (a JSON pointer into the answer) and `equals` (the
     value expected there): the tool is called on the result through a server with no rights, so
     a check never changes the result;
   - `command` (the program and its arguments) with `exit` (default 0): run in the result's folder
     with at most 60 seconds, a cleared environment but `PATH` and `LANG=C`, and `HOME` in the run's
     own temporary folder.
   A task passes when every check passes. [GEN]

### Recorded runs
3. A recorded run in `scripts/<name>.toml` has `agent` and `model` (names for the report; `model`
   is `none` for a hand-written script), an optional `cost_cents`, and `[[call]]` tables in order:
   `tool`, `args` (a table) and `expect` (`ok`, the default, or `error`). A run whose name starts
   with `fail` is a negative control: the checks must catch it. [GEN]
4. A run copies `project/` into a new temporary folder outside the repository and serves it with
   Ferris IDEA's MCP server over an in-memory pipe (SPEC-0281 behaviour 5), with exactly the task's
   grants, no grant file, and the server's user files (settings, keymap, history, plugins,
   recent projects, layouts, connectors) in the run's temporary folder, so a run never reads or
   changes the user's own. The calls are made one after the other in one MCP session. [ADR-0023
   rule 7, SPEC-0053]
5. A call whose outcome is not what `expect` says ends the run as failed at that step; the
   remaining calls are not made, the checks still run. The run also stops when it reaches
   `max_tool_calls` or `max_seconds`, and it then fails with the limit named. [GEN]
6. After the calls the checks run on the result; the temporary folder is removed afterwards. [GEN]

### Report
7. A run reports the task, the script, `agent`, `model`, `success`, `failed_step` (index and why)
   or none, `limit` or none, every check with `passed` and a short `detail`, `tool_calls`,
   `tool_errors`, `duration_ms`, `cost_cents` (from the script, or none when not known) and per
   call the tool, whether it answered `ok` and its milliseconds. Tool answers are not in the report,
   so it never carries file contents. [GEN, ADR-0023 rule 7]
8. A suite report lists the runs and per agent and model the number of runs, successes, the
   success rate, the total duration and the total cost when every run knows it; negative controls
   count apart, as `controls_caught` of `controls`. The report is JSON. [GEN]

### Running
9. `ferris-idea agent-bench [<tasks folder>] [--out <file>]` runs every recorded run of every task
   (default folder `tests/agent-benchmark`), writes the suite report to standard output or the
   file, and exits with 0 only when every run passes and every negative control fails. [GEN]
10. MCP `agent_benchmark` lists the tasks of `tests/agent-benchmark` in the workspace (id, title,
    grants, limits, number of checks, scripts) and, with `task`, runs its recorded runs (or the one
    named by `script`) and answers their reports. Running is a dry run by default that answers what
    would run; `dry_run=false` needs the `run` right and a trusted project, because the checks run
    the task's commands. [ADR-0008, SPEC-0053, SPEC-0278]
11. The benchmark's own test runs every task of the repository with its recorded runs, so a change
    that breaks a tool agents use fails CI. [GEN]

## Acceptance criteria
- **AK-1** `crates/mcp/src/agent_bench.rs` unit tests: `task_files_and_their_bounds`: a full
  `task.toml`, unknown keys, ids, grants and limits out of bounds refused; script files with
  `expect`.
- **AK-2** `crates/mcp/tests/it/agent_bench.rs`: `repository_tasks_pass`: every task under
  `tests/agent-benchmark` passes with its `solve` runs and its `fail` controls are caught; the
  repository's `project/` folders are unchanged afterwards.
- **AK-3** `agent_bench.rs`: `checks_and_steps`: each check kind passing and failing; a call with
  an unexpected outcome stops the run at that step; the tool-call limit; a tool that needs a right
  the task does not grant is refused; the report has no file content.
- **AK-4** `agent_bench.rs`: `suite_report_and_tool`: the suite summary per agent and model; MCP
  `agent_benchmark` lists the tasks, answers a dry run, and refuses a real run without `run`.

## Out of scope
Driving a live agent over ACP or as a terminal program (the agent host of ROADMAP M19.5 brings the
driver; the report and the checks stay); calling any model; recording a run from a live session
(M19.10 keeps the sessions); the check gates of M19.3 as a check kind; costs from the providers
(M19.7); comparing results over time (a history like SPEC-0260's).

## Known deviations from the reference
None: the reference has no agent benchmark.
