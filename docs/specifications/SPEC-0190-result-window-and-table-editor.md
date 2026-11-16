# SPEC-0190: Result window tabs and the table data editor

- **Version:** 7 · **Date:** 2026-11-16 (v7: a run of some statements of a file replaces only the
  result tabs of those statements, behaviour 1 — OBS-0070 V2); 2026-11-16 (v6: every drop-down list of
  the Services window's toolbar stays inside the window, behaviour 16 — OBS-0061 H1); 2026-11-10 (v5: the toolbar keeps one row and puts what does not fit
  into a ⋮ menu instead of wrapping into a ragged second row — F-054 window check, behaviour 3;
  v4 2026-11-07: the side panel's value can be edited; data editors start in
  the connection's commit mode — OBS-0026 L6, SPEC-0108 v4; v3: transaction isolation level, page size
  of data editors — OBS-0026; v2 2026-11-05: the toolbar wraps instead of squeezing the clause fields —
  hand test 113, D41; a sorted column is wide enough for its name and mark — hand test 22)
- **Sources:** GEN (database tools show each query's result in its own tab that can be pinned, open a
  table's rows in an editable grid from the database tree, filter and sort that grid on the server with
  `WHERE`/`ORDER BY` fields, page through large tables, show the selected row as a record and the
  selected value in a side panel, preview the DML of pending changes, and offer an automatic or manual
  transaction mode; F-054, F-055), STD (`LIMIT`/`OFFSET`, `OFFSET … FETCH`, `COUNT(*)`, `SAVEPOINT`,
  `SAVE TRANSACTION`), OBS-0010 A4/A7 (reference: transaction mode "Auto" by default, pending changes
  kept until submitted, DML preview ordered delete, update, insert with `DEFAULT` for omitted columns,
  side panel with record, value and aggregate views), OBS-0070 P3 (v7: running one statement after the
  script replaces only that statement's tab; a second script run replaces all), SPEC-0093, SPEC-0099, SPEC-0107, SPEC-0017,
  SPEC-0007 (value summaries), SPEC-0089 (Database tool window), SPEC-0021 v2 (copy and export),
  ADR-0008, ADR-0009
- **Feature matrix rows:** F-054, F-055

## Purpose
Look at and change a table's data without writing SQL: open it from the Database tool window, narrow
it down on the server, inspect a row or a long value, see exactly which statements a change runs, and
decide when they are committed. Results of several queries stay side by side.

## Behaviour
1. **Result tabs.** Each run of the SQL console shows its result sets as tabs "Result 1", "Result 2",
   … after "Output". The next run replaces those tabs unless a tab is pinned: a pinned tab (📌 on the
   tab, or ⌥⇧P (Alt+Shift+P) on the shown tab) stays and keeps its rows, grid state and pending edits;
   new results are added after it. A pinned tab is unpinned by the same action. Table data editor tabs
   (behaviour 2) stay until closed. Tabs other than "Output" close with × on the tab or ⌘W (Ctrl+F4)
   while the grid has focus. [GEN]
   **v7 – Tabs per statement (OBS-0070 V2):** a result tab remembers the file and the statement it
   came from. A run of a file's whole script (every statement of it, chosen in the popup of SPEC-0192
   behaviour 4 or selected) replaces every unpinned result tab, as before. A run of some of a file's
   statements (the statement at the caret, or a selection holding some of them) replaces only the
   unpinned tabs of the statements that ran, each in its place, and keeps the other tabs of that file.
   A tab belongs to a statement that ran when it came from the same file and the statement has the same
   text, or has the same position in a file with the same number of statements (the statement was
   edited). A statement that ran without a result set leaves no tab; statements after a failure did not
   run and keep theirs. Tabs from another file and from other kinds of run (a plan, a comparison, an
   HTTP request) are replaced as before. Results without an earlier tab come after the kept tabs.
   After each run the unpinned result tabs are numbered "Result 1", "Result 2", … in their order;
   pinned tabs keep their title. Before v7 every run replaced all unpinned tabs. [GEN]
