# SPEC-0017: Editing table data

- **Version:** 1 · **Date:** 2026-10-31
- **Sources:** GEN (database tools let users change cells, add and delete rows in a result grid and submit
  the changes; F-055), STD (`UPDATE`/`INSERT`/`DELETE`; NULL-safe comparison: PostgreSQL `IS NOT DISTINCT
  FROM`, MySQL `<=>`, SQLite `IS`; transactions), OBS-0005 (reference grid: add/delete row, revert, submit,
  transaction mode "Auto"), SPEC-0007 (identifier quoting), SPEC-0013 (keys)
- **Feature matrix rows:** F-055

## Purpose
Change rows of a table safely: collect edits, show exactly which SQL will run, apply all of them in one
transaction, and never overwrite a row that someone else changed in the meantime.

## Behaviour
1. Editing needs a row identity: the primary key, else the first unique index whose columns are all
   NOT NULL. A table without one cannot be edited; the reason is stated. [GEN]
2. Edits are collected without touching the database: set a cell (row identity, column, value loaded,
   new value), insert a row (column values), delete a row (row identity and loaded values). Setting several
   cells of the same row gives one `UPDATE`. [GEN]
3. The SQL is available before anything runs. Identifiers are quoted per dialect (SPEC-0007). Values are
   string literals with quotes doubled (and backslashes doubled for MySQL); NULL stays `NULL`. The database
   converts the literal to the column type. [STD]
4. **Concurrency check** (IMP-0014): `UPDATE` and `DELETE` match the row identity *and* the loaded values
   of the columns they change (NULL-safe comparison). A statement that affects no row means the row was
   changed or deleted since it was loaded. [STD]
5. Applying runs all statements in one transaction. Every `UPDATE`/`DELETE` must affect exactly one row and
   every `INSERT` one row; otherwise — or on any error — the whole transaction is rolled back and the
   result names the first failing change and why (conflict, error message with SQLSTATE). Nothing is
   partially applied. [GEN]
6. MCP tool `edit_table_data` (connection, table, changes, `apply`): without `apply` it only returns the SQL;
   it needs `read-schema`, and `modify-data` to apply (SPEC-0012). [IMP-0007, ADR-0008]

## Acceptance criteria
- **AK-1** Two cell edits in one row produce one `UPDATE … SET a = '…', b = '…' WHERE "id" = '1' AND "a"
  IS NOT DISTINCT FROM '…' AND …` (PostgreSQL); MySQL uses backticks and `<=>`, SQLite `IS`.
- **AK-2** Quotes and backslashes in values are escaped per dialect; NULL is written as `NULL`.
- **AK-3** A table without primary key and without a NOT NULL unique index is refused with a reason.
- **AK-4** On SQLite (and PostgreSQL/MySQL when configured), applying an update, an insert and a delete
  changes the table accordingly.
- **AK-5** If the row was changed after loading, applying reports a conflict for that change and leaves the
  table exactly as before (including the other changes of the batch).
- **AK-6** A failing statement (e.g. NOT NULL violation) rolls back everything and reports the error.
- **AK-7** `edit_table_data` returns the SQL without applying, refuses to apply without `modify-data`, applies
  with it, and reports a conflict when the same changes are applied twice.

## Out of scope
The grid UI, editing results of joins or views, generated/default column handling beyond omitting
unspecified columns in `INSERT`, binary values.

## Known deviations from the reference
- Concurrency check against loaded values and all-or-nothing apply (IMP-0014). OBS-0010 A4: in mode
  "Auto" the reference also keeps changes pending until they are submitted, and its statements match
  rows by key only (no check of the loaded values).
- MySQL literals double backslashes; with the server mode `NO_BACKSLASH_ESCAPES` a value containing `\`
  would be stored with doubled backslashes (known limitation until the session reads `sql_mode`).
