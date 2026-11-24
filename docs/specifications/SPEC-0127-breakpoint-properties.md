# SPEC-0127: Breakpoint properties in the window

- **Version:** 15 · **Date:** 2026-11-16 (v15: Toggle Breakpoint Enabled and ⌥-click on a line
  number, behaviour 31; Remove All Breakpoints and Remove All Breakpoints in File, also through
  `set_breakpoint`'s `remove_all`, behaviour 32; AK-20 to AK-22; F-071 checked without a screen;
  behaviour 30 and AK-19 belong to v14 on another branch); 2026-11-16 (v14: the popup's and the dialog's keys in the system's
  notation, More from the keymap, behaviour 30 — PR #866 Linux window self-test); 2026-11-16 (v13.1: AK-3 follows behaviour 19 — on the caret's breakpoint ⇧⌘F8 opens its popup and ⇧⌘F8 there the dialog; its wording from before v10 had ⇧⌘F8 open the dialog at once; no change of behaviour, found by the Linux window self-test; v13: one breakpoint, two views — the popup and the dialog show a line breakpoint's Enabled, Suspend and All/Thread by one rule from the breakpoint's own state, the popup's switches in the semantic tree, behaviour 29; OBS-0056 D1, D2; v12: the dialog shows All chosen and its switches as a vertical list of check boxes, behaviours 27 and 28; OBS-0056 D1, D2; the dependency chosen by clicks in a window test, OBS-0054 B4; v11: the popup's title, check boxes and options, All chosen, Watch, the condition's expansion and history; OBS-0056 P2–P6; v10: the small popup at the gutter, Suspend All/Thread, a check box per row, "Disable until hitting" chosen from the breakpoints; OBS-0054 B2–B4; v9: acceptance criterion AK-14, the rest of hand test 54 in a window of the test harness with real keys; v8: Toggle Temporary Line Breakpoint, the keymap's `ToggleTemporaryLineBreakpoint`; v7: Go sessions, where Delve evaluates the properties; v6: agents get the breakpoints as the adapter has them after the first wait, change them while a session goes on and start with the saved ones; v5: a breakpoint the running session cannot bind is
  drawn hollow and its tooltip says why, agents get `verified` and the reason, OBS-0035 L14; v2: no line background for a switched-off breakpoint; v3:
  suspend policy, hit message and stack, removal after the hit, dependent breakpoints, instance,
  class and caller filters, groups in the dialog; OBS-0025 L14; v4: the dialog's rows name the file
  and line first, red marks, OBS-0025 re-check 2)
- **Sources:** GEN (IDEs give a line breakpoint a condition, a pass count and a log message that
  replaces stopping, switch breakpoints off without removing them, and list all of them in a
  Breakpoints dialog opened from the Run menu, with ⇧⌘F8 / Ctrl+Shift+F8 or a right click on the
  breakpoint; F-071), DOC (action ids `ViewBreakpoints`, `EditBreakpoint` with ⇧⌘F8 /
  Ctrl+Shift+F8), STD (DAP `SourceBreakpoint`: `condition`, `hitCondition`, `logMessage`),
  SPEC-0039 v2 (capability check), SPEC-0091 (breakpoints in the editor), ADR-0008, OBS v3
  (OBS-0025 item 14: the reference's breakpoint options), third-party open source v3 (java-debug
  stops only the hitting thread; its `pauseOthers` and `continueAll`), OSS v15 (action ids
  `ToggleBreakpointEnabled` in the Run menu's breakpoint group, `Debugger.RemoveAllBreakpoints`
  and `Debugger.RemoveAllBreakpointsInFile` without a menu, in the platform's debugger action
  files of `JetBrains/intellij-community`; ids only, no code), DOC v15 (holding Alt while
  clicking a breakpoint in the gutter switches it off or on; on a line without one it sets a
  temporary breakpoint), SPEC-0178 (Ferris IDEA
  evaluates what the adapter cannot, at each hit), STD v5 (DAP `Breakpoint.verified` and
  `Breakpoint.message` in the answer to `setBreakpoints` and in `breakpoint` events), SPEC-0039
  behaviour 8 (Ferris IDEA's hint for a breakpoint that is not bound), OBS v5 (OBS-0035 L14: a
  breakpoint that never stops looked like any other), STD v6 (DAP `setBreakpoints` replaces a
  source's breakpoints at any time, also while the program runs; tried 2026-11-13 with lldb-dap,
  debugpy 1.8.21, js-debug 1.95.3 and java-debug 0.53.1), OBS v10 (OBS-0054: the small popup on ⇧⌘F8 with a breakpoint at the caret, Suspend with All/Thread, check boxes in the list, the dependency chosen from a list with "disable again / leave enabled")
