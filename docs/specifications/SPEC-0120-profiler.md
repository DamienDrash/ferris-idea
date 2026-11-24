# SPEC-0120: Profiler

- **Version:** 5 · **Date:** 2026-11-19 (v5: costs per line in the editor's gutter, frames outside the
  project set apart and an "Only project code" filter, both also over MCP, behaviours 10–12, gaps P1 and P3 of
  OBS-0113); 2026-11-09 (v4: gaps from OBS-0029; v2: memory, attaching, Linux and Gradle moved to
  SPEC-0250–SPEC-0252; v3: hot spots per function, time as share of the run)
- **Sources:** GEN (IDEs run a program under a CPU profiler and show the result as call tree,
  hot spots and flame graph, with a jump to the source; F-083), STD (macOS `/usr/bin/sample`:
  `sample <pid> <seconds> <interval_ms> -mayDie -f <file>` and its report's "Call graph" section;
  Python's `cProfile` (`python -m cProfile -o <file>`) and `pstats.Stats(...).stats` with callers),
  SPEC-0047/SPEC-0087 (run configurations), SPEC-0053 (agent `run` right and log), ADR-0008,
  ADR-0009 (secondary text colour), OBS-0113 (side by side with the reference, gaps P1–P3; v5),
  STD (SHA-256, FIPS 180-4; v5)
- **Feature matrix rows:** F-083

## Purpose
Find out where a program spends its time without leaving the IDE and without installing a
profiler: run it once, see the expensive call paths and functions, and jump to their code.

## Behaviour
1. **Native programs (macOS):** the program is started and `/usr/bin/sample` attaches to it at
   once with one sample per millisecond until the program ends or the time limit passes. The
   report's call graph becomes one tree per thread; each frame keeps its function, library and,
   where debug information has it, source file and line. Self cost is a frame's samples minus its
   callees'. Elsewhere native programs are refused with the reason. [STD]
2. **Python programs:** a configuration whose program is a Python interpreter (`python`,
   `python3…`, `pypy3`) runs under `-m cProfile -o <file>` of that interpreter; interpreter options
   (`-u`, `-X dev`, `-W …`) stay in front, scripts and `-m` modules both work; `-c` and a call
   without script are refused. The same interpreter turns the statistics into JSON with `pstats`
   (run isolated, `-I`), so file format and Python version always match. Caller edges become a
   tree from the functions nobody called; the profiler's own frames are left out; recursion is
   not repeated below itself; built-ins read as `builtins.sum`. [STD]
3. **Profile:** engine, unit (samples, one per millisecond, or seconds), total cost, the roots
   with total, self cost, calls (cProfile) and callees sorted by cost, plus notes such as "the
   program ended before the sampler could attach". [GEN]
4. **Hot spots:** every function (by name and location) with its self cost summed over all call
   paths and its total counted once per stack, so recursion is not counted twice; sorted by self
   cost; functions without self cost are left out. [GEN]
5. **Profiling a run configuration:** `Profiler.ProfileSelected` profiles the selected
   configuration: its before-launch configurations run normally first, each once (a failing one
   stops with its name); then the configuration runs under its profiler with its own environment,
   working directory and time limit (the program is killed at the limit). Output goes to a log;
   the last 40 lines are kept with the run. Profiling runs in the background; a second request
   while one runs is refused. [GEN]
6. **Kept:** the run (command, exit code, time-limit flag, duration, output, profile) is stored
   as `.ferris-idea/profiles/last.json` with the raw profiler output next to it; opening the window
   shows the last stored profile. [GEN]
7. **Source:** Enter (or a double click) on a call tree row, a hot spot or a flame graph frame
   opens its file at its line: an existing absolute path as it is, else the workspace file at that
   relative path, else the only workspace file with that name (`sample` reports bare names);
   otherwise the status bar says the file is not in the workspace. [GEN]
8. **Window:** `Profiler.ToolWindow` opens "Profiler" at the bottom with three views — Call Tree,
   Hot Spots, Flame Graph — switched by Tab, `1`/`2`/`3` or clicking the view's name. A summary
   line names configuration, engine, duration, samples or measured time and how the program
   ended. The call tree opens the most expensive path down to the first function that spends
   most of its time itself; → opens, ← closes or goes to the caller; rows show total and self
   share and `file:line`. Hot spots show self share and time, total share and `file:line`: one row
   per function and file at its costliest line, the time being the share of the run's wall time
   for sampled profiles (v3, hand tests 47 and 62, D37: a C function came once per line, and JFR's
   samples were read as milliseconds). The
   flame graph draws callers above callees, width = share of the run, frames under 0.5 % and
   below level 32 left out, colours by function name. F5 profiles again; Escape and ⇧Esc hide
   it. [GEN]
9. **MCP:** `profile_run` (`name`, `dry_run` default true: before-launch names, engine, command,
   timeout) — a real run needs the user-granted `run` right, is logged in
   `.ferris-idea/agent-log.jsonl` and returns exit code, duration, notes, output, the 25 hottest
   functions and the call tree pruned to 1 % and 16 levels; `profile_last` (`min_percent`,
   `max_depth`) returns the stored profile the same way without running anything. [ADR-0008,
   SPEC-0053] **v5:** `profile_run`, `profile_last` and `profile_attach` (SPEC-0251) also take
   `only_project` (default false) and `file`. Every call tree node and every hot spot carries
   `in_project` (behaviour 11); with `only_project` the hot spots and the call tree are those of
   behaviour 12. The window's toggle does not apply to agents: they say what they want. With `file`
   (a workspace-relative path) the answer has `lines`: the file, `granularity` (`line`, or
   `function` for cProfile), `matches` (whether the file's text on disk still has the digest kept
   with the profile, behaviour 10) and `costs` sorted by line, each with line, function, total,
   self, both as percent of the run and, for CPU profiles, `time_ms` (the total as share of the
   run's wall time, as in the window). A file without costs answers an empty list; a path outside
   the workspace is refused. [ADR-0008]
10. **Costs per line (v5, OBS-0113 P1):** a line's total cost is what the samples (or measured
    time, or bytes) count whose stack has a frame at that line — once per sample, also when
    recursion puts the line on the stack twice; its self cost is what the innermost frame spends
    there. What each engine gives:
    - `sample` with debug information, JFR (SPEC-0136 v3), tracemalloc, `leaks`/`malloc_history`
      with debug information and Massif: per line;
    - cProfile: only the line of each function's `def`, with the function's total and self time
      (granularity `function`): cProfile does not record lines inside a function;
    - Linux `perf` (its script output has no lines), `heap` snapshots and frames without a line:
      nothing.

    **Gutter:** while a profile is shown in the Profiler window, every editor of a project file it
    has costs for shows them in a column at the left of the gutter, right of the Git annotations
    when those are shown too: the line's total as time (for sampled profiles its share of the run's
    wall time, like hot spots in behaviour 8) or as a size for memory profiles; lines without
    cost stay empty. The column's accessible name per line, also the hover text, says total and
    self as share and time and the function. A click on a line's cost shows the Profiler window
    with its Call Tree, opens the path to the costliest call of that line's function in that file
    and selects it (with "Only project code" on, in the folded tree), and moves the focus there.

    **When they go:** the annotations belong to the profile shown. They go when it is closed (the
    window's Close control, or Backspace in the window), and when another profile, or the error
    of a failed run, replaces it; hiding the window keeps them. Opening the window shows the stored
    profile (behaviour 6) and with it the annotations again.

    **When lines no longer match:** a profile is stored with the SHA-256 digest of each project
    source file it has costs for, read when the profile is stored. An editor shows a file's
    costs only while its text has exactly that digest: any edit — typed, reloaded from disk or
    written by an agent — hides them, and undoing back to the recorded text shows them again.
    Profiles stored before v5 have no digests and show no costs in the gutter. [GEN]
11. **Frames in the project (v5, OBS-0113 P3):** a frame is in the project when its reported
    source file resolves to one of the workspace's files as the project view lists them (without
    version control, Ferris IDEA, build output and dependency folders such as `target`,
    `node_modules`, `__pycache__`), and not inside a `site-packages` or `dist-packages` folder (a
    virtual environment in the project holds libraries): an absolute path of such a file (links
    resolved), a relative path that is one, a relative path with folders that is the end of
    exactly one such file's path (`demo/Busy.java`), or a bare file name that exactly one such
    file has (`sample` reports bare names). Everything else is outside: a
    frame without a file (a library function, a built-in, a native method), a file outside the
    workspace (the JDK, installed libraries, the Python standard library) and a name several
    workspace files share. Unlike the jump to the source (behaviour 7), a path with folders does
    not fall back to its bare name, so `java/util/Objects.java` is not taken for a project file
    named `Objects.java`. The marks are set when the profile is stored; a profile stored before v5
    gets them when it is read. In Call Tree, Hot Spots and Flame Graph the frames outside the
    project are set apart: their text in the secondary text colour (ADR-0009), their flame graph
    bars paler; the project's frames look as before. [GEN]
