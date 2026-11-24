# SPEC-0099: Result grid — sorting, filtering, paging

- **Version:** 4 · **Date:** 2026-11-10 (v4: Submit keeps sort and filters — window check 2026-11-10;
  v1 2026-11-01; v2: page size, numbers aligned right, transposed
  view, column widths by dragging — side-by-side comparison OBS-0026; v3: a value filter with check
  boxes per column, Tree and Text views — OBS-0026 L5, L7)
- **Sources:** GEN (result grids sort by a click on a column header, with several columns by
  modifier-click, filter rows and show large results page by page with first/previous/next/last
  buttons; default page size 500; F-054), SPEC-0093 (SQL console and its grid in the Services window),
  SPEC-0007 (stable order, IMP-0011), OBS-0026 (reference: page size menu 10/100/250/500/1000/all,
  numeric columns aligned right, a transposed view, header edges that resize columns)
- **Feature matrix rows:** F-054

## Purpose
Find the rows that matter in a result without writing another query.

## Behaviour
1. **Sort:** a click on a column header sorts the result by that column: ascending, then descending,
   then back to the order the database returned. ⇧-click adds the column as a further sort key (or
   cycles its direction in place); the header shows ▲/▼, numbered when several columns sort. NULL comes
   first ascending and last descending; two numbers compare by value, other text case-insensitively and
   then exactly. Sorting is stable: rows equal in every sort column keep the database order, so no row
   is repeated or skipped across pages (IMP-0011). Sorting starts again at the first page. [GEN]
2. **Filter:** the "Filter" field above the grid keeps the rows with a cell containing its text,
   case-insensitively; NULL cells never match. Each result tab keeps its own filter. [GEN]
3. **Pages:** a result shows 500 rows per page. ⏮ ‹ › ⏭ go to the first, previous, next and last page;
   the label reads "1–500 of 1200", "2 rows" or "2 rows (filtered from 4)". Row numbers count the
   visible rows. The line under the grid repeats the label with the statement's time. [GEN]
4. Agents get the same view from `run_sql`: optional `sort` (columns with `descending`), `filter` and
   `page` (of `max_rows` rows) apply to each result set and add `matching_rows`, `page`, `pages` and
   `unknown_sort_columns`; `row_count` stays the number of loaded rows. [ADR-0008]

5. **v2 – Page size:** a "Page: 500 (default) ▾" button next to the filter opens a menu with 10, 100,
   250, 500 (default), 1000 and All. Choosing one shows that many rows per page from the first page;
   All shows every loaded row on one page. Data editors (SPEC-0190) load pages of that size from the
   server; All is a limit of one billion rows there. Each result tab keeps its own size. [GEN, OBS-0026]
6. **v2 – Numbers on the right:** a column whose non-NULL values are all numbers (sign, digits, one
   decimal point, exponent) and that has at least one value shows its cells and header aligned right;
   other columns stay left-aligned. Columns of added rows keep room for the `<default>` text. [GEN,
   OBS-0026]
7. **v2 – Transposed view:** "☐ Transpose" switches the shown page to one line per column (name in
   bold, then the value of each row, numbered as the rows), and back. A click on a value selects that
   cell, so Record/Value/Summary follow it; editing happens in the normal view. [GEN, OBS-0026]
8. **v2 – Column widths:** dragging the right edge of a column header changes that column's width (at
   least 24 px); a double-click on the edge fits the column to its content again. A click on the edge
   does not sort. Widths belong to the result tab. [GEN, OBS-0026]

9. **v3 – Value filter per column:** a "⏷" mark in each header of a console result opens the column's
   value list: every distinct value with how often it occurs, counted over **all loaded rows** (not the
   page, IMP-0012), NULL first as `<null>`, in sort order, each with a check box and "Only"; "Select
   all" at the top. Unchecking a value hides the rows with it; "Only" keeps that value alone; a list
   with every value checked is no filter. The mark is coloured while the column filters. Value filters
   of several columns and the text filter combine (a row must pass all of them); changing a filter goes
   back to the first page; the label counts as in behaviour 3 ("1 row (filtered from 2)"). Each result
   tab keeps its own filters. Data editors filter on the server: there the mark is not shown and the
   note points to the `WHERE` field. Escape closes the list. [GEN, OBS-0026]
