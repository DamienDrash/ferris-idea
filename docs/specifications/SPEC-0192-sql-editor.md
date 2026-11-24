# SPEC-0192: SQL editor bound to a data source

- **Version:** 7 · **Date:** 2026-11-16 (v7: a row of the Database window counts as selected only once the user selected it, so behaviour 3b also holds while that window is open, behaviour 3b; a table the script creates earlier, temporary ones included, is known in its later statements, behaviour 11b — window check of F-050 in OBS-0070, G1 and side finding); 2026-11-16 (v6: ⌘⏎ or ▶ in a file without a data source opens the data-source popup at once and runs after the choice, behaviour 3b — OBS-0070 V1, OBS-0068 N2); 2026-11-16 (v5: an unbound file that reads like another dialect is offered a connection of its own dialect and says so, behaviour 3a; a run with DDL makes the file know what it created, behaviour 11a — OBS-0068 L12, L2); 2026-11-15 (v4: behaviour 11 holds for every Refresh of the Database window,
  also its toolbar button — re-check of PR #780, SPEC-0089 v11 behaviour 23); 2026-11-15 (v3: inspections follow a refreshed schema at once, behaviour 11 — OBS-0051 L7); 2026-11-05 (v2: running a file binds it to the connection it ran on — hand
  test 78, D31)
- **Sources:** GEN (SQL editors attach a script to a data source and schema, run the statement at the caret
  or the whole script, show how long a statement took and point out problems while typing; F-053, F-050),
  OBS-0010 A5 (a new console starts without a current schema; ⌘⏎ with several statements asks whether to
  run the statement at the caret or the whole script; the time stands behind the statement; the warning for
  `DELETE` without `WHERE` is only an underline whose text needs the mouse; "don't ask again" switches the
  check off for good), OBS-0070 V1 (v6: a run of an unbound file opens the reference's list of data
  sources at once and runs after the choice), OBS-0070 window check of F-050 (v7: G1, the popup did not open
  while the Database window was open; a temporary table of the same file was marked unknown), OSS (action ids `Console.Open` and `Console.Execute.Multiline` with their shortcuts in
  the ported keymaps), SPEC-0093 (console run and Services window), SPEC-0014 v2 (completion), SPEC-0015 v2
  (inspections and fixes), SPEC-0089 v3 (Database tool window), IMPROVEMENTS IMP-0060, IMP-0062,
  RESEARCH_BOUNDARIES 5a (proprietary area: own texts and layout)
- **Feature matrix rows:** F-053, F-050

## Purpose
Write SQL against one chosen database the way one writes code: the editor knows which data source and
schema a script belongs to, offers its names, flags mistakes in place, runs exactly what was meant and
says how long it took.

## Behaviour
1. **Data source of a file:** a `.sql` file under `.ferris-idea/consoles/<connection id>/` belongs to that
   connection. Any `.sql` file can be bound to a connection; bindings are kept in
   `.ferris-idea/sql-files.toml` (`[files]` table, path relative to the workspace → connection id) and win
   over the folder. A file without a binding uses the connection of SPEC-0093 behaviour 2 (selected in the
   Database window, else the only profile). A binding to a connection that no longer exists is ignored.
   **v2:** when such a file is run, it is bound to the connection it ran on, so it keeps its data source
   after a restart, when the Database window has nothing selected.
   [GEN]
2. **New console:** `Console.Open` (⌃⇧F10, as in the ported keymaps; also in Search Everywhere) creates
   `console.sql` (then `console_2.sql`, …) in the connection's console folder and opens it. The connection is
   the one selected in the Database window, else the one of the active `.sql` editor, else a popup lists the
   connections. A new console starts in the data source's default schema — the schema the connection lands
   in (PostgreSQL `current_schema()`, MySQL/MariaDB the profile's database, SQL Server the user's default
   schema, SQLite `main`, Oracle the user) — so names resolve at once without choosing a schema
   (**IMP-0060**). [GEN, OSS]
