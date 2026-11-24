# SPEC-0194: Pause, Run to Cursor and Set Value in the debugger

- **Version:** 4 · **Date:** 2026-11-05 (v2: the parent's summary follows a changed field; v3: the
  Debug window's toolbar with rerun, pause, run to cursor, mute and view breakpoints, OBS-0014; v4:
  the toolbar buttons have tooltips with the shortcut of the keymap in use, SPEC-0294)
- **Sources:** GEN (IDE debuggers pause a running program, run to the line at the caret, and change a
  variable's value while paused; F-070), STD (Debug Adapter Protocol `pause`, `setBreakpoints`,
  `continue`, `setVariable` with capability `supportsSetVariable`, `scopes`), OSS (action ids `Pause`,
  `RunToCursor` with ⌥F9 / Alt+F9, `XDebugger.SetValue` with F2, `Rerun` with ⌘R / Ctrl+F5,
  `XDebugger.MuteBreakpoints`, `ViewBreakpoints` with ⇧⌘F8 / Ctrl+Shift+F8 in the ported keymaps,
  `JetBrains/intellij-community`), OBS (OBS-0014: the reference's debug toolbar offers rerun, pause,
  run to cursor, mute and view breakpoints, Ferris IDEA only resume, three steps and stop), SPEC-0039,
  SPEC-0054, SPEC-0091, SPEC-0127, IMP-0022, ADR-0008
- **Feature matrix rows:** F-070

## Purpose
Stop a program that runs too long to see where it is, get to a line without placing a breakpoint, and
try a different value without restarting.

## Behaviour
1. **Pause** (`Pause`, Run menu): while the program runs, the session asks the adapter to pause the
   thread shown last, or the first thread the adapter lists. The program then stops like at any other
   stop (reason `pause`) with stack and variables. Xcode's lldb-dap reports that stop as the signal
   `SIGSTOP`; right after a pause request Ferris IDEA reports it as `pause`. While paused or not debugging
   it does nothing. [STD]
2. **Run to Cursor** (`RunToCursor`, ⌥F9 / Alt+F9): while paused, the session adds a breakpoint on the
   caret line of the active editor to that file's breakpoints, continues the shown thread and, at the
   next stop wherever it is, puts the file's own breakpoints back. An adapter that moves the
   breakpoint to the next line with code stops there. If the adapter does not verify the added
   breakpoint (no code at or after that line), the file's breakpoints are put back, the program is not
   resumed and the console says so. [GEN, STD]
3. **Set Value** (`XDebugger.SetValue`, F2 in the Debug window): a click selects a row of the Variables
   list; F2 on a selected row puts its value into the expression field, selected, and the field then sets
   instead of evaluating: Enter sends `setVariable` with the row's container (the frame's local scope
   for a top-level variable, the parent's reference for a child), name and the new text; Escape leaves
   setting. The answer's value replaces the row; an error from the adapter is shown as a result line.
   After a field changed, the top-level variables take their new values and types, so the object's
   summary shows the new field, while open rows stay open (v2, hand test 125). [GEN, STD]
4. An adapter without `supportsSetVariable` is refused before asking it, with "The debug adapter cannot
   change values" (improvement IMP-0022: unsupported features are refused, not ignored). [STD]
5. **MCP** (right `debug`): `debug_pause` pauses the MCP session's program and returns the state as
   `debug_resume` does; `debug_run_to_line` (path, 1-based line) runs to that line; `debug_set_variable`
   (name, value, optional frame id, default the top frame, or the reference of a structured variable
   for one of its fields) changes the variable, logs the change in the agent log and returns the new
   value. [ADR-0008] **(v3)** Rerun for an agent is `debug_start` again. Muting is not offered
   through MCP: an MCP session gets exactly the breakpoints its agent sets, so it has nothing to
   mute.
6. **Toolbar (v3):** the Debug window's toolbar holds, in this order: Rerun, Resume, Pause, Stop, a
   gap, Step Over, Step Into, Step Out, Run to Cursor, a gap, View Breakpoints and Mute Breakpoints.
   Rerun is enabled once the window has started a session (also after it ended); Pause while the
   program runs; Resume, the steps and Run to Cursor while it is paused; Stop while a session goes;
   View and Mute Breakpoints always. Mute Breakpoints is drawn pressed while breakpoints are muted.
   [GEN, OSS, OBS]
