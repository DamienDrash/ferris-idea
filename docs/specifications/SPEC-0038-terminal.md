# SPEC-0038: Integrated terminal — PTY session and screen model

- **Version:** 3 · **Date:** 2026-11-13 (v2: wide and combining characters; v3: conformance run
  without a window — origin, autowrap and insert modes, tab stops, line-drawing set, repeat, soft
  reset, cursor save with attributes, alternate screen 1047/1048/1049 as xterm, SGR sub-parameters,
  resizing, answers and limits)
- **Sources:** GEN (IDEs open a shell in the project folder inside a tool window, with colours, cursor
  movement, full-screen programs and scrollback; F-076), STD (ECMA-48 control functions and xterm control
  sequences: C0 controls, CSI cursor/erase/insert/delete/scroll, SGR with 16, 256 and true colours, DEC
  private modes 25 and 1049, DECSTBM, DSR; ITU T.416 colon form of colours; the terminfo entry
  `xterm-256color`, which names what programs send to a terminal with our `TERM` value; the common
  2 083-byte limit of OSC 8 addresses; parsing with `vte`, PTY with `portable-pty`, both MIT/Apache),
  rule 12 (security first)
- **Feature matrix rows:** F-076

## Purpose
Run a shell or command in a pseudo terminal and keep an exact model of what the terminal shows, so the UI
can draw it and tools can read it.

## Behaviour
1. A session starts the user's shell (`$SHELL`, else `/bin/sh`) or a given command in a pseudo terminal with
   the workspace root as working directory, a size in rows and columns and `TERM=xterm-256color`. Input is
   written as bytes; output is read continuously in the background and fed into the screen model. The
   session can be resized (the program sees the new size), killed, and reports the exit status. [GEN, STD]