3. **Toolbar:** a `.sql` editor shows a thin bar above the text with a run button (▶, the same as ⌘⏎) and
   the data source as "<connection> · <schema>" (schema once known). Clicking the data source, or
   `Sql.ChooseDataSource` (Search Everywhere), opens a popup of the workspace's connections with the
   dialect; choosing one binds the file (behaviour 1) and reads that connection's schema for completion and
   inspections. [GEN]
3a. **v5 – The dialect of the text (OBS-0068 L12):** for a file not bound yet, Ferris IDEA looks for marks
   only one dialect family uses (MySQL's `DELIMITER` and backticks, PostgreSQL's dollar quotes and `::`,
   SQL Server's `GO` lines and `NVARCHAR`, Oracle's `/` lines and `VARCHAR2`, SQLite's `AUTOINCREMENT`
   and `PRAGMA`; comment lines do not count, a tie says nothing). When one family is clear, the popup's
   title names it and a connection of that family is preselected (MySQL and MariaDB count as one); and
   running the file on a connection of another dialect says in the status bar that the file reads like
   another dialect and that Choose Data Source binds another. Nothing is refused: the guess only
   informs. [GEN]
3b. **v6 – Running a file without a data source (OBS-0070 V1, OBS-0068 N2):** when ⌘⏎ or ▶ runs a
   `.sql` file that has no connection by behaviour 1 (not bound, not in a console folder, nothing
   selected in the Database window, and more than one connection in the workspace), the popup of
   behaviour 3 opens at once, with the connection preselected as there (behaviour 3a: one of the
   dialect the text reads like). Choosing a connection binds the file and goes on with the same run:
   a selection runs, a script of one statement runs, several statements bring the choice of behaviour 4.
   Escape runs nothing and leaves the file unbound. Without any connection the status bar says how to
   add one. Before v6 the status bar only pointed to the Database window, and after choosing the data
   source a second ⌘⏎ was needed. **v7:** "selected in the Database window" means selected by the user:
   a click, the keys, or an action that selects a node there. A tree nobody touched has no selected
   row (also after its connections are read again), so the popup opens while the Database window is
   open; ↑, ↓ or Home there select the first row, End the last, another tree key selects the first
   row and acts on it. Before v7 the first row counted as selected from the start, and the file ran on
   the first connection without asking. [GEN]
4. **What runs (⌘⏎, `Console.Execute.Multiline`):** a selection runs as it is. Without a selection, a
   script of one statement runs that statement. With several statements a small popup at the caret offers
   "Statement at caret" (its first line, shortened) and "Whole script (N statements)"; the statement is
   preselected, ↑/↓ and Enter or a click choose, Escape runs nothing. A caret outside every statement (only
   white space or comments) runs nothing and the status bar says so. [GEN]
5. **Timing next to the statement:** after a run, the last line of each statement that ran shows a grey
   note after its text: "<time> · N rows", "<time> · N rows affected" or, in red, "<time> · failed".
   Times under one second read "12 ms", longer ones "1.4 s". A new run replaces the notes; editing a
   statement's lines removes its note, editing elsewhere moves notes with their lines. [GEN]
6. **Results:** statements run through the session of the file's data source; results and the output log
   go to the Services window as in SPEC-0093 behaviours 3–6 (result model shared with the grid of F-054).
   [GEN]
7. **Inspections in place:** for a file whose data source schema is loaded, SPEC-0015 inspections run on
   open and shortly after typing stops; problems are underlined with their severity and listed in the
   Problems window under "SQL". The text of an unsafe-change warning (`UPDATE`/`DELETE` without `WHERE`)
   also stands in orange after the statement's first line, so it is seen without the mouse (**IMP-0062**).
   ⌥⏎ on a problem offers its fixes from SPEC-0015 v2; for an unknown name the "Replace with `closest`"
   fixes come first (**IMP-0060**). [GEN]