- **Feature matrix rows:** F-071

## Purpose
Stop only where it matters — on the interesting pass of a loop, or not at all but log a value —
without leaving the window.

## Behaviour
1. **Gutter:** a breakpoint with a condition or pass count shows a red dot with a light centre, one
   with a log message an amber dot, a switched-off one a red ring without the breakpoint line
   background (v2, hand test 54); a plain one stays a red dot. The
   properties move with their breakpoint when edits move it (with as many breakpoints as before
   they follow by position; a removed breakpoint takes its properties along). [GEN]
2. **Breakpoints dialog:** `ViewBreakpoints` / `EditBreakpoint` (⇧⌘F8 / Ctrl+Shift+F8, Run menu
   "View Breakpoints…") and a right click on a breakpoint dot open it. Left: every breakpoint of the
   workspace as `path:line` with its mark, the one on the caret line (or the clicked one) selected.
   Right: Enabled, Condition, Pass count, Log message ("{expr}" parts are evaluated). Up/Down choose
   the breakpoint, Tab moves between the fields, Enter (Done) applies and closes, F4 or a double
   click applies and shows the line, Escape closes without applying the open form. [GEN, DOC]
3. **Applying:** changed properties are saved with the workspace
   (`.ferris-idea/breakpoint-properties.json` beside `.ferris-idea/breakpoints`; plain breakpoints need
   no entry) and sent to a running debug session at once; a new session starts with them. A
   switched-off breakpoint is not sent. The adapter's capabilities are checked as in SPEC-0039 v2.
   [STD]
4. **Remove** in the dialog removes the selected breakpoint, also of files that are not open.
5. **MCP:** `saved_breakpoints` lists the saved breakpoints with their properties; `set_breakpoint`
   (path, 1-based line, condition, hit_condition, log_message, enabled, remove; `dry_run` default
   true) adds, changes or removes one. A window that is already open keeps its own list.
   [ADR-0008]
6. **Suspend policy (v3):** the dialog's "Suspend" switches between the debugger's own way
   (java-debug stops the thread that hit, debugpy and lldb-dap every thread), all threads, this
   thread, and none. With "all threads" java-debug suspends the others right after the stop
   (`pauseOthers`, so evaluations in the stopped thread keep working) and the next Resume resumes
   them all. "This thread" with an adapter that always stops every thread is said once in the
   console. "None" writes the breakpoint's messages and goes on. [OBS, OSS]
7. **Hit message (v3):** "Log "breakpoint hit"" writes `Breakpoint hit: File.java:12 in
   Class.method(…)` to the console at each hit. [OBS]
8. **Stack (v3):** "Log stack trace" writes `Stack at File.java:12:` and a line `at name
   (file:line)` per frame. [OBS]
9. **Remove once hit (v3):** after its first hit (the one that stops or logs) the breakpoint is
   removed from the session, the editor and the saved breakpoints. [OBS]
10. **Disable until hitting (v3):** a breakpoint that names another one (`path:line`, as the list
   shows it) does nothing until that one is hit — stopped there, or passed with "none". With "Stay
   enabled after that hit" it then stays active; otherwise it waits again after each own hit.
   [OBS]
11. **Filters (v3):** instance filter — object ids as the values show them (`33`, `@35`, `0x1f`),
   compared with the frame's `this`/`self`; class filter — patterns with `*`, separated by spaces or
   commas, a leading `-` excludes, `$` and `.` alike, a bare name matches the simple name; caller
   filter — the same patterns for `Class.method` of the calling frame. A hit that does not pass
   goes on without stopping. The class is that of `this`/`self`, else the class part of the frame's
   name. [OBS]
12. **Groups (v3):** the dialog lists Line Breakpoints, Exception Breakpoints, Method Breakpoints and
   Field Watchpoints (SPEC-0146 v3) under headings, and is larger (760 × 560), its form scrolling.
   [OBS]
13. **How (v3):** behaviours 6–11 are Ferris IDEA's own at each hit (as SPEC-0178 for conditions the
   adapter cannot evaluate): a breakpoint with such an option stops the program briefly at every
   hit, also when it then goes on. The options are saved in `breakpoints.toml` as the table `rules`
   of the breakpoint (`depends_on` relative to the workspace, SPEC-0303), and agents set them
   with `set_breakpoint` (`suspend`, `log_hit`, `log_stack`, `remove_once_hit`, `depends_on`,
   `depends_keep`, `instance_filter`, `class_filter`, `caller_filter`); `saved_breakpoints` lists
   them. A breakpoint with options shows the dot with a light centre, one that does not suspend the
   amber dot. [GEN, ADR-0008]

