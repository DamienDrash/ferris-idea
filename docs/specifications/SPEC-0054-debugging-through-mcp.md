# SPEC-0054: Debugging through MCP

- **Version:** 5 · **Date:** 2026-11-16 (v5: `debug_sessions` lists the open sessions, behaviour 10, AK-5, OBS-0071 G5; deviation IMP-0446); 2026-11-13 (v4: Go programs and tests with Delve; v3: Python scripts through debugpy, waiting for a program that still runs, inspecting is logged; v2: pending breakpoints in the state, `source_map`)
- **Sources:** GEN (agents set breakpoints, run to them, step and inspect variables; F-070, F-071), STD
  (Debug Adapter Protocol: `launch`, `setBreakpoints`, `stopped`/`exited`/`terminated` events,
  `stackTrace`, `scopes`, `variables`, `evaluate`, `continue`/`next`/`stepIn`/`stepOut`, `disconnect`),
  SPEC-0039 (DAP client and session), SPEC-0053 (rights and log), the project owner's decision on PO-STATUS question 3
- **Feature matrix rows:** F-070, F-071

## Purpose
Let an agent debug a native program the way a developer does in the IDE — breakpoints, stepping,
variables, expressions — under the user's `debug` right.

## Behaviour
1. `debug_start` takes a workspace-relative program, arguments, working directory, breakpoints (file,
   line, condition, hit condition, log message) and whether to stop on entry. The dry run (default) returns
   the launch request and the breakpoints; a real start needs the `debug` right, refuses breakpoint features
   the adapter does not announce (IMP-0022), and is logged. Adapter: lldb-dap (C, C++, Rust, Swift).
   [STD, SPEC-0053]
2. Preconditions are reported before anything starts: no adapter installed, or on macOS Developer Mode off
   (otherwise the start would hang on a password dialog), or already 4 open sessions. [GEN]
3. After starting or resuming, the tool waits (default 30 s, at most 300 s) and returns the state:
   `stopped` with reason, thread, the top 20 frames and the local variables of the top frame; `exited`
   with the exit code; or `running` when the wait ended first. Stopped and exited states list breakpoints
   that are still not bound, with a hint (SPEC-0039 behaviour 8); `debug_start` accepts a `source_map`. Program output and debug console text
   (logpoint messages) received so far are returned with it. [STD]
4. `debug_resume` (`continue`, `next`, `step_in`, `step_out` on the thread that stopped last),
   `debug_evaluate` (expression in a frame, default the top frame; logged because it can change the
   program), `debug_variables` (children of a structured value). These need the `debug` right. [STD]
5. `debug_stop` terminates the program and the adapter and returns the output; it needs no right, so
   an agent can always clean up. [GEN]
