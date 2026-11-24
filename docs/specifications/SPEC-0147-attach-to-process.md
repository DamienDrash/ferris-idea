# SPEC-0147: Attach to Process

- **Version:** 3 · **Date:** 2026-11-13 (v3: Go programs with Delve; v2: the project is named to java-debug when attaching, so conditions, log messages and expressions are evaluated; exception and method breakpoints for `debug_attach`)
- **Sources:** GEN (IDE debuggers attach to a program that already runs: a local process chosen
  from a list, or a debug port such as a JVM's JDWP agent; detaching leaves the program
  running; F-070), OSS (action id `XDebugger.AttachToProcess` with ⌥⇧F5 in the macOS keymap and
  Ctrl+Alt+F5 in the default keymap, `JetBrains/intellij-community`), STD (Debug Adapter
  Protocol `attach` request and `disconnect` with `terminateDebuggee: false`; adapter arguments:
  lldb-dap `pid`, `gdb-remote-port`/`gdb-remote-hostname`; debugpy `processId` and `connect`, and
  `debugpy --listen` serving the protocol itself; js-debug `pwa-node` attach with `processId` or
  `address`/`port` of `node --inspect`; java-debug `hostName`/`port` of a JDWP agent started with
  `-agentlib:jdwp=…,server=y,address=…`), SPEC-0039, SPEC-0054, SPEC-0091, SPEC-0146, IMP-0048,
  ADR-0008, OSS v2 (java-debug's attach argument `projectName` and jdtls's
  `vscode.java.resolveMainClass`; tried 2026-11-13 with java-debug 0.53.1: without the name every
  evaluation is refused)
- **Feature matrix rows:** F-070

## Purpose
Debug a server, a script or a native program that was started outside the IDE, without
restarting it, and leave it running afterwards.

## Behaviour
1. **Dialog:** Run › Attach to Process… (⌥⇧F5, Ctrl+Alt+F5) lists the processes of this user
   except the IDE itself: runtime (Native, Python, Node.js, Java), name, process id, debug port
   and command line. Processes whose command line opens a debug port come first, then the rest
   by name. Typing filters by name, command line or exact process id; ↑/↓ select, Enter or a
   double click attaches, Escape closes. [GEN, OSS]
2. **Runtime and port:** the runtime follows the executable (`java`, `node`, `python…`, anything
   else native). The debug port is read from the command line: `-agentlib:jdwp=…server=y…address=
   [host:]port`, `node --inspect[=…]`/`--inspect-brk`/`--inspect-wait` (default 9229),
   `python -m debugpy --listen [host:]port`. Typing `host:port` or `:port` adds one row per
   runtime for that address above the processes (JDWP, `debugpy --listen`, `node --inspect`,
   gdb-remote server). [STD]
3. **Attaching:** a process with a debug port is attached through that port, otherwise through
   its process id: native with lldb-dap (`pid`; an address as gdb-remote), Python with debugpy
   (`processId`; a `debugpy --listen` port is talked to directly, without starting an adapter),
   Node.js with js-debug (`processId` or `address`/`port`), Java with java-debug in jdtls (JDWP
   port only). The workspace's line, exception and method breakpoints are set right after
   attaching; the Debug window opens with "Attaching to …" and then behaves as for a started
   program. [STD]
3a. **Java project (v2):** java-debug evaluates nothing in an attached JVM unless the attach
   request names the jdtls project: every expression is refused, a breakpoint with a condition
   stops at every hit and a log message writes nothing. Ferris IDEA therefore opens the sources that
   have breakpoints in the debugger's jdtls, asks it for the main classes of the workspace
   (at most 20 seconds while it imports) and names the project of a main class declared in one of
   these sources, else the one project all main classes are in. With several projects and no
   source to choose by, none is named; what java-debug then cannot evaluate is said in the debug
   console (SPEC-0039 v6 behaviour 3a). [OSS]
4. **Refusals:** a JVM without a JDWP agent cannot be selected; Enter says how to start it with
   an agent and to type its port. A missing adapter or, on macOS, missing Developer Mode for
   lldb-dap is shown in the status line as for Debug. Browser pages are not attached to. [GEN]
5. **Detaching:** Stop (⌘F2) on an attached session detaches (`terminateDebuggee: false`); the
   Debug window says "Detached; the program goes on running" and the program continues. A
   started program still ends on Stop. [STD]
6. **MCP:** `debug_processes` lists the processes as in behaviour 1 (filter, limit; read-only).
   `debug_attach` takes `runtime`, `pid` or `port` (with `host`), breakpoints and a wait; it is a
   dry run by default that returns the attach request, and with `dry_run: false` needs the
   `debug` right, is logged in `.ferris-idea/agent-log.jsonl` and returns a session for
   `debug_resume`, `debug_variables`, `debug_evaluate`. `debug_stop` says `detached: true` for
   it. [ADR-0008] **(v2)** `debug_attach` also takes `exception_breakpoints`, `exception_types`
   and `function_breakpoints` as `debug_start` does (SPEC-0146 behaviour 6), shows them in its
   dry run as `special`, and leaves out a breakpoint given with `enabled: false`.
