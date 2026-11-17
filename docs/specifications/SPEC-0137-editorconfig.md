# SPEC-0137: EditorConfig

- **Version:** 3 · **Date:** 2026-11-16 (v3: the hand test's steps in a window test, AK-7; v2: `max_line_length` draws the right margin and a changed `.editorconfig` reaches open editors, AK-5, AK-6, #852; v1: 2026-11-02)
- **Sources:** GEN (the reference's open EditorConfig plugin applies `.editorconfig` files to the
  editor: indentation, tab width, line breaks, trailing whitespace and final line break on save;
  F-081), STD (EditorConfig specification, editorconfig.org, read for the rules, not quoted: files
  named `.editorconfig` from the file's folder upwards until one with `root = true` before its
  first section; sections `[glob]`, later sections and nearer files winning; `unset`; globs `*`,
  `**`, `?`, `[name]`, `[!name]`, `{a,b}`, `{n1..n2}`, `\` escapes, a glob without `/` matching the
  file name at any depth; properties `indent_style`, `indent_size` (number or `tab`), `tab_width`
  (default: `indent_size`), `end_of_line`, `charset`, `trim_trailing_whitespace`,
  `insert_final_newline`, `max_line_length`; standard values case-insensitive; comments with `#`
  or `;`), SPEC-0129 (save options), SPEC-0058 (tab expansion), SPEC-0150 (Code Style, right margin), ADR-0008,
  OBS-0067 (v2: `max_line_length` draws the margin in the reference; a changed `.editorconfig`
  reaches open files there only after reloading everything from disk by hand), IMP-0413
- **Feature matrix rows:** F-081

## Purpose
Projects that keep their formatting rules in `.editorconfig` get them in IDECraft without
configuring anything, as in the reference with its bundled open EditorConfig plugin. The plugin's
function is rebuilt from the public specification; no plugin code was read or ported.

## Behaviour
1. **Lookup:** for an opened file, the `.editorconfig` files from its folder upwards are read until
   one says `root = true` (or the file system root); each property keeps the file and line that
   set it. Unreadable files and values that are not understood (`indent_style = tabs`,
   `insert_final_newline = yes`) are listed as problems. [STD]
2. **Editor:** `indent_style`/`indent_size` set the indentation unit (Tab, Enter, indent and
   unindent, the status bar's indent label); `tab_width` (else a numeric `indent_size`) sets how
   far a tab advances in the editor and the tab size given to formatters. Saving a `.editorconfig`
   file applies the change to every open editor. Without EditorConfig, four spaces and a tab
   width of 4 stay. [STD]
2a. **Right margin:** `max_line_length = n` draws the editor's right margin after column `n` for the
   file, whether or not Code Style shows its margin (SPEC-0150 behaviour 3); `max_line_length = off`
   draws none; without the property (or with `unset`) Code Style decides. A value that is neither a
   positive number nor `off` is listed as a problem and Code Style decides. Wrapping at that column
   is not done. [STD, OBS-0067]
2b. **Changes reach open editors:** a `.editorconfig` that is saved in IDECraft, or created, changed
   or deleted by another program anywhere in the lookup chain of an open file, is read again for
   that file within the regular look at the disk (SPEC-0058 behaviour 10, every 2 seconds) and at
   once by Reload All from Disk (Synchronize). Indentation, tab width, the status bar's indent
   label, the save rules of behaviour 3 and the right margin follow without reopening the file; the
   text is not changed. Files opened later read the current state. [IMP-0413]
3. **Saving:** `trim_trailing_whitespace` and `insert_final_newline = true` override Settings ›
   Editor (SPEC-0129) for the file, including switching them off; `insert_final_newline = false`
   removes the final line breaks; `end_of_line` (`lf`, `crlf`, `cr`) converts every line break.
   The caret line keeps its trailing spaces; all of it is one undo step. [STD]
4. **MCP:** `editorconfig` (`path`) returns the effective indentation, tab width, line break,
   save rules, `max_line_length`, `charset`, every property with file and line, the files read
   and the problems. [ADR-0008]

## Acceptance criteria
- **AK-1** Globs, lookup chain with `root`, `unset`, nearer files winning, derived tab width and
  indent unit, problems, save edits (trim with caret line, CRLF/LF conversion, final break added
  and removed, overriding the IDE settings) (unit tests `editorconfig::tests`).
- **AK-2** MCP: indentation, tab width, line break, the line of a property, a problem, a path
  outside the workspace refused (`crates/mcp/tests/it/protocol.rs`: `editorconfig_tool`).
- **AK-3** Window self-test: a `.txt` file indents with a tab of width 8; Tab inserts a tab;
  saving trims and adds the final line break; saving a changed `.editorconfig` switches the open
  editor to two spaces.
- **AK-4** Hand test (round 4): a project with `.editorconfig` (spaces and tabs, CRLF), next to
  the reference.
- **AK-5** `max_line_length`: a number, `off`, missing and an invalid value give the margin of
  behaviour 2a; the editor draws its margin at that column (unit tests `editorconfig::tests`,
  `editor_view::tests`).
- **AK-6** An open file follows an `.editorconfig` changed on disk without reopening: Tab inserts
  the new indentation, the status bar names it, the right margin moves; a created and a deleted
  `.editorconfig` count too; Synchronize reads it at once (window test of the harness,
  `crates/workbench/tests/it/editorconfig_live.rs`; unit tests of the change stamp).

- **AK-7 (v3)** Hand test 63 in a window of the test harness with real keys
  (`crates/workbench/tests/it/editorconfig_handtest_63.rs`: `hand_test_63_editorconfig_in_the_window`):
  `[*.py]` with four spaces, `[Makefile]` with a tab of width 8, `[*.bat]` with CRLF and, for every
  file, trimmed line ends and a final line break; Tab and typing in each file and ⌘S give the files
  byte for byte (the caret line keeping its spaces); a changed `.editorconfig`, saved, switches the
  open Python editor to two spaces at once. AK-4, next to the reference, follows the cases of
  `docs/research/PLAN-F081.md`.

## Out of scope
`charset` other than UTF-8 (IDECraft reads and writes UTF-8; the property is reported, not
applied), the reference's
IntelliJ-specific `ij_*` code style properties, wrapping at `max_line_length`, completion and highlighting inside `.editorconfig`
files, showing which `.editorconfig` applies in the status bar.

## Known deviations from the reference
- Compared byte for byte in OBS-0064, OBS-0065 and OBS-0067. IDECraft enforces `end_of_line`,
  `trim_trailing_whitespace` and `insert_final_newline = false` as the specification says, while
  the reference does not (OBS-0064).
- **Better than the reference (IMP-0413):** the reference keeps indenting with the old value after
  an `.editorconfig` changed on disk, also in a file closed and opened again, until everything is
  reloaded from disk by hand, and shows nothing about the stale state (OBS-0067 e17b–e17d).
  IDECraft applies the change to open and newly opened files by itself (behaviour 2b).
- The right margin from `max_line_length` is drawn as in the reference (OBS-0067 e14).