6. **Python (v3):** a program ending in `.py` is started with debugpy (`type` `python`, the
   internal console, only the project's own code) and the workspace's selected interpreter, as
   the window's Debug action does (SPEC-0091 v2); Developer Mode is not needed for it. Before,
   `debug_start` handed a Python script to lldb-dap, which refused it as not executable, and an
   agent could reach Python only by attaching. [STD, GEN]
7. **A program that still runs (v3):** when a wait ends first the state is `running` and no
   thread is stopped; `debug_resume` with the action `wait` then resumes nothing and waits again
   for the next stop or the end, with the same answer. It is refused while the program is
   stopped. The refusal of the other actions for a running program names `wait` and
   `debug_pause`. Before, pausing was the only way to hear of a later breakpoint hit. [GEN]
8. **What runs code in the program (v3):** only what an agent asks for explicitly —
   `debug_evaluate`, `debug_inspect` (it evaluates its expression too, also for a view),
   `debug_set_variable`, and the conditions and log messages of the breakpoints the agent set.
   Each `debug_evaluate`, `debug_inspect` and `debug_set_variable` is written to the agent log
   before it runs. For the stack, the variables and their children IDECraft sends no `evaluate`
   (SPEC-0091 v16 behaviour 42). One thing still runs code of the program without being asked
   for: java-debug calls an object's own `toString()` when IDECraft fetches the text of a Java
   row it marked lazy, for up to 30 rows of an answer (SPEC-0091 v11 behaviour 17); the state of
   a stop and `debug_variables` contain these texts. [GEN]
9. **Go (v4):** `debug_start` debugs a `.go` file or a package folder (one with a `go.mod` or Go
   files) with Delve as SPEC-0039 v7 behaviour 11 describes; `args` are the program's arguments.
   `go_test` debugs the tests of that package instead: a test function's name runs only that one
   (`-test.run ^Name$`; a text with other characters is go's own pattern), an empty text all of
   them, and `args` then go to the test binary. A `_test.go` file as `program` means its package's
   tests. A `source_map` becomes Delve's path
   substitution. The dry run returns Delve's launch request. The preconditions of behaviour 2 hold (Delve
   installed, Developer Mode on macOS), the project must be trusted, and breakpoint conditions, hit
   counts and log messages are Delve's. `debug_attach` takes `runtime: go` (SPEC-0147 v3).
   [SPEC-0039, ADR-0008]
10. **The open sessions (v5):** `debug_sessions` lists the debug sessions the server holds, oldest
    first, each with `session` (the number the other debug tools take), `stopped` (true with the
    `thread` that stopped last; false while the program runs or after it ended, which the next
    `debug_resume` with `wait` tells apart), `adapter` (the debug adapter's id, e.g. `lldb-dap`,
    `debugpy`) and `attached` (attached to a program that ran before, SPEC-0147), plus `limit`,
    the number of sessions that may be open at once. It talks to no adapter, changes nothing and needs
    no right, so an agent that lost a session number can find it and stop it. A server knows only the
    sessions started through it: over stdio each `idecraft call` is a server of its own, the window's
    HTTP server keeps them while it runs. [GEN; OBS-0071 G5]

## Acceptance criteria
- **AK-1** Without the `debug` right: the dry run returns the launch request; start, resume, evaluate and
  variables are refused naming the right; stopping an unknown session is an error; a program outside the
  workspace is refused.
- **AK-2** With lldb-dap and a real C program (Linux CI; on the Mac after Developer Mode is on): stop at a
  breakpoint on the right line with local `x = 42`, `x * 2` evaluates to 84, step into `add` and out,
  continue to the end with exit code 6 and output `y=46`, a logpoint in a loop writes `i=0` … `i=2` to the
  console, stop the session; start and evaluation are in the agent log.
- **AK-3 (v3)** The dry run of a `.py` program names debugpy and a `python` launch; `debug_breakpoints`
  and the action `wait` need the `debug` right (`debugging_needs_the_debug_right`). With real
  debugpy: started by `debug_start`, stopped at its breakpoint, `running` after a short wait,
  `wait` reports the next breakpoint hit and is refused while stopped
  (`python_breakpoints_change_while_the_program_runs`; skipped without debugpy).
- **AK-4 (v4)** `protocol.rs`: `debug_start_go_dry_runs` — the launch requests of a file, a package
  folder, a test function and a `_test.go` file, without Delve; `debug_a_go_program_through_mcp`
  (skipped without `dlv` or `go`) — a breakpoint with a condition in a loop stops with the
  counter, an evaluation answers, a test function stops in the code under test, `debug_stop` ends
  both.
- **AK-5 (v5)** `protocol.rs`: `debug_sessions_lists_nothing_without_a_session` — without any right
  the list is empty with `limit` 4; `debug_a_program_through_mcp` (skipped without lldb-dap) — after
  the start one session, stopped with a thread and adapter `lldb-dap`, and none after `debug_stop`.

## Out of scope
Starting Java programs (the window does; agents attach to a JDWP port, SPEC-0147), watch
expressions as a list kept by the server (an agent evaluates what it wants to watch after each
stop), data breakpoints.
Later specifications added attaching (SPEC-0147), js-debug (SPEC-0039 v5), Kotlin (SPEC-0132),
changing variables (SPEC-0194) and threads (SPEC-0091 v12).

## Known deviations from the reference
Not compared side by side; rights and logging per SPEC-0053 (IMP-0028).

v5 (OBS-0071, without a session): the reference lists its open debug sessions like `debug_sessions`
does. It listed four exception breakpoints under one and the same id, so none could be addressed on
its own, and its error for a missing session named a tool its list does not have. In IDECraft saved
line breakpoints are keyed by file and line and the exception settings per language, and messages name
tools by the names `tools/list` gives (IMP-0446).
