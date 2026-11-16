# SPEC-0013: Schema introspection

- **Version:** 7 · **Date:** 2026-11-16 (v7: the definitions of routines, triggers and types, behaviour 20, AK-16–17, MCP `object_definition`, the DDL tab of SPEC-0089 v13; v6: the kinds of objects of a schema and its sequences, routines, triggers and types, over MCP `schema_objects`, behaviours 18–19, AK-14–15 — OBS-0071 O7; v5: PostgreSQL check and unique constraints of each table, view
  definitions written relative to their schema, behaviours 16–17 — OBS-0068 L5, L7); 2026-11-13 (v4: on SQLite the condition of a partial index, the collation
  of an index term and the reason a view cannot be read — from the window check of the Database tool
  window with SQLite files; v3: SQLite read completely — generated columns, names with
  commas, expression indexes, references without columns, storage tables of virtual tables left out, a
  view that cannot be read, nothing written — from the check of the catalogue sample without a screen;
  v1 2026-10-31; v2: view definitions and constraint names for SPEC-0018 v2, SPEC-0019 v2 and SPEC-0022 v2)
- **Sources:** STD (PostgreSQL system catalogs `pg_class`, `pg_attribute`, `pg_constraint`, `pg_index`;
  MySQL `information_schema` `COLUMNS`, `TABLE_CONSTRAINTS`, `KEY_COLUMN_USAGE`, `STATISTICS`; SQLite
  `sqlite_master` and the `table_info`, `table_xinfo`, `table_list`, `foreign_key_list`, `index_list`,
  `index_info`, `index_xinfo` pragmas; v5: PostgreSQL `pg_get_constraintdef`, `pg_get_viewdef` writes a
  name with its schema only when the search path does not find it, `set_config(…, true)` holds until the
  end of the transaction; v6: PostgreSQL `pg_proc`, `pg_trigger`, `pg_type`, `pg_depend`,
  MySQL/MariaDB `information_schema` `ROUTINES`, `TRIGGERS`; v7: PostgreSQL `pg_get_functiondef`,
  `pg_get_triggerdef`, `pg_aggregate`, `pg_enum`, `pg_range`, `pg_constraint`; MySQL/MariaDB
  `SHOW CREATE FUNCTION/PROCEDURE/TRIGGER`), GEN, OBS-0068 L5 and L7 (v5: a check was lost
  and a unique constraint became an index in the migration; a view body kept the source schema),
  OBS-0004 (reference shows per table: columns with type, NOT NULL, default; foreign keys; indexes; keys —
  weaknesses IMP-0007, IMP-0010)
- **Feature matrix rows:** F-052

## Purpose
Read the structure of a database — tables and views with their columns, primary key, foreign keys and
indexes — for the schema tree, completion and agents.

## Behaviour
1. Introspection runs through the session of SPEC-0009–0011 with read-only catalog queries. [STD]
2. It covers the connection's current schema: PostgreSQL `current_schema()`, MySQL `database()`, SQLite
   `main`. No manual schema selection is needed (IMP-0010). [STD, IMP-0010]
3. For each table or view, sorted by name: kind (`table`, `view`), columns in definition order (name,
   type as the database spells it, nullable, default expression), primary key columns in key order,
   foreign keys (name if any, columns, referenced table and columns), indexes (name, unique, columns or
   expressions). Internal objects (`sqlite_%`) are left out. [STD]
4. A failing catalog query is an error naming the connection and the database message; partial results
   are not returned as complete. [GEN]
5. MCP tools `list_tables` (names and kinds) and `describe_table` (one table) need the `read-schema`
   permission of SPEC-0012 and refuse with the same message format when it is missing. Both tools apply the
   same check (IMP-0007). [IMP-0007]
6. **View definitions (v2):** on request, the stored query of each view of the current schema as text:
   PostgreSQL `pg_get_viewdef`, MySQL/MariaDB `information_schema.VIEWS`, SQLite the `CREATE VIEW`
   statement from `sqlite_master` (the part after `AS`), SQL Server `sys.sql_modules`, Oracle
   `user_views`. [STD]
7. **Constraint names (v2):** on request, the named constraints of each table (primary key, unique,
   check, foreign key) with their kind, from the dialect's catalog; SQLite reports none. [STD]

