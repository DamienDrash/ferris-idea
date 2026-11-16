# SPEC-0093: SQL console and result grid

- **Version:** 8 · **Date:** 2026-11-16 (v8: the server's notices in Output and in `run_sql`, behaviour 6a — OBS-0068 L1); 2026-11-15 (v7: an error names its place, a connection without Auto-commit starts its consoles in Manual, Manual stays one transaction after a COMMIT or ROLLBACK in the script, a changed data source leaves the old Manual session, behaviours 6, 10–11 — hand test 68 retest); 2026-11-14 (v6: on a comment line right before a statement the statement runs, behaviour 1); 2026-11-13 (v5: the history list by the keymap action `Console.History.Browse`, behaviour 16); 2026-11-10 (v4: the history also keeps statements run from project `.sql`
  files bound to the connection, and its list closes like other popups — window check 2026-11-10; with completion from the connection's schema; v2: running closes
  the completion list — hand tests 22 and 43, D12; v3: the console bar with transaction mode, Commit and
  Rollback, cancelling the running statement and the query history — OBS-0026 L12)
- **Sources:** GEN (a SQL file can be run against a data source; the statement at the caret, or the
  selection, runs with ⌘↩ / Ctrl+Enter; results appear in the Services tool window as a grid with column
  headers, row numbers and NULL shown distinctly, with a line giving the row count and the time; errors
  and update counts go to an output log; F-054), OSS (action ids `Console.Execute.Multiline` and
  `ActivateServicesToolWindow` with their shortcuts in the ported keymaps; platform icons
  `expui/toolwindows/services`), SPEC-0005 (script splitting), SPEC-0007 (script results), SPEC-0008 (connection profiles),
  SPEC-0009/0010/0011 (sessions), SPEC-0089 (Database tool window), SPEC-0014 (SQL completion), SPEC-0072 (completion list), RESEARCH_BOUNDARIES 5a
  (proprietary area: own texts and layout details, open icons)
- **Feature matrix rows:** F-054, F-053

## Purpose
Run SQL from the editor and look at the rows without leaving the IDE.

## Behaviour
1. **Run:** in an editor on a `.sql` file, `Console.Execute.Multiline` (⌘↩ on macOS, Ctrl+Enter
   elsewhere) runs the selection, or without a selection the statement at the caret. The statement at
   the caret is the one whose text contains the caret; on a line after the last `;` of a statement it is
   that statement. In other files the key keeps its other bindings (Split Line). **v2:** running closes
   an open completion list and cancels one that was about to open automatically. **v6 (window check
   2026-11-14):** on a comment line (`--`, `/* … */`, `#` in MySQL and MariaDB) with only comment
   lines between it and the next statement, that statement runs; a blank line in between ends the
   comment's tie and the console says there is no statement at the caret. Until v6 a comment line
   right before a statement always said so. [GEN, OSS]
2. **Connection:** the statements run against the connection selected in the Database tool window
   (SPEC-0089), else the only profile of `.idecraft/connections.toml`. Without one the status bar says
   "Choose a connection in the Database tool window" and nothing runs. The editor tab is unchanged; the
   Services window names the connection. [GEN]
3. **Background:** connecting and running happen off the UI thread. While they run, the Services
   window shows "Running…" and another ⌘↩ waits for the first run to end. [GEN]
4. **Services tool window:** `ActivateServicesToolWindow` (⌘8 / Alt+8) or the stripe button shows it at
   the bottom; the first run opens it. Its header names the connection; the tabs are "Output" and one tab
   per result set of the last run ("Result 1", "Result 2", …; the last one is selected).
   `HideActiveWindow` hides it. [GEN, OSS]
5. **Grid:** a row of column headers, a row-number column, then the rows. Cells use the editor font; SQL
   NULL shows as `<null>` in a dimmed colour; long values end in "…". Columns are as wide as their
   longest shown value (at least the header, at most 320 px). The grid scrolls both ways and shows at most
   500 rows. Under it: "N rows · T ms", or "first 500 of N rows · T ms". [GEN]
6. **Output:** each statement adds a line: the first line of its text, then "N rows", "N rows
   affected" or the error in red with its SQLSTATE, and the time. A failed statement stops the run (the
   rest are listed as not run), and the Output tab is selected. [GEN] **v7:** an error whose database
   reports a position (SPEC-0007 behaviour 5; PostgreSQL, Oracle) names it after the message's first
   line: `(at column 8)`, or `(at line 2, column 3 of the statement)` for a statement of several
   lines; a position outside the statement is left out. [STD]
6a. **v8 – The server's notices (OBS-0068 L1):** what the server reports while a statement runs
   without failing it — PostgreSQL's `RAISE NOTICE`, `RAISE WARNING` and its own notices — follows
   that statement's Output line, one indented line each as `SEVERITY: message`, in the order they
   came, and belongs to that statement only. `run_sql` returns them per statement as `notices`
   (left out when there are none). Other dialects' messages of this kind are out of scope. [STD,
   ADR-0008]
