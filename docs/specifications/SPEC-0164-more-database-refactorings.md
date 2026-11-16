# SPEC-0164: Renaming views, indexes and routines; changing a column's type

- **Version:** 5 · **Date:** 2026-11-16 (v5: a sequence's name in `nextval('…')` and `'…'::regclass` changes when it names the renamed sequence without doubt, behaviour 8; undo of a database refactoring says that the database keeps the change and offers the way back, behaviour 16 — OBS-0070 V3, V4, IMP-0419); 2026-11-15 (v4: a file bound to another connection is listed, never changed, behaviour 15 — OBS-0051 L1); 2026-11-14 (v3: sequences and constraints, the Database tree as entry point, one transaction where the database has transactional DDL, what a statement risks, behaviours 8–14 — F-057; v2: a run that changed nothing is reported, never taken for a change, behaviour 7 — hand test 90, D32); 2026-11-03
- **Sources:** GEN (database IDEs rename any schema object and change column types from the
  editor and the database tree), STD (`ALTER VIEW … RENAME`, `RENAME TABLE`, `ALTER INDEX …
  RENAME`, `ALTER TABLE … RENAME INDEX`, `ALTER ROUTINE … RENAME`, `sp_rename`,
  `ALTER COLUMN … TYPE`, `MODIFY COLUMN`, per the PostgreSQL, MySQL/MariaDB, SQL Server, Oracle
  and SQLite manuals; v3: `ALTER SEQUENCE … RENAME TO`, `ALTER TABLE … RENAME CONSTRAINT`,
  transactional DDL in PostgreSQL, SQLite, SQL Server and DuckDB, the implicit commit of DDL in
  MySQL, MariaDB and Oracle, MariaDB sequences being renamed like tables), SPEC-0019, SPEC-0128, SPEC-0152, ADR-0008, OBS (v4: OBS-0051 L1, renaming a PostgreSQL index changed a file bound to a MySQL connection; v5: OBS-0070 R1, R4 — the reference changes `nextval('pd_seq')` with the sequence, and after undo its file names a column the database no longer has), STD (v5: PostgreSQL reads a `regclass` literal as an optionally schema-qualified name, unquoted parts folded to lower case; `search_path`), IMPROVEMENTS IMP-0419
- **Feature matrix rows:** F-057

## Purpose
Close the rest of the database refactorings: views, indexes and functions get renamed with the
statement their database understands, and a column's type changes without writing DDL by hand.

## Behaviour
1. **Views and tables per dialect:** renaming a view uses `ALTER VIEW … RENAME TO` (PostgreSQL),
   `RENAME TABLE` (MySQL, MariaDB), `RENAME … TO` (Oracle) or `sp_rename` (SQL Server); SQL Server
   tables and columns use `sp_rename` too. What a database cannot do (SQLite: rename a view;
   most: rename a view's column) is refused with the reason instead of a wrong statement. [STD]
2. **Indexes:** ⇧F6 on an index name renames it (`ALTER INDEX`, `ALTER TABLE … RENAME INDEX`,
   `sp_rename … 'INDEX'`; refused on SQLite) and changes the name in the workspace's index
   statements only. [STD]
3. **Routines:** ⇧F6 on a function or procedure (after FUNCTION, PROCEDURE, CALL, EXEC, or a call
   that is no built-in function) renames it on PostgreSQL (`ALTER ROUTINE`) and SQL Server
   (`sp_rename`) and changes its definitions and calls; other databases have no rename and say so.
   [STD]
4. **Column type:** ⌘F6 (Change Signature) on a column in a SQL editor opens the dialog for the new
   type; the statement keeps NOT NULL and the default where the dialect restates the column
   (MySQL, MariaDB, SQL Server). Only plain type names are accepted. Views and SQLite are refused.
   [GEN, STD]
5. **One dialog:** all use the rename dialog of SPEC-0128 — title, field, statement preview, the
   places in the workspace's SQL files, then run the statement and change the files. [GEN]
6. **MCP:** `rename_sql_object` takes `index` or `routine`; `change_column_type` returns or runs
   the statement (modify-schema permission). [ADR-0008]
7. **v2 – Nothing changed, said so (hand test 90, D32):** when the database refuses the statement,
   or no statement of it runs at all, the dialog stays open with the reason and the status bar
   says it too ("The database refused it; nothing changed: …"); neither the success message nor a
   change of the workspace's files follows. [GEN]
8. **v3 – Sequences:** a sequence is renamed with `ALTER SEQUENCE … RENAME TO` (PostgreSQL),
   `RENAME TABLE` (MariaDB), `RENAME … TO` (Oracle) or `sp_rename` (SQL Server). MySQL and SQLite
   have no sequences and DuckDB no statement for it; they refuse with that reason. In the SQL
   files the name changes in statements about a sequence (`CREATE`, `ALTER`, `DROP SEQUENCE`),
   after `NEXT VALUE FOR` and before `.nextval` / `.currval`. A name inside a literal
   (`nextval('name')`) is listed as a place left unchanged, with its line. [STD]
   **v5 – Names in sequence literals (OBS-0070 V3):** in a file read as PostgreSQL, a string literal
   that is the first argument of `nextval`, `currval` or `setval`, or that is cast with `::regclass`,
   names a sequence. Its text is read the way PostgreSQL reads it: a dot separates schema and name,
   unquoted parts are folded to lower case, a part in double quotes keeps its case. The literal changes
   with the rename, and the change is one of the places of the preview, when it names the renamed
   sequence without doubt:
   - written with the sequence's schema (`'public.pd_seq'`), or
   - written without a schema, and the file sets no search path of its own before it (no `SET
     search_path`, `SET SCHEMA` or `set_config('search_path', …)`), or sets one whose first schema is
     the sequence's: the file then runs where the rename runs, so the name finds the same sequence.
   The new name is written like the old one: in double quotes where the old part was quoted or the new
   name needs them, a `'` doubled. A literal written with another schema names another sequence and is
   neither changed nor listed. Listed with their line and left unchanged, as before: a literal without
   a schema after the file set another search path (or one IDECraft cannot read), a literal with a
   schema while the sequence's schema is not known, an escape string (`E'…'`), and every other string
   that holds the name. The sequence's schema is the one the connection works in, where the Database
   tree lists the sequence; the dialog and `rename_sql_object` read it from the database. Literals in
   other dialects stay listed (they have no such literals). [STD]
9. **v3 – Constraints on their own:** a named constraint of a table is renamed with `ALTER TABLE …
   RENAME CONSTRAINT` (PostgreSQL, Oracle) or `sp_rename … 'OBJECT'` (SQL Server); on MySQL and
   MariaDB a unique key with `ALTER TABLE … RENAME INDEX`, a foreign key or check is refused with
   the reason; SQLite and DuckDB refuse. The files change as for a derived name (SPEC-0019 v3
   behaviour 11). [STD]
10. **v3 – From the Database tree:** ⇧F6 / Shift+F6 and "Rename…" of the context menu work on a
    table, a view, a column, an index row, a sequence row and a named foreign key row; ⌘F6 /
    Ctrl+F6 and "Change Column Type…" on a column row. On another row the status bar says what can
    be renamed there. [GEN]
11. **v3 – One transaction where the database can:** on PostgreSQL, SQLite, SQL Server and DuckDB
    the statement and the chosen statements for derived names (SPEC-0128 v2 behaviour 5) run in
    one transaction: when the database refuses one of them, all are rolled back, the dialog names
    the statement and the refusal, and no file changes. On MySQL, MariaDB and Oracle every such
    statement commits on its own (their DDL cannot be rolled back); there SPEC-0128 v2 behaviour 5
    stays: a refused derived statement is named, the main rename and the files stay done. The
    preview says which of the two holds before anything runs. [STD]
12. **v3 – What it risks, before it runs:** the preview lists notes under the statement. A type
    change is marked as one that may lose data: the database converts every stored value and
    refuses or changes values that do not fit. A rename says that views, triggers and routines
    stored in the database that use the old name are not rewritten by IDECraft. Nothing runs
    before Refactor (Enter) in the dialog; over MCP nothing without `apply`. [GEN]
13. **v3 – Taking the file changes back:** an open file changes as one undo step of its editor
    (SPEC-0128 behaviour 3). A file no editor has open gets a revision in the local history
    before and after the change, like Replace in Files, so Show History has the old text. The
    database change itself is not undone; renaming back is a second refactoring. [GEN]
    **v5:** undoing such a step says what the database keeps and offers the way back (behaviour 16).
14. **v3 – MCP:** `rename_sql_object` takes `sequence`, or `constraint` with `table`. Its result
    and that of `change_column_type` carry `transaction` (whether a run is one transaction) and
    `notes` (text, and `data_loss` for a statement that may lose data). With `apply`, behaviour 11
    holds: in one transaction a refusal changes nothing; a run in which no statement ran is an
    error (behaviour 7), never `applied: true`. [ADR-0008]
15. **v4 – Files of another connection are only listed (OBS-0051 L1):** a rename or type change on
    connection C changes only `.sql` files bound to C (SPEC-0093: the file's entry in
    `sql-files.toml` or its console folder) and files bound to no connection. A file bound to
    another connection D is analysed in D's dialect and listed with D's name and its places, but
    never changed: the dialog shows it unchecked and a click does not include it, and over MCP it
    appears in `files` with `other_connection: "<D>"` while no option changes it. The count of
    changed places counts only files that change. This narrows SPEC-0019 v2 behaviour 8 (every SQL
    file in its own dialect) for every rename and type change from the dialog, the Database tree and
    `rename_sql_object`: D's database still has the old name, so its files keep it. [GEN]
16. **v5 – Undo after a database refactoring (OBS-0070 V4, IMP-0419):** ⌘Z (Ctrl+Z) on the undo step
    that a rename or type change made in an open file takes back that file's text only. At that
    moment the status bar says so and names what the database keeps ("Undo took back q.sql only: in
    the database the column is still pd_note.qty"), and a popup at the caret offers two answers:
    "Keep the database as it is" (preselected; Escape does the same) and "Rename back in the
    database…" ("Change the type back in the database…" after a type change). The second opens the
    dialog of behaviour 5 for the inverse change — the new name back to the old one, or the column
    back to its old type — on the database's current state, with its statement, notes and file
    places; nothing runs before Refactor (Enter) there (behaviour 12), and files that still hold the
    new name change with it. Redo (⇧⌘Z) re-applies the file change without a question. Over MCP,
    `rename_sql_object` and `change_column_type` return `inverse_ddl`: the statement that takes the
    database change back (after a type change it converts the values again). It never runs by
    itself. [GEN]

## Acceptance criteria
- **AK-1** Statements per dialect, refusals, index and routine edits, object at the caret, type
  changes (`refactor::tests`: 4 new tests).
- **AK-2** The statements run on the reference servers: view and index renamed, type changed
  keeping NOT NULL on PostgreSQL, MySQL and MariaDB; a function renamed and called on PostgreSQL
  (`crates/database/tests/it/refactor_live.rs`).
- **AK-3** Window self-test: ⌘F6 on a column opens Change column type; on SQLite the preview names
  the limitation.
- **AK-4** Hand test (round 4, no. 90).
- **AK-5 (v2, behaviour 7)** `db_rename_dialog::tests::a_refused_or_empty_run_is_no_change`: a
  refusal and an empty run are no change, a statement that ran is.
- **AK-6 (v3, behaviours 8–9)** `refactor::tests::{sequences_are_renamed_per_dialect,
  constraints_are_renamed_on_their_own}`: statements, refusals, file places, the literal left alone.
- **AK-7 (v3, behaviour 10)** `db_tree_model::tests::index_sequence_and_key_rows_are_objects`;
  `context_menu::tests` for the menu entries; window self-test (`db_refactoring_steps`): ⌘F6 on a
  column row opens Change column type, ⇧F6 on an index row previews the refusal of SQLite.
- **AK-8 (v3, behaviour 11)** `refactor::tests::which_databases_roll_ddl_back`; live
  (`refactor_live.rs::a_refused_derived_statement_rolls_everything_back`): on PostgreSQL and
  SQLite a refused second statement leaves the first undone, on MySQL and MariaDB it stays.
- **AK-9 (v3, behaviour 12)** `refactor::tests::notes_name_what_a_statement_risks`;
  `db_rename_dialog::tests::the_preview_carries_notes_and_the_transaction`.
- **AK-10 (v3, behaviour 13)** `db_rename_dialog::tests::closed_files_get_history_revisions`.
- **AK-11 (v3, behaviour 14)** `crates/mcp/tests/it/protocol.rs::database_refactorings_carry_notes_and_refusals`.
- **AK-12 (v4, behaviour 15)** Two bound files: `db_rename_dialog::tests::files_of_another_connection_are_listed_and_never_changed`
  (a file bound to the renamed connection and an unbound one change, a file bound to another
  connection is listed, unchecked, and a click leaves it out); over MCP
  `crates/mcp/tests/it/protocol.rs::a_rename_changes_no_file_bound_to_another_connection` with two
  SQLite connections: `apply` renames the column in the database and in the file bound to it, the
  file bound to the other connection is listed with `other_connection` and keeps its text.
- **AK-13 (v5, behaviour 8)** `refactor::tests::sequence_literals_follow_when_they_name_the_sequence`:
  `nextval('pd_seq')`, `'pd_seq'::regclass`, `setval('PD_SEQ', 1)`, `currval('public.pd_seq')` and
  `nextval('"pd_seq"')` change; `nextval('other.pd_seq')` stays without a note; a literal after `SET
  search_path TO other`, a qualified literal with the schema unknown, `E'pd_seq'` and `select 'pd_seq'`
  are listed; a new name that needs quotes is quoted. Live on PostgreSQL
  (`crates/database/tests/it/refactor_live.rs::a_renamed_sequence_follows_in_nextval`): after the
  rename the changed file's `nextval` runs.
- **AK-14 (v5, behaviour 8)** In the dialog the literal is one of the places
  (`db_rename_dialog::tests::a_sequence_literal_is_a_place_of_the_preview`).
- **AK-15 (v5, behaviour 16)** The inverse of each refactoring and the texts
  (`db_rename_dialog::tests::each_refactoring_has_its_way_back`); in a window of the test harness on
  SQLite: after ⇧F6 renames a column, ⌘Z in the file brings back the old name, the status bar names
  the column the database keeps and the popup offers both answers with Keep preselected; Escape changes
  nothing in the database; the second answer opens the dialog renaming the new name back to the old one
  (`crates/workbench/tests/it/db_gaps_obs0070.rs`: `undo_after_a_rename_says_what_the_database_keeps`).
- **AK-16 (v5, behaviour 16)** Over MCP `rename_sql_object` and `change_column_type` carry
  `inverse_ddl` (`crates/mcp/tests/it/protocol.rs::database_refactorings_carry_notes_and_refusals`).

## Known deviations from the reference
v3: the reference's handling of a refused statement among several, of sequences and constraints
from its tree, and its warnings before a type change were not compared (to be compared).
Renames of views, indexes and routines search the project's SQL files like SPEC-0019 v2 behaviour 8:
every file, bound or not, in its own dialect; files of another dialect are only listed (IMP-0056).
v4: files bound to another connection are only listed, never changed (behaviour 15); how the
reference treats them is not observed.
Compared with a connection (OBS-0070):
- A type change is written without a forcing cast (behaviour 12); the reference writes
  `USING col::new_type`, which cuts values that do not fit without an error, and gives no note
  (IMP-0418).
- The dialogs work on the database's current state; the reference warned about objects changed in
  the database after its own rename until the refactoring was forced (IMP-0420).
- Derived constraint and index names are offered; the reference keeps the old names (IMP-0056).
- Undo takes back the files only (behaviour 13), as in the reference. v5: IDECraft says at once what
  the database keeps and offers the inverse change through the dialog; the reference said it only
  after undoing and offered nothing (behaviour 16, IMP-0419).
- v5: a sequence name inside `nextval('…')` or `'…'::regclass` changes like in the reference
  (behaviour 8); a literal whose schema or search path leaves a doubt is listed instead (how the
  reference treats such a literal was not observed).

## Out of scope
Routines in the database tree (the catalog does not list them yet), recreating SQLite tables to
change types, rewriting view definitions after a type change, Inline. v3: dropping an object
with a preview of its usages, and changing a column's nullability or default from the same
dialog, are not specified: what the reference offers there has not been observed.
