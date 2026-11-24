# QG-1 criterion 3: the window self-test on Linux

- **Gate:** [ADR-0015](../adr/0015-quality-gate-ai-native.md) criterion 3 ("workspace tests and window
  self-test pass on macOS and Linux"); the gate report is [QG-1-REPORT.md](QG-1-REPORT.md) §3c
- **Measured:** 2026-11-14 19:56 to 2026-11-15 00:23 CET (first result, PR #771) and 2026-11-15 00:48
  to 02:37 CET (pointer input and tools, branch `linux-selftest-mouse`), by agents on the coordinator's
  order
- **Rule:** every value is what was measured; nothing is rounded up

## Where it runs

The project's build server has no X server and no Vulkan driver, and system packages must not be
installed there. The Linux window run therefore happens in a local VM on the Mac:

| | |
|---|---|
| VM | Lima 2.2.1, `idecraft-linux-ui`, Ubuntu 24.04 (noble, cloud image 20260926), aarch64, vz, 6 CPUs, 12 GiB, 60 GiB disk |
| Isolation | Lima plain mode: no folder of the Mac mounted, no port forwarded, no containerd; Lima's SSH on 127.0.0.1 only. The tree goes in as an rsync copy without `.git` and without what `.gitignore` names. |
| Packages | Ubuntu archive (`rustup` too; the toolchain follows `rust-toolchain.toml`, stable, rustc 1.100.0) |
| Tools | from their projects' own distribution places, each pinned (SHA-256 or version) by the script: clangd 21.1.8 (apt.llvm.org, key checked by fingerprint), perf 6.8 (`linux-tools` of the VM kernel), JDK 25 for jdtls (archive; `java` stays 21), jdtls 1.64.0 (Eclipse), java-debug 0.53.1 (Maven Central), Node.js 24.18.0 (nodejs.org), js-debug 1.95.3 (Microsoft's release), pyright 1.1.414 (npm), Gradle 8.10.2 with the Kotlin Gradle plugin 2.0.21 in its cache, kotlin-debug-adapter 0.4.4 (fwcd's release), ruby-lsp 0.26.11 (rubygems.org) with bundler (archive) |
| Display | `xvfb-run -a -s '-screen 0 1920x1080x24'`, no window manager |
| GPU | Mesa lavapipe (`llvmpipe (LLVM 20.1.2, 128 bits)`, software Vulkan) |
| Build | `cargo build -p ferris-idea` (debug, without incremental state), linked with lld |

The other Lima VMs (`idecraft-db`, `idecraft-docker`, `idecraft-k3s`, `idecraft-mssql`) were not
touched. The VM is stopped after the run and kept.

## Commands

```sh
scripts/linux-ui-selftest-lima.sh          # create or start the VM, provision, copy, build, run, stop
scripts/linux-ui-selftest-lima.sh --keep   # the same, the VM keeps running
FERRIS_IDEA_SELFTEST_ONLY=editor scripts/linux-ui-selftest-lima.sh --keep   # one group
```

The script prints the tools and groups the self-test skipped, the slowest groups and the result line
`{"checks":N,"failed":[…],"skipped":[…]}`; the logs are in `target/linux-ui-selftest/`.

## Result

Final run: branch `linux-selftest-mouse` at `e12d45ea`, `scripts/linux-ui-selftest-lima.sh` from
2026-11-15 02:30 CET (build up to 02:31, self-test 02:31 to 02:37, VM stopped 02:37); the self-test ended
on its own (exit code 0) and printed its result line:

```
{"checks":1067,"failed":[],"skipped":[]}
self-test groups: 356.8 s in 119 groups      (2897 steps)
```

**1067 checks, 0 failed, 0 groups skipped on request. The window self-test passes on Linux** (X11 on
Xvfb without a window manager). The run is one run; it is not a series. Wayland was not run: the
pointer path there is the same code (the events go to the window, not to the platform layer), but no
headless compositor is provisioned.

What the run does not cover, each printed as its own line:

- **browser-debug** has no steps: js-debug is there, but Google builds Chrome for Linux on x86_64 only,
  and the VM is aarch64. Ubuntu's Chromium is a snap; it was not installed.
- **run-config-folders** and **run-config-templates** need `FERRIS_IDEA_SELFTEST_DRIVER=1`, and
  **merge-edit** checks its keys only, as on the Mac (the default run does not use the driver).
- **profiler-attach** samples nothing: Ubuntu's kernel default `kernel.perf_event_paranoid=4` refuses
  perf, and the step accepts that refusal as the Linux form of the macOS permission note (item 8
  below). The sysctl of the VM was not changed.

### The runs, in order

| Run | Code | Checks | Failed | What changed before it |
|---|---|---|---|---|
| 1: 2026-11-14 23:11 (PR #771) | `7a4c7d83` | 990 | 132 | — (114 of them pointer steps and their follow-ups) |
| 2: 2026-11-15 | `bbf74a70` | 1061 | 27 | in-process pointer input; the tools provisioned (the java-*, node-, kotlin-, package-script and ruby groups run now) |
| 3: 2026-11-15 | `3240b5d8` | 1067 | 2 | Linux variants of four macOS-only steps (6–9 below), ruby-lsp with bundler |
| 4: 2026-11-15 02:30 | `e12d45ea` | 1067 | 0 | the profiler fixture (10 below) |

In run 2 every pointer step passed. Its 27 failures were: 20 in the debug groups (Shift+F9
started the selected configuration, a Python one, instead of the current file: item 6), 3 in tabs and
split (no Linux shortcut for Reopen Closed Tab and Next Splitter: items 7 and 8, one follow-up),
3 Control-clicks in the mouse group (item 9, one follow-up), the title bar's double click (item 9),
and Ruby's quick documentation (ruby-lsp without bundler). The 18 non-pointer failures of the first
run (code-insight Call Hierarchy, problems, refactor, python, profiler, profiler-attach,
run-config-before-launch, projects, tool-window-replace) passed in run 2: clangd 21 instead of
Ubuntu's 18 fixed Call Hierarchy, Problems and Extract Method, pyright the Python diagnostics (pylsp
reports no unresolved import, so the status bar kept the install failure), perf the attach step, and
the rest had followed from the pointer failures before them. The two profiler failures of run 3 had
passed in run 2: the order of the first Hot Spot changed between runs (item 10).

Earlier runs of 2026-11-14, for the record: the first run never got past its first step (defect 2
below); two full runs ended without a result line when the python-debug group closed its debug session
(defect 3); with those fixed, python-debug-more had 5 failures from the wrong key (step fix 4).

## Defects found and fixed

1. **The debug build did not link on Linux on 64-bit ARM** (Linux-specific, aarch64 only). GNU ld
   stopped with `relocation truncated to fit: R_AARCH64_CALL26` (ten times, in the standard library's
   cold code): the debug binary has 330 MB of code, more than an AArch64 call reaches (±128 MiB), and
   GNU ld does not insert the range-extension stubs for these calls. Fix: `.cargo/config.toml` links
   `aarch64-unknown-linux-gnu` with lld (package `lld`); the backend build container installs `lld`.
   the build server (x86_64), the Mac and GitHub's x86_64 runners are not affected. Evidence: the build links
   (2.7 GB binary) and the self-test ran on it.
2. **On X11 without a window manager a window never drew its first frame** (Linux-specific, X11). gpui
   starts drawing a window when it handles the window's MapNotify, and it handles events only when its
   socket becomes readable. Events that libxcb read while gpui waited for a reply (it sets the title and
   other properties with round trips right after the map) stay in libxcb's queue; with a window manager
   more events follow and wake the loop, on a bare X server nothing does. The window stayed empty, the
   process idle in `epoll_wait` without a timer, the self-test stood at its first step. Confirmed by
   strace (no X traffic after the map) and by one property change from outside (`xprop -set`), after which
   the editor group ran through (28 checks, 0 failed). Fix: `x11_wake::until_first_frame` — until a new
   window's first frame (at most 10 s), a thread with its own X connection changes a property on this
   process's top-level windows (found by `_NET_WM_PID`), whose PropertyNotify wakes gpui's loop. Every
   window the app, the spikes and the workbench open does this; Wayland and macOS are untouched. Test:
   `x11_wake::the_windows_of_this_process_get_a_property_notify` (live under Xvfb; returns early without
   `DISPLAY`), `x11_wake::only_an_x11_session_is_woken`.
3. **Ending a debug adapter sent SIGTERM to every process of the user** (Linux-specific: procps-ng's
   `kill`, Ubuntu 24.04). `DapClient` ended the adapter's process group with the `kill` program,
   `kill -TERM -<pgid>`; procps-ng 4.0.4 misreads that (`kill -TERM -99999` calls `kill(-9, …)`) and
   with the adapter's six-digit pid it called `kill(-1, SIGTERM)` (strace, python-debug group): the IDE,
   its shells, the user's systemd manager and the SSH session ended when the debug session closed. Two
   full runs died that way without a result line at the python-debug group. Fix: `killpg` directly,
   never for group 0 or 1. Tests: `client::tests::ending_an_adapter_signals_its_group_only`,
   `client::tests::the_own_group_and_every_process_are_never_signalled`; strace of the python-debug
   groups afterwards shows only `kill(-<adapter pid>, SIGTERM)`. macOS's BSD `kill` read the argument
   correctly, so the Mac never showed it.
4. **Self-test: Debug Current File used a Recompile key on Linux** (self-test step relying on macOS
   behaviour). The java-smart, java-debug-more and python-debug-more steps pressed Ctrl+Shift+F9 on
   Linux; the default keymap binds that to Recompile and gives `DebugClass` no shortcut. Named Linux
   variant (`with_debug_current_file`): Find Action, the id `DebugClass`, a check that it is selected,
   Enter; macOS keeps ⌃⇧D. python-debug-more went from 5 failed to 0 failed on Linux.
5. **Self-test: the terminal group's mouse-mode step typed the next command together with ^C**
   (shell-specific, seen on Linux). bash, the login shell of the VM, drops what was typed while the
   foreground program ended; zsh on the Mac kept it. The step now presses Ctrl+C, waits until the shell
   prompts again, and only then types the command; the checks are unchanged.

6. **Self-test: Debug Current File in five more groups used Shift+F9 on Linux** (self-test steps
   relying on macOS behaviour). java-debug-session, node-debug, kotlin-debug, browser-debug and
   breakpoint-condition-debug pressed Shift+F9, which the default keymap binds to Debug (the
   selected configuration); the console showed a Python program ending. Named Linux variant, as in
   item 4: Find Action, the id `DebugClass`, a check that it is selected, Enter
   (`with_keys_by_find_action`); macOS keeps ⌃⇧D.
7. **Reopen Closed Tab had no shortcut on Linux, unlike what SPEC-0059 v9.1 behaviour 22b said**
   (a wrong claim of the specification and a dead line in the keymap code; not seen by users as a
   change). The default keymap was to get Ctrl+Alt+Shift+T if free, but the bundled keymap gives that
   chord to Refactor This, so nothing was bound; the reference has no shortcut for it there either.
   SPEC-0059 v9.9 says so, the dead line is gone, and `keymap::tests::
   reopen_closed_tab_has_no_shortcut_in_the_default_keymap` holds it. The tabs group reaches the
   action through Find Action on Linux.
8. **Self-test: Next Splitter has no Linux shortcut** (only the macOS keymap binds ⌥⇥). The split group
   reaches `NextSplitter` through Find Action on Linux. The profiler-attach step's Linux variant
   accepts the kernel's refusal of perf (`kernel.perf_event_paranoid` above 2) and, without perf on
   the PATH, prints a skip line instead of waiting; macOS keeps the Developer Tools note.
9. **Self-test: two macOS-only pointer gestures.** A Control-click is a secondary click on macOS only
   (SPEC-0275 behaviour 2, AK-2): the Linux variant of the mouse group's two Control-click steps opens
   the same menus with the right button. The toolbar lies in the macOS title bar, whose double click
   zooms; on Linux the window manager draws the title bar above the toolbar (SPEC-0061 behaviour 61:
   "nothing changes there"), so the Linux variant checks that a double click on the empty part of the
   toolbar leaves the window's size.
10. **Self-test: the profiler group's first Hot Spot changed between runs** (both systems). With
    `sum(i * i for i in range(n))` cProfile splits the work between the generator (0.44 s) and `sum`
    (0.39 s), so either came first. The fixture now does the work in `crunch` itself; the check is
    "Hot Spots lists crunch, the busy function, first" and Enter opens its line 1.

## Pointer steps on Linux (open question 40, answered)

The coordinator decided on 2026-11-15 (PO-STATUS, open question 40): **no XTEST or any display-wide
input injection on Linux**. Instead the self-test's `click(…)` and `hover(…)` steps on Linux deliver
the toolkit's own pointer events in process, into Ferris IDEA's window input handler — the handler gpui's
X11 and Wayland layers call for real input — only from the self-test, never through the X server or
the compositor (SPEC-0319 v5 behaviour 20c, UISEC-9).

- **The hook:** gpui 0.2.2's `Window::dispatch_event` is public but returns a type private to the crate,
  so no code outside it can call it. The workspace now uses gpui 0.2.2 vendored in `vendor/gpui/`
  (byte for byte the published crate, checksum as in `Cargo.lock` before) with one addition,
  `Window::dispatch_in_process_pointer_input`, which takes pointer events only and refuses keys and
  file drops; `vendor/gpui/FERRIS_IDEA.md` names every difference. The macOS path is unchanged (native
  `NSEvent`s posted to Ferris IDEA's own queue).
- **The boundary:** `crates/app/src/native_click.rs` builds the events of a click (move, each press and
  release with its count, Control on a Control-click and a move without it afterwards) and hands them
  to the window after the current frame. The UISEC-9 scan (`scans::uisec9_no_system_wide_input`) checks
  that only this file calls the hook, that the hook refuses keys, modifier changes and file drops, and
  now also bans XTEST through x11rb, events sent through the X server, the compositor's virtual pointer
  and keyboard and `/dev/uinput`. External clients keep `unsupported` for the native level and for
  pointer actions at the window level (SPEC-0319 16a, 20a).
- **The proof that nothing reaches other X clients:** `native_click::tests::
  a_click_reaches_this_window_and_no_other_x_client` (live under Xvfb, `xvfb-run cargo test -p ferris-idea
  --bin ferris-idea native_click`; returns early without an X11 session): a gpui window of the test process
  gets a double click and a right click (`left 1, up 1, left 2, up 2, right 1`), while an observer with
  its own X connection — a window under the whole screen that takes button and motion events, and the
  raw input of every device on the root window (XInput 2.2, which XTEST input and every real device
  produce) — receives no input event, and the display's pointer has not moved. Passed in the VM on
  2026-11-15 together with the two event-shape tests (3 passed).

## Rerun of 2026-11-16: red on `main`, green again

A full run of 2026-11-16 06:30 CET (a branch on `main` before #854) had 9 failed checks; 8 of them failed
the same way on a clean `main` (`24ca3d0d`). None was a product defect, on Linux or elsewhere: three
self-test steps no longer matched behaviour specified after the run of 2026-11-15, and the other five
followed from them. The macOS window self-test has not run since then either (its last run is at
`fdeeb75c`), so the same steps would have failed there.

| Failed check | Cause | Fix (branch `linux-selftest-red`) |
|---|---|---|
| View Breakpoints lists both with the caret's one selected | step: since SPEC-0127 v10 behaviour 19 (#800) ⇧⌘F8 / Ctrl+Shift+F8 on the caret's breakpoint opens the small popup; the step expected the dialog | the key opens the popup (new check), the key again in it opens the dialog with both listed and the caret's selected; AK-3 restated (v13.1) |
| Enter applies the condition, closes, and the dot shows it | follow-up: no dialog, so no condition | — |
| the conditional breakpoint stops the loop only on the pass where it holds; resume runs the loop to its end without stopping again | follow-up: the breakpoint had no condition and stopped at `total = 0`; the Node.js program stayed paused | — |
| the condition moves with its breakpoint | follow-up: no condition | — |
| the port the host program listens on is offered with Forward | follow-up, by elimination: the check takes the first offer; it passes once the breakpoint steps pass, with the port code unchanged, and its own forward worked in the red runs. The in-process host offers ports of every child of the window process, so the Node.js program left paused (debug port) was most likely offered first; not observed directly | — |
| the Breakpoints dialog saves uncaught exceptions and a method breakpoint | step: since SPEC-0146 v6 behaviour 15 (#800) each language has its own exception row; ⌥U on the first row saves uncaught for Java under `languages`, the step read the shared setting | the check reads the row's language (uncaught on, caught off), that the next language stays off, and the method breakpoint |
| Refresh in the Database window refreshes the SQL editors' schema | step: it asked for no cached schema right after Refresh; since SPEC-0192 v3 behaviour 11 (#782) Refresh inspects the open `.sql` files again at once, and on the software renderer that read ended before the next frame | an open file marks a table that does not exist; the table is created outside and stays unknown until Refresh; after it the mark goes without an edit (SPEC-0089 v11.1 AK-7); the table is dropped and read once more for the steps after |
| the new interpreter is pushed to the Python language server (SPEC-0170) | timing under load in that run only; green in the group run and in the full run below | — |

Explain and the Compare Schemas steps failed only in runs with `FERRIS_IDEA_SELFTEST_ONLY=db-compare`: the
group uses the `shop` connection of the database groups before it, and `database` needs the
`.ferris-idea` folder the `run` group makes. With `run` and the database groups in front they pass.

Runs on the branch (VM as above, `main` at `ae7db55b` plus the fix):

| Run | Groups | Checks | Failed |
|---|---|---|---|
| 2026-11-16 07:38–07:39 | run, the database groups to db-compare, er-diagram, the breakpoint groups, python, remote-tool-windows | 209 | 1 (the exception row; fixed after it) |
| 2026-11-16 07:41–07:48 | all | **1070** | **0** |

The full run: `{"checks":1070,"failed":[],"skipped":[]}`, 2906 steps, 405.1 s in 119 groups; the
three new checks are the popup and the two before the Refresh. Not covered, as before: browser-debug,
run-config-folders, run-config-templates, merge-edit's clicks.

## What was not changed

- No check was weakened or skipped by name for Linux. Where a step relied on macOS behaviour it got a
  named Linux variant that checks the same behaviour the Linux way (items 4, 6–9); the macOS steps are
  unchanged except the profiler fixture (item 10), which applies to both.
- No input is posted to the X server or the compositor: XTEST, sent events, virtual pointers and uinput
  stay forbidden (UISEC-9).
- No system setting of the VM was weakened for a check (`kernel.perf_event_paranoid` stays 4).
- The macOS self-test was not re-run in this branch (ADR-0015 criterion 3 for macOS is §3c of the gate
  report).
