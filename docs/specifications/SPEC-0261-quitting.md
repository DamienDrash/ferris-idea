# SPEC-0261: Quitting with unsaved files

- **Version:** 9 · **Date:** 2026-11-14 (v9: the user may ask for a question also with nothing pending, behaviour 1, SPEC-0337 behaviour 2; v8: quitting through MCP always needs the right `quit`, also with nothing pending, behaviours 7 and 8 — SPEC-0053 v8 behaviour 13; v7: the close button of a project window asks it too,
  behaviour 19; v6: closing project windows asks the same question with the
  words of closing, behaviour 19; v5: the drawn question is sized to its content,
  behaviour 5; v4 after the coordinator's window run of 2026-11-10,
  macOS 27 with Stage Manager on, release bundle of `main` @ d87a477f: with a browser debug session
  or `npm run dev` running, ⌘Q, the menu item and `SIGTERM` showed no question and did not quit; the
  question is now drawn in the window, quitting has a deadline, behaviours 2, 5, 15 changed, 16–18
  new; v1 2026-11-03: the question in the window; v2: quitting through
  MCP and the command line, running programs named, behaviours 6–12; v3 after hand-test round 5
  defects D51, D57, D58: running work asked about and ended, one shared step for every way of
  quitting, every project window counted, behaviours 13–15)
- **Sources:** OBS-0002 (the reference always asks before quitting, in a dialog automation could not
  answer → IMP-0002), GEN (editors ask before discarding unsaved text; command-line tools ask on a
  terminal and take flags otherwise), STD (macOS: native alerts are exposed to accessibility clients
  with their buttons; MCP tool calls), ADR-0008, SPEC-0220, SPEC-0221, SPEC-0053, SPEC-0124
- **Feature matrix rows:** F-087, F-084

## Purpose
Quitting never loses typed text or running work silently, never asks when there is nothing to lose, and
can be done by a script or an agent as well as by the user.

## Behaviour
1. **Nothing pending:** Quit IDECraft (⌘Q, application menu) quits at once, without a question, when
   no file is unsaved and nothing runs (behaviour 6). [IMP-0002] (v9) With the user's setting
   "Confirm before quitting" on (Settings › Appearance & Behavior › System Settings, off by
   default), ⌘Q and the menu item ask "Quit IDECraft?" first (SPEC-0337 behaviour 2);
   `quit_ide`, `idecraft quit` and signals still quit without it.
2. **Unsaved files:** when any open file in any project window has unsaved changes, IDECraft asks
   first in a question drawn in the window in front (behaviour 5): "Save changes before quitting?", with the files
   named (relative to their project, at most 10, then "and n more") and the buttons "Save All and
   Quit" (default), "Quit Without Saving" and "Cancel". Running work (behaviour 6) is named below the
   files under "Still running, ended when IDECraft quits:". With running work but no unsaved file the
   question asks "Stop running processes and quit?" with "Stop and Quit" (default) and "Cancel".
   Return chooses the default answer, Escape cancels; each button can be clicked. [GEN, IMP-0002]
3. **Save All and Quit** saves every editor of every project window the way Save does (EditorConfig
   tidying, local history, the changed-on-disk check) and quits. If a file could not be saved, or
   changed on disk meanwhile, IDECraft does not quit; the editor says why. [GEN]
4. **Quit Without Saving** quits; **Cancel** (and Escape) keeps everything open. [GEN]
5. **Drawn, not a system alert.** The question is drawn by IDECraft inside the window in front (the
   project window that is active, else the first project window), above everything else in it,
   with the message, the detail and the buttons of behaviour 2; the window is brought to the front.
   v3 asked in a native alert (a system sheet on the window). In the coordinator's window run of
   2026-11-10 (macOS 27, Stage Manager on) no sheet appeared and nothing quit, and an unanswerable
   alert keeps IDECraft open with no way out; the window self-test could never answer it either.
   A decision that keeps IDECraft from quitting or a window from closing is therefore never left to
   a sheet that may not appear (SPEC-0281 v2 behaviour 6). Screen readers do not see a drawn
   question yet (ADR-0019); every answer stays available through `quit_ide` and `idecraft quit`
   (behaviours 7, 11). v5: the frame is as tall as its content (message, detail, buttons and its
   padding, at most 560 px); it no longer stretches to the window and leaves an empty area below
   the buttons. The same frame draws every drawn confirmation (delete, close, discard).
   [GEN, IMP-0002]
