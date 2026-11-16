# SPEC-0118: Mouse in the terminal

- **Version:** 2 · **Date:** 2026-11-05 (v2: paste)
- **Sources:** GEN (terminals in IDEs pass the mouse to programs that ask for it, scroll their
  history with the wheel, and select and copy text with the mouse; F-076), STD (xterm control
  sequences: private modes 9, 1000, 1002, 1003 for mouse tracking, 1005 and 1006 for the UTF-8 and
  SGR encodings, button codes 0–2, 3 for a release, 64/65 for the wheel, +4/+8/+16 for
  Shift/Meta/Control, +32 for motion), SPEC-0038 (screen model), SPEC-0062 (terminal tool window),
  ADR-0008
- **Feature matrix rows:** F-076

## Purpose
Use full-screen programs (editors, pagers, `htop`, `tmux`) with the mouse, read output that has
scrolled away, and copy from the terminal.

## Behaviour
1. **Reporting:** the screen model keeps the mouse mode (off, X10 = 9, press = 1000, drag = 1002,
   motion = 1003) and encoding (default, UTF-8 = 1005, SGR = 1006) set by the program; a reset
   turns them off. While a mode is on, presses, releases, the wheel and — for drag and motion —
   movement are sent to the program in its encoding, with the cell under the mouse (1-based) and
   Shift, Alt and Control. X10 sends presses only, without modifiers. Positions the default encoding
   cannot hold (beyond 223) are not sent. Holding Shift selects text instead. [STD]
2. **Wheel:** with reporting on, each wheel line is a report; in a full-screen program without
   reporting (alternate screen) it sends ↑/↓ (in the program's cursor-key mode); otherwise it
   scrolls the view through the history, at most 10 lines per event. [STD, GEN]
3. **History view:** the view shows scrollback and screen as one history; while scrolled back it
   stays on the same lines when new output arrives, and hides the cursor. Typing or pasting returns
   to the live screen. [GEN]
4. **Selection:** with reporting off (or Shift held), a drag selects characters, a double click a
   word (letters, digits and `_-./~:@%+=#?&`, so paths and URLs stay whole), a triple click whole
   lines, across scrollback and screen. The selection is painted over the text; a click without a
   drag clears it, typing clears it. ⌘C (macOS) or Ctrl+Shift+C copies it as plain text, trailing
   blanks dropped, lines joined with `\n`; without a selection the shortcut goes to the IDE or the
   program as before. ⌘V (macOS) or Ctrl+Shift+V, and Edit › Paste while the shell has the focus,
   send the clipboard's text with line ends as Return, in bracketed-paste marks when the program
   turned mode 2004 on (v2, hand test 29, D1: ⌘V pasted nothing). [GEN, STD]
5. **MCP:** `terminal_render` also reports the mouse mode and encoding the output set, and encodes
   given mouse events as the terminal would send them (`null` when not reported). [ADR-0008]

## Acceptance criteria
- **AK-1** Modes and encodings from the output, reset; which events each mode reports; SGR codes
  with modifiers, release and motion; default and UTF-8 encodings and their limits; selection by
  character, word and line, across the scrollback (unit tests `screen::tests`, `mouse::tests`,
  `selection::tests`).
- **AK-2** MCP: drag mode with SGR, a press with Control, drag, unreported motion and release;
  an unknown action is refused (`crates/mcp/tests/it/protocol.rs`: `terminal_render`).
- **AK-3** Window self-test with a real shell: after `seq 1 300` the wheel scrolls back ten lines,
  a triple click selects the top line, ⌘C puts it on the clipboard, typing returns to the live
  screen; after `printf '\033[?1000h\033[?1006h'` the shell's `cat -v` shows SGR press, release
  and wheel reports; switching the mode off again.
- **AK-4** Hand test (round 4): `vim` with `:set mouse=a`, `htop`, `less`, scrolling back and
  copying in a shell, next to the reference.

## Out of scope
Highlight tracking (mode 1001), URXVT (1015) and pixel (1016) encodings, clicking links and file
paths, middle-click paste, a scroll bar, rectangular selection, selection following the content
when the history overflows its limit.

## Known deviations from the reference
The wheel sends arrow keys in full-screen programs without mouse reporting (common terminal
behaviour). Not compared side by side yet.
