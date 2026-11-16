# SPEC-0191: Copying a table to another schema or table

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** GEN (database tools copy a table's structure and rows into another schema or under
  another name, with a preview of the DDL), STD (`CREATE TABLE`, `PRIMARY KEY`, `UNIQUE`, `NOT NULL`,
  `DEFAULT`, `CHECK`, `INSERT … SELECT`; catalogs `pg_constraint`, `information_schema.CHECK_CONSTRAINTS`,
  `sys.check_constraints`, `user_constraints`, SQLite `sqlite_master`), OBS-0010 A7 (reference: the
  dialog for copying a table is titled as an import; target schema and new table chosen automatically;
  column mapping with types; errors go to a file in a temporary system folder by default; the new table
  lost the primary key, NOT NULL and CHECK constraints and took a stale column name from the metadata
  cache → IMP-0058), SPEC-0013 (introspection), SPEC-0018 (DDL), SPEC-0017 (literals), SPEC-0012
  (permissions), ADR-0008
- **Feature matrix rows:** F-054, F-058

## Purpose
Duplicate a table — for a backup before a risky change, a test copy, or to move it to another schema —
and get a table that behaves like the original: same columns, same keys and rules, same rows.

## Behaviour
1. **Start.** F5 on a table in the Database tool window, or "Copy Table To…" in its context actions,
   opens a dialog titled "Copy Table <table> To…". [GEN, IMP-0058]
2. **Target.** Fields: target schema (empty = the table's own schema) and target table (default
   `<table>_copy` in the same schema, else the same name). A target that already exists is refused
   before anything runs. [GEN]
3. **What is kept.** Switches, all on by default: primary key, unique constraints (from unique
   indexes), NOT NULL, defaults, CHECK constraints, rows. Foreign keys are a further switch, **off** by
   default, because the referenced tables may not exist in the target schema; when on, they reference
   the same tables as the original. Identity/serial defaults that call a sequence of the source table are
   kept as written (the copy draws from the same sequence) and said so in the preview. [GEN, IMP-0058]
4. **Fresh structure.** The structure is read from the database when the dialog opens and again when
   copying starts — never from a cached model — so renamed columns get their current names. [GEN,
   IMP-0058]
5. **Preview.** The dialog shows the statements that will run, updating with every switch and name:
   one `CREATE TABLE` with the kept constraints, then `INSERT INTO <target> (<columns>) SELECT
   <columns> FROM <source>` when rows are copied. [GEN]
6. **Running.** On PostgreSQL, SQLite and SQL Server the statements run in one transaction; on MySQL,
   MariaDB and Oracle, where DDL commits implicitly, a failure after the `CREATE TABLE` drops the new
   table again. The result — the number of rows copied, or the failing statement with the database
   message — is shown in the dialog and in the Services Output; an error file is written only when the
   user named one. The Database tool window reads the connection again afterwards. [STD, IMP-0058]
7. **Agents.** MCP `copy_table` (connection, table, `target_schema`, `target_table`, the switches,
   `apply`): without `apply` it returns the statements; it needs `read-schema`, and `modify-schema` plus
   `modify-data` (when rows are copied) to apply. [ADR-0008, IMP-0007]

## Acceptance criteria
- **AK-1** Statement building per dialect: CREATE TABLE with NOT NULL, DEFAULT, PRIMARY KEY, UNIQUE,
  CHECK; each switch off removes its part; foreign keys only when switched on; qualified target names;
  existing target refused; SQLite CHECK clauses parsed from the table's SQL (unit tests).
- **AK-2** Live against the reference PostgreSQL, MySQL and SQLite with a throwaway table: the copy has
  the same primary key, NOT NULL columns, defaults and CHECK constraints (a violating insert into the
  copy fails), and the same rows; a failing copy leaves no target table (integration tests).
- **AK-3** MCP `copy_table` previews, refuses without `modify-schema`, copies with it (protocol test).
- **AK-4** Window self-test: F5 on `customers` opens the dialog with `customers_copy`, the preview shows
  `PRIMARY KEY`; copying adds `customers_copy` to the tree with the same rows.
- **AK-5** By hand (round 4, hand test 114): copy `item` to a second schema next to the reference.

## Out of scope
Copying between different connections or dialects, mapping or converting columns, copying indexes
other than unique ones, triggers, grants and comments, copying several tables at once.

## Known deviations from the reference
- Keys, NOT NULL, defaults and CHECK constraints are kept by default (IMP-0058); the reference created
  every column nullable without constraints.
- The dialog is named after what it does; errors are shown in the result and written to a file only on
  request (IMP-0058). Column names come from the database at copy time, never from a cache.
- No column mapping or type conversion; the copy has the source's types.
