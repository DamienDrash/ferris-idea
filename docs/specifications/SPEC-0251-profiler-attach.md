# SPEC-0251: Attaching the profiler to a running process

- **Version:** 3 · **Date:** 2026-11-15 (v3: AK-6, hand test 181 with real keys; v2: `profile_processes` returns only the program, never arguments)
- **Sources:** GEN (IDE profilers attach to a process that is already running, picked from a list,
  and record it for a while without restarting it; F-083), STD (`ps -U <uid> -o pid=,args=`;
  macOS `/usr/bin/sample <pid> <seconds> 1 -mayDie -f <file>`, `/usr/bin/heap <pid>` class table
  `COUNT BYTES AVG CLASS_NAME TYPE BINARY`; Linux `perf record -F 1000 -g -e cpu-clock:u -p <pid>
  -o <file> -- sleep <seconds>` and `perf script`), `docs/legal/RESEARCH_BOUNDARIES.md`,
  SPEC-0120, SPEC-0252, SPEC-0053, ADR-0008
- **Feature matrix rows:** F-083

## Purpose
Look at a program that is already running — a server, a long job, something that only misbehaves
after a while — without stopping and restarting it under the profiler.

## Behaviour
1. **Processes:** the list shows the user's own processes (`ps -U <uid>`) by PID and command line,
   newest first, without IDECraft itself. Nothing of other users is listed. [STD]
2. **Never the reference IDE:** a process whose command line belongs to a JetBrains IDE (contains
   `JetBrains`, `IntelliJ`, `idea.vmoptions` or `com.intellij`, or its program is `idea`,
   `idea.sh` or `idea64`) is listed as refused and cannot be attached to, from the window or MCP.
   This follows `RESEARCH_BOUNDARIES.md` (no profilers or debuggers on the reference's process).
   [Legal]
3. **CPU:** the process is sampled from outside for the chosen time (default 10 s, 1–120 s) —
   macOS `sample` (one sample per millisecond), Linux `perf` (1000 Hz, user space only). The
   process keeps running; nothing is loaded into it. The result is a CPU profile (SPEC-0120,
   SPEC-0252) whose run names the PID and command line. A process that ends while it is sampled
   keeps what was recorded. [STD]
4. **Memory snapshot (macOS):** `heap <pid>` reads the process's malloc zones; the classes and
   types become one root each with bytes and count (`non-object` for plain blocks). On Linux no
   unprivileged tool can read another process's heap; the snapshot is refused with that reason.
   [STD]
5. **Refusals:** an unknown PID, a process of another user, the reference IDE (behaviour 2), a
   missing tool, and the system's refusal (macOS Developer Tools permission, Linux
   `kernel.perf_event_paranoid` above 2) each say what is wrong. [STD]
6. **Window:** `Profiler.AttachToProcess` ("Attach Profiler to Process…") opens a list of the
   processes to pick from (type to filter, Enter attaches for CPU, ⌥Enter takes a memory snapshot
   on macOS, Escape closes); refused processes are marked and Enter on them says why. Recording
   shows "Attached to <pid>…" and runs in the background; the result is kept like any profile
   (`last.json`). [GEN]
7. **MCP:** `profile_processes` lists the user's processes by id and program name only, never their arguments, which may hold secrets (as `dap.processes`, SEC-10), with `refused` and its reason;
   `profile_attach` (`pid`, `seconds`, `kind` `cpu`/`memory`, `dry_run` default true: the tool
   and command) — a real recording needs the user-granted `run` right, is logged in
   `.idecraft/agent-log.jsonl` and returns the profile as `profile_run` does. [ADR-0008, SPEC-0053]

## Acceptance criteria
- **AK-1** Reference IDE recognised from command lines (macOS app bundle, Linux `idea.sh`, JBR with
  `idea.vmoptions`), other Java programs not; `ps` lines parsed; `heap` table parsed; attach plans
  per platform and kind (unit tests in `crates/profiler`).
- **AK-2** Real run: a busy C program started by the test is sampled by PID for 2 s while it keeps
  running, `spin` is the hottest function (or the macOS Developer Tools refusal is reported); a
  heap snapshot of a C program holding blocks lists `non-object` with their bytes; Linux skips
  without `perf` (`crates/profiler/tests/real.rs`).
- **AK-3** MCP: processes listed, dry run, refusal without `run`, refusal of a process that looks
  like the reference IDE (`profiles_memory_and_attaches`).
- **AK-4** Window self-test: the picker lists a process the test started, filtering finds it, a
  process named like the reference is marked refused; attaching shows a profile or the permission
  note.
- **AK-5** Hand test (round 4, no. 181): attach to a running Python server and a native program,
  CPU and heap snapshot; the reference IDE is refused in the list.
- **AK-6 (v3)** Hand test 181 in a window of the test harness with real keys and `/usr/bin/sample`
  (`crates/workbench/tests/it/f083_handtests.rs`: `hand_test_181_attach_to_a_running_process_with_keys`): the list shows a busy
  Python process the test started and a look-alike of the reference marked refused; typed text
  narrows the list, Enter on the look-alike gives the reason, Enter on the process samples it for
  2 s, the call tree names `process <pid> · sample` and "attached to process <pid>", and the
  program goes on running. Not covered: ⌥Enter, Linux `perf`, the comparison with the reference.

## Out of scope
Attaching to processes of other users or containers, Python-level stacks of a running
interpreter (needs a tool that is not shipped, e.g. py-spy with root on macOS), Java attach via the
attach API, heap snapshots with call stacks (needs the process started with `MallocStackLogging`),
Windows.

## Known deviations from the reference
The profiler never loads an agent into the target; it samples from outside with the system's
tools, so the process cannot be changed or crashed by attaching (IMP-0121). JetBrains IDEs are
refused by rule.
