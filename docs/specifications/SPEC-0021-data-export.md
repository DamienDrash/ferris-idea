# SPEC-0021: Exporting result data

- **Version:** 4 · **Date:** 2026-11-16 (v1 2026-10-31: formats and MCP tool; v3 2026-11-05: the
  export dialog fits the panel and answers to Enter — hand test 115, D42; v4: the preview shows its
  lines at the Services window's default height and scrolls with a scroll bar — screen run of
  2026-11-16, B1)
- **Sources:** GEN (database tools export query results and tables as CSV, TSV, JSON, Markdown and SQL
  `INSERT` statements; F-054), STD (RFC 4180 CSV; JSON RFC 8259; SQL string literals per dialect,
  SPEC-0017), OBS-0005 (export not reachable from menu bar or visible toolbar → IMP-0013), SPEC-0012,
  OBS-0010 A7 (reference: list of copy formats — SQL inserts/updates, a WHERE clause, CSV variants,
  HTML, JSON, Markdown, XML; CSV copied without a header while Markdown has one, numeric Markdown
  columns aligned left → IMP-0061; export dialog with format, header switch, target file or clipboard
  and a live preview), STD (HTML 5 tables, XML 1.0 escaping), SPEC-0190
- **Feature matrix rows:** F-054

## Purpose
Turn a result set (column names and text values, SQL NULL as none) into a file format without losing the
difference between NULL and an empty string.

## Behaviour
1. Formats: `csv`, `tsv`, `psv` (pipe-separated), `json`, `markdown`, `html`, `xml`, `sql-insert`,
   `sql-update`, `where-clause`. **One header setting applies to every text format with a header row
   (`csv`, `tsv`, `psv`, `markdown`, `html`) and is on by default** — for copying, exporting and agents
   alike. [GEN, IMP-0061]
2. CSV (RFC 4180): fields separated by `,`, lines by CRLF. A field is quoted when it contains the
   separator, a quote, CR or LF, or begins or ends with a space; quotes are doubled. **NULL is an empty
   unquoted field, an empty string is `""`**, so both survive a round trip. [STD, improvement]
3. TSV: fields separated by TAB, lines by LF; TAB, LF, CR and backslash in values are written as `\t`,
   `\n`, `\r`, `\\`; NULL is `\N`. [STD (PostgreSQL text format)]
4. JSON: an array with one object per row in column order; values are strings, NULL is `null`. Repeated
   column names get a suffix `_2`, `_3`, … so no value is lost. [STD]
5. Markdown: a pipe table; `|` is written as `\|`, line breaks as `<br>`; NULL is an empty cell. A
   column whose non-NULL values are all numbers (and at least one exists) is aligned right (`---:`).
   Without a header the alignment row is left out too. [GEN, IMP-0061]
6. SQL `INSERT`: one `INSERT INTO <table> (<columns>) VALUES (…);` per row with identifiers quoted and
   literals escaped for the dialect (SPEC-0017); NULL is `NULL`. A table name is required. [STD]
7. PSV: like CSV with `|` as separator and LF line ends; a field containing `|`, a quote, CR or LF is
   quoted. [GEN]
8. HTML: a `<table>` with a `<thead>` row of `<th>` (when the header is on) and one `<tr>` of `<td>` per
   row; `&`, `<`, `>`, `"` escaped; NULL is an empty cell with class `null`. XML: `<rows>` with one
   `<row>` per row and one element per column named after it (characters not allowed in names become
   `_`), text escaped; NULL is an empty element with `null="true"`. [STD]
9. SQL `UPDATE`: one `UPDATE <table> SET <non-key columns> WHERE <key columns>;` per row; the key is the
    table's key when given, else the first column. `where-clause`: one condition per row
    (`col = 'v' AND …`, `IS NULL` for NULL), rows joined by `OR`, in parentheses. A table name is
    required for `sql-update`. [STD]