**(v4)** Each row of the dialog's list shows `File.java:22` first and the folder after it in grey,
so a narrow list never cuts the line number off; an enabled breakpoint's mark is red like the
gutter's, a switched-off one grey. [OBS]

**(v5)** 14. **Bound or not:** while a debug session runs, the debug adapter says for every line
   breakpoint sent to it whether it could bind it (`verified`) and may give a reason (`message`),
   in its answer and again whenever that changes (`breakpoint` events, e.g. once a class is
   loaded). The window follows both. A breakpoint the session reports as not bound is drawn
   hollow: a ring two pixels wide in its colour (red, amber for one that logs), its line keeping
   the breakpoint background — so it differs from a switched-off breakpoint, a thin ring without
   that background. Its tooltip adds a line `Not bound: <reason>`, the reason being the adapter's
   message, else Ferris IDEA's hint of SPEC-0039 behaviour 8 (the file does not exist; the path is
   reached through a symbolic link; no code at this line in the loaded program, or the program
   was built from another path or without debug information). The breakpoint is matched to its
   gutter line by the line asked for, not the line the adapter moved it to. Without a session —
   before the start, after the end, for a switched-off breakpoint and while breakpoints are muted
   (nothing is sent) — every breakpoint is drawn filled. Until an adapter has loaded the code,
   breakpoints it has not bound yet are hollow and fill when it binds them. [STD, GEN, OBS]

**(v5)** 15. **Agents:** the debugging tools that place breakpoints (`debug_start`, also for
   attaching and remote hosts) list each placed breakpoint with `verified`, the line asked for
   (`requested_line`) and the adapter's `reason` when it gave one; the breakpoints still not bound
   in a state (`pending_breakpoints`) carry that `reason` beside Ferris IDEA's `hint`.
   `saved_breakpoints` stays a list of the saved breakpoints and says nothing about binding, as no
   session is involved. [ADR-0008]

**(v6)** 15a. **After the wait:** the breakpoints `debug_start` and `debug_attach` list are those
   the adapter has when the first wait ended, not its first answer. js-debug answers every
   breakpoint as not bound and binds it when the script is loaded; an agent was told
   `verified: false` for a breakpoint the program had just stopped at, and never saw the line
   the adapter moved it to. [STD]

**(v6)** 16. **Agents and a running session:** `debug_breakpoints` (session; optional `path` and
   `breakpoints`) lists every file's line breakpoints as the adapter has them now (`verified`,
   `line`, `requested_line`, `reason`), the ones not bound with Ferris IDEA's hint, and the debug
   console. With `path` and `breakpoints` it first replaces that file's line breakpoints by the
   list (line, condition, hit_condition, log_message, `enabled`; an empty list takes them all
   away) — while the program is stopped or while it runs, as the dialog does for the window's
   session (behaviour 3). The saved breakpoints of the workspace are not changed by it. A list
   without `path` is refused. It needs the `debug` right. `debug_start` with
   `saved_breakpoints: true` adds the workspace's saved line breakpoints with their properties
   and options, switched-off ones left out, and starts from the saved exception, method and
   field breakpoints (SPEC-0146); a line the arguments name is set as the arguments say (or not
   at all with `enabled: false`), and a file is always sent under its resolved path, so the same
   file never goes to the adapter under two spellings, the second replacing the first. A
   breakpoint an agent gives with `enabled: false` is not set. Before, an agent could neither
   change breakpoints after the start nor start with what `set_breakpoint` saved. [STD, ADR-0008]

17. **Go (v7):** in a Delve session (SPEC-0039 v7) the condition, the pass count and the log
    message are Delve's own: the condition is a Go expression, the pass count has the forms of
    SPEC-0178 (`N` alone is the N-th hit) and a log message's `{expr}` parts are Go expressions.
    The message reaches the debug console with Delve's goroutine prefix. The options of behaviours
    6–11 stay Ferris IDEA's at each hit. [OSS: Delve's public documentation]