7. Agents run SQL through `run_sql` (SPEC-0012) and complete it through `complete_sql` (SPEC-0014).
   [ADR-0008]
8. **Completion:** in a `.sql` file the completion list (SPEC-0072: `CodeCompletion`, ⌥Esc on macOS,
   and while typing) offers the tables and views of the console's connection, the columns of the
   tables named in the statement, and keywords, from SPEC-0014. The kind ("table", "view", "column",
   "keyword") stands at the right. The schema is read in the background the first time and kept for the
   session. Without a connection the status bar says so (only when opened by hand). [GEN]

9. **v3 – Console bar:** the bar above a `.sql` editor (SPEC-0192 behaviour 3) holds ▶ (run), ■
   (cancel), the data source and schema, the transaction button and History. [GEN, OBS-0026]
10. **v3 – Transaction mode:** "Tx: Auto" runs each run on its own connection and commits each
    statement (as before). A click switches the console to "Tx: Manual": the console keeps one
    connection of its own while it is open; the first run starts a transaction (at the database's
    default isolation, SPEC-0190 v3 statements), later runs go into the same transaction, the button
    shows "Tx: Manual (n)" with the number of statements run in it, and Commit and Rollback appear;
    each ends the transaction with a line in Output ("Committed 3 statement(s)", "Rolled back …") and
    the next run starts a new one. Switching back to Auto, or closing the window, with an open
    transaction rolls it back and Output says so — nothing is committed implicitly. A connection whose
    option Auto-commit is off (SPEC-0108 v4) is the user's choice for its consoles; the button shows
    the mode in force. [GEN, STD, OBS-0026] **v7:** such a connection starts its consoles (and the
    project `.sql` files bound to it) in Manual: the button says "Tx: Manual" before the first run and
    the run opens the transaction. A console switched to Auto by hand stays Auto while it is open.
    Before v7 only data editors followed the option and consoles committed every statement, so a
    ROLLBACK the user relied on did nothing. [GEN]
