# SPEC-0263: Lifetime of helper processes

- **Version:** 2 · **Date:** 2026-11-10 (v2: debug adapters end with what they started, also
  when Ferris IDEA is killed — hand test 60 window run)
- **Sources:** OBS-0002 (the reference's per-connection driver processes outlive it for ~25 s after
  it is ended by a signal → IMP-0001), STD (LSP `initialize.processId`: a server should exit when
  that process is gone; `shutdown`/`exit`; closing a pseudo terminal's controlling side hangs up its
  session; POSIX process groups), SPEC-0090, SPEC-0039, SPEC-0038, ADR-0011
- **Feature matrix rows:** F-084, F-076, F-070

## Purpose
No program Ferris IDEA starts for its own work keeps running after Ferris IDEA has ended, however it
ended.

## Behaviour
1. **Databases:** drivers run inside the IDE process (ADR-0011); a connection never starts a helper
   process. [IMP-0001]
2. **Language servers:** when Ferris IDEA ends normally (window quit, end of an `ferris-idea mcp` session)
   every server of the pool gets `shutdown` and `exit`; a server still running 2 s later is killed,
   so all are gone within 3 s. Each server is told Ferris IDEA's process id in `initialize` and reads
   requests from a pipe of the IDE, so when the IDE is killed the server sees the pipe close and the
   process id end. [STD, IMP-0001]
3. **Debug adapters:** each runs in its own process group; ending the session sends `SIGTERM` to the
   group and kills the adapter if it is still running. [STD]
   - **What an adapter started (v2):** a server adapter (js-debug, Delve, also on a remote host)
     ends together with its direct children — the debugged program, the browser of a page, which
     js-debug starts in a process session of its own — noted before the adapter ends:
     `SIGTERM`, `SIGKILL` after the grace period, each only while it still runs the command line
     it had when noted, so a process id the system gave to another program is left alone. [STD]
   - **When Ferris IDEA is killed (v2):** each such adapter has a guard, a small shell in a process
     group of its own that reads a pipe only Ferris IDEA writes to. A normal end tells the guard to
     stand down; when Ferris IDEA ends without ending the adapter (quit with a session running, crash,
     `SIGKILL`) the system closes the pipe and the guard ends the adapter and its direct children
     the same way within 3 s. The same holds for the backend on a remote host. [STD]
4. **Terminals:** shells run in a pseudo terminal owned by the IDE; ending the IDE closes it and the
   system hangs up the shell's session. [STD]
5. **Port forwards and run configurations:** started children end with the process that started
   them (SPEC-0241, SPEC-0087). [GEN]

## Acceptance criteria
- **AK-1** `ferris-idea mcp` with a language server that ignores `exit` and stays alive after its input
  closes: after the MCP client disconnects, the MCP process ends and the server is gone within 3 s;
  the server was given the MCP process's id (`crates/app/tests/it/helper_processes.rs`).
- **AK-2** Opening a database connection starts no process (code review: no `Command` in
  `crates/database` outside tests).
- **AK-3** (v2) A process with a child of its own ends with that child; a guard whose owner is gone
  ends both, a guard that stands down leaves both alone (`crates/debugger/tests/it/lifetime.rs`). A
  process debugging a page in headless Chrome is killed with `SIGKILL`: the browser and js-debug
  are gone within 5 s (`crates/debugger/tests/it/jsdebug.rs`:
  `a_killed_owner_leaves_no_browser_or_adapter`).

## Out of scope
Language servers that neither honour `processId` nor notice their input closing when Ferris IDEA is
killed with `SIGKILL` (debug adapters have the guard of behaviour 3). Grandchildren a debugged
program starts in a process group of its own after Ferris IDEA is gone.

## Known deviations from the reference
No helper process outlives Ferris IDEA by more than the 2 s grace period of a normal end, and none
exist per database connection (IMP-0001).
