# SPEC-0270: Keyboard diagnostics

- **Version:** 1 · **Date:** 2026-11-04 (hand test round 4 Nr. 8 and 9: ⌃⌥R did the wrong thing, ⌃G
  nothing, on a German layout, possibly with a keyboard that swaps Option and Command; the keymap was
  right and nothing reproduced)
- **Sources:** GEN (a diagnostic mode that shows each keystroke as the program receives it and what it
  did; modifier keys can be swapped by the operating system or keyboard software per keyboard),
  STD (macOS key events: the toolkit reports the key ignoring modifiers, the typed character, the
  modifier flags; ⌃ and ⌘ keystrokes do not type), SPEC-0059 (keymap dispatch), SPEC-0071 v4
  (shortcuts moved to the key in the same place on other layouts), ADR-0008 (MCP tool per feature)
- **Feature matrix rows:** F-017, F-018

## Purpose
Find out, on the user's own machine, why a shortcut does nothing or the wrong thing: whether the key
reaches IDECraft at all, with which key and modifiers, and which action it resolved to.

## Behaviour
1. **Switching:** the action `IDECraft.KeyboardDiagnostics` ("Keyboard Diagnostics", in Help ›
   Diagnostics and in Find Action) turns the mode on; running it again turns it off. Turning it on
   starts with an empty list and says where the log is. [GEN]
2. **Shown:** while on, a panel at the bottom right of the window lists the last 12 keystrokes, oldest
   first: time, modifiers and key as received, the US key a layout key stands in for (SPEC-0071 v4
   behaviour 6), the keystroke looked up as the keymap shows it (⌃G), the action that ran and who ran it
   (editor, workbench, project tree, completion list, menu bar) or `none` with the actions the keymap
   binds to it, and what had the focus (editor, project tree, popup, other). Changes of the held
   modifier keys get a row of their own ("modifiers held: ⌃⌥"), so a keyboard that reports ⌘ for the
   key labelled ⌥ shows up. The panel header names the keyboard layout. [GEN]
3. **Log:** each row is also appended to `keyboard-diagnostics.log` in the IDECraft log folder
   (`$IDECRAFT_LOG_DIR`, else `~/Library/Logs/IDECraft` on macOS, `%LOCALAPPDATA%\IDECraft\Logs` on
   Windows, `$XDG_STATE_HOME/idecraft/log` or `~/.local/state/idecraft/log` elsewhere), one line per
   keystroke: UTC time, event (`down`, `repeat`, `modifiers`) and tab-separated `name=value` fields
   `key`, `char`, `mods`, `us_key`, `stroke`, `bound`, `ran`, `focus`, `layout`. Over 1 MiB the log
   moves to `keyboard-diagnostics.log.1` first. [GEN]
4. **No typed text:** a keystroke without ⌃, ⌘ or fn that types a character and has no keymap action
   is typed text; its key and character are neither shown nor logged (`key=(typed)`), so the log never
   holds what was typed, e.g. a password. Shortcuts stay visible. [GEN]
5. **What it cannot see:** a keystroke the operating system or keyboard software takes before IDECraft
   leaves no row; that absence is the finding. The hardware key code is not available from the toolkit
   (GPUI 0.2); the layout name, the key as the layout reports it and the US key stand in for it. [STD]
6. **Agents:** the MCP tool `keyboard_diagnostics` returns the log's path, whether it exists, and its
   last `lines` lines (default 50, at most 500) parsed into fields. [ADR-0008]

## Acceptance criteria
- **AK-1** Rows and log lines of a shortcut, of typed text with ⌥ (hidden), of a bound ⌥ shortcut
  (shown) and of modifier changes; the mode records only while on and keeps 12 rows (unit tests
  `key_diagnostics::tests::records_shortcuts_and_hides_typed_text`,
  `key_diagnostics::tests::keeps_the_last_rows_while_on`).
- **AK-2** The log folder, appending, rotation over 1 MiB and reading the tail (unit test
  `logs::tests::appends_rotates_and_reads_the_tail`).
- **AK-3** MCP `keyboard_diagnostics` reads back lines written to the log with their fields (protocol
  test `keyboard_diagnostics_reads_the_log`).
- **AK-4** By hand (round 5, Nr. 8 and 9): with the mode on, ⌃⌥R and ⌃G on the tester's keyboard;
  the rows and the log say what arrived and what ran.

## Analysis of round 4 (2026-11-04)
How the toolkit (GPUI 0.2.2, macOS) delivers keys, read from its source: a key with ⌃ (and no input
method composing text) goes straight to the window's key handling and never through the text input
system (`interpretKeyEvents`), so ⌃G is neither turned into a control character nor swallowed; the key
is the layout's letter ignoring modifiers (`g` on QWERTZ too). Keys with ⌘ and no ⌃ go to the input
system first, which hands them back as a command. IDECraft's editor resolves ⌃G to
`SelectNextOccurrence` before the workbench sees it, and the menu bar's key equivalents only fire for
keys IDECraft did not handle. Nothing in the code explains Nr. 8 and 9; the remaining suspects are an
operating-system or keyboard-software modifier swap and an unfinished dead-key composition (German
`^`, `´`, `` ` ``), during which the editor leaves every key to the input method. The mode settles it.
[STD, GEN]

## Out of scope
Recording key-up events, mouse buttons, remapping keys in IDECraft to undo an operating-system swap
(the keymap settings, SPEC-0105, do that).

## Known deviations from the reference
Not compared with the reference. IDECraft shows the keystrokes live and never logs typed text.