18. **Toggle Temporary Line Breakpoint (v8):** `ToggleTemporaryLineBreakpoint` (⌥⇧⌘F8 in the
    macOS keymap, Ctrl+Alt+Shift+F8 in the default one; Run › Toggle Temporary Line Breakpoint,
    Find Action) acts on the caret line of the focused editor like Toggle Line Breakpoint
    (SPEC-0091 behaviour 1): on a line without a breakpoint it sets one whose only property is "Remove once
    hit" (behaviour 9), so it stops the program once and is gone — from the session, the editor
    and the saved breakpoints; on a line that has a breakpoint, temporary or not, it removes it
    with its properties. The new breakpoint is drawn, saved, listed in the dialog and sent to a
    running session like any breakpoint with an option (behaviour 13). Without an editor the
    action does nothing. Agents set the same breakpoint with `set_breakpoint` and
    `remove_once_hit: true`; no further tool is needed. [DOC: a temporary line breakpoint is
    removed when it is hit; OSS: the action id and its shortcuts in the bundled keymaps; GEN]

19. **Small popup at the gutter (v10, OBS-0054 B2):** ⇧⌘F8 / Ctrl+Shift+F8 (`EditBreakpoint`) with a
    line or method breakpoint on the caret line, and a right click on a breakpoint mark, open a
    small popup beside that line in the gutter instead of the dialog. It holds Enabled (⌥E),
    Suspend (⌥S) with All (⌥A) or Thread (⌥T) for a line breakpoint (a method breakpoint
    stops as its debugger does, so it shows no Suspend), the Condition field (focused), for a
    method breakpoint Method entry (⌥N) and Method exit (⌥X) (SPEC-0146 v6 behaviour 13), "More
    (⇧⌘F8)" and Done. Enter (Done) applies and closes; ⇧⌘F8 in the popup applies and opens the
    Breakpoints dialog with that breakpoint selected; Escape closes without applying. Without a
    breakpoint on the caret line ⇧⌘F8 opens the dialog as before; View Breakpoints (Run menu,
    Find Action) always opens the dialog. Changes are saved and sent as the dialog's are
    (behaviour 3). Before, ⇧⌘F8 always opened the large dialog. [OBS]
20. **Suspend (v10, OBS-0054 B3):** in the dialog and the popup Suspend is a check box — off: the
    breakpoint does not stop, only its messages are written (behaviour 6 "none") — and, when on,
    a choice of All or Thread. A breakpoint without a choice stops as its debugger does and shows
    neither All nor Thread chosen (since v11 in the popup and since v12 in the dialog: All is shown
    chosen, behaviours 24 and 27). ⌥S, ⌥A and ⌥T in the dialog as in the popup. [OBS]
21. **A check box per row (v10, OBS-0054 B3):** each line and method breakpoint row of the
    dialog's list starts with a check box for Enabled, and each language's exception row with one
    that is on while any of its exception options is; ⌥E switches the selected row at once (saved
    and sent like Enabled): a breakpoint on or off; an exception row off (all its options) or on
    (uncaught). [OBS]
22. **Disable until hitting (v10, OBS-0054 B4):** chosen from the other line breakpoints of the
    workspace as the list names them, or none: ⌥D steps to the next choice (none comes first and
    after the last). "After hit" is "Disable again" or "Leave enabled" (⌥K switches; the
    `depends_keep` of behaviour 10). The free text field is gone; `set_breakpoint` keeps taking
    `depends_on`. [OBS]

23. **The popup's look (v11, OBS-0056 P2, P3):** the popup is headed by the kind of breakpoint —
    "Line breakpoint", "Method breakpoint" — and under it, dimmed, the place (`Inventory.java:8`,
    `lab.inventory.Inventory.main`). Enabled, Suspend, Method entry and Method exit are check
    boxes (a box and the label, the mnemonic dimmed after it); All and Thread are option buttons
    under Suspend, shown while Suspend is on. A click on a row switches it as its key does; ⌥E,
    ⌥S, ⌥A, ⌥T, ⌥N and ⌥X stay. "More (⇧⌘F8)" is a link-like text, Done the default button.
    Until v10 every switch was a button. [OBS]
24. **All chosen (v11, OBS-0056 P4):** a line breakpoint without a choice of threads (behaviour
    20) shows All chosen while Suspend is on, as the reference does. Applying keeps the breakpoint
    without a choice as long as nothing else changed; once anything changes, All is saved with
    it, so the breakpoint then stops all threads as the popup showed. ⌥T still chooses Thread.
    [OBS, GEN]
25. **Watch (v11, OBS-0056 P5):** for a method breakpoint Method entry and Method exit stand under
    the heading "Watch", one under the other. The reference's third option there, Emulated, is not
    offered: it switches between the debugger's own method events and breakpoints the IDE sets on
    the method's first and last lines; Ferris IDEA has only one way (a function breakpoint for the
    entry, its own emulation of the exit, SPEC-0146 v6 behaviour 13), so the switch would change
    nothing. Named under Known deviations. [OBS, GEN]