10. **Copying from the grid** (SPEC-0190): ⌘C (Ctrl+C) copies the selected cells in the format chosen in
    the "Copy as" list of the grid toolbar (TSV by default, so the cells paste into spreadsheets); a
    single selected cell is copied as its plain value. The chosen format and the header setting are
    remembered for the window. [GEN]
11. **Export dialog** ("Export…" in the grid toolbar, ⌥⇧⌘E (Ctrl+Alt+Shift+E)): format list, header
    switch, rows (the selection, the loaded rows, or — for a table data editor — all rows of the table
    with the current `WHERE`/`ORDER BY`), target (clipboard or a file path, default
    `<project>/<table or result>.<extension>`), and a live preview of the first 20 rows that changes with
    every choice. Export writes the text and reports the number of rows and the target in the note;
    a write error is shown in the dialog. [GEN]
    **v3:** the dialog always fits inside the Services panel: it is held between the panel's top and
    bottom, the format list and the preview shrink (and scroll) first, and the Cancel/Export row stays
    visible at the bottom however low the panel is. Enter and ⌘Enter run the export, Escape closes the
    dialog. [GEN]
    **v4 (B1, screen run 2026-11-16):** the preview is never cut off unseen. It is as high as its
    lines (at most about ten lines), so at the Services window's default height a short preview —
    two selected rows with the header — shows every line in full; the choices above it and the
    format list give up room first. A preview longer than its box, or a dialog in a panel too low
    for all its parts, scrolls, and a scroll bar beside the scrolling part shows that more is there.
    Until v4 the preview shrank first and showed only its first two lines, without a scroll bar.
    [GEN]
12. MCP tool `export_data` (connection, one `SELECT`, format, optional table and header): needs
   `read-data`; the statement must only read; all rows are exported (not a page). Exports larger than
   5 MiB are refused with the size, so the caller can narrow the query. Files are not written. [ADR-0008,
   IMP-0013]

## Acceptance criteria
- **AK-1** CSV quoting rules, CRLF, NULL vs empty string, header on/off.
- **AK-2** TSV escapes and `\N`; Markdown escapes; JSON objects with `null` and suffixed duplicate names.
- **AK-3** SQL `INSERT` per dialect (quoting, escaping, NULL); missing table name is an error.
- **AK-4** Round trips on SQLite: the 10 000 `movement` rows exported as CSV give 10 001 lines that parse
  back to the same values; `item` exported as `INSERT`s and run into an empty copy gives the same rows.
- **AK-5** The MCP tool exports all rows, refuses non-reading statements and missing `read-data`.
- **AK-6** (v2) One header setting for all header formats, default on; Markdown right-aligns numeric
  columns; PSV, HTML, XML, SQL `UPDATE` and WHERE-clause output, escaping and NULL (unit tests).
- **AK-7** (v2) Window self-test: two selected cells copied as TSV and as Markdown give the expected
  clipboard text with a header; the export dialog's preview follows the format and header switch and
  writes the file.
- **AK-8** (v2) MCP `export_data` accepts the new formats (protocol test).
- **AK-9** (v2) By hand (round 4, hand test 115): copy formats and export dialog next to the reference.
- **AK-10** (v3) Window self-test: with the export dialog open the bottom of its button row lies inside
  the Services panel, and Enter writes the file (self-test `table_editor_steps`).
- **AK-11** (v4) Window test in the test harness (`tests/it/sql_beta_handtests.rs`,
  `b1_the_export_preview_shows_its_lines`, SQLite): two rows selected, CSV with header, the dialog
  at the default window and Services height — all three preview lines are painted in full inside the
  preview; hand test 115's low window still keeps the Export button inside the panel (AK-10).

## Out of scope
Excel/XLSX, user-defined or scripted formats, transposed output, streaming exports larger than memory, writing files (the editor does that),
importing.

## Known deviations from the reference
- Distinguishing NULL from empty strings in CSV is required here.
- One header setting for every text format, on by default; the reference copies CSV without a header
  but Markdown with one (IMP-0061). Numeric columns are right-aligned in Markdown (IMP-0061).
- No user scripts for formats; the built-in list covers the observed text formats.