12. **Only project code (v5, OBS-0113 P3):** a check box "Only project code" in the window's header,
    also `P` in the window, filters the profile; it is stored per project in
    `.ferris-idea/profiles/view.json` (what counts as project code is a property of the project, so
    the choice is not a user-wide setting) and is off by default. When on, Hot Spots lists only
    the project's functions, with shares still of the whole run; Call Tree and Flame Graph show the
    folded tree: every frame outside the project is left out, its callees in the project move up to
    the nearest kept caller (merged with the same function there), and its own cost is added to
    that caller's self cost, so the totals of the kept frames do not change. Threads and the entry
    points of cProfile stay as roots. [GEN]

## Acceptance criteria
- **AK-1** `sample` report: threads, nesting by column, library, file and line, self samples, a
  report without call graph; cProfile JSON: tree from caller edges, profiler frames left out,
  recursion, built-in labels; self cost, hot spots with recursion, pruning; Python rewrite with
  options and `-m`, refusals; source lookup (unit tests in `crates/profiler`, `profiler_view::tests`).
- **AK-2** Real runs: a C program built with `cc -g` under `sample` has `inner` (busy.c) as the
  hottest function; a Python script under cProfile has `outer` at `app.py:4`; the time limit ends
  an endless script; a configuration with a before-launch step and an environment variable
  (`crates/profiler/tests/real.rs`).
