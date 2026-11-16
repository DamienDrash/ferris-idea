# SPEC-0120: Profiler

- **Version:** 4 · **Date:** 2026-11-09 (v4: gaps from OBS-0029; v2: memory, attaching, Linux and Gradle moved to
  SPEC-0250–SPEC-0252; v3: hot spots per function, time as share of the run)
- **Sources:** GEN (IDEs run a program under a CPU profiler and show the result as call tree,
  hot spots and flame graph, with a jump to the source; F-083), STD (macOS `/usr/bin/sample`:
  `sample <pid> <seconds> <interval_ms> -mayDie -f <file>` and its report's "Call graph" section;
  Python's `cProfile` (`python -m cProfile -o <file>`) and `pstats.Stats(...).stats` with callers),
  SPEC-0047/SPEC-0087 (run configurations), SPEC-0053 (agent `run` right and log), ADR-0008
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
   as `.idecraft/profiles/last.json` with the raw profiler output next to it; opening the window
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
   `.idecraft/agent-log.jsonl` and returns exit code, duration, notes, output, the 25 hottest
   functions and the call tree pruned to 1 % and 16 levels; `profile_last` (`min_percent`,
   `max_depth`) returns the stored profile the same way without running anything. [ADR-0008,
   SPEC-0053]

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

## Out of scope
Java profiling (now SPEC-0136, JDK Flight Recorder), memory and
allocation profiles (now SPEC-0250), attaching to an already running process (now SPEC-0251),
Linux `perf` and Gradle `run` (now SPEC-0252), Windows ETW (refused, SPEC-0252 behaviour 4),
comparing two profiles, a timeline per thread, line-level costs, exporting to other formats.

## Known deviations from the reference
Profiles come from the operating system's sampler and Python's standard profiler instead of a
bundled agent, so nothing needs to be installed or granted, and a result is a readable JSON file
in the project that agents read through MCP (IMP-0043). Python is profiled deterministically
(cProfile) rather than by sampling, which adds overhead to call-heavy code. The process list is read
once when the picker opens and does not reorder under the pointer (IMP-0387). Compared with the reference's window in OBS-0029
from the specification and MCP; no profile recorded there on IDECraft's side, IDECraft's window not yet.

## Gaps found side by side (OBS-0029, v4; planned, not built)
Open `.jfr` and `.hprof` files; timeline per thread; events; CPU versus total time; thread dump; live CPU and memory charts.
