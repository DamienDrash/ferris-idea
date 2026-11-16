# SPEC-0062: Terminal tool window

- **Version:** 3 · **Date:** 2026-11-13 (v2: several terminal tabs; v3: keys with modifiers as the
  terminfo entry names them, pasted text without control characters)
- **Sources:** GEN (integrated terminals of IDEs; F-076), STD (xterm key sequences, DECCKM, bracketed paste
  mode 2004, xterm 256-colour palette; the key capabilities of the terminfo entry `xterm-256color`,
  which is our `TERM` value), rule 12 (security first), OSS (console colour keys `CONSOLE_*_OUTPUT` of the bundled schemes;
  action id `ActivateTerminalToolWindow`; tool window icon of the open terminal plugin), SPEC-0038
  (session and screen model), SPEC-0061 (window chrome), OBS-0007
- **Feature matrix rows:** F-076

## Purpose
A shell in the window, as in the reference's Terminal tool window, built on the existing PTY session and
screen model.

## Behaviour
1. **Session:** opening the tool window the first time starts the user's shell (`$SHELL`, else `/bin/sh`) in
   the project folder; the view redraws whenever the program wrote output (the session counts output
   chunks; the view checks every 16 ms). [GEN]
2. **Drawing:** the visible screen in the editor font with the scheme's console colours: palette 0–15 from
   `CONSOLE_BLACK_OUTPUT` … `CONSOLE_WHITE_OUTPUT`, 16–231 the xterm colour cube, 232–255 the grey ramp, true
   colour as given; background `CONSOLE_BACKGROUND_KEY`, default text `CONSOLE_NORMAL_OUTPUT`; bold and
   inverse cells; wide characters take two columns. The cursor is a block while the terminal has focus and a
   thin bar otherwise; hidden when the program hides it. [STD, OSS]
3. **Input:** typed and committed text is sent as UTF-8. Keys send xterm sequences: Enter `CR`, Backspace
   `DEL`, Tab and Shift+Tab, Escape, arrows (`ESC O x` in application cursor mode, `ESC [1;m x` with
   modifiers), Home/End, Page Up/Down, Insert/Delete, F1–F12, Control+letter as control characters, Alt as
   `ESC` prefix. Paste text uses `CR` line breaks and is wrapped when the program enabled bracketed paste.
   Command (⌘) shortcuts are left to the IDE. [STD]
   - **Modifiers (v3):** Shift, Alt and Control give the xterm modifier parameter m = 1 + Shift (1)
     + Alt (2) + Control (4). With it the arrows, Home and End send `ESC [1;m x` in both cursor key
     modes, F1–F4 `ESC [1;m P`…`S`, and Insert, Delete, Page Up/Down and F5–F12 `ESC [n;m ~` — the
     sequences terminfo names for these keys (`kDC`, `kLFT`, `kf13`–`kf63` and the like). Enter,
     Backspace, Tab and Escape have no modifier form: Alt puts `ESC` in front, Control+Backspace
     sends `BS`, Control and Shift otherwise change nothing (the modifyOtherKeys and other
     keyboard protocols are not offered; a program's request for them is ignored). [STD]
   - **Control combinations (v3):** Control with a letter sends its control character whatever
     Shift is; Control+Space, `@` or `2` send NUL, `[` or `3` ESC, `\` or `4` FS, `]` or `5` GS, `^`
     or `6` RS, `_`, `-`, `7` or `/` US, `?` or `8` DEL. [STD]
   - **Backspace (v3):** sends `DEL`, the erase character of the pseudo terminal. Some terminfo
     databases (macOS) name `BS` as `kbs` of `xterm-256color`, newer ones `DEL`; programs that read
     the erase character from the terminal settings work with both. [STD]
   - **Keypad (v3):** the application keypad mode is kept by the screen model (SPEC-0038), but
     the keys of the numeric keypad send what they type in both modes. [GEN]
   - **Pasted text (v3):** control characters other than tab and the line ends are left out of a
     paste, with or without bracketed paste. Text from the clipboard is text, not key presses,
     and an escape character in it could end the bracketed paste early (`ESC [201~`), so that
     the rest would run as typed commands. [rule 12]
4. **Size:** rows and columns follow the space of the tool window; the program is told the new size. [GEN]
5. **Tool window:** an island under the editor with the header "Terminal"; the stripe has a Terminal button
   at the bottom; `ActivateTerminalToolWindow` (⌥F12) shows and focuses it, or hides it when it already has
   focus. [OSS, OBS]
6. The MCP counterpart is `terminal_render` (SPEC-0038). [ADR-0008]
7. **Tabs (v2):** the header lists one tab per shell ("Local", "Local (2)", …) with a × and a "+" button.
   ⌘T (Ctrl+Shift+T elsewhere) in the terminal or `Terminal.NewTab` opens a tab with a new shell in the
   project folder and shows it; a click selects a tab; ⌘W (Ctrl+Shift+W) or × closes it and ends its
   shell; closing the last tab hides the window. Each tab keeps its own screen. [GEN]

## Acceptance criteria
- **AK-1** Key sequences for special keys, modifiers, application cursor mode, control and Alt
  combinations; paste with and without bracketed paste (unit tests).
- **AK-2** Cursor key mode and bracketed paste are tracked by the screen (unit test); the session counts
  output (integration test).
- **AK-3** xterm palette mapping and console colours from the scheme (unit test).
- **AK-4** In a real window: ⌥F12 opens the terminal, typed keys reach the shell and its output appears
  (`idecraft ui --selftest`).
- **AK-5** Use by hand: colours, cursor, an interactive program (e.g. `top`), resizing.

- **AK-6 (v2)** In a real window (`idecraft ui --selftest`): ⌘T opens a second tab that is shown, ⌘W
  closes it and the first shell's output is shown again.
- **AK-7 (v2)** By hand: several tabs, closing the shown and another tab, next to the reference.
- **AK-8 (v3)** Every key capability of the recorded terminfo entry `xterm-256color` that has a
  key here (cursor and editing keys, their Shift forms, `kf1`–`kf63`, `kcbt`) equals what the key
  sends in application cursor mode; arrows, Home and End with every modifier combination in both
  modes; Control combinations; Alt as Meta (`crates/terminal/tests/conformance.rs`, `k01`–`k06`).
- **AK-9 (v3)** A paste cannot end the bracketed paste and carries no escape character (`c25`).

## Out of scope
Renaming tabs, splitting a tab, scrollback with the wheel, selection and copying with the mouse, links, shell
integration (command blocks), search, closing the session on exit.
Application keypad sequences for the numeric keypad; modifyOtherKeys, CSI u and other keyboard
protocols; F13 and above as keys of their own.

## Known deviations from the reference
Not compared side by side yet. One session per window.
A paste never carries control characters (v3); xterm has a setting for it.