11. **v3 – Its own session:** in Manual mode the console's statements, the reading of editable results
    (SPEC-0107) and Commit/Rollback all run on that one connection, so the grid shows the uncommitted
    state. [GEN] **v7:** a `COMMIT` or `ROLLBACK` typed in the script (not `ROLLBACK TO` a savepoint)
    ends the transaction and the next statement starts a new one, so Manual stays Manual for the rest
    of the run and for later runs (before, everything after it was committed at once). When the
    console runs on another connection than its Manual session's — its data source changed — the old
    session is left, its open transaction rolled back with a line in Output ("Manual transaction
    rolled back (n statement(s)): the data source is now …"), and nothing more runs on it. [GEN]
12. **v3 – Cancel:** while a statement runs, ■ asks the database to end it: PostgreSQL's cancel request
    (with the profile's TLS settings), MySQL's and MariaDB's `KILL QUERY` on a second connection,
    SQLite's interrupt; the statement ends with the database's own error (PostgreSQL SQLSTATE 57014) or
    early (MySQL `SLEEP`), which Output shows, and the session stays usable. SQL Server, Oracle and
    DuckDB have no cancel yet (■ stays grey). MCP `cancel_sql` (connection) cancels a running
    `run_sql` of that connection and says whether something ran. [GEN, STD, ADR-0008]
13. **v3 – History:** every statement run in one of the connection's consoles (`.idecraft/consoles/`)
    is kept in the connection's query history, newest first, at most 500, a repeat of the newest only
    moves its time, with whether it failed. History opens the list (failed ones in the error colour); a
    click puts the statement at the caret. The history lives in the user's settings folder
    (`sql-history/<connection>-<workspace hash>.jsonl`, folder 0700, file 0600), never in the
    workspace; statements that carry a password (user or role statements with `PASSWORD` or
    `IDENTIFIED`, `SET PASSWORD`, connection strings with `password=`, `\password`) are never kept, also
    when such a line was put into the file by hand (IMP-0372). MCP `sql_history` (connection, optional
    `query` and `limit`) returns the entries. [GEN, ADR-0008]

14. **v4 – History from every bound SQL file:** the history of behaviour 13 also keeps the statements
    run from a project `.sql` file bound to the connection, not only from its consoles, since the same
    History button is on that file's bar. [window check 2026-11-10]
15. **v4 – History list closes:** the list closes with Escape, a click outside it, a pick, or a second
    click on History. [GEN, window check 2026-11-10]
16. **v5 – History by action:** `Console.History.Browse` (⌥⌘E on macOS, Ctrl+Alt+E elsewhere;
    "Browse Query History" in Find Action) opens and closes the list of behaviour 13, as a click
    on History does, for the console or bound `.sql` file in the active editor. Without such an
    editor it is not available and the status line says so. MCP stays `sql_history` (behaviour
    13). [OSS: id and shortcut of the bundled keymaps; GEN]

## Acceptance criteria
- **AK-1** Statement at the caret (inside, after the `;`, between statements, last without `;`),
  grid column widths, NULL text, row limit and the summary and output texts (unit tests of
  `sql_console_model`).
- **AK-2** In a real window (`idecraft ui --selftest`), with the SQLite connection of the workspace: a
  `.sql` file with two statements, ⌘↩ on the second shows its rows in the Services window ("2 rows"),
  a failing statement selects Output with the error, ⌘8 hides the window.
- **AK-3** By hand: grid look and scrolling with a larger table of the reference databases, side by side
  with the reference.
- **AK-4** In a real window: in `select * from cu`, completion lists `customers` (`--selftest`).
- **AK-5 (v2)** Window self-test: ⌘↩ in `edit.sql` runs the query and leaves no completion list open
  (`grid_edit_steps`).
- **AK-6 (v3)** Labels of the transaction button and the history list
  (`console_session::tests::labels`).
- **AK-7 (v3)** Cancelling: an endless SQLite query is interrupted; PostgreSQL `pg_sleep(30)` ends with
  57014 and MySQL `SLEEP(30)` ends early, both within 10 s, and the MySQL session stays usable (live,
  `tests/cancel.rs`).
- **AK-8 (v3)** History: newest first, repeats folded, limit 500, search; never a statement with a
  password; saved with mode 0600 and read back; a password line put into the file is dropped; one file
  per connection and workspace (`history::tests`).
- **AK-9 (v3)** MCP `cancel_sql` without a running call, `sql_history` (protocol test
  `grid_values_object_doc_history_and_connection_extras`).
- **AK-10 (v3)** Window self-test (`grid_edit_steps`, written, not run yet): in Manual a run opens the
  transaction ("Tx: Manual (1)"), Commit ends it with "Committed 1 statement(s)", Auto again; in the
  window next to the reference with PostgreSQL and MySQL: cancel of a long statement, history, Rollback
  (L12).

- **AK-11 (v4)** A statement run from a bound project `.sql` file appears in the connection's history
  (unit test of the recording decision); Escape and a click outside close the list (window check).
- **AK-12 (v5, behaviour 16)** In a window of the test harness with a file that is no console, the
  action opens no list and the status line says that it is not available
  (`tree_and_popup_actions_keymap.rs::history_actions_need_their_place`); the title and the effect
  (`everywhere::tests::fourth_package_actions_have_titles`,
  `security::tests::fourth_package_actions_declare_their_effects`). With a console: by hand.
- **AK-13 (v6, behaviour 1)** `sql_console_model::tests::statement_at_the_caret`: the caret on a
  comment line right before a statement selects it; after a blank line it does not.
- **AK-14 (v7, behaviours 6, 10, 11)** `sql_console_model::tests::errors_name_their_place`,
  `console_session::tests::transaction_ends`; in a window of the test harness on SQLite
  (`crates/workbench/tests/it/db_dialect_retest.rs`
  `consoles_of_a_connection_without_auto_commit_start_in_manual`): the console of a connection with
  Auto-commit off shows Manual before its first run and its ROLLBACK takes the insert back; switched
  to Auto it commits and stays Auto; a Manual session left for a read-only data source is rolled
  back and the insert is refused; against Oracle Free (`hand_test_68_oracle_through_the_instant_client`)
  `COMMIT`, a PL/SQL block and `ROLLBACK` keep one row and the error reads `ORA-00904 … (at column 8)`.
- **AK-15 (v8, behaviour 6a)** A `DO` block with `RAISE NOTICE` and `RAISE WARNING` gives both
  notices in order on its own outcome and none on the next statement (live against PostgreSQL,
  `crates/database/tests/it/postgres.rs`: `notices_belong_to_their_statement`); the Output line is
  indented under the statement (`sql_console_model::tests::a_notice_is_indented_under_its_statement`).

## Out of scope
Editing cells in the grid (F-055), paging beyond 500 rows, sorting and filtering, choosing the connection
per file, query parameters, export from the grid (SPEC-0021 has it for
agents), explain plans in the UI, reading the schema again after it changed (restart the window), SQL
inspections in the editor (SPEC-0015).

## Known deviations from the reference
The reference attaches a console to a data source and keeps many result tabs and a full data editor.
IDECraft runs any `.sql` file against the connection selected in the Database window and shows the
results of the last run. **v3:** the console bar has transaction mode, Commit/Rollback, cancel and
history like the reference's toolbar (OBS-0026 item 1), without its parameter and settings buttons;
leaving Manual with an open transaction rolls back and says so instead of asking; the history never
keeps statements with passwords and lives outside the workspace (IMP-0372); a manual console
transaction is a UI mode — agents put `BEGIN`/`COMMIT` into the `run_sql` script themselves.
Inside the body of a routine, ⌘↩ offers the whole `CREATE` statement and the whole script with its
number of statements; the reference pre-selects the inner statement, a fragment of the routine, and
names the whole script only by its first line (IMP-0416, OBS-0068 M1, P2).