- **AK-3** MCP: no profile yet, dry run, refusal without the `run` right, a real run with hot
  spots, `profile_last`, the log entry (`crates/mcp/tests/it/protocol.rs`:
  `profiles_a_python_configuration`).
- **AK-4** Window self-test: the selected Python configuration is profiled, the call tree opens
  `main`, Hot Spots lists the busy function first, Enter opens `prof.py` line 2, the flame graph
  draws `main` above `crunch`, Escape hides, the profile is kept.
- **AK-5** Hand test (round 4): profile a C/C++ and a Python configuration in a real project,
  read all three views and jump to the source; compare with the reference's profiler.
- **AK-6 (v5)** Model: line costs from stacks (recursion once, self at the innermost frame), from a
  `sample` tree and at cProfile's `def` lines; the rule of behaviour 11 for absolute, relative,
  path-end, bare and ambiguous names and frames without a file; the folded tree keeps totals and
  moves the cost of frames outside into their caller's self cost; hot spots carry `in_project`;
  digests and marks kept with a stored profile, marks set when an older one is read; the stored
  filter choice (unit tests `model::tests`, `run::tests` in `crates/profiler`).
- **AK-7 (v5)** Window model: the gutter label and hover text per unit, lines of a file found
  through the source rule; dimmed rows in all three views, the filter in Hot Spots and the folded
  call tree, the click on a line's cost selecting its function's costliest call, Close emptying
  the window (`profiler_view::tests`, `profile_lines_model::tests`).
- **AK-8 (v5)** MCP: `in_project` on hot spots and call tree nodes, `only_project`, `file` with
  `lines` and `matches` before and after the file changes, a path outside refused
  (`crates/mcp/tests/it/protocol.rs`: `profile_lines_and_project_code`).
- **AK-9 (v5)** Window test with real keys and a real JDK (`crates/workbench/tests/it/f083_handtests.rs`:
  `profile_costs_in_the_gutter_and_project_code`; skipped without `java`): after profiling a Java
  file, its editor shows costs at the hot loop's line and at the line that calls it, a click on
  the loop's cost selects `Rechen.teuer` in the Call Tree, JDK frames are dimmed and `P` leaves
  only project functions in Hot Spots (kept after the window is opened again), typing in the
  file hides the costs and undo shows them, Close removes them.

## Out of scope
Java profiling (now SPEC-0136, JDK Flight Recorder), memory and
allocation profiles (now SPEC-0250), attaching to an already running process (now SPEC-0251),
Linux `perf` and Gradle `run` (now SPEC-0252), Windows ETW (refused, SPEC-0252 behaviour 4),
comparing two profiles, a timeline per thread, events and the choice between CPU and total time
(gap P2 of the side-by-side check OBS-0113, which named them; v5 leaves them out), costs per line
for Linux `perf` and heap snapshots (behaviour 10), exporting to other formats.

## Known deviations from the reference
Profiles come from the operating system's sampler and Python's standard profiler instead of a
bundled agent, so nothing needs to be installed or granted, and a result is a readable JSON file
in the project that agents read through MCP (IMP-0043). Python is profiled deterministically
(cProfile) rather than by sampling, which adds overhead to call-heavy code. The process list is read
once when the picker opens and does not reorder under the pointer (IMP-0387). Compared with the reference's window in OBS-0029
from the specification and MCP; no profile recorded there on Ferris IDEA's side, Ferris IDEA's window not yet.
(v5) Compared with the reference's Java profiler in OBS-0113: like the reference, frames outside the
project are set apart rather than hidden; Ferris IDEA also filters them on request (behaviour 12), for
agents too. The costs next to the lines are hidden as soon as the file's text differs from the one
profiled, so a cost never stands next to a line it was not measured on (behaviour 10); whether the
reference keeps them after an edit was not observed. The flame graph keeps callers above callees.

## Gaps found side by side (OBS-0029, v4; planned, not built)
Open `.jfr` and `.hprof` files; timeline per thread; events; CPU versus total time; thread dump; live CPU and memory charts.
