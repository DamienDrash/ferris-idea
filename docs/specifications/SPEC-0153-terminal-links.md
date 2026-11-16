# SPEC-0153: Links in the terminal

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** GEN (IDE terminals turn web addresses and file paths with line numbers in the
  output into links that ⌘-click / Ctrl+click opens, the file in the editor at that line; F-076),
  STD (OSC 8 hyperlinks `ESC ] 8 ; params ; URI ST`, as used by `ls --hyperlink`, `gcc`,
  `systemd`), SPEC-0062, SPEC-0118, ADR-0008
- **Feature matrix rows:** F-076

## Purpose
Go from a compiler error, a stack trace or a printed address straight to the place it names.

## Behaviour
1. **Recognised links** in each line of the screen and the history: web addresses (`http://`,
   `https://`, `ftp://`, `file://`) without punctuation or unbalanced brackets at their end; file
   paths with a folder separator or a file extension, optionally with `:line`, `:line:column` or
   `(line,column)`; Python's `File "path", line N`. [GEN]
2. **OSC 8:** text a program marks with an OSC 8 hyperlink is one link to its address (`file://`
   addresses are files); it outlasts colour changes and ends with an empty address. Recognised links
   inside it are not added. [STD]
3. **Opening:** ⌘-click (Ctrl+click elsewhere) on a link opens a web address in the default
   browser and a file in the editor, with the caret at its line and column. Relative paths are
   resolved against the terminal's starting folder, `~/` against the home folder; a path that is
   not an existing file is no link. Without the modifier a click selects as before (SPEC-0118).
   [GEN]
4. **Hover:** the link under the mouse is underlined and the pointer becomes a hand. Text the
   program underlines (SGR 4) is drawn underlined too. [GEN]
5. **MCP:** `terminal_render` returns `links` of the visible rows: row, start and end column and
   target (`url`, or `file` with path, line, column). [ADR-0008]

## Acceptance criteria
- **AK-1** Addresses with brackets and trailing punctuation, files with line and column in the
  formats above, no links in version numbers, Python tracebacks, OSC 8 with wide characters
  (`links::tests`).
- **AK-2** MCP: a file link with line and column, an OSC 8 hyperlink and a web address
  (`terminal_render_lists_links`).
- **AK-3** Window self-test: after `echo 'see b.rs:1:4 and https://example.com/'` the mouse over
  `b.rs:1:4` underlines the file link, over the address the web link; opening the file link shows
  `b.rs` with the caret on line 1, column 4.
- **AK-4** Hand test (round 4, no. 79): ⌘-click on a `cargo build` error, a web address and
  `ls --hyperlink` output next to the reference.

## Out of scope
Following the shell's current folder after `cd` (needs shell integration), opening folders,
links that wrap over two lines, custom link patterns.

## Known deviations from the reference
Relative paths are resolved against the folder the terminal started in, not the shell's current
one; a path that does not exist there is not a link.
