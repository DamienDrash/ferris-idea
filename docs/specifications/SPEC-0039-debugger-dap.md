# SPEC-0039: Debugger — Debug Adapter Protocol client and session model

- **Version:** 9 · **Date:** 2026-11-18 (v9: js-debug's own module entry pause, when it reaches the client, is continued, AK-17); 2026-11-17 (v8: Step Into a call in the arguments of another call with js-debug, AK-16, beta finding B4 of hand test 20); 2026-11-13 (v7: Go programs and tests under Delve; v6: a condition or log message the adapter fails to evaluate is told, also for java-debug and debugpy; what each adapter does with a failure, a hit count and a log message; v2: conditional breakpoints, hit counts, logpoints; v3: launching through MCP with SPEC-0054; v4: pending breakpoints and source maps, IMP-0030; v5: js-debug with child sessions)
- **Sources:** GEN (IDEs launch a program under a debugger, stop at line breakpoints, show threads, call
  stack, variables, evaluate expressions and step over/into/out; F-070), STD (Debug Adapter Protocol
  specification: base protocol with `Content-Length` framing, requests/responses/events, `initialize`,
  `launch`, `initialized`, `setBreakpoints`, `configurationDone`, `stopped`, `threads`, `stackTrace`,
  `scopes`, `variables`, `evaluate`, `continue`, `next`, `stepIn`, `stepOut`, `output`, `exited`,
  `terminated`, `disconnect`), adapter `lldb-dap` (LLVM) for the tests, OSS v6 (java-debug's event
  `usernotification` with `notificationType` and `message`; debugpy's request `setDebuggerProperty`
  with `skipPrintBreakpointException` and `skipSuspendOnBreakpointException`; both beside the
  protocol), own tests v6 (2026-11-13: lldb-dap of Xcode, debugpy 1.8.21, js-debug 1.95.3,
  java-debug 0.53.1, each with a loop and a breakpoint in it, driven through the MCP tools),
  OSS v7 (Delve's public
  documentation of `dlv dap`: it serves one client on `--listen`, a TCP address or `unix:<path>`; the
  launch attributes `mode` `debug`/`test`, `program`, `dlvCwd`, `cwd`, `args`, `env`, `buildFlags`,
  `output`, `outputMode`, `substitutePath`, `stopOnEntry`; goroutines as DAP threads; tried with Delve
  1.27.2 and Go 1.27 on macOS), GEN v7 (`go run`, `go test` and the test binary's `-test.*` flags)
- **Feature matrix rows:** F-070, F-071

## Purpose
Drive any debug adapter so the IDE can debug programs in every language that has one.

## Behaviour
1. The client starts an adapter process and talks to it over stdin/stdout with `Content-Length` framed JSON.
   Requests carry increasing sequence numbers; responses are matched by `request_seq`; events are queued
   until asked for; requests from the adapter (reverse requests such as `runInTerminal`) are answered with a
   failure, since the IDE runs programs itself (later step). A failed response is an error with the
   adapter's message; every wait has a timeout. [STD]
2. A session initialises the adapter (client `ferris-idea`, 1-based lines and columns, paths as file paths),
   sends `launch` with the adapter-specific arguments, waits for `initialized`, sets the line breakpoints
   per source file, sends `configurationDone` and then expects the `launch` response — so it works for
   adapters that answer `launch` before or after configuration. [STD]
3. Breakpoints are set per file as a full list; each has a line and optionally a condition (stop only when
   the expression is true), a hit condition (stop only after so many hits) or a log message (log with `{expr}`
   parts evaluated instead of stopping). A feature the adapter did not announce in its capabilities
   (`supportsConditionalBreakpoints`, `supportsHitConditionalBreakpoints`, `supportsLogPoints`) is refused
   with a message instead of being sent, because adapters may ignore it and stop every time. The answer tells
   which breakpoints are verified and on which line they ended up. Logpoint messages and adapter notes are
   collected as debug console output, separate from program output. [STD, improvement IMP-0022]
3a. **Failures are told (v6):** a condition or a log message that cannot be evaluated must not
   pass without a word (IMP-0022). What the adapters do, and what Ferris IDEA adds:
   - **lldb-dap** stops at every hit and writes the error to the debug console. Nothing added.
   - **java-debug** stops at every hit for a failed condition and writes nothing for a failed log
     message; it says both only in an event of its own, `usernotification`. Ferris IDEA writes each
     such message to the debug console on a line of its own. Before, a condition java-debug could
     not evaluate made the breakpoint stop every time with nothing said.
   - **debugpy** by itself goes on after any failed condition and says nothing at all when the
     failure is a `NameError` (a misspelt variable). Ferris IDEA asks it, before the breakpoints are
     set, to report every failure and to stop at that hit (`setDebuggerProperty` with both skip
     lists empty); the error then stands in the debug console and the program stops there, as for
     conditions Ferris IDEA evaluates itself (SPEC-0178 behaviour 5). A debugpy that does not know
     the request keeps its own way.
   - **js-debug** refuses a condition with a syntax error when the breakpoint is set: the
     breakpoint is never bound and is listed as pending, the error comes as program error output.
     A condition that fails when it runs is written as program error output at every hit and the
     program goes on. Nothing added: these are the adapter's messages, and Ferris IDEA does not know
     which output line is one.
   What stays the adapters' own and differs between them, as tested: a hit count `3` stops only
   on the third hit with debugpy, js-debug and java-debug, and on the third and every later hit
   with lldb-dap; the text of a log message comes as debug console output from lldb-dap and
   java-debug, and as program output from debugpy and js-debug, so there it stands in the
   program's output and not in the console of behaviour 3; a log message whose expression fails
   is written with the error in its place and the program goes on (lldb-dap, debugpy, js-debug). A breakpoint on a
   line without code is moved to a line with code and reported there with the line asked for
   (lldb-dap and js-debug to a later line, debugpy to an earlier one; debugpy moves one past the
   end of the file to the last line), or left unbound and listed as pending (java-debug; lldb-dap
   and js-debug past the end). [STD, OSS, own tests]
4. When the program stops, the session reports the reason (breakpoint, step, entry, exception, pause) and
   thread; threads, the call stack (frame name, file, line), the scopes and variables of a frame (name,
   value, type, child reference for structured values) and expression evaluation in a frame are
   available. [STD]
5. Continue, step over, step into and step out act on a thread and wait for the next stop or the end. Program
   output (`output` events) is collected; the end reports the exit code. Disconnect ends the program and the
   adapter. [STD]
6. Known adapters are detected: `lldb-dap` (C, C++, Rust, Swift; on the PATH or through `xcrun` on macOS),
   `dlv dap` (Go; v7: `FERRIS_IDEA_DLV`, else `dlv` on the PATH, else where `go install` puts it: `$GOBIN`,
   `$GOPATH/bin`, `~/go/bin`), `debugpy` (Python, `python3 -m debugpy.adapter`). [GEN]
7. MCP tool `debug_adapters` listing the known adapters, their languages, command and whether they are
   installed. Launching and stepping through MCP: SPEC-0054. [ADR-0008]
8. Breakpoints the adapter reports as unverified are tracked through its `breakpoint` events (macOS
   lldb-dap verifies only once the program is loaded; Linux lldb-dap at once). After the first stop or the
   end, breakpoints still not bound are reported with the requested line and a hint (file missing, path
   reached through a symbolic link, no code at the line or a different build path). A launch may pass a
   source map (`[as built, as on disk]` pairs) so breakpoints bind in programs built through a symbolic
   link. No mapping is added automatically: a default `/var` → `/private/var` map on macOS made
   breakpoints in normally built programs stop binding (tested). [STD, improvement IMP-0030]
9. Adapters may also listen on a TCP port (java-debug in jdtls, js-debug); the client connects instead of
   starting a process. [STD]
10. **js-debug (v5):** `dapDebugServer.js` of the js-debug release (`FERRIS_IDEA_JS_DEBUG`, else the newest
   `~/.local/opt/js-debug*`) runs under `node` on a free local port. The session connects, announces
   `supportsStartDebuggingRequest` (only to js-debug), launches `pwa-node` and answers js-debug's
   `startDebugging` reverse request with success; the child configuration it carries is launched on a
   second connection, which sets the breakpoints and is the session that stops, steps and evaluates. The
   first connection and the server live as long as the session. Other reverse requests are still refused.
   The MCP tool `debug_start` (SPEC-0054) uses js-debug for `.js`, `.mjs` and `.cjs` programs.
   [STD (DAP `startDebugging`), OSS (js-debug's DAP server)]
   (v8) Step Into is DAP `stepIn` with the stopped thread only, no `targetId` (that is optional
   and only for Smart Step Into, SPEC-0091 v11 behaviour 18); on a line such as
   `console.log("result", result, helper(1))` js-debug stops in `helper`, whose call is evaluated
   before `console.log`'s, from a stop at a breakpoint on the line and after Step Over onto it.
   [STD (DAP `stepIn`); beta screen run of workflow (a), finding B4]
   (v9) **js-debug's own entry pause is not the program's stop.** js-debug sets an internal
   breakpoint at line 1, column 1 of every file that has breakpoints and resumes from it by itself;
   the runtime binds it to the first place it can stop, for a file that opens with a function
   declaration that function's first statement. Under load js-debug can report that pause to the
   client: as a `stopped` event with reason `breakpoint` and no (or an empty) `hitBreakpointIds`, or
   with reason `pause` and the description "Paused on debugger statement" without hit breakpoints.
   A js-debug session continues such a stop instead of reporting it when all of these hold: Ferris IDEA
   did not ask for a pause; no breakpoint of the session (requested or placed by the adapter) is on
   the top frame's file and line; and that source line can be read and has no `debugger` statement
   (an unreadable line counts as having one, so the stop is reported). Steps, exceptions, function
   breakpoints, stops that hit a breakpoint and pauses Ferris IDEA asked for are reported as before.
   Other adapters are not affected. [OSS (js-debug's DAP server, its handling of paused events);
   own trace 2026-11-18 with js-debug 1.95.3 and 1.140.1: the runtime paused at the first statement
   of the function on line 2 with the entry breakpoint as the hit breakpoint]
11. **Go under Delve (v7):** a `go run` or `go test` command is debugged with `dlv dap`; Delve builds
   the program itself, without optimisations. [OSS, GEN]
   - **Command.** `go run [build flags] <file or package> [arguments]` debugs that file or package
     (`mode: debug`) with the arguments; `go test [flags] [package]` debugs the tests of one package
     (`mode: test`): `-run`, `-count`, `-short`, `-timeout` and the other test flags become the test
     binary's `-test.…` flags, `-json` and `-v` its verbose output, what follows `-args` is passed on
     as it is, and build flags (`-race`, `-tags …`, `-mod=…`) stay build flags for both. A test
     function's ▶ marker (SPEC-0045) therefore debugs that function alone (`-test.run ^Name$`).
   - **Refused with the reason, before anything starts:** another `go` subcommand, several files,
     several packages or a `...` pattern (Delve builds one package), flags that move the build or
     its result (`-C`, `-o`, `-exec`, `-c`, `-n`), and a `go test` flag that is not known. A `go`
     command is never debugged as the native program `go`.
   - **Where it builds and runs.** Delve builds in the package's folder with the package named
     relative to it, so the build does not depend on how the folder is reached. A program runs in
     the configuration's working directory, tests run in their package's folder, as `go test` runs
     them. The built program is written into the session's private folder, not into the project,
     and is removed with the session.
   - **Server.** Delve serves the session on a Unix socket in a new folder of mode 0700 below the
     temporary folder (in `/tmp` when the path would not fit a socket address), which other users
     cannot reach and which is removed with the session; no TCP port is opened. Where there are no
     such sockets (Windows) it listens on the loopback address on a port it picks itself
     (`--listen=127.0.0.1:0`) and names it. It is started only for a trusted project (SPEC-0278),
     in a process group of its own, and ends with the session together with the program it
     started, by process id, also when Ferris IDEA is killed (SPEC-0263 behaviour 3).
   - **Start.** Building can take longer than a request: the session waits up to two minutes for
     `initialized`. A launch that fails before that (the program does not build) ends at once with
     Delve's message and what the compiler wrote, not with a timeout; this holds for every adapter.
   - **Symbolic links.** The go tool names sources by their resolved path. When the package's
     folder is reached through symbolic links, each link on the way is given to Delve as a path
     substitution (as opened → as resolved), so breakpoints bind and stops name the files as the
     project names them. This does not contradict behaviour 8: the mapping is not a guess, it is
     read from the folder Delve builds in.
   - **Session.** Goroutines are the threads. Conditions, hit counts and log messages are
     evaluated by Delve (SPEC-0127). The program's output arrives as output events; what Delve says
     while building and a logpoint's message, which Delve sends as program output naming its source
     line, are debug console text.
   - **Remote projects** (SPEC-0322): the same launch request with host paths; the host starts
     Delve on its private socket. The built program is then written beside the package under
     Delve's own temporary name and removed by Delve at the end.

## Acceptance criteria
- **AK-1** Framing: write and read messages, split and joined reads, invalid header.
- **AK-2** With `lldb-dap` and a C program built with `-g`: a breakpoint is verified and hit; the stack
  shows `main` at the breakpoint line; locals contain the expected value; evaluation computes an
  expression; step into enters the called function, step out returns, step over advances; continue runs to
  the end with the expected exit code and program output. (Skipped without `lldb-dap` or a C compiler.)
- **AK-3** A failed request (evaluating an unknown name) returns the adapter's error message; a request the
  adapter leaves unanswered ends with a timeout error.
- **AK-4** The MCP tool lists the adapters with availability.
- **AK-15** A program compiled with its absolute source path through a symbolic link: the breakpoint on
  the resolved path is reported as pending with a symbolic-link hint at the end; with a source map from
  the link to the real folder it binds and stops on the line.
- **AK-5** With `lldb-dap`: a condition `i == 3` in a loop stops once with `i` = 3; a hit condition 3 first
  stops at the third iteration; a logpoint logs every iteration without stopping. Unsupported features are
  refused (unit test with capabilities). (Adapter part skipped without `lldb-dap` or unattended debugging.)

- **AK-10 (v5)** With node and js-debug: a script stops at a breakpoint in a function through the child
  session with its locals (`doubled = 42`), evaluates `n + 1`, and runs to its end with its output
  (`crates/debugger/tests/it/jsdebug.rs`; skipped without them).
- **AK-16 (v8, behaviour 10)** With node and js-debug: stopped at a breakpoint on
  `console.log("result", result, helper(1))`, Step Into stops in `helper`
  (`crates/debugger/tests/it/jsdebug.rs::step_into_an_argument_call_stops_in_it`); in the harness
  window, with F7, after Step Over onto the line and from a breakpoint on it
  (`workflow_a_beta_handtests.rs::hand_test_20_debug_node`).
- **AK-17 (v9, behaviour 10)** A js-debug `stopped` event with reason `breakpoint` and no hit
  breakpoints, or with reason `pause` and "Paused on debugger statement", is taken for js-debug's
  entry pause only on a line with no breakpoint of the session and no `debugger` statement, and
  never when the line cannot be read; a stop that hit a breakpoint, a pause asked for, a step, an
  exception and a function breakpoint are never taken for it
  (`crates/debugger/src/session.rs::tests::js_debug_entry_pause_is_told_apart`).
- **AK-11 (v6)** Against a scripted adapter (`crates/debugger/tests/it/condition_errors.rs`): a
  `usernotification` reaches the console on a line of its own; an adapter with the id `debugpy`
  gets `setDebuggerProperty` with both lists empty before `setBreakpoints`, another adapter does
  not. With real debugpy: a condition naming an unknown variable stops at the first hit with the
  `NameError` in the console (`a_failed_condition_stops_and_is_told_with_debugpy`; skipped
  without debugpy). java-debug's messages with the real adapter: by hand through MCP on the Mac,
  2026-11-13 (`Breakpoint condition 'i == == 3' error: …` in the console).
- **AK-12 (v7)** With `dlv` and `go` (`crates/debugger/tests/it/delve.rs`, skipped without them or
  without unattended debugging): a program in a folder behind a symbolic link stops in its loop
  where the condition `i == 3` holds, with its locals and a struct value, evaluates
  `total * 2 + i`, lists its goroutines as threads, stops once at a hit count, writes a logpoint's
  five messages to the console and not to the output, and ends with exit code 0; afterwards Delve
  and its socket folder are gone and the project folder holds nothing new.
- **AK-13 (v7)** Same file: `go test -json -run ^TestAdd$ ./calc` stops in the function under
  test below `TestAdd` with its arguments, steps over a line, ends with `--- PASS: TestAdd`, and
  the package's other test did not run.
- **AK-14 (v7)** Same file: a program that does not build is refused within the build's time with
  the compiler's message. Unit tests: the commands (`delve::tests::go_run_commands`,
  `go_test_commands`), the launch request (`go_launch_requests`), the link mapping
  (`symbolic_links_map_by_their_prefix`), the plan of a run configuration
  (`debug_plan::tests::go_commands_are_planned_for_delve`).

## Out of scope
The debugger UI (M3.7), attach to running processes, exception breakpoints, watches, memory and disassembly views, run configurations, `runInTerminal`, multiple
sessions.

## Known deviations from the reference
Not yet compared with the reference debugger (its Java debugger uses JDWP, not DAP; DAP covers the other
languages).

**(v8) Finding B4 of the beta screen run (2026-11-17):** in the reference-style run, F7 on the
`console.log` line was reported to run the program to its end. It was not reproduced: the DAP
request, the debug model and the window (key F7, the editor focused) step into `helper` in both
paths of AK-16, in the debug build of the same `main` with #927. The step is sent as DAP defines
it, so nothing of Ferris IDEA's was found to change; when it is seen again, the program and the
steps before F7 are to be recorded with it.

**(v7) Delve's limits, stated as deviations:** it builds exactly one package per session, so a
configuration over several packages (`go test ./...`) is refused with the reason instead of
debugged. `go test -json` is not reproduced: the test binary runs directly with its verbose
output, so the result tree of SPEC-0045 is not filled by a debug session. The first build of a
program compiles the standard library again without optimisations and can take a minute. A
logpoint's line carries Delve's goroutine prefix (`> [Go 1]: …`). On macOS Delve needs Developer
Mode, as lldb-dap does. The built program is kept out of the project folder (Delve's default
writes it beside the package); how the reference handles this was not observed.