6. **Pending work** is what quitting would lose: unsaved files (as in behaviour 2) and running work —
   run configurations and tests still running in the Run window, debug sessions, and a database console
   statement still running — each named with its project. Terminals are not counted (their tabs are
   restored, SPEC-0205). [GEN, IMP-0002]
7. **MCP tool `quit_ide`** (served by the window over HTTP, SPEC-0221 v2) with `pending` = `ask`
   (default), `save_all`, `discard` or `cancel`, and `check_only`:
   - with no pending work it quits at once, whatever `pending` says (v8: with the right `quit`);
   - with pending work and `ask` (or `check_only`) nothing happens; the result lists `unsaved` and
     `running` and the `options` `save_all`, `discard` and `cancel`;
   - `save_all` saves every editor as behaviour 3 and then quits; a file that could not be saved is
     named and nothing quits;
   - `discard` quits without saving; `cancel` does nothing and reports the pending work.
   The answer is sent before the window quits. [GEN, IMP-0002, ADR-0008]
8. **Right:** quitting needs the `quit` right (v8: also with nothing pending — closing the user's
   windows changes the state of the IDE, SPEC-0053 v8 behaviour 13); `save_all` also needs `write`
   (SPEC-0053 v4). Over HTTP a missing right is asked in the window (SPEC-0053 v4 behaviour 8), so an
   agent cannot discard the user's work unless the user grants it. [GEN, IMP-0002, IMP-0007]
9. Quitting through MCP is checked again in the window: if work became pending between the check and
   the quit of behaviour 7's first case, nothing quits and the result names it. [GEN]
10. A server without a window (stdio, `idecraft mcp`) answers `quit_ide` with an error that names
    `idecraft quit`. [GEN]
11. **Command `idecraft quit [--save-all | --discard | --force] [--pid <pid>]`** finds the running window
    through the discovery files of SPEC-0221 and calls `quit_ide` as MCP client `idecraft-cli`. `--force`
    is `--discard`. Without a flag and with pending work it prints the unsaved files and running work;
    on a terminal it asks "[s]ave all and quit, [d]iscard and quit, [c]ancel" and repeats the call with
    the answer; without a terminal it exits with status 2 and names the flags. With several running
    windows it needs `--pid`; without a running window (or with `mcp.http` off) it says so and exits
    with status 1. Exit status 0 when the window quits. [GEN, IMP-0002]
12. Rights asked for the command are asked in the window like for any client (behaviour 8). [GEN]
13. **Running programs end with IDECraft:** when IDECraft quits — by any way of behaviours 1–4, 7, 11
    or a termination signal (SPEC-0262) — every program started by a run configuration (Run window, MCP
    `run_configuration`) is ended with its whole process group: first asked to end (SIGTERM), killed
    after half a second (SIGKILL); a before-launch chain does not go on. `quit_ide` lists them as
    `stopped`, `idecraft quit` prints "Saved:" and "Stopped:". [GEN, IMP-0002]