26. **Condition (v11, OBS-0056 P6):** ⇧↩ in the condition field expands it to five lines (and back);
    while expanded, ↩ starts a new line and ⌘↩ (Ctrl+Enter) is Done; collapsed, ↩ is Done. ↑ and ↓
    in the collapsed field step through the conditions used last — the most recent first, at most
    20, the same condition once — and back to the text being typed. A condition becomes recent when
    the popup applies it. The list is kept while Ferris IDEA runs, not in a file. [OBS,
    GEN]

27. **All chosen in the dialog (v12, OBS-0056 D1):** the Breakpoints dialog shows a line breakpoint
    without a choice of threads with All chosen while Suspend is on, as the popup does
    (behaviour 24), and saves All with it once anything of the breakpoint changes; a breakpoint left
    as it was keeps having no choice. Until v11 the dialog showed neither All nor Thread. [OBS]
28. **The dialog's switches (v12, OBS-0056 D2):** the form of a line breakpoint lists Enabled,
    Suspend, All and Thread (option buttons indented under Suspend, shown while it is on),
    Log "breakpoint hit" message, Log stack trace and Remove once hit one under the other as check
    boxes, as the popup's rows look (behaviour 23): a box, the label and, dimmed, the mnemonic
    (⌥E, ⌥S, ⌥A, ⌥T). A click on a row switches it as its key does; the keys are unchanged. Each
    row carries its role (check box, option), name and state in the semantic tree (SPEC-0319), as do
    "Disable until hitting" (its choice as value) and "After hit". Until v11 they were buttons side
    by side. [OBS]
29. **One breakpoint, two views (v13, OBS-0056 D1, D2):** the popup (behaviour 19) and the dialog
    show a line breakpoint's Enabled, Suspend and the choice All/Thread by one rule, from the
    breakpoint's own saved state, never from a state of their own: without a choice of threads
    both show All chosen while Suspend is on (behaviours 24 and 27), with Suspend off neither shows
    All or Thread, and both save All with the first other change. What one of them applies is
    what the other shows when it opens next, also when More (⇧⌘F8) applies the popup and opens the
    dialog at once. The popup's switches carry their role (check box, option), name, state and
    test id in the semantic tree (SPEC-0319) as the dialog's do (behaviour 28), so agents and
    window tests read what the popup shows. The dialog's further switches (the two log options,
    Remove once hit) stay the dialog's. The named deviations stay: no Emulated switch, and a
    method breakpoint shows no Suspend in either view (behaviours 19 and 25). Until v12 the two
    views agreed in their results but each had its own copy of the rule. [OBS, GEN]
30. **Keys in the system's notation (v14):** the popup's button "More" names the first shortcut of
    View Breakpoints (`ViewBreakpoints`) in the keymap in use — "More (⇧⌘F8)" on macOS with the
    bundled keymap, "More (Ctrl+Shift+F8)" on Linux and Windows with the default one, "More" when
    the action has no shortcut. The popup's mnemonics (⌥E, ⌥S …) and its hint "⇧↩ collapse · ⌘↩
    done", and the dialog's mnemonics and buttons ("+ Method Breakpoint (⌘N)", "+ Field Watchpoint
    (⇧⌘N)", "(⌥D)", "(⌥K)", "(⌥C)", "(⌥U)"), are written for the platform as SPEC-0028 v7
    behaviour 12 says: `Alt+E`, `Shift+Enter collapse · Ctrl+Enter done`, `Ctrl+N`, `Ctrl+Shift+N`.
    The keys themselves do not change. [GEN]
31. **Switching a breakpoint off and on (v15):** `ToggleBreakpointEnabled` (Run › Toggle
    Breakpoint Enabled, after Toggle Temporary Line Breakpoint; Search Everywhere; no shortcut in
    the bundled keymaps) switches the breakpoint on the caret line of the active editor off, or
    on again, and keeps its condition and every other property; on a line without a breakpoint
    it does nothing. ⌥-click (Alt-click) on a line number does the same for that line's
    breakpoint and, on a line without one, sets a temporary breakpoint as
    `ToggleTemporaryLineBreakpoint` does (behaviour 18); a plain click still sets or removes. The
    change is saved and sent to a running session as any change of Enabled (behaviour 3); the
    gutter shows the ring of behaviour 1. MCP: `set_breakpoint` with `enabled` (behaviour 5).
    [OSS, DOC]
