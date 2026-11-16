# SPEC-0075: Reformat Code from the editor

- **Version:** 6 · **Date:** 2026-11-14 (v6: Reformat File… with its options, behaviour 8, AK-6 and AK-7; ninth package of docs/research/KEYMAP_COVERAGE.md); 2026-11-13 (v5: the layout of both formatters follows the second comparison, SPEC-0043 v6, OBS-0036 FM5b and FM6b, and the Markdown formatter can refuse a file, behaviour 7); 2026-11-13 (v4: SQL and Markdown files are formatted by IDECraft itself, behaviour 7, SPEC-0043 v5, OBS-0036 FM5 and FM6; SCSS and Less indent with two spaces, behaviour 1, OBS-0036 FM1); 2026-11-13 (v3: a file nothing can format says so in the status bar instead of doing nothing, behaviour 6, with the sentences of SPEC-0043 v4; headless run for F-039); 2026-11-12 (v2: Python files are formatted with the project's ruff or black, SPEC-0329; OBS-0034 L9)
- **Sources:** GEN (reformat the file or the selection by shortcut; F-039), OSS (action id `ReformatCode`
  with ⌥⌘L / Ctrl+Alt+L in the ported keymaps), SPEC-0043 (formatting through the language server),
  SPEC-0072 (requests on unsaved text)
- **Feature matrix rows:** F-039

## Purpose
Lay out code according to the language's formatter without leaving the editor.

## Behaviour
1. **Start:** `ReformatCode` formats the active file through its language server; with a non-empty
   selection only the selected range (range formatting). The server receives the editor's current text,
   so unsaved changes are formatted. Options come from the editor: indentation unit (tab or n spaces),
   trim trailing whitespace, final newline. [GEN, OSS, STD] (v4) The editor's indentation is the
   file's `.editorconfig`, else Code Style for its language (SPEC-0150 v3: two spaces for JSON, YAML,
   SCSS, Less and Ruby, four for CSS and most others), the same order `format_file` follows
   (SPEC-0043 v5 behaviour 1). [OBS-0036 FM1]
2. **Apply:** the edits change the editor as one undo step with the caret kept in place. The status bar
   says how many changes were made, "Already formatted." when there were none, or why formatting failed
   (e.g. the server cannot format ranges). [GEN]
3. Edits planned on unsaved text refer to that text, also for rename (SPEC-0074) and other plans built
   from server edits: the text a replaced range held is taken from what the server saw, not from disk.
   [GEN]
4. The MCP counterpart is `format_file` with a preview by default (SPEC-0043). [ADR-0008]
5. **Python (v2):** the Python language servers have no formatter, so a Python file — the whole
   file or the selection — is formatted with the project's `ruff` or `black` as SPEC-0329 says;
   behaviours 1 and 2 hold otherwise, except that the editor's indentation options are not passed
   on. Without a formatter the status bar says what to install. [OBS-0034 item 41, SPEC-0329]
6. **Nothing to format with (v3):** the action never ends without a word. In a file whose type
   IDECraft knows no language for, and in a file of a language it serves itself without a
   formatter (SQL), the status bar shows the sentence of SPEC-0043 v4 behaviour 8 at once and no
   server is started; before v3 the action did nothing in a file of unknown type. In the other
   cases of SPEC-0043 behaviours 8 and 9 — no usable server, a server without a formatter, a
   server that formats whole files only when a selection exists, a missing `shfmt` or
   `.scalafmt.conf`, a server that asks or complains instead of formatting — the status bar
   shows "Reformat failed: " followed by that sentence, and the text stays as it is. A file of a
   project with Prettier (SPEC-0123 behaviour 4a) and a Python file (behaviour 5) go their own
   way before any of this. [GEN, SPEC-0043]

7. **SQL and Markdown (v4):** in an SQL file, and in a Markdown file of a project without
   Prettier, the action formats the editor's text with IDECraft's own formatter (SPEC-0043 v5
   behaviours 11 and 12) at once, without a language server, as one undo step with every caret
   kept on its text (as SPEC-0329 v3 behaviour 7). The status bar says "Reformatted with
   IDECraft's SQL formatter." (or Markdown), "Already formatted.", or, when the SQL formatter
   refuses the text, "Reformat failed: IDECraft's SQL formatter changed nothing: line `<n>`: …".
   (v5) The layout is that of SPEC-0043 v6 (OBS-0036 FM5b and FM6b), and the Markdown formatter
   refuses in the same way, "Reformat failed: IDECraft's Markdown formatter changed nothing: …",
   when its own check finds that it would have changed more than whitespace.
   With a selection nothing is changed and the status bar says that these formatters format
   whole files only. Before v4 both files only got the sentence of behaviour 6. [OBS-0036 FM5,
   FM6, FM5b, FM6b; SPEC-0043]