8. **Confirming unsafe changes:** before a run that contains an `UPDATE` or `DELETE` without `WHERE`, a
   popup names the statement kind, the table and how many rows it will touch ("Delete all 42 rows of
   `stock`?"), counted with `SELECT count(*)` on the same connection; if counting fails it says "all rows".
   Choices: "Run", "Run, don't ask again for <connection> in this session", "Cancel" (preselected).
   Suppression is kept in memory for that connection only and ends when the window closes;
   `Sql.AskAgainForUnsafeChanges` (Search Everywhere) ends it earlier (**IMP-0062**). [GEN]
9. **Completion** in the editor uses SPEC-0014 v2 with the file's data source, including the check that
   cached metadata is still current. [GEN]
10. **Agents (ADR-0008):** `sql_console` (connection) creates a console like behaviour 2 and returns its
    path and schema; `sql_file_data_source` (path, optional connection) returns or sets the binding of
    behaviour 1; `run_sql` refuses `UPDATE`/`DELETE` without `WHERE` unless `confirm_unsafe` is true, and its
    refusal names the table and row count like behaviour 8. [ADR-0008]
11. **v3 – Inspections follow the schema (OBS-0051 L7):** Refresh in the Database window (one connection or
    all) inspects every open `.sql` file of that connection again with the schema read anew, so a table
    created meanwhile is no longer marked unknown without typing first. An inspection that started before
    a refresh neither shows its problems nor keeps its schema for later inspections: it was read before the
    change. **v4:** this holds for every way to refresh the Database window, its toolbar button included
    (SPEC-0089 v11 behaviour 23); in v3 the button reached the tree only. [GEN]
11a. **v5 – After a run with DDL (OBS-0068 L2):** when the statements a `.sql` file or console ran
    include DDL or what may run DDL inside (`CREATE`, `ALTER`, `DROP`, `RENAME`, `COMMENT ON`, `DO`,
    `CALL`, `EXEC`, PL/SQL blocks), the connection's schema is read again for the SQL editors as in
    behaviour 11 once the run is over, so a table the script created is known in the same file
    without a Refresh. [GEN]
11b. **v7 – Tables the script creates:** a table that an earlier statement of the same text creates
    (`CREATE [TEMPORARY | TEMP | GLOBAL | LOCAL | UNLOGGED] TABLE [IF NOT EXISTS] [schema.]name`) is
    no unknown table in the later statements, before and after a run; temporary tables are not in the
    schema read again by behaviour 11a. Before its `CREATE` it stays unknown. [GEN]

## Acceptance criteria
- **AK-1** Binding resolution (console folder, `sql-files.toml` wins, missing connection ignored), next
  console name, run choice (selection, single statement, several statements, outside statements), note
  texts and placement, note movement on edits, unsafe-change confirmation text with and without a count
  (unit tests of `sql_editor_model`).
- **AK-2** Live against the reference databases (PostgreSQL, MySQL, SQLite): the default schema of a new
  console is `public` / the profile's database / `main`; counting the rows of an unsafe `DELETE`
  (integration tests, `db.sh` of the reference environment).
- **AK-3** In a real window (`ferris-idea ui --selftest`) with the workspace's SQLite connection:
  `Console.Open` opens a console bound to it with the schema in the toolbar; ⌘⏎ in a two-statement script
  shows the choice popup, Enter runs the statement at the caret and a timing note appears on its last line;
  "Whole script" runs both; `delete from orders` shows the inline warning text and on ⌘⏎ the confirmation
  naming `orders`, Cancel runs nothing; `select nme from customers` underlines `nme` and the first fix
  replaces it with `name`.
- **AK-4** Over MCP: `sql_console` and `sql_file_data_source` round trip; `run_sql` with `delete from t`
  is refused with the table name until `confirm_unsafe` is set (protocol test).
- **AK-5** By hand: consoles, toolbar, choice popup, notes and confirmation side by side with the reference
  (hand tests 116, 117).
- **AK-6 (v2)** Window self-test: after ⌘↩ in the unbound `edit.sql` the file is bound to the connection
  it ran on (`grid_edit_steps`).
- **AK-7 (v3, behaviour 11)** `sql_editor_model::tests::a_schema_read_before_a_refresh_is_not_kept` (a
  read started before the refresh of its connection, or of all, is stale; one started after is not) and
  `sql_editor_model::tests::a_refresh_names_the_open_files_to_inspect` (the open `.sql` files of the
  refreshed connection, all of them for a refresh of all).
- **AK-8 (v4)** The window test of SPEC-0089 AK-25 (`tests/it/sql_refresh_from_toolbar.rs`, macOS) goes
  the way of the re-check: toolbar Refresh, then an edit.
- **AK-9 (v5, behaviour 11a)** What counts as a schema change (`console_session::tests::ddl_and_blocks_change_the_schema`);
  a SQLite script that renames a table and reads it under the new name: after the run the table is
  no longer marked unknown (window test of the harness, `crates/workbench/tests/it/db_gaps_obs0068.rs`:
  `a_table_the_script_renames_is_known_in_the_same_file`; until v7 the script created the table,
  which behaviour 11b now knows before the run).
- **AK-10 (v5, behaviour 3a)** The dialect a text reads like, ties and comments, and the hint, with
  MySQL fitting MariaDB (`sql_editor_model::tests::the_dialect_a_file_reads_like`).
- **AK-11 (v6, behaviour 3b)** In a window of the test harness with two SQLite connections, ⌘⏎ in an
  unbound file opens the data-source popup with the connection of the file's dialect preselected;
  choosing one binds the file and runs the selected statement without a second ⌘⏎; Escape runs
  nothing and leaves the file unbound (`crates/workbench/tests/it/db_gaps_obs0070.rs`:
  `running_an_unbound_file_asks_for_its_data_source`); which of the three answers a run without a
  data source gets (`sql_editor_model::tests::a_run_without_a_data_source_asks_for_one`).
- **AK-12 (v7, behaviours 3b and 11b)** In a window of the test harness with two connections and the
  Database window open and untouched, nothing is selected there, ⌘⏎ in an unbound file opens the
  data-source popup with the connection of the file's dialect preselected, and Enter runs on it;
  after the user selects a connection, an unbound file runs on that one without asking; ↓ in an
  untouched tree selects the first row, Delete does nothing
  (`crates/workbench/tests/it/db_run_picker_f050.rs`: `an_untouched_database_tree_selects_nothing`,
  `an_unbound_file_asks_for_its_data_source_while_the_database_window_is_open`;
  `tree_nav::tests::keys_in_a_tree_without_selection`). A temporary table created earlier in the
  script is known in its later statements (`inspect::tests::unknown_tables`).

## Out of scope
Several consoles' result tabs at once (the Services window shows the last run, SPEC-0093), choosing a
schema other than the default per console, transaction controls, cancelling a running statement, query
parameters, run history, customising the choice popup (the reference offers this; Ferris IDEA always asks).

## Known deviations from the reference
- A new console starts in the data source's default schema instead of without one (IMP-0060).
- An unbound file is offered the connection of the dialect it reads like (behaviour 3a), and its
  first run is split in the dialect of the chosen connection; the reference offered its data sources
  in a fixed order and split the first run with the generic dialect, so the statements before a cut
  dollar quote or `DELIMITER` procedure had already run (IMP-0417, OBS-0070).
- The unsafe-change warning text is shown inline, the confirmation names the table and row count, and
  "don't ask again" is limited to one connection for the session instead of switching the check off for
  good (IMP-0062).
- Consoles are ordinary files in the workspace (`.ferris-idea/consoles/`), so they can be versioned or
  searched like other files.