7. **Rerun** (`Rerun`, ⌘R / Ctrl+F5 while the Debug window has the focus; elsewhere the key keeps
   its other bindings, such as Replace in the editor): the running session, if any, ends, and the
   same configuration starts again (or attaches again to the same target) with the workspace's
   current line, exception and method breakpoints. Console, results and the state start over as at
   any start; watches stay. [GEN, OSS]
8. **Mute Breakpoints** (`XDebugger.MuteBreakpoints`, toolbar, Search Everywhere): while muted, the
   session gets no line, exception or method breakpoints; the breakpoints stay saved and in the
   gutter, drawn grey. Muting again (unmuting) sends them all again. A session started while muted
   starts without breakpoints. Run to Cursor still stops (its breakpoint is its own). Muting lasts for
   the window, not across restarts. [GEN, OSS]
9. **View Breakpoints** in the toolbar opens the Breakpoints dialog (`ViewBreakpoints`, SPEC-0127).
   [OSS]
10. The toolbar's own icons (pause, run to cursor, rerun, mute, view breakpoints) are Ferris IDEA's own
   16 px drawings in the style of the open New UI icons; the platform's open icons can replace them
   when the bundled icon set is next updated. [ADR-0009]
11. **Tooltips (v4).** Every toolbar button shows its action's name and its shortcut in the keymap in
   use after a short hover (Rerun ⌘R, Resume, Pause, Stop, Step Over, Step Into, Step Out, Run to
   Cursor, View Breakpoints, Mute Breakpoints), as SPEC-0294 describes. [OSS, SPEC-0294]

## Acceptance criteria
- **AK-1** Session (integration test with `lldb-dap`; skipped where it cannot run): a program in an
  endless loop is paused and stops with reason `pause`; Run to Cursor from a breakpoint stops on a later
  line, a line past the code is refused, and only the file's own breakpoint is left afterwards; setting
  `int x` to 42 makes `evaluate x` answer 42. Through MCP (protocol test with lldb-dap) Set Value and
  Run to Cursor work the same way.
- **AK-2** The container of a row (local scope or parent reference) and the refusal without
  `supportsSetVariable` are unit tests.
- **AK-3** The MCP tools are listed and refused without the `debug` right (protocol test).
- **AK-4** In a real window (`ferris-idea ui --selftest`, with lldb-dap): ⌥F9 from the first breakpoint
  stops on the caret line; F2 on `x`, typing 42 and Enter shows 42 in the Variables list.
- **AK-5** Use by hand: Pause on a busy program, Run to Cursor, Set Value on a nested field.
- **AK-6 (v3)** The toolbar's enabled states per session state and the breakpoints sent while muted
  (none) or not are unit tests (`toolbar_states`, `muted_breakpoints_are_not_sent`). In a real window
  (`--selftest`, with lldb-dap), after a session ended: only Rerun and the breakpoint buttons are
  enabled; Rerun stops at the saved breakpoint again; Mute Breakpoints greys it (it stays in the
  gutter) and Rerun then runs the program to its end without stopping; unmuted, the View Breakpoints
  button opens the dialog.
- **AK-7 (v3)** By hand: the toolbar's look next to the reference, Pause and Run to Cursor from the
  toolbar, grey breakpoints while muted.
- **AK-8 (v4)** Every toolbar button is declared with a tooltip; Rerun reads `Rerun ⌘R` with the macOS
  keymap and `Rerun Ctrl+F5` with the default one (unit test); in a real window hovering Rerun shows
  it (self-test, SPEC-0294 AK-7).

## Out of scope
Force Run to Cursor (ignoring breakpoints on the way), Smart Step Into and Force Step Into, Drop Frame
/ restart frame, Jump to Line (`goto`), setting values through expressions (`setExpression`), memory
and disassembly views, data breakpoints, `runInTerminal`, muting kept across restarts.

## Known deviations from the reference
The reference edits a value inline in the tree; Ferris IDEA uses the expression field above the list.
Rerun ends a running program before starting again without asking. The toolbar was compared side by
side for Java in OBS-0014; its icons are Ferris IDEA's own (behaviour 10).