2. The screen is a grid of cells (character plus style: foreground, background, bold, italic, underline,
   inverse) with a cursor. Printing writes at the cursor and wraps at the right margin (deferred wrap as in
   xterm); CR, LF/VT/FF, BS, TAB and BEL are handled. Characters take their Unicode
   East Asian width: wide characters occupy two cells (wrapping early and leaving the last column blank when
   only one is left), combining marks join the preceding character, and overwriting half of a wide character
   clears the other half. [STD, Unicode UAX #11 via `unicode-width`]
   - **Pending wrap (v3):** after a character in the last column the cursor stays there; the next
     character wraps. CR, BS, cursor movement and positioning cancel the pending wrap; a line feed
     moves down in the same column. With autowrap off (`?7l`) nothing wraps and the last column is
     overwritten. [STD]
   - **Tab stops (v3):** every 8 columns at the start; HTS (`ESC H`) sets one at the cursor, TBC
     (`CSI g`, `CSI 3 g`) clears one or all, CHT (`CSI I`) and CBT (`CSI Z`) move by stops. A tab
     stops at the last column and does not wrap. [STD, terminfo `hts`, `tbc`, `cbt`]
   - **Width per code point (v3):** the width is that of each code point alone. A variation
     selector or a zero-width joiner adds no column and is kept with the character before it, so
     an emoji with a variation selector keeps the width of its base character, and a joined emoji
     sequence takes the columns of its parts. A cell keeps at most 16 combining marks. [STD]
   - **Line-drawing set (v3):** `ESC ( 0` and `ESC ) 0` put the DEC special graphics set into G0
     and G1 (any other set is text), SO and SI switch between them; with it the characters `_` to
     `~` are shown as the line-drawing characters. [STD, terminfo `smacs`, `rmacs`, `acsc`]
3. Control sequences: cursor up/down/forward/back, next/previous line, column, position (CUP/HVP, row),
   erase in display (0, 1, 2, 3) and line (0, 1, 2), insert/delete lines and characters, erase characters,
   scroll up/down, scroll region (DECSTBM), save/restore cursor (CSI s/u, ESC 7/8), index, reverse index,
   next line, full reset, cursor visibility (?25), alternate screen (?1049, ?1047, ?47), window title (OSC 0
   and 2), cursor position report (DSR 6) answered to the program. Unknown sequences are ignored. [STD]
   - **v3:** HPA, HPR and VPR (`CSI` `` ` ``, `a`, `e`); REP (`CSI b`) repeats the character printed
     just before; insert mode (`CSI 4 h`/`l`) makes printed characters push the rest of the line
     right; origin mode (`?6`) counts positions from the scroll region, keeps them inside it and
     reports them so; soft reset (`CSI ! p`) puts modes, rendition, region and saved cursor back
     and keeps the text. A sequence with a private marker or an intermediate that is not listed
     here is ignored, never read as its unmarked form. [STD, terminfo `smir`, `is2`]
   - **Saved cursor (v3):** saving keeps the position with its pending wrap, the rendition, the
     origin mode and the character sets; each of the two screens has its own. [STD]
   - **Alternate screen (v3):** `?1047` and `?47` switch the screen and leave the cursor where it
     is; `?1048` saves and restores the cursor; `?1049` does both, with the cursor of the primary
     screen kept apart from one a program saves on the alternate screen. The alternate screen
     starts blank. [STD]
   - **Modes kept for the keys (v3):** application cursor keys (`?1`) and the application keypad
     (`ESC =`, `ESC >`). [STD, terminfo `smkx`, `rmkx`]
   - **Window title (v3):** may contain `;`; at most 1 024 characters, control characters left
     out. [STD, rule 12]
4. SGR: reset, bold, italic, underline, inverse and their resets, the 8 + 8 bright colours, 256-colour and
   true-colour forms with `;` or `:` separators, default colours. [STD]
   - **v3:** a parameter with `:` sub-parameters is read on its own: `4:0` ends the underline,
     `4:1`–`4:5` are an underline, `38:2:r:g:b` and `38:2:<colour space>:r:g:b` are both true
     colours. Double underline (21) is an underline. The underline colour (58) takes its
     parameters with it and is not kept. A colour value above 255 selects nothing. Dim, blink,
     conceal and strike-through are not kept. [STD]
5. Lines scrolled off the top of the primary screen go to a scrollback (default 10 000 lines); the alternate
   screen has none and the primary screen is restored when it is left. [GEN]
6. The visible text (rows with trailing blanks removed) and the scrollback text can be read at any time.
   [GEN]
7. MCP tool `terminal_render` (output text with escape sequences, optional rows and columns) returning the
   screen text, cursor, title and scrollback, so tools can interpret captured terminal output. Starting
   processes through MCP is not offered (open question about permissions, see PO-STATUS). [ADR-0008]
   It also returns `modes` (application cursor keys, application keypad, bracketed paste) and
   `answers`, the bytes the terminal sends back for the queries in the output (v3).
8. **Resizing (v3):** lines are not reflowed. A narrower screen cuts the lines at its edge (a wide
   character cut in half goes), a wider one pads them; the text cut off does not come back. When
   rows no longer fit, blank rows below the cursor go first, then rows leave at the top: those of
   the primary screen go to the scrollback — also while the alternate screen is shown —, those of
   the alternate screen are dropped. The scroll region becomes the whole screen; tab stops that
   were set stay. [GEN]
9. **Answers and limits (v3, security first):** what the terminal answers reaches the program — in
   a shell, the command line — as if it were typed, and output can come from anywhere (a file shown
   with `cat`, a log, a remote host). So:
   - Answered are only the cursor position (`CSI 6 n`), the status (`CSI 5 n` → `CSI 0 n`) and the
     primary device attributes (`CSI c` → `CSI ? 1 ; 2 c`, the answer terminfo names): fixed text
     and numbers, nothing a program chose.
   - Never answered: the window title and icon name (`CSI 21 t`, `CSI 20 t`) and the other window
     reports, colour queries (OSC 4, 10, 11 with `?`), the clipboard (OSC 52), settings (DECRQSS),
     modes (DECRQM), terminfo capabilities (XTGETTCAP), the secondary and tertiary device
     attributes and the terminal version (`CSI > c`, `CSI = c`, `CSI > q`), the answerback (ENQ).
   - Output cannot write the clipboard: OSC 52 is not carried out, in either direction.
   - Output cannot make the terminal grow without bound: an operating system command of more
     than 64 KiB is dropped whole; a hyperlink address of more than 2 083 bytes is no link, and
     addresses are kept up to 1 MiB in all (later links are plain text); a reported folder of
     more than 4 096 bytes is not taken; a prompt mark repeated on one line is one prompt and
     there are never more prompts than lines of history; answers nobody collects are kept up to
     4 096 bytes; the scrollback keeps its limit of behaviour 5.
   - Malformed and unknown sequences change nothing and never stop the terminal. [STD, rule 12]

## Acceptance criteria
- **AK-1** Printing, wrapping, CR/LF/BS/TAB, scrolling into scrollback; wide and combining characters
  (model and real program output).
- **AK-2** Cursor movement, erase, insert/delete lines and characters, scroll region.
- **AK-3** SGR styles and colour forms; reset.
- **AK-4** Alternate screen enter/leave restores the primary screen; title; cursor visibility; DSR answer.
- **AK-5** A real PTY session: a command's coloured output appears in the model; `stty size` reports the
  size and a changed size after resize; the working directory is the workspace; input reaches the program;
  exit status reported; kill ends a running program.
- **AK-6** The MCP tool renders output with escape sequences.
- **AK-7 (v3)** Conformance run of the model without a window (`crates/terminal/tests/conformance.rs`,
  cases `c01`–`c38`, `c45`–`c47`): cursor movement and positioning, save and restore, erase, insert
  and delete, scroll region, origin mode, autowrap and the pending wrap, tab stops, SGR with 256 and
  true colours and sub-parameters, the alternate screen with and without the saved cursor, modes,
  soft and full reset, title, hyperlinks, folder, wide characters, combining marks and emoji, the
  line-drawing set, repeat, resizing on both screens, the answers; and recordings of `tput`, `ls`
  and `git log` with `TERM=xterm-256color` replayed into the model.
- **AK-8 (v3)** Answers and limits (cases `c39`–`c44`): the queries of behaviour 9 get no answer,
  OSC 52 changes nothing, malformed sequences and a pseudo-random stream in any chunking with
  resizes keep the grid and cursor sound, and every limit of behaviour 9 holds.
- **AK-9 (v3)** The MCP tool returns `modes` and `answers`.

## Out of scope
The terminal view (M3.7), mouse reporting,
bracketed paste, hyperlinks (OSC 8), shell integration, Windows ConPTY specifics beyond what
`portable-pty` provides, tabs of several sessions (these have their own specifications: SPEC-0062,
SPEC-0118, SPEC-0153, SPEC-0205, SPEC-0206).
Reflow on resize; left and right margins (DECSLRM) and the 132-column mode; dim, blink, conceal,
strike-through, underline styles and underline colour as attributes of a cell; images (sixel and
others); one cell for a joined emoji sequence; keyboard protocols beyond SPEC-0062; setting colours
(OSC 4, 10, 11); reading or writing the clipboard from output.

## Known deviations from the reference
Not yet compared with the reference terminal.
From xterm (v3): the secondary device attributes are not answered (programs that ask, such as
editors probing the terminal version, go on after their own timeout); the title report and the
clipboard are refused outright instead of being a setting; a program-chosen title is cut at 1 024
characters. These follow from behaviour 9.
