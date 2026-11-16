# SPEC-0128: Renaming tables and columns from the Database tool window

- **Version:** 4 · **Date:** 2026-11-14 (v4: one transaction where the database can, notes before running, local history for closed files, behaviour 7; v1 2026-11-02; v2: files of other dialects and derived names in
  the dialog — IMP-0056; v3: files outside the project's own sources are listed last and left out,
  behaviour 6)
- **Sources:** GEN (IDEs rename a table or column from the database tree with ⇧F6 / Shift+F6: a
  dialog with the new name and a preview of the statement and of the SQL places that change, then
  one action that alters the database and the scripts; F-057), DOC (action id `RenameElement` with
  ⇧F6), SPEC-0019 (analysis of references), SPEC-0089 (Database tool window), ADR-0008
- **Feature matrix rows:** F-057

## Purpose
Rename a table or column where it is shown, and have the database and the project's SQL scripts
agree afterwards.

## Behaviour
1. **Opening:** ⇧F6 / Shift+F6 on a table or column of a loaded connection in the Database tool
   window opens the dialog; the name field holds the old name, selected. [GEN, DOC]
2. **Preview** while typing: the `ALTER TABLE` statement (SPEC-0019 behaviours 5–6), the number of
   places changed and files, and per SQL file of the workspace (an open file with its editor text,
   saved or not) the number of changes and each ambiguous place with its line and reason. An empty
   name, the old name, an existing name or an unknown object show the reason instead. [GEN]
3. **Refactor** (Enter or the button): on a read-only connection nothing happens and the dialog
   says so. Otherwise the statement runs on the connection in the background. If the database
   refuses it, the dialog shows the error and nothing else changes. If it succeeds, the files
   change: an open file in its editor as one undo step, a closed one on disk; a file whose text
   changed since the preview is left alone and named. The connection's tree is read again and the
   status bar says what was renamed, how many places changed and how many ambiguous places were
   left. Escape or Cancel closes without renaming. [GEN]
4. **MCP:** `rename_sql_object` with `apply=true` (only without `sql`) runs the statement — only
   with the `modify-schema` permission and not on read-only connections — and writes the edits into
   the workspace's `.sql` files only if it succeeded; without `apply` it stays a preview
   (SPEC-0019 behaviour 7). v2: the result lists `derived` offers and per file its `dialect` and
   `other_dialect`; `include_other_dialects` (default false) and `rename_derived` (default true)
   choose what `apply` does. [ADR-0008]
5. **Other dialects and derived names (v2):** the preview lists every project SQL file with hits
   (SPEC-0019 v2 behaviour 8); files of another dialect are marked with their dialect and unchecked,
   and a click includes them. Below, the derived constraint and index names (SPEC-0019 v2 behaviour
   9) are listed with their statement, checked; a click leaves one out; names that cannot be renamed
   show the reason. Refactor runs the main statement, then the checked derived statements, then
   changes the checked files. A derived statement the database refuses is named in the status bar;
   the main rename and the files stay done. [GEN, OBS: IMP-0056]
6. **Only the project's own sources (v3; decision of the coordinator of 2026-11-13 under the Project Owner's general authority: the rule the Project Owner chose for Rename, Find in Files, Search Everywhere, Inspect Code and Endpoints; SPEC-0019 v4 behaviour 12):** in the dialog's
   preview the SQL files that are not the project's own come after the others, unchecked, with the
   reason next to the number of changes ("in a virtual environment", "ignored by .gitignore");
   they do not count in the summary and Refactor leaves them unchanged. A click includes such a
   file on purpose, as for a file of another dialect. Over MCP `apply` changes them only with
   `include_outside` (SPEC-0019 v4 behaviour 12). [GEN]
7. **v4 – Transaction, notes, history:** Refactor runs the statements as SPEC-0164 v3 behaviour 11
   says: in one transaction on PostgreSQL, SQLite, SQL Server and DuckDB (a refused derived
   statement then undoes the main rename too, and behaviour 5's "the main rename stays" holds only
   on MySQL, MariaDB and Oracle). The preview shows a line saying which holds, and the notes of
   SPEC-0164 v3 behaviour 12. A changed file without an editor gets local history revisions
   (SPEC-0164 v3 behaviour 13), and the status bar ends with how to take the file changes back.
   [GEN, STD]

## Acceptance criteria
- **AK-1** The preview of the dialog uses the SPEC-0019 analysis (its unit tests); the tree knows
  column rows (`db_tree_model::tests`).
- **AK-2** MCP: `apply` refused without `modify-schema`; with it the column is renamed in SQLite and
  the clear place in a file changes while the ambiguous one stays (`crates/mcp/tests/it/protocol.rs`:
  `rename_sql_object_rewrites_text_and_workspace_files`).
- **AK-3** Window self-test (`db_rename_steps`): ⇧F6 on `orders.note` previews
  `ALTER TABLE orders RENAME COLUMN note TO remark` and one place in `notes.sql`; Enter renames the
  column in SQLite, the file and the tree; renaming back works the same way.
- **AK-4** Hand test (round 4): rename a table and a column of a PostgreSQL database with scripts
  next to the reference.
- **AK-5 (v2)** Window self-test: a file under `mysql/` with the column is listed unchecked for a SQLite
  connection and stays unchanged after Refactor; including it changes it.
- **AK-6 (v2)** MCP: `derived`, `other_dialect` and `include_other_dialects` (protocol test).
- **AK-7 (v2)** Hand test 122: rename `item.unit_price_cents` on PostgreSQL in `sql-basic`.
- **AK-8 (v3)** The preview lists files outside the project's own sources after the others, not
  included and not counted; included by label they change and count
  (`db_rename_dialog::tests::files_outside_the_own_sources_are_listed_last_and_left_out`). MCP:
  SPEC-0019/AK-12. Window self-test: not written yet.
- **AK-9 (v4)** SPEC-0164/AK-8 to AK-10.

## Out of scope
Schemas; constraints on their own (SPEC-0164 v3); references stored in the database (views, triggers); undo of the
database change.

## Known deviations from the reference
- Every project SQL file is searched, bound or not, in its own dialect; derived constraint and index
  names are offered (IMP-0056).
- Ambiguous places are listed and left unchanged instead of being changed or silently skipped
  (SPEC-0019).
- v3: SQL files outside the project's own sources are unchecked in the preview (behaviour 6); the
  reference was not compared for this.