8. **v3 – Generated columns (SQLite):** columns are read with `table_xinfo`, so a generated column is a
   column of its table like the others, marked `virtual` or `stored` (MCP `describe_table`: `generated`,
   left out for ordinary columns); its type is the declared type without the words of the clause. The
   internal columns of a virtual table (the ones `SELECT *` does not return, such as `rank` of an FTS5
   table) are not listed. [STD: `PRAGMA table_xinfo`, column `hidden`]
9. **v3 – Names and expressions (SQLite):** a column name that contains a comma stays one name in keys
   and indexes. An index on an expression lists the expression's text, taken from the stored
   `CREATE INDEX` statement without its `ASC`/`DESC` (a term that cannot be read is `<expression>`). A
   foreign key written without referenced columns (`REFERENCES parent`) names the parent's primary key
   when the numbers of columns agree. The storage tables SQLite creates for a virtual table (FTS5,
   R-Tree: kind `shadow` in `table_list`) are internal objects like `sqlite_%` and left out; the virtual
   table itself is listed as a table. [STD: `PRAGMA table_list`, `index_info`, `foreign_key_list`]
10. **v3 – A view that cannot be read (SQLite):** when a view names a table or column that no longer
    exists, SQLite refuses to describe it. The tables and the other views are still read; that view is
    listed without columns. Behaviour 4 holds for every other failure. [STD, GEN]
11. **v3 – Reading only:** introspection never changes a database file and creates no file next to it
    that was not there (SPEC-0011 v2 behaviour 7 says what SQLite itself needs for a database in WAL
    mode). The tools that read a schema open their own connection read-only even when the profile allows
    writing. [GEN, ADR-0012]

12. **v4 – Partial indexes (SQLite):** an index that covers only the rows of a condition
    (`CREATE INDEX … WHERE …`; `PRAGMA index_list` column `partial`) carries that condition: the text
    after `WHERE` of the stored `CREATE INDEX` statement, as it was written, without the closing `;`
    (a statement that cannot be read gives `<condition>`). A unique partial index says nothing about
    the rows outside its condition, so it is never taken for a key of the table: not as the row
    identity when data is edited (SPEC-0017), not as a unique constraint of a copied table
    (SPEC-0191), not as the sign of a one-to-one relation in the diagram (SPEC-0022), and it is not
    left out of generated DDL as "the primary key's own index". MCP `describe_table`: `condition` on
    the index, left out for an index over all rows. [STD: `PRAGMA index_list`, SQLite schema table]