2. **Opening a table.** F4 or a double-click on a table or view in the Database tool window opens its
   data editor: a Services tab named after the table, showing the first page of `SELECT * FROM <table>`.
   Opening a table that already has a tab selects that tab. Opening uses the connection's own session
   for this tab (behaviour 9) and the column names it returns at that moment, never cached names. F4 on
   a connection still edits the connection (SPEC-0108). [GEN]
3. **Server filter and sort.** A data editor shows two fields instead of the text filter: `WHERE` and
   `ORDER BY`. **v2:** the fields keep their width. **v5:** the toolbar is always one row; when the
   panel is too narrow for all of it, whole groups move into a ⋮ menu at the row's right end, in this
   order: Record, Transpose, the Table/Tree/Text switch, Copy as, page size, Export, the transaction
   group, the edit group. The pager and the fields always stay. The menu lists the moved actions
   in row order and runs them like their buttons; the page size and transaction menus opened from it
   hang from ⋮. Escape closes the toolbar's menus. (v2 wrapped onto a second line, which left a ragged
   second row at some widths.) A sorted column's header is at
   least as wide as its name plus the sort mark (`▼1`), so the mark never covers the name. Enter in either runs the table query again with the field texts as the clause bodies and
   goes to the first page; an empty field leaves its clause out. A field is refused, with the reason in
   the note and without running anything, when its text would end the statement (a `;` outside quotes
   or comments) or contains a comment that hides the rest of the query. A click on a column header of a
   data editor puts that column into `ORDER BY` (ascending, descending, removed — ⇧-click adds a further
   key as in SPEC-0099) and runs the query; console results keep sorting their loaded rows. Errors of
   the database are shown in the note with their message; the grid keeps its previous rows. [GEN, STD]
4. **Server pages.** A data editor loads 500 rows per page with `LIMIT 500 OFFSET n` (PostgreSQL, MySQL,
   MariaDB, SQLite), `OFFSET n ROWS FETCH NEXT 500 ROWS ONLY` (SQL Server, with `ORDER BY (SELECT NULL)`
   when no order is given; Oracle). The total comes from `SELECT COUNT(*)` with the same `WHERE`; the
   label reads "1–500 of 1200" and ⏮ ‹ › ⏭ load the first, previous, next and last page. Without an
   `ORDER BY` the table's key is appended as the order, so pages neither repeat nor skip rows (SPEC-0007
   behaviour 7); a table without a key says that pages may overlap. [STD, IMP-0011]
5. **Views and tables without a key** open read-only; trying to edit says why (SPEC-0107 behaviour 1).
   [GEN]
6. **Side panel.** ⇧↩ (Shift+Enter) on the grid, or the "Record" button, shows a panel next to the grid
   with three views: *Record* — the selected row as one line per column (name, value, pending values
   included, NULL shown as `<null>`); *Value* — the selected cell's full text, wrapped, with JSON
   indented when the whole value parses as a JSON object or array; *Summary* — the selected column's
   distinct values with counts over the loaded rows, with the scope stated (SPEC-0007 behaviours 9–10).
   The panel follows the selection and is hidden again by the same action. [GEN]
7. **DML preview.** "Preview" (⌥⇧⌘↩, Ctrl+Alt+Shift+Enter) shows the statements that submitting would
   run, in the order they run — deletes, then updates, then inserts — one per pending change, with the
   values of the conflict check (SPEC-0017 behaviour 4). From the preview, "Submit" applies them and
   "Close" goes back to the grid; the preview updates when the pending changes change. Deleting first
   lets a row take over a unique value of a row deleted in the same batch. [GEN, OBS-0010 A4]
8. **Added rows** show `<default>` in columns left empty; the `INSERT` leaves those columns out, so the
   database fills in their defaults (identity columns included). [GEN]