7. **Go (v3):** runtime Go attaches with Delve. By process id Delve is started as in SPEC-0039 v7
   behaviour 11 (private socket, ends with the session) and attaches to the process
   (`mode: local`); Stop detaches and the program goes on. A Delve server (`dlv … --headless
   --listen=…`) is listed as a Go process with the port of its command line and is talked to
   directly, without starting an adapter (`mode: remote`); typing `:port` offers a Go row beside
   the others. What Stop does to the program there is the server's doing: a server started without
   `--accept-multiclient` ends when its client leaves, together with the program it started; this
   is said in the debug console right after attaching. Such a port is neither encrypted nor authenticated, so only one on this machine's
   loopback is accepted; another host is refused with the reason, in the dialog the Go row is not
   offered for it. A Go program cannot be told from another native program by its command line:
   it is listed as Native, and Delve is chosen through `debug_attach` with `runtime: go` or
   through a Delve server's row. On macOS attaching by process id needs Developer Mode. In a
   remote project Go is not attached to (refused with the reason); its programs are started under
   Delve there. [OSS: Delve's public documentation, ADR-0025]

## Acceptance criteria
- **AK-1** Runtime and port detection, targets, attach arguments per adapter
  (`attach::tests::runtimes_and_debug_ports`, `targets_of_processes`, `attach_requests_per_adapter`,
  `this_process_is_not_listed`); dialog rows and filter (`attach_view::tests`).
- **AK-2** Live: a running C loop attached by process id with lldb-dap, a Node.js loop through
  its `--inspect` port with js-debug, a Python loop under `debugpy --listen`; each stops at a
  breakpoint in the loop, evaluates its counter, detaches, and the program still runs
  (`crates/debugger/tests/it/attach.rs`).
- **AK-3** MCP: `debug_processes` without the own process, `debug_attach` dry runs (Java port,
  native pid), refusals (Java pid, unknown runtime, pid and port together, missing right), and a
  live attach to `debugpy --listen` that `debug_stop` detaches (`attach_lists_processes_and_detaches`).
- **AK-6 (v2)** Which project is named (`java_debug::tests::the_project_for_attaching_comes_from_the_sources_or_is_the_only_one`);
  the `special` of a `debug_attach` dry run (`debug_start_takes_the_saved_breakpoints`). Live with
  jdtls and java-debug on the Mac, 2026-11-13, by hand through MCP: a JVM waiting on its JDWP
  port, attached with a breakpoint whose condition is `i == 3`, stops once with `i = 3`; a log
  message writes its six lines to the console; `add(40, 2)` evaluates to `42`. No automatic live
  test yet; in the window not checked.
- **AK-4** Window self-test: ⌥⇧F5 opens the dialog, `:port` offers each runtime, Python attaches
  to a `debugpy --listen` loop and stops at its breakpoint with the counter, Stop detaches and
  the program still runs.
- **AK-5** Hand test (round 4, no. 73): a Java server with JDWP, a Node.js script with
  `--inspect` and a native program next to the reference.
- **AK-7 (v3)** With `dlv` and `go` (`crates/debugger/tests/it/delve.rs`:
  `go_process_is_attached_and_detached`): a running Go loop is attached by process id, stops at a
  breakpoint in the loop with its counter, and still runs after the detach; a Delve server on
  another host is refused; a Delve server on a loopback port (`dlv exec --headless`) is attached
  through that port without an adapter, stops at the breakpoint, the console names
  `--accept-multiclient`, and the server ends when the client leaves
  (`go_delve_server_is_attached_through_its_port`). Unit tests: a Delve server's port and the Go attach requests
  (`attach::tests`), the Go row only for a loopback address (`attach_view::tests`). MCP:
  `debug_attach` dry runs for Go (`attach_lists_processes_and_detaches`).

## Out of scope
Attaching over SSH or to containers (the reference's attach providers for remote hosts and
Docker), attaching to Chrome pages, Java attach by process id through the Attach API, saving an
attach as a run configuration ("Remote JVM Debug"), reattaching to the last process with one key.

## Known deviations from the reference
- The reference needs a separate "Remote JVM Debug" run configuration to attach to a JDWP port;
  here the port is found on the process's command line or typed as `host:port` in the same dialog
  (IMP-0048).
- The list puts processes that can be debugged at once (debug port open) first and filters by the
  whole command line, not only the name (IMP-0048).
- **(v3)** A Go process in the list is attached to as a native program unless Go is chosen as
  described in behaviour 7; a Delve server on another machine is refused, because its port is not
  protected. Stop on a Delve server's session ends a server that takes one client only, with its
  program; Ferris IDEA cannot detach from it and leave it running.