13. **v4 – Collation of an index term (SQLite):** each term that names a column carries the collation
    its values are compared with when that is not `BINARY`, spelled as `PRAGMA index_xinfo` reports it
    (column `coll`; it is the one written in the index or, without one, the column's own). The text of
    an expression term (behaviour 9) already holds what was written, so it gets none. MCP
    `describe_table`: `collations` on the index, one entry per term in the order of `columns`, `null`
    for a term without one; left out when no term has one. [STD: `PRAGMA index_xinfo`]
14. **v4 – Why a view cannot be read (SQLite):** the view of behaviour 10 carries SQLite's message for
    it (for example `no such table: main.gone`). MCP `describe_table` and `list_tables`: `unreadable`
    with that message on the view, left out for every object that was read. [STD, GEN]
15. **v4 – Generated DDL keeps both:** wherever a `CREATE INDEX` is generated from this structure
    (SPEC-0018, SPEC-0089 behaviour 11) a term with a collation is written `column COLLATE name` and
    a partial index ends in `WHERE condition`; two indexes that differ in a collation or in the
    condition are different indexes when schemas are compared. [STD]
16. **v5 – Check and unique constraints (PostgreSQL, OBS-0068 L5):** each table carries its check and
    unique constraints: name, kind (`check`, `unique`), columns (a unique constraint's in key order, a
    check's empty) and the definition as the database writes it (`CHECK (qty >= 0)`, `UNIQUE (body)`,
    with `NULLS NOT DISTINCT`, `DEFERRABLE` or `NOT VALID` where set). The unique index PostgreSQL keeps
    for a unique constraint stays in the indexes under the constraint's name. Primary and foreign keys
    stay as in behaviour 3, `NOT NULL` stays a property of the column. They are read on PostgreSQL
    through its own catalogue queries, not through a connector's (SPEC-0300); the other database
    systems report none here yet, so their unique keys stay unique indexes. MCP `describe_table`:
    `constraints`, left out for a table without any. [STD]
17. **v5 – Definitions relative to their schema (PostgreSQL, OBS-0068 L7):** view definitions
    (behaviour 6), column types and defaults, and constraint definitions (behaviour 16) are read with
    the search path set to the schema being read, the current one included, for that one catalogue
    query only. So a name in that schema is written without its schema and a name in any other
    schema with it, whatever the session's search path is (`nextval('pd_seq'::regclass)` for the
    schema's own sequence); the session's search path is the same afterwards. Before v5 the text
    followed the session's search path: a view in a schema outside it named its own tables with the
    schema (`FROM pd_b.item`), and a table of `public` lost its schema in every view, so a view of
    `pd_b` on `public.item` read like one on its own `item`. [STD]

18. **v6 – Objects besides tables and views:** for a schema (the current one, or another from
    SPEC-0089 v3) IDECraft reads which kinds of objects the database can hold and its objects of
    the kinds other than tables and views, from the catalogue and nothing else [STD, GEN]:
    a. **Kinds** in this order: PostgreSQL table, view, sequence, function, procedure, aggregate,
       trigger, type; MySQL table, view, function, procedure, trigger; MariaDB the same with
       sequence after view; SQLite table, view, trigger; DuckDB, Oracle and SQL Server table,
       view, sequence (their routines, triggers and types are not read).
    b. **Sequences** as SPEC-0089 v5 behaviour 12 and SPEC-0116 v3 behaviour 11 read them.
    c. **Routines:** name and kind; on PostgreSQL also the arguments in parentheses (`()` without
       any), so overloads are told apart; window functions count as functions. Members of an
       extension are left out (they belong to the extension), and so are the constructor functions
       PostgreSQL makes itself for a range type and its multirange (they belong to the type).
       MySQL and MariaDB: functions and procedures (MariaDB packages are not read).
    d. **Triggers:** name, table and when they fire — timing and events as the database states
       them (`BEFORE INSERT OR UPDATE`, `AFTER UPDATE OF qty` on SQLite, whose default timing is
       `BEFORE`); PostgreSQL leaves out the triggers it makes itself for constraints.
    e. **Types (PostgreSQL):** enums, domains, ranges and composite types made with
       `CREATE TYPE`, each with what it is; the row types of tables, array types and the types
       of extensions are not listed.
    f. Each kind is sorted by name. A failing catalogue query is an error with the database's
       message; nothing is written, and on SQLite the schema's fingerprint stays the same.
19. **v6 – MCP `schema_objects`** (read-schema, read-only, ADR-0008): `connection`, optional
    `schema` and `kind`. The answer has `kinds`, each with its `count` (tables and views counted as
    `list_tables` lists them), and `objects` besides tables and views with `name`, `kind`, and
    `table` and `detail` where behaviour 18 gives them; empty fields are left out. With `kind` only
    that kind is listed; a kind the connection does not have is refused with the list of its
    kinds. The answers of `list_tables` and the other tools stay as they are. [GEN]
20. **v7 – Definitions of routines, triggers and types:** the definition of an object of
    behaviour 18, read from the catalogue with one query and nothing else [STD, GEN]:
    a. **PostgreSQL:** functions and procedures as `pg_get_functiondef` gives them, the overload
       chosen by its arguments; aggregates, which that function refuses, written as
       `CREATE AGGREGATE` with `SFUNC`, `STYPE` and, where set, `FINALFUNC`, `COMBINEFUNC`,
       `INITCOND`; triggers as `pg_get_triggerdef` gives them, chosen by name and table, ending in
       `;`; types written from the catalogue — an enum with its labels in their order, a domain
       with its base type, default, `NOT NULL` (once, also where the server keeps it as a
       constraint of the domain, PostgreSQL 17 and later) and named checks, a range with its
       subtype, a composite type with its attributes.
    b. **MySQL and MariaDB:** `SHOW CREATE FUNCTION`, `SHOW CREATE PROCEDURE` and `SHOW CREATE
       TRIGGER` — the statement column. When the server gives the row but no statement (the user
       is not the definer and lacks the privilege), it is an error saying so, not an empty text.
    c. **SQLite:** a trigger's statement as stored in `sqlite_master` of the schema, ending in `;`.
    d. Sequences, and every kind of DuckDB, Oracle and SQL Server, have no definition here; an
       object that is gone gives none. A failing query is an error with the database's message.
    e. **MCP `object_definition`** (read-schema, read-only, ADR-0008): `connection`, optional
       `schema`, `kind` and `name`, and `table` or `detail` (a PostgreSQL routine's arguments)
       where the name alone names several objects — the call then lists them and asks for one. The
       answer is the object as `schema_objects` lists it with `ddl`. An unknown object, or a kind
       without a definition, is refused with a message. [GEN]

## Acceptance criteria
Run against the reference databases (PostgreSQL and MySQL via `IDECRAFT_TEST_PG` / `IDECRAFT_TEST_MYSQL`,
SQLite built from the seed files).
- **AK-1** Tables `item`, `movement`, `stock`, `warehouse` and view `stock_value` are listed in that order.
- **AK-2** `stock` has columns `warehouse_id`, `sku`, `qty` in that order, all not null, `qty` with
  default `0`; primary key `(warehouse_id, sku)`.
- **AK-3** On PostgreSQL and SQLite `stock` has foreign keys `(warehouse_id) → warehouse(id)` and
  `(sku) → item(sku)`. On MySQL the result matches what the server created (inline `REFERENCES` clauses are
  recorded as observed, see the note below).
- **AK-4** `stock` has a unique index on `(warehouse_id, sku)`.
- **AK-5** Over MCP, `describe_table` with default permissions works; with `read-schema` removed,
  `list_tables` and `describe_table` are both refused naming `read-schema`.

- **AK-6 (v2)** On PostgreSQL, MySQL, MariaDB and SQLite the view `stock_value` has a definition naming
  `stock` and `item`; on PostgreSQL `item` has the check constraint `item_unit_price_cents_check`.

- **AK-7 (v3)** Against the catalogue sample (`tests/reference-projects/sqlite-catalogue/catalogue.sql`:
  every column affinity, defaults, generated columns, composite keys, `WITHOUT ROWID`, `STRICT`,
  `AUTOINCREMENT`, unique and check constraints, every foreign key action, deferred, composite and self
  references, unique, partial, expression, descending and collated indexes, views on views, triggers,
  FTS5 and R-Tree tables, names with spaces, keywords, quotes, commas and non-ASCII letters): every
  table and view is listed once and no storage table; `generated` has `gross` (virtual) and
  `gross_kept`, `tag` (stored); `doc_search` has `title`, `body`; the index `idx comma` has the columns
  `comma,name` and `quo"te`; `indexed_mixed` has `city`, `amount * 2`, `name`; `implicit_pk` references
  `parent(id)` (`crates/database/tests/it/sqlite_catalogue.rs`
  `the_catalogue_is_read_as_the_pragmas_report_it`; unit tests
  `introspect::tests::{index_terms_are_split_outside_brackets_and_quotes,
  sqlite_rows_keep_commas_expressions_and_generated_columns}`).
- **AK-8 (v3)** A database with a view over a dropped table lists its tables and its sound view with
  their columns and the broken view without columns (`a_view_over_a_dropped_table_does_not_hide_the_rest`).
- **AK-9 (v3)** After reading the schema, the DDL, the schema list and the fingerprint through a
  connection that may write, the folder holds the same files with the same bytes
  (`reading_the_schema_changes_nothing_on_disk`); the tables of an attached database are read with their
  keys and indexes (`a_relative_file_is_found_from_the_workspace_and_attached_databases_are_read`).

- **AK-10 (v4)** Against the catalogue sample: `indexed_active` has the condition `deleted = 0` and no
  collations; `indexed_city_nocase` has the collation `nocase` on `city`; `indexed_mixed` has one on
  `name` only and keeps its terms as before; an index over all rows with binary terms has neither
  (`sqlite_catalogue.rs` `partial_indexes_and_collations_are_read`; unit tests
  `introspect::tests::{the_condition_of_a_partial_index_is_the_text_after_where,
  sqlite_rows_carry_condition_and_collations}`). Generated `CREATE INDEX` keeps both and a changed
  condition is a difference (`ddl::tests::generated_indexes_keep_collation_and_condition`); a unique
  partial index is no row identity (`edit::tests::a_partial_unique_index_is_no_row_identity`).
- **AK-11 (v4)** The broken view of AK-8 carries SQLite's message naming the dropped table
  (`a_view_over_a_dropped_table_does_not_hide_the_rest`). Over MCP, against a SQLite file built in
  the test: `describe_table` gives the index's `condition` and `collations`, and `unreadable` for the
  broken view, which `list_tables` repeats while it still lists the other objects (`protocol.rs`
  `partial_indexes_collations_and_a_view_that_cannot_be_read`).
- **AK-12 (v5)** Catalogue rows of a check and a unique constraint are attached to their table with
  kind, columns and definition, and rows of another table or a view are ignored
  (`introspect::tests::check_and_unique_constraints_belong_to_their_table`).
- **AK-13 (v5)** Live on PostgreSQL in throwaway schemas (`crates/database/tests/it/er_compare_live.rs`
  `postgres_reads_constraints_and_views_relative_to_their_schema`): a table with `check (qty >= 0)` and
  `constraint … unique (body)` has both with their definitions, and the unique index of the same name is
  still listed; in a schema outside the search path a view on its own `item` is written `FROM item`, one
  on `public.item` (another table of the same name) `FROM public.item`, and a default on its own
  sequence `nextval('…_seq'::regclass)` without the schema; the session's search path is the same before
  and after; skipped without the server.

- **AK-14 (v6)** Unit tests in `crates/database/src/schema_objects.rs` with fixed catalogue rows and
  the queries of each dialect (`catalogue_rows_become_objects`,
  `queries_per_dialect_read_the_catalogue`, `sqlite_trigger_events`); against SQLite files built in
  the test, the triggers of the main and of an attached database with table and timing, the
  fingerprint unchanged (`catalog.rs` `triggers_of_a_sqlite_schema`). Live (skipped without
  `IDECRAFT_TEST_PG` / `IDECRAFT_TEST_MYSQL` / `IDECRAFT_TEST_MARIADB`): every kind of a
  PostgreSQL schema with overloads, aggregates, triggers and the four kinds of types, without the
  range type's constructors (`postgres_schema_objects`); functions, procedures and triggers of a
  MySQL or MariaDB database (`mysql_schema_objects`, `mariadb_schema_objects`). First run on
  2026-11-16 against PostgreSQL 18 and MySQL 8.4, the same day against MariaDB 12.3 (built from
  source, on loopback): all three green.
- **AK-15 (v6)** Over MCP against a SQLite file: `schema_objects` counts two tables, one view and
  one trigger and lists the trigger; `kind: function` is refused naming the kinds; without
  read-schema the call is refused (`protocol.rs` `schema_objects_lists_kinds_and_triggers`).
- **AK-16 (v7)** Unit tests in `crates/database/src/schema_objects.rs`: one catalogue query per kind
  and dialect, names written as literals or quoted identifiers, only `SELECT` or `SHOW`
  (`definition_queries_per_kind`); the statement taken from fixed `SHOW CREATE` rows, a row
  without one gives the privilege message (`definitions_from_catalogue_rows`). Live (skipped
  without `IDECRAFT_TEST_PG` / `IDECRAFT_TEST_MYSQL` / `IDECRAFT_TEST_MARIADB`): every object of a
  PostgreSQL schema, routines and triggers equal to the server's `pg_get_functiondef` and
  `pg_get_triggerdef`, each overload by its arguments, aggregates and the four kinds of types as
  written, none for a sequence (`postgres_object_definitions`); MySQL and MariaDB equal to the
  statement column of `SHOW CREATE` (`mysql_object_definitions`, `mariadb_object_definitions`).
  First run on 2026-11-16 against PostgreSQL 18 and MySQL 8.4, the same day against MariaDB 12.3:
  all green. MariaDB keeps a trigger's statement as it was written (keywords in their case, the
  name qualified by the database) where MySQL rewrites its head; the text is still the
  server's own. The missing-privilege path against a live server is open.
- **AK-17 (v7)** Against SQLite files built in the test: the definitions of the triggers of the main
  and of an attached database, none for a trigger that is gone (`catalog.rs`
  `triggers_of_a_sqlite_schema`); over MCP the trigger's statement with its table, an unknown name
  refused, and without read-schema the call refused (`protocol.rs`
  `schema_objects_lists_kinds_and_triggers`).

## Out of scope
Other schemas than the current one, the definitions of sequences and the routines, triggers and types of
DuckDB, Oracle and SQL Server (v7 reads the others, behaviour 20), check constraint expressions (except on
PostgreSQL since v5), exclusion constraints, comments,
column statistics, incremental refresh. On SQLite also: foreign key actions and deferral, the collation of a
column and the sort order of an index term as structure (the DDL of SPEC-0089 v7 shows them as text), the
expression of a generated column, conditions and collations of the indexes of other database systems, objects of the `temp` schema.

## Notes
- MySQL ignores column-level `REFERENCES` clauses (only table-level `FOREIGN KEY` creates a constraint).
  Introspection reports what exists; warning about such ignored clauses while editing is a candidate for
  the SQL editor's inspections.