9. **Transaction mode.** A data editor has a mode selector "Tx: Auto" / "Tx: Manual", Auto by default.
   *Auto*: Submit applies the pending changes in one transaction and commits it (SPEC-0017). *Manual*:
   the tab keeps its session open; the first Submit starts a transaction, each Submit applies its
   changes inside it behind a savepoint, and the table is read again in the same transaction, so the
   grid shows the uncommitted state. A batch that fails or conflicts is rolled back to its savepoint;
   earlier batches stay. The toolbar then shows "Commit (n)" and "Rollback" with the number of applied
   changes; Commit ends the transaction, Rollback discards all of it and reads the table again.
   Switching to Auto, closing the tab or closing the window with an open transaction rolls it back and
   writes a line saying so into Output — nothing is committed implicitly. Console results always use
   Auto. [GEN, STD]
10. **Copy and export** of the grid follow SPEC-0021 v2: ⌘C (Ctrl+C) copies the selected cells in the
    chosen format, "Export…" opens the export dialog. [GEN]
11. **Selection.** A click selects a cell; ⇧-click or ⇧+arrow keys extend a rectangular selection from
    the anchor cell; ⌘A (Ctrl+A) selects all shown rows. The record view follows the cursor cell. [GEN]
12. **Agents.** MCP `read_table_data` (connection, table, optional `where`, `order_by`, `page`) returns
    one page of the table with the same query building, refusals and stable order as the data editor,
    plus `total_rows`, `page`, `pages` and the SQL it ran; it needs `read-data`. Previews of pending
    changes are `edit_table_data` without `apply` (SPEC-0017); manual transactions are a UI-only mode
    because an agent's change set is already applied atomically. [ADR-0008, IMP-0007]

13. **v3 – Isolation level.** The "Tx: Auto ▾" button opens a menu with the mode (Auto, Manual) and the
    isolation level of the transactions the tab opens: Database default, Read committed, Repeatable
    read, Serializable. A manual transaction starts at the chosen level (PostgreSQL: `BEGIN` then `SET
    TRANSACTION ISOLATION LEVEL …`; MySQL, MariaDB, SQL Server: the `SET` before the start; Oracle: the
    `SET` as the first statement). A level the database does not have is refused with the reason in the
    note (SQLite and DuckDB: always serializable; Oracle: no repeatable read); the level cannot change
    while a transaction is open. Auto submits stay one short transaction at the database default.
    The page size of SPEC-0099 v2 behaviour 5 applies to data editor pages. [STD, OBS-0026]

14. **v4 – Editing in the value view:** the side panel's Value view has "Edit" (also when the cell is
    selected): the panel turns into a text field with the cell's full text, several lines allowed;
    "Apply" or ⌘↩ (Ctrl+Enter) makes the text the cell's pending value — exactly like an edit in the
    cell (marked as changed, in the preview, applied by Submit, taken back by Revert) — "Cancel" or
    Escape drops it. A result that cannot be edited says why in the note, as in the cell (behaviour 5).
    Agents change values through `edit_table_data` (SPEC-0017). [GEN, OBS-0026]
15. **v4 – Commit mode of the connection:** a data editor of a connection whose option Auto-commit is
    off (SPEC-0108 v4 behaviour 10) opens in Manual. [GEN]
16. **v6 – Drop-down lists inside the window (OBS-0061 H1):** every list a button of the Services
    window's toolbar opens — the ⋮ menu, the page size, "Tx: … ▾" and "Env: … ▾" (SPEC-0102) — opens
    under its button, aligned with it, and is moved in when it would reach past the window's left
    or right edge, so it lies wholly inside the window; nothing of it is cut off. Before v6 these
    lists hung from their button's edge wherever that put them. [GEN]

## Acceptance criteria
- **AK-1** Table query building per dialect: `SELECT * FROM "t" WHERE … ORDER BY … LIMIT 500 OFFSET
  500` (PostgreSQL), backticks for MySQL, `OFFSET … ROWS FETCH NEXT … ROWS ONLY` with `ORDER BY (SELECT
  NULL)` for SQL Server; the count query; the key as default order; refusal of `1=1; drop table t`,
  `1=1 --` and `/*` in a field, acceptance of `name = 'a;b'` (unit tests).
- **AK-2** Pinning keeps a result across the next run; unpinned results are replaced; the side panel's
  record lines, JSON indentation and summary; DML preview order delete, update, insert (unit tests).
