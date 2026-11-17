# SPEC-0206: Terminal search and shell integration

- **Version:** 2 · **Date:** 2026-11-17 (v2: the search over MCP; found headless for F-076, OBS-0075)
- **Sources:** GEN (terminals find text in the scrollback; IDE terminals install shell integration
  themselves so that prompts and commands are known: marks for finished commands and their exit
  status, jumping between prompts; F-076), STD (OSC 133 `A`/`B`/`C`/`D[;status]` prompt marks as
  FinalTerm defined them and current terminals use them; OSC 7; zsh `ZDOTDIR` start-up files and
  `precmd`/`preexec` hooks, bash `--rcfile`, `PROMPT_COMMAND` and the `DEBUG` trap), SPEC-0038,
  SPEC-0062, SPEC-0161
- **Feature matrix rows:** F-076

## Purpose
Long output needs a search, and the terminal should know where commands start and how they ended.

## Behaviour
1. **Search:** ⌘F (Ctrl+Shift+F elsewhere) in the terminal opens a find field above it with the
   selected text, else the last query. Every occurrence in history and screen is highlighted (case
   insensitive unless "Cc" is on; a match may not span lines); the counter shows "n/m" or
   "0 results". Enter, ⌘G or ↓ goes to the next match upwards in time from the bottom, ⇧Enter, ⇧⌘G
   or ↑ to the other direction; the view scrolls so that the current match is shown. Escape closes
   the field and returns to the live screen and the shell. [GEN]
2. **Prompt marks:** the screen records OSC 133 marks with the history line they were written on:
   `A` prompt start, `B` command start, `C` output start, `D;n` command end with exit status `n`. The
   history numbering is stable while old lines leave the scrollback; marks of lines that left it are
   dropped. [STD]
3. **Gutter:** a finished command's prompt line gets a thin mark at the left edge: green for status 0,
   red otherwise; a running command's prompt line a grey one. [GEN]
4. **Jump:** ⌘↑/⌘↓ (Ctrl+Shift+Up/Down elsewhere) scrolls to the previous/next prompt start. [GEN]
5. **Shell integration:** when the terminal starts zsh or bash itself (not a program set in Settings
   other than these, not an SSH login), it adds hooks that write OSC 133 marks and OSC 7 without
   touching the user's files: zsh gets `ZDOTDIR` pointing to a folder of generated start-up files
   that load the user's own (`$ZDOTDIR` or `$HOME`) first and then add `precmd`/`preexec`; bash is
   started with `--rcfile` of a generated file that loads `~/.bashrc` first and then adds
   `PROMPT_COMMAND` and a `DEBUG` trap. Hooks of the user stay in place. The environment variable
   `IDECRAFT_SHELL_INTEGRATION=0` turns it off. [STD, GEN]
6. **MCP:** `terminal_render` returns `prompts`: per OSC 133 prompt the history line, the command text
   between `B` and `C` and the exit status. [ADR-0008]
7. **v2 – MCP search:** `terminal_render` takes `find` and `match_case` and returns `matches`:
   per occurrence the history line (0 is the oldest scrollback line, the numbering of
   `prompts`), the first column and the column after the last, top to bottom, found as the find
   field of behaviour 1 finds them (case-insensitive unless `match_case`; a match does not span
   lines). Without `find`, `matches` is null. Before v2 an agent could not check what the find
   field highlights. [ADR-0008]

## Acceptance criteria
- **AK-1** Search over history and screen with and without case; marks with stable line numbers when
  the scrollback is full (unit tests).
- **AK-2** zsh and bash with the generated start-up files report prompts, commands and statuses and the
  folder after `cd` (`pty` integration tests; skipped when the shell is missing).
- **AK-3** In a real window (`idecraft ui --selftest`): ⌘F finds a line scrolled out of view and shows
  it, Escape returns; ⌘↑ jumps to the previous prompt; the failed command's mark is red.
- **AK-4** MCP `terminal_render` with `prompts` (protocol test).
- **AK-5** By hand next to the reference (round 4, no. 141).
- **AK-6 (v2)** MCP `terminal_render` with `find`: a match in the scrollback and one on the
  screen, case-insensitive; with `match_case` only the exact one; no `find`, no `matches`
  (protocol test `terminal_render_tool`).

## Out of scope
Regular expressions and whole words in the search, command blocks drawn as boxes, fish and
PowerShell integration, completion popups from the shell.

## Known deviations from the reference
Not compared side by side yet.