14. **Every project window counts:** the unsaved files, the running work and Save All cover every
    project window of the process, whatever view the window has at its root (the first window
    wraps the workbench in the self-test's view). [GEN]
15. **One step for every way of quitting:** ⌘Q, the menu item, `quit_ide` and `idecraft quit` all
    go through the same step: check (nothing pending), save every editor (Save All; nothing quits if a
    file stays unsaved) or discard; then the application quits and behaviour 13 ends the programs. If
    Save All in the question leaves a file unsaved, a second drawn notice "IDECraft did not quit"
    names it, with the button "OK". [GEN]
16. **The question is asked whatever asked to quit.** ⌘Q, the menu item and `quit_ide` ask or
    quit the same way whether the window is busy handling the key or the menu click: the quit step
    runs after that handling, never inside it. (v3 asked from within the handling of ⌘Q and the
    menu item, where the window in front could not be reached, so nothing was asked and nothing
    quit as soon as anything was pending; with nothing pending it quit at once.) [GEN]
17. **Quitting never hangs.** Once quitting is decided — no pending work, an answer to the
    question, `quit_ide`, `idecraft quit` or a termination signal (SPEC-0262 v2 behaviour 3) —
    IDECraft has ended at the latest 5 s later: if the normal quit has not finished by then (the
    platform refused it, or ending debug sessions and their browsers takes longer), the programs
    of behaviour 13 are ended as described there and the process exits. (In the same window run,
    five windows sent `SIGTERM` took about two minutes to end.) Debug adapters and browsers end
    through their own exit guards when the process is gone (SPEC-0263 v2). [GEN, IMP-0004]
18. **No window, no question.** Without a project window (no window open, or only the folder
    chooser) nothing can hold unsaved text, so quitting quits at once. [GEN]

19. **Closing project windows (v6, v7):** closing one or more project windows (SPEC-0094 v3 behaviour 7;
    v7: also with a local window's close button, SPEC-0094 v4 behaviour 11)
    asks this question for the files and the running work of the windows that close, in the window
    the close was asked in, drawn as behaviour 5. Its words are those of closing: "Save changes
    before closing <project>?" (or "… the <n> projects?"), Save All and Close, Close Without Saving,
    Cancel; for running work alone "Stop running processes and close?", Stop and Close, Cancel; the
    running work is listed as "Still running, ended when the window closes". [GEN]
## Acceptance criteria
- **AK-1** The question names the files and the running work, cuts lists after 10, offers "Stop and
  Quit" for running work alone and is not asked without pending work (`quit_model::tests`).
- **AK-2** Hand test (round 4, no. 190): edit a file, ⌘Q, each of the three answers.
- **AK-3** MCP with a stand-in window: nothing pending → quits without a right; pending → lists files
  and running work and does not quit; `discard` without `quit` → refused, not quit; with `quit` → quits
  discarding; `save_all` needs `write` too; work that appears before the quit → not quit; stdio →
  error naming `idecraft quit`.
- **AK-4** `idecraft quit` against a running server: nothing pending → status 0 and the window quit;
  pending without a terminal → status 2, files listed; `--discard` → quit; no running window → status 1.
- **AK-5** Hand test: with an unsaved file, `idecraft quit` in a terminal, then each answer.
- **AK-6** Ending all runs ends a running program, its child in the process group (also one that
  ignores SIGTERM) and stops its before-launch chain (`crates/core/tests/run_quit.rs`).
- **AK-7** Self-test in the first window (root view: the self-test's): with an unsaved file the
  pending work names it and the question is asked; a running configuration is listed; the shared save
  step saves the file (`idecraft ui --selftest`). MCP with a stand-in window: running work alone is
  pending and is reported as `stopped` after `discard` and `save_all`
  (`quit_ide_counts_running_work_and_reports_it_stopped`).
- **AK-8** Hand tests (round 5, no. 190, 203, 204) again after the fix of D51, D57, D58.
- **AK-9** The drawn question: message, detail and buttons as behaviour 2, Return the default,
  Escape cancels, the "did not quit" notice has only OK (`dialogs::tests`, `quit_model::tests`).
- **AK-10** The deadline: a quit that does not finish in time ends the process; one that finishes
  first is not cut (`lifecycle::tests`).
- **AK-11** Self-test in the first window: with a long-running configuration running, a quit request
  draws "Stop running processes and quit?" in the window; Escape (Cancel) keeps the window and the
  run (`idecraft ui --selftest`). v5: the frame ends at most 20 px below its buttons and is less
  than 300 px tall.
- **AK-12** Window check (coordinator, macOS with Stage Manager on): with a browser debug session and
  `npm run dev` running, ⌘Q and the menu item each draw the question; Cancel keeps everything;
  Stop and Quit ends IDECraft, Chrome and `npm run dev` within 5 s; `kill <pid>` ends IDECraft
  within 5 s without a question.

- **AK-13 (v6, behaviour 19)** Unit tests `close_model::tests::the_question_uses_the_words_of_closing`
  and `quit_model::tests` (unchanged words of quitting).
## Out of scope
Quitting several windows with one command (v6: closing them is SPEC-0094 v3).

## Known deviations from the reference
No question when nothing is unsaved or running; the question names the unsaved files and the running
work and offers to save them; running programs end with their whole process group instead of being
left behind; the same decision is available to scripts and agents with explicit options and a right
(IMP-0002); quitting is bounded by a 5 s deadline (IMP-0004).