8. **Reformat File… with options (v6):** `ShowReformatFileDialog` (⌥⇧⌘L in the bundled macOS
   keymap, Ctrl+Alt+Shift+L in the default one; Code › Reformat File…) opens a list of options
   with check boxes for the active file, titled "Reformat File: `name`". Only options IDECraft
   can carry out are offered:
   a. **Selected text only** — only with a selection, checked at first: the selection is
      formatted, as `ReformatCode` does with a selection.
   b. **Only changed lines (Git)** — only for a file under Git (one whose editor shows change
      markers, SPEC-0064): the lines added or changed against HEAD are formatted, each run of
      such lines as one range, the last first, so that the earlier ranges keep their lines;
      lines that were only deleted have nothing to format. With a), only the changed lines
      inside the selection. Without changed lines nothing is formatted (c still runs) and
      the status bar says so.
   c. **Optimize imports** — after formatting, the imports are optimized as `OptimizeImports`
      does (Java and Kotlin by IDECraft, Python by ruff or the language server, SPEC-0329;
      JavaScript, TypeScript, Vue and Svelte by the language server, SPEC-0123 v4). In a file
      of another language the status bar says that imports were not optimized there.
   The list shows the options' last choice in this window (Optimize imports unchecked at
   first). Space toggles the selected row, Enter runs with the checked ones (none checked:
   the whole file without imports), Escape changes nothing. The formatter is the one of
   behaviours 1 and 5–7; formatting ranges goes through the language server or, for Python,
   ruff or black, and a formatter of whole files only (IDECraft's SQL and Markdown formatters,
   behaviour 7) refuses a) and b) with its sentence. Formatting and optimizing are computed on
   the editor's text in the background and applied together as **one undo step** with the
   carets kept on their text; if the text changed meanwhile nothing is applied and the status
   bar says so. The status bar names what was done ("Reformatted 2 changed ranges; removed 1
   import", "Already formatted."). Not offered, because IDECraft has no such function:
   rearranging the members of a class and code cleanup. MCP: the steps exist as tools —
   `git_diff` gives the changed lines, `format_file` with `range` formats them, and
   `optimize_imports` follows. [OSS: id, keys and the title "Reformat File…" of the open
   platform; GEN]

## Acceptance criteria
- **AK-1** Formatting and rename plans on unsaved text, with the replaced texts taken from that text
  (integration test with clangd).
- **AK-2** In a real window (`idecraft ui --selftest`, with clangd): ⌥⌘L lays out a function with two
  statements over several lines.
- **AK-3** Use by hand: selection only, other languages, undo.
- **AK-4 (v3, behaviour 6)** The sentences are those of SPEC-0043/AK-5, from the same functions
  the MCP tool uses (`format::no_formatter` before any server, the error of
  `format::plan_format_in` otherwise). In the window, by hand next to the reference: ⌥⌘L in a
  text file, an SQL file, a Markdown file, a shell script without shfmt, and with a selection in
  a Go file (window check open).

- **AK-5 (v4, v5, behaviour 7)** The formatters and their sentences are those of SPEC-0043/AK-9 to
  AK-11, called from `WorkbenchView::reformat` before any server is looked for
  (`format::OwnFormatter`). In the window, by hand next to the reference: ⌥⌘L in an SQL file and
  in a Markdown file, undo, a selection (window check open).

- **AK-6 (v6, behaviour 8)** The rows offered for a file with and without a selection and Git,
  the ranges of changed lines (merged runs, deleted lines left out, clipped to a selection) and
  the status lines (unit tests `keymap_actions::tests::reformat_file_rows_ranges_and_lines`).
- **AK-7 (v6, behaviour 8)** In a window of the test harness: ⌥⇧⌘L in a Java file of a Git
  repository lists Only changed lines and Optimize imports; with both checked and no changed
  line the unused import is removed as one undo step and nothing is formatted; Only changed lines in an SQL file is refused with
  the formatter's sentence (`ninth_package_keymap.rs::reformat_file_dialog_*`). With a language
  server formatting changed ranges: not checked.

## Out of scope
Optimize imports from `ReformatCode` itself (v6: from Reformat File…, behaviour 8), rearrange code,
code cleanup, code style settings dialog, formatting on save, a preview of wide
changes.

## Known deviations from the reference
The reference uses its own formatter and code style settings; IDECraft uses the language server's
formatter (e.g. clang-format through clangd), and formatters of its own for SQL and Markdown. Compared
side by side per language on 2026-11-13 (OBS-0036); the differences that stay are D1 to D9 of
SPEC-0043 v6. A selection in an SQL or Markdown file is not formatted alone (not observed in the
reference).