- **AK-3** Live against the reference PostgreSQL and MySQL and SQLite with a throwaway table of 1 200
  rows: page 3 holds rows 1001–1200, a `WHERE` and an `ORDER BY desc` are applied on the server, the
  count follows the `WHERE`; manual mode: a submitted change is visible in the same session, invisible
  to a second session until commit, a conflicting batch rolls back to its savepoint only, rollback
  restores the table (integration tests, `--ignored` style like the other live tests).
- **AK-4** Window self-test (`idecraft ui --selftest`): F4 on `customers` in the Database tool window
  opens a tab "customers"; `WHERE` `name like 'A%'` keeps one row; a header click orders on the server;
  ⇧↩ shows the record; an edit shows one `UPDATE` in the preview; manual mode keeps the change pending
  until Commit; pinning keeps a console result over the next run.
- **AK-5** MCP `read_table_data`: page, where, order, refusal of a second statement, missing
  `read-data` (protocol test).
- **AK-6** By hand (round 4, hand test 113): data editor next to the reference — paging a large table,
  `WHERE`/`ORDER BY`, side panel, preview, manual transaction.
- **AK-7 (v3)** Statements per dialect and refusals (`edit::tests::isolation_levels_per_dialect`);
  window self-test: on SQLite read committed is refused and serializable kept, the preview still shows
  the one `UPDATE` (`grid_edit_steps`); in the window next to the reference with MySQL (OBS-0026).

- **AK-8 (v4)** Window self-test (`grid_edit_steps`, written, not run yet): the value panel's text
  `Linus⏎Torvalds` becomes the cell's pending value (one change), Revert takes it back; in the window
  next to the reference (L6).
- **AK-9 (v5)** Which groups stay and which move, in their order, with the ⋮ button counted
  (`toolbar_fit::tests::groups_give_way_in_their_order_and_never_wrap`); window self-test
  (`sql_console_steps`): at 420 px the pager and the filter stay and Transpose and Export move under ⋮;
  a click on ⋮ opens the menu, Transpose in it transposes the grid and closes the menu.
- **AK-10 (v6, behaviour 16)** The lists share one placement (`services_view::drop_down`); in a
  window of the test harness the list of environments opened near the window's left edge lies
  inside the window (`crates/workbench/tests/it/http_client_h1_h3.rs`:
  `the_environment_list_stays_inside_the_window`).
- **AK-11 (v7, behaviour 1)** Which tabs a run replaces, where the new results go and how they are
  numbered: a whole script, one statement, an edited statement, a failure in between, another file,
  a run without a file, pinned tabs (`sql_console_model::tests::a_run_replaces_the_tabs_of_its_statements`);
  in a window of the test harness on SQLite: a script of three queries gives three tabs, running the
  second statement alone keeps three tabs and refreshes only the second, running the whole script again
  replaces all (`crates/workbench/tests/it/db_gaps_obs0070.rs`:
  `one_statement_replaces_only_its_own_result_tab`).

## Out of scope
Editing results of joins (SPEC-0107), special value editors for dates/JSON/binary (the value view edits
text), a column value-list filter for data editors (SPEC-0099 v3 has it for console results),
aggregates over all rows computed by the server, reordering or hiding columns, opening a table's data
in an editor tab instead of the Services window.

## Known deviations from the reference
- The isolation level applies to transactions the tab opens in manual mode; an Auto submit is one short
  transaction at the database default (the reference offers the level in the same menu for both).
- Submitting checks each changed row against the values it was loaded with; a row changed by someone else
  in the meantime stops the whole batch with a note (the reference overwrote such a change, OBS-0026).
- `WHERE`/`ORDER BY` fields refuse text that would end the statement or comment out the rest, instead of
  sending whatever is typed. Pages of a table without explicit order are ordered by its key, so pages
  are stable (IMP-0011).
- Manual transaction mode puts each Submit behind a savepoint, so one failing batch does not discard the
  earlier ones; leaving manual mode with an open transaction rolls back and says so instead of asking.
- Submitting deletes, updates and inserts in that order, like the reference's preview.