32. **Remove all (v15):** `Debugger.RemoveAllBreakpoints` removes every line breakpoint of the
    workspace, also of files without an editor, and `Debugger.RemoveAllBreakpointsInFile` those
    of the active editor's file, each with its properties; both are reached through Search
    Everywhere ("Remove All Breakpoints", "Remove All Breakpoints in File", group Run) and have no
    menu item and no shortcut, as in the reference. The change is saved and sent to a running
    session; the status bar says how many were removed ("3 breakpoints removed", "1 breakpoint
    removed in this file", "No breakpoints to remove"). Exception, method and field breakpoints
    (SPEC-0146) are kept. MCP: `set_breakpoint` with `remove_all: true` removes those of `path`,
    or of the whole workspace with `path` `.`, answers `removed` with the count and, as every
    write of the tool, only previews with the default `dry_run: true`. [OSS, GEN, ADR-0008]

## Acceptance criteria
- **AK-1** Looks, properties following moved breakpoints, what is sent to the debugger (switched-off
  ones left out), saving and reading (`ferris_idea_debugger::saved` tests).
- **AK-2** MCP: list, preview, set, switch off, remove, removing a missing breakpoint refused
  (`crates/mcp/tests/it/protocol.rs`: `saved_breakpoint_tools`).
- **AK-3** Window self-test (`breakpoint_steps`): ⇧⌘F8 on the caret's breakpoint opens its popup,
  not the dialog, and ⇧⌘F8 in the popup lists the file's breakpoints with the caret's selected
  (behaviour 19; restated in v13.1); a condition `i === 3` is applied with Enter and shown; with js-debug a Node.js loop stops
  only on that pass (`total = 3`) while a switched-off breakpoint above is skipped, and runs to the
  end on resume; the condition moves with an edit; Remove takes both away.
- **AK-4** Hand test (round 4): conditional breakpoint, logpoint and switched-off breakpoint in a C
  and a Python program next to the reference.
- **AK-5 (v3)** Against a fake adapter (`crates/debugger/tests/it/breakpoint_rules.rs`): a dependent
  breakpoint waits for its master, without "stay enabled" again after each own hit; hit message
  and stack with "none" (six messages, no stop); removal after the first hit; instance, class and
  caller filters; "this thread" explained once for an adapter that stops every thread. Saving and
  reading the options with the dependency relative (`saved::tests::breakpoint_rules_round_trip`);
  the published schema (`format::tests::published_schemas_match_the_shapes`).
- **AK-6 (v3)** With real jdtls and java-debug (`crates/workbench/tests/it/java_debug_more.rs`): a
  breakpoint with "all threads" leaves the worker thread suspended, evaluations still answer, and
  Resume runs every thread on.
- **AK-7 (v3)** MCP (`saved_breakpoint_tools`): the options saved and listed, the dependency
  relative, a wrong `suspend` refused.
- **AK-8 (v3)** Window self-test steps `breakpoint_option_steps` (written, not yet run in the
  window): the dialog saves suspend, hit message, removal and a class filter and reads them back.
- **AK-v4** `breakpoint_dialog::tests::rows_name_the_file_first`; window self-test `breakpoints` 10/10.
- **AK-9 (v5)** Against a scripted adapter (`crates/debugger/tests/it/breakpoint_binding.rs`): the
  answer's `verified` and `message` reach the placed breakpoints with the line asked for, a later
  `breakpoint` event that binds one is reported once as a change of its file, and the pending
  breakpoints carry the adapter's reason. Unit tests: which lines of a file are not bound and why
  (`debug_model::tests::unbound_lines_by_requested_line`), the tooltip line
  (`tooltip::tests::breakpoint_not_bound_line`). MCP: the shape of a placed breakpoint
  (`session::tests` serialisation). In the window: to be seen on the Mac with a Kotlin or Java
  session (hollow until bound, filled after the end).
- **AK-10 (v6)** MCP dry runs (`crates/mcp/tests/it/protocol.rs`: `debug_start_takes_the_saved_breakpoints`):
  the saved breakpoints in a start with the switched-off one left out, a line of the arguments
  winning, one file under one path, `enabled: false`; `debug_breakpoints` without a session and a
  list without `path` refused. The breakpoints after `breakpoint` events
  (`crates/debugger/tests/it/breakpoint_binding.rs`: `DebugSession::placed`).
- **AK-11 (v6)** With real debugpy (`python_breakpoints_change_while_the_program_runs`; skipped
  without it): while the program runs, `debug_breakpoints` sets a breakpoint (the switched-off one
  of the list is not set) that the next wait reports on its line; the list afterwards; an empty
  list takes it away. By hand through MCP on the Mac, 2026-11-13, also with lldb-dap, js-debug
  and java-debug (a condition `n == 30` set while a loop ran stopped once with `n = 30`).
- **AK-12 (v7)** As SPEC-0039 AK-12: condition, hit count and log message with a real Delve, each
  marked as evaluated by the adapter.

- **AK-13 (v8, behaviour 18)** The properties of a temporary breakpoint (only "Remove once hit",
  not plain, the look of a breakpoint with an option), toggling on a free line and on a line with
  a breakpoint, and the properties of the other lines staying
  (`ferris_idea_debugger::saved::tests::a_temporary_breakpoint_is_removed_once_hit_and_toggles`);
  the action has its title, its place in the Run menu and a shortcut in both keymaps
  (`everywhere::tests::run_debug_and_window_actions_from_the_keymap`,
  `menus::tests::run_debug_and_window_actions_are_in_their_menus`). In a window of the test
  harness the action sets the breakpoint on the caret line, saves it with `remove_once_hit`,
  removes it again and removes a plain one
  (`run_debug_actions_keymap::the_temporary_breakpoint_is_set_saved_and_removed`). On screen and
  with a debug session that hits it: not checked.

- **AK-14 (v9, behaviours 1–3; SPEC-0039 v6 behaviour 3a)** The steps of hand test 54 that the
  agent run of 2026-11-13 left out, in a window of the test harness with real keys and real
  debugpy (`crates/workbench/tests/it/breakpoint_dialog_keys.rs`:
  `hand_test_54_condition_moves_with_an_inserted_line_and_a_failed_condition_is_told`; skipped
  without debugpy): ⌘F8 sets two breakpoints in a Python loop, ⇧⌘F8 opens the dialog on the
  caret's one with the focus in Condition, `i == 3` is typed and applied with Enter; a line typed
  above both moves the breakpoints and the condition one line down, saved so; Debug stops once
  with `i = 3`, Resume runs to the end; a condition naming an unknown variable, typed over the old
  one in the dialog, stops at the first hit and the debug console names the `NameError`. Not
  covered: a C program, the switched-off breakpoint (no key switches it), the look on screen,
  the dialog next to the reference.

- **AK-15 (v10, behaviours 19–22)** In a window of the test harness with real keys (`crates/workbench/tests/it/breakpoint_popup_keys.rs`):
  `the_popup_edits_a_breakpoint_and_more_opens_the_dialog` — ⇧⌘F8 on a breakpoint opens the popup
  with the focus in Condition, typed `n > 2`, ⌥S off and on, ⌥T, Enter saves condition and
  suspend `thread`; ⇧⌘F8 again, ⌥E, Escape changes nothing; ⇧⌘F8, ⇧⌘F8 opens the dialog with the
  breakpoint selected; on a line without a breakpoint ⇧⌘F8 opens the dialog; on a method
  breakpoint the popup offers ⌥N/⌥X and saves `exit`, refusing to switch both off.
  `exception_breakpoints_per_language_and_row_checks` — ⌥E in the list switches the selected
  breakpoint off and on at once; ⌥D chooses the other breakpoint as the dependency, ⌥K leaves it
  enabled, both saved; ⌥A in the dialog sets suspend `all`. Unit tests of the popup model
  (`breakpoint_popup::tests`).

- **AK-16 (v11, behaviours 23–26)** Unit tests of the popup model (`breakpoint_popup::tests`):
  the title and place of a line and of a method breakpoint; the rows as check boxes and options
  with their keys; All shown chosen for a breakpoint without a choice, nothing emitted when nothing
  changed, All saved with another change, ⌥T choosing Thread; the history (most recent first, no
  duplicates, at most 20, stepping and back to the typed text). In a window of the test harness
  with real keys (`crates/workbench/tests/it/breakpoint_popup_keys.rs`:
  `the_popup_shows_kind_rows_and_history`): ⇧⌘F8 on a line breakpoint shows "Line breakpoint",
  All chosen; a condition typed and applied comes back with ↑ in the next popup; ⇧↩ expands, ↩
  adds a line, ⌘↩ applies a two-line condition; on a method breakpoint the popup says "Method
  breakpoint" and shows the Watch rows.

- **AK-17 (v12, behaviours 22, 27 and 28)** In a window of the test harness, by real keys and by
  clicks on controls found through the semantic tree (`crates/workbench/tests/it/breakpoint_popup_keys.rs`:
  `the_dialogs_switches_are_a_list_and_the_dependency_is_chosen`): with two line breakpoints the
  dialog shows All chosen; the seven switches stand one under the other with All and Thread
  indented, checked as the breakpoint is; a click chooses Thread and ⌥A goes back to All; a click
  on Log "breakpoint hit" switches it; clicks on "Disable until hitting" step from None to the other
  breakpoint and back; a click on "After hit" gives "Leave enabled"; after Done the dialog opened
  again shows Thread, the log switch, the dependency and "Leave enabled". This closes OBS-0054 B4
  without a screen.

- **AK-18 (v13, behaviours 27–29)** In a window of the test harness, by real keys and by clicks on
  controls found through the semantic tree (`crates/workbench/tests/it/breakpoint_popup_keys.rs`:
  `the_dialog_and_the_popup_show_one_state`): on a line breakpoint without a choice of threads
  ⇧⌘F8 opens the popup with All chosen, and More (⇧⌘F8) opens the dialog with All chosen and
  Thread not, nothing saved; the dialog's switches are check boxes one under the other with All
  and Thread as options under Suspend; a click on Suspend switches it off (All and Thread go) and
  Done saves it — the popup then shows Suspend off without All and Thread; in the popup ⌥S and ⌥T,
  then More: the dialog shows Thread chosen; a click on Enabled and Done — the popup shows Enabled
  off and Thread chosen. Unit tests: the dialog's shared switches equal the popup's rows for every
  Suspend state (`breakpoint_dialog::tests::shared_switches_are_the_popups_rows`); All saved only
  with another change (`breakpoint_popup::tests::all_is_saved_only_with_a_change`). Not covered:
  the look on screen next to the reference (window recheck).
- **AK-19 (v14, behaviour 30)** Unit test `breakpoint_popup::tests::keys_follow_the_platform`:
  "More (⇧⌘F8)" with the bundled macOS keymap on macOS, "More (Ctrl+Shift+F8)" with the default
  keymap on Linux, "More" without a shortcut; the hint and a mnemonic for both.
- **AK-20 (v15, behaviours 31 and 32)** Unit tests: switching Enabled keeps the condition, does
  nothing without a breakpoint and switches back to a plain breakpoint; removing all of one file
  keeps the other file's breakpoints and properties, removing all removes the rest, each counted
  (`ferris_idea_debugger::saved::tests::enabled_is_toggled_and_all_breakpoints_are_removed`); the
  status bar's text for none, one and several
  (`debug_model::tests::the_removal_of_all_breakpoints_is_counted`).
- **AK-21 (v15, behaviour 32)** MCP (`saved_breakpoints_are_removed_all_at_once`): three saved
  breakpoints in two files; `remove_all` on `.` as a dry run reports 3 and removes nothing; on
  `a.py` it removes its two and keeps `b.py`'s with its condition; on `.` the last one, after which
  the breakpoints file is gone.
- **AK-22 (v15, behaviours 31 and 32)** In a window of the test harness
  (`crates/workbench/tests/it/breakpoint_toggles.rs`): Toggle Breakpoint Enabled does nothing on a
  free line, switches a breakpoint with a condition off and on with the condition kept and the
  breakpoint still in the gutter (`enabled_is_toggled_by_the_action_and_by_alt_click`); a real ⌥
  click on its line number switches it off and on, on a free line sets a temporary breakpoint, and
  a plain click removes that one again; Remove All Breakpoints in File empties the editor's gutter
  and keeps the saved breakpoint of a file without an editor, Remove All Breakpoints removes that
  one too, the status bar says "2 breakpoints removed in this file", "1 breakpoint removed" and
  "No breakpoints to remove", and the three actions have their titles in Search Everywhere
  (`all_breakpoints_are_removed_in_the_file_and_everywhere`). Not covered: a real mouse and the
  look on screen.

## Out of scope
Conditions with completion, breakpoints of the reference's own project files, user-made groups of
breakpoints (the reference can move breakpoints into named groups), evaluate-and-log of an
expression besides the log message (the log message's `{expr}` parts do that).

## Known deviations from the reference
The reference's dialog also lists exception breakpoints and offers more options; Ferris IDEA offers
line breakpoints with the DAP properties. Not compared side by side. **(v3)** The reference opens a
small window on a right click and the large dialog with "More"; Ferris IDEA opens the dialog at once
(OBS-0025 L13). The options of behaviours 6–11 cost a short stop at each hit, also for hits that
go on. The dependency is written as `path:line` instead of chosen from a list (v10: chosen from the list). **(v5)** The
reference's own mark for a breakpoint that cannot be bound was not compared; Ferris IDEA's hollow
ring and its tooltip line are its own. **(v11)** The popup offers no Emulated switch for method
breakpoints (behaviour 25); the recent conditions are kept for the running IDE only, the
reference keeps them across restarts (behaviour 26). **(v13)** Neither the popup nor the dialog
offers Suspend for a method breakpoint, which stops as its debugger does (behaviours 19 and 29);
the reference offers it there too.