10. **v3 – Tree and Text views:** "Table · Tree · Text" in the toolbar switches how the shown page is
    drawn. *Tree*: one node per row, numbered as the rows ("3 {id: 7, sku: A-1, …}", the first three
    columns), a click expands it into one line per column ("sku: A-1"); a click on such a line selects
    that cell, so the side panel follows. *Text*: the page as a plain-text table with `+`, `-` and `|`,
    the header first, numbers aligned right, NULL as `<null>`, line breaks as `⏎`, values cut after 60
    characters with `…` — ready to copy into a ticket. Editing happens in the table view. Each result
    tab keeps its view. MCP `run_sql` takes `view: "text"` and returns the same text as `text`. [GEN,
    OBS-0026, ADR-0008]
11. **v3 – Agents:** `run_sql` takes `value_filter` (per column the values to keep, `null` for NULL)
    and `values_of` (a column: returns `values`, its distinct values with counts over all loaded rows)
    and names unknown columns in `unknown_filter_columns`. [ADR-0008]

12. **v4 – Submit keeps the view:** after Submit (or Revert) reloads the result, the sort order, the
    value filters, the text filter, the page and the selected view (Table, Tree, Text) stay as they
    were; the row count line says how many rows the filters show. [GEN, window check 2026-11-10]

## Acceptance criteria
- **AK-1** Header clicks cycle and add keys; numeric, NULL-first, stable sorting; filter; page ranges
  and labels (unit tests `grid::tests` in `crates/database`).
- **AK-2** MCP: sorting descending with an unknown column, the second page of one row, a filter
  (`crates/mcp/tests/it/protocol.rs`, SQLite part).
- **AK-3** In a real window (`ferris-idea ui --selftest`): two clicks on "name" sort descending, the filter
  "ada" keeps one row ("1 row (filtered from 2)"), a 1200-row result shows "1–500 of 1200" and its last
  page starts at 1001.
- **AK-4** By hand: clicking headers, ⇧-click, typing a filter and paging next to the reference.
- **AK-5 (v2)** Page sizes, labels and ranges with sizes, number detection, numeric columns and the
  transposed lines (unit tests `grid::tests::page_sizes`, `numbers_and_numeric_columns`,
  `transposed_view`); window self-test: page size 100 → "1–100 of 1200", All → "1200 rows" with 1200
  rows, transposed, a dragged width kept at its minimum, and back (`sql_console_steps`); in the window
  next to the reference (OBS-0026).

- **AK-6 (v3)** Value list with counts over every loaded row, toggling, "Only", combination with the text
  filter, a full list is no filter (`grid::tests::value_filters_per_column`).
- **AK-7 (v3)** Text view alignment, NULL, line breaks and cutting; tree numbering from the page start
  and expansion (`grid::tests::text_and_tree_views`).
- **AK-8 (v3)** MCP `run_sql` with `value_filter`, `values_of` and `view: "text"`, an unknown filter
  column (protocol test `grid_values_object_doc_history_and_connection_extras`).
- **AK-9 (v3)** Window self-test (`sql_console_steps`, written, not run yet): unchecking `Ada` keeps
  `Linus` and marks the column, Select all brings both back, the text view draws the table, the tree
  view numbers and expands the first row; in the window next to the reference (L5, L7).

- **AK-10 (v4)** After a reload of the same query, sort keys, value filters, text filter, page and view
  are restored (unit test of the grid state); in the window: filter, edit a value, Submit — the filter
  stays.

## Out of scope
Server-side sorting and filtering with `ORDER BY`/`WHERE` fields (SPEC-0190 has them for data
editors), a value list for data editors (they filter with `WHERE`), fetching more rows than the console
loaded, editing cells in the transposed, tree or text view (F-055), reordering and hiding columns, a
search field inside the value list.

## Known deviations from the reference
The reference sorts and filters by re-running the query on the server by default; Ferris IDEA sorts and
filters the rows the console already loaded, so nothing runs again and statements with side effects
are never repeated. The filter is a text search instead of a `WHERE` clause. Compared side by side in OBS-0026: the reference
sorts and counts a console result over the first 500 rows only, Ferris IDEA over all loaded rows; the
reference reaches the transposed view through a view menu, Ferris IDEA through a toolbar switch. **v3:**
the value list counts over every loaded row, where the reference counted the first 500 (IMP-0012); the
views are three buttons in the toolbar instead of an eye menu.
