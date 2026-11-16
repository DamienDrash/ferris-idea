# SPEC-0205: Terminal sessions — names, splits, end of a shell, restore

- **Version:** 3 · **Date:** 2026-11-05 (v2: terminals of SSH logins and remote runs stay open; v3:
  a split across the first one stacks shells inside a cell)
- **Sources:** GEN (integrated terminals of IDEs: tabs with stable names that can be renamed, a tab
  split into several shells side by side or one above the other, a tab that closes when its shell
  ends, tabs reopened with their folders when the project opens again; F-076), STD (OSC 7 current
  folder), SPEC-0062 v2 (tabs), SPEC-0161 (current folder), SPEC-0118 (selection)
- **Feature matrix rows:** F-076

## Purpose
SPEC-0062 v2 gave the terminal several tabs. The names shifted when a tab closed, a tab could not be
renamed or split, a shell that ended left a dead tab, and the tabs were gone after a restart.

## Behaviour
1. **Names:** a new tab is called "Local", the next ones "Local (2)", "Local (3)", … with the lowest
   number not in use; closing a tab does not rename the others. [GEN]
2. **Rename:** a double click on a tab, or `Terminal.RenameSession`, turns its name into a field with
   the name selected; Enter keeps the new name (an empty name keeps the old one), Escape or leaving
   the field cancels. [GEN]
3. **Split:** ⌘D (Ctrl+Shift+D elsewhere) or `Terminal.SplitVertically` starts a new shell to the
   right of the shell with focus, ⇧⌘D (Ctrl+Shift+Alt+D) or `Terminal.SplitHorizontally` below it,
   in the shell's current folder (SPEC-0161). The tab's first split chooses its cells: a row or a
   column. A later split in the same direction adds a cell after the one with focus; a split across
   it puts the new shell into the cell with focus, below or beside the shell (v3, hand test 140,
   D47: ⇧⌘D after ⌘D also split to the right). Cells and the shells in a cell share the space
   equally, and the cells are restored with the tab (behaviour 6); a click focuses a shell, ⌥⌘→/⌥⌘← (Ctrl+Alt+Right/Left)
   or `Terminal.NextSplitter`/`Terminal.PrevSplitter` move the focus to the next or previous one. [GEN]
4. **Close:** ⌘W (Ctrl+Shift+W) closes the shell with focus; a tab closes with its last shell, the
   window hides with the last tab (as SPEC-0062 v2). × on a tab closes all its shells. [GEN]
5. **End of a shell:** when the user's shell ends (e.g. `exit`), it closes as if by ⌘W. Terminals that
   run an SSH login or a remote run (SPEC-0121) stay open with their output. A program that fails to
   start shows its error instead. [GEN]
6. **Restore:** the tabs of a project are written to `.idecraft/terminal-sessions.json` whenever tabs
   or splits change and when the IDE quits: per tab its name and per shell its current folder and the
   last up to 1,000 lines of its history (screen included, without trailing empty lines). Opening the
   terminal of the project the next time starts the saved tabs, splits and names, each shell in its
   saved folder (the project folder when it no longer exists), with the saved lines in front of the
   new shell's first prompt. SSH terminals (SPEC-0121) are not saved. [GEN]
7. **MCP:** `terminal_sessions` reads the saved tabs (names, folders, line counts and the last lines);
   it is read-only. [ADR-0008]

## Acceptance criteria
- **AK-1** Name numbering, the saved-state format with its line cap, and history lines put in front of a
  new screen (unit tests).
- **AK-2** A real shell that runs `exit` ends its session (`pty` integration test).
- **AK-3** In a real window (`idecraft ui --selftest`): rename a tab, split right, the second shell has
  focus and runs a command, ⌘W closes it, `exit` closes a tab, and a saved state is restored with its
  names and lines.
- **AK-4** MCP `terminal_sessions` (protocol test).
- **AK-5** By hand next to the reference (round 4, no. 140).

## Out of scope
Dragging shells between tabs or splits, resizing splits with the mouse, nested splits in both
directions, restoring the programs that ran (only the folder and the output come back).

## Known deviations from the reference
Not compared side by side yet.
