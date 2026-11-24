# SPEC-0018: Schema comparison and DDL generation

- **Version:** 9 · **Date:** 2026-11-16 (v9: a PostgreSQL table's storage parameters are compared and written, behaviour 21 — OBS-0070 V5); 2026-11-16 (v8: two single objects compared, behaviour 20 — OBS-0068 L8); 2026-11-16 (v7: the script always pins its target schema, on PostgreSQL only for its own transaction, behaviour 19 — IMP-0415); 2026-11-16 (v6: check and unique constraints stay constraints, a left-out table drop keeps the steps that only exist for it, view bodies move to the target schema, behaviours 16–18 — OBS-0068 L5, L6, L7); 2026-11-15 (v5: SQLite adding a column in place leaves the views on the table alone, behaviour 12 — hand test 121 retest); 2026-11-13 (v4: SQLite defaults that are expressions, behaviour 15; v3 2026-11-05; v1 2026-10-31; v2: views, dependency order, destructive
  statements left out by default, transactions — IMP-0055; v3: only views that use a dropped or
  changed column are re-created — hand test 121)
- **Sources:** GEN (database tools compare two schemas, list the differences and generate a migration
  script; they generate `CREATE` scripts for existing tables; F-058), STD (PostgreSQL, MySQL and SQLite
  `CREATE TABLE`, `ALTER TABLE`, `CREATE INDEX`, `DROP …`; the SQLite table rebuild procedure from the
  SQLite documentation "Making Other Kinds Of Table Schema Changes"; v6: PostgreSQL `ALTER TABLE … ADD
  CONSTRAINT` / `DROP CONSTRAINT`, a table cannot be dropped while a foreign key of another table
  references it; v7: PostgreSQL `SET LOCAL` lasts until the end of the transaction, committed or
  not, and the temporary schema is searched first for tables unless `pg_temp` is named in
  `search_path`; MySQL and MariaDB `USE` sets the session's default database; Oracle `ALTER SESSION
  SET CURRENT_SCHEMA`; DuckDB `SET search_path`, which has no `LOCAL` form; SQL Server resolves an
  unqualified name in the database user's default schema; v9: PostgreSQL storage parameters, `pg_class.reloptions`, `CREATE TABLE … WITH (…)`,
  `ALTER TABLE … SET (…)` / `RESET (…)`), SPEC-0013 (schema model; v5 constraints and view definitions), OBS (OBS-0010, A2;
  OBS-0068 C2, C3 with the gaps L5–L7; IMP-0415; v9: OBS-0070 C3 —
  the reference's script keeps `with (autovacuum_enabled = false)`)
- **Feature matrix rows:** F-058

## Purpose
Show how two schemas of the same dialect differ and produce the statements that turn the target into the
source; produce a `CREATE` script for a schema.

## Behaviour
1. Input: two schemas as read by SPEC-0013 (source and target), optionally with their view definitions
   (SPEC-0013 v2 behaviour 6), and the dialect. Without definitions views are not compared. [GEN]
2. Tables match by name. For matching tables, columns match by name and differ when type (case and
   whitespace ignored), nullability or default differ; the primary key differs when its column list differs.
   [GEN]
3. Indexes and foreign keys match by content, not by name: an index by uniqueness and column list, a
   foreign key by columns, referenced table and referenced columns. Indexes that only implement the
   primary key, and MySQL indexes created for a foreign key of the same name, are not compared. [GEN]
4. The differences are available as a structured list (tables added/removed/changed; per table columns
   added/removed/changed, primary key, indexes and foreign keys added/removed). [GEN]
5. The migration runs in this order: drop foreign keys, drop indexes, drop tables, create tables, change
   columns and primary keys, create indexes, add foreign keys — so every statement only refers to objects
   that exist at that point. [STD]
6. PostgreSQL and MySQL: new tables are created without foreign keys; all foreign keys are added at the end
   (`ALTER TABLE … ADD FOREIGN KEY`), so creation order and cycles do not matter. Column changes use
   `ALTER COLUMN … TYPE / SET|DROP NOT NULL / SET|DROP DEFAULT` (PostgreSQL) and `MODIFY COLUMN` with the
   full definition (MySQL). [STD]
7. SQLite: adding a column that SQLite can add (`ALTER TABLE … ADD COLUMN`, nullable or with default) is
   done in place; every other change to an existing table rebuilds it: create the new table under a
   temporary name, copy the common columns, drop the old table, rename, recreate its indexes. Foreign keys
   are written inside `CREATE TABLE`. A migration with a rebuild is wrapped in
   `PRAGMA foreign_keys = OFF` … `PRAGMA foreign_keys = ON`. [STD]
8. Every statement says whether it can lose data (dropping a table or column, changing a column's type, a
   rebuild that leaves out columns). [GEN]
9. The `CREATE` script of a schema is the migration from an empty schema. [GEN]
10. MCP tools: `generate_ddl` (connection, optional table names) and `compare_schemas` (source and target
    connection of the same dialect); both need `read-schema` on every connection they read and never
    run the statements. `compare_schemas` (v2) also returns per statement `selected` and the objects it
    touches, the `script` text of behaviour 14, and takes `include_destructive` (default false). [ADR-0008,
    SPEC-0012]
11. **Views (v2):** views match by name and differ when their definitions differ (case, white space and
    a trailing `;` ignored). A view only in the source is created (`CREATE VIEW name AS definition`), one
    only in the target dropped, a changed one dropped and created again. [GEN, STD]
12. **Dependency order (v2):** a view of the target is dropped before anything it uses is dropped or
    changed (table dropped, column dropped or changed, SQLite rebuild), and created again at the end
    when the source still has it; views are dropped with dependants first and created with what they use
    first (view dependencies as in SPEC-0022 v2 behaviour 7). Foreign keys keep the order of
    behaviour 5 (dropped before their tables, added after both tables exist). So the order is: drop
    views, drop foreign keys, drop indexes, drop tables, create tables, change columns and keys, create
    indexes, add foreign keys, create views. [STD, OBS: IMP-0055]
    **v3:** on servers that alter tables in place (not SQLite), a dropped or changed column only
    affects views whose definition names that column or selects all columns (`*`, `t.*`); other
    views using the table stay untouched. A new primary key or a dropped table still affects every
    view using the table. [STD]
    **v5:** a SQLite change made in place (only nullable or defaulted columns added outside the
    primary key, maybe with indexes: `ALTER TABLE … ADD COLUMN`) leaves the views on the table
    alone like such servers; a change that rebuilds the table (behaviour 7) still drops and
    re-creates every view using it. Before, any SQLite change re-created them. [STD]
13. **Destructive statements (v2):** every statement names the objects it touches and whether it is
    destructive (behaviour 8). Destructive statements are not selected by default; dropping and
    re-creating a view only to change what it uses is not destructive. A statement that depends on a
    left-out statement stays in (its order still works because each group only refers to objects that
    exist). [GEN, OBS: IMP-0055]
14. **Script and transaction (v2):** the script contains the selected statements, each ending in `;`;
    destructive ones are preceded by a comment naming the data that is lost, left-out destructive ones
    appear as comment lines saying they were left out. On PostgreSQL, SQLite and SQL Server the
    statements are wrapped in one transaction (`BEGIN` … `COMMIT`; on SQLite the `PRAGMA foreign_keys`
    lines of behaviour 7 stay outside it, as SQLite ignores them inside a transaction). On MySQL,
    MariaDB and Oracle, where DDL commits on its own, a comment says the statements cannot be rolled
    back together. **v7:** the script's first statement pins its target schema (behaviour 19). A
    name in a comment (what a statement loses) is written on one line, a line break as a space, so
    a table or column whose name holds a line break cannot end the comment and turn the rest of
    its name into a statement — also not in a left-out statement. [STD, OBS: IMP-0055]

15. **SQLite expression defaults (v4):** SQLite reports the default `(strftime('%s', 'now'))` without its
    brackets, and its grammar accepts only a literal, a signed number or a constant such as
    `CURRENT_TIMESTAMP` bare after `DEFAULT`. The generated `CREATE TABLE` and `ADD COLUMN` put any other
    default back into brackets. A foreign key read without referenced columns is written with the
    parent's primary key (SPEC-0013 v3 behaviour 9). A generated column (SPEC-0013 v3 behaviour 8) is
    written as an ordinary column of its type with a comment saying so, because the catalogue does not
    give its expression; the DDL of SPEC-0089 v7 has the full definition. [STD: SQLite `CREATE TABLE`
    grammar]
16. **Check and unique constraints (v6, OBS-0068 L5):** where the schema model carries constraints
    (SPEC-0013 v5 behaviour 16, PostgreSQL), they are compared by content like indexes (behaviour 3):
    by kind and definition, with case and white space folded, names ignored. A unique index that backs a
    unique constraint of the same name is compared as that constraint, not as an index. The differences
    list per table the constraints only in the source (added) and only in the target (removed). A new
    table is created with its constraints inside `CREATE TABLE` (`CONSTRAINT name CHECK (…)`,
    `CONSTRAINT name UNIQUE (…)`), after the primary key and before the foreign keys. For a table in
    both, a constraint only in the target is dropped (`ALTER TABLE … DROP CONSTRAINT name`) right after
    the foreign keys are dropped, and one only in the source is added (`ALTER TABLE … ADD CONSTRAINT
    name definition`) after the column changes and before the foreign keys are added, which may need
    it. Neither loses data; adding a check that a row breaks fails, and the error is shown. So a
    unique constraint stays a constraint and no check is lost, and comparing again after the script
    ran finds no difference. [STD]
17. **What a left-out table drop keeps (v6, OBS-0068 L6):** every statement that only exists so that a
    table can be dropped is in the group of that table's `DROP TABLE` (behaviour 13) and is left out
    with it: the drops of the table's own foreign keys, the drop of a foreign key of a table that
    stays when it references the dropped table, and the dropped views that use it (v2). Tables to be
    dropped that reference one another form one group, since one cannot be dropped while the other
    still references it. A SQLite rebuild (behaviour 7) of a table that stays which removes a foreign
    key to a dropped table joins that table's group. These statements count as destructive (they
    carry the table's text and are not selected by default) but lose no data themselves (`data_loss`
    false), so the data-loss question names only the tables and views. The indexes of a dropped table
    are never dropped on their own; `DROP TABLE` drops them. So whatever is chosen, a table whose drop
    is left out keeps every foreign key, and a foreign key to such a table stays: the script never
    weakens a table that stays. [STD, GEN]
18. **Two schemas (v6, OBS-0068 L7, IMP-0415):** when source and target are different schemas (two
    schemas of one connection, SPEC-0110 v7 behaviour 16, or the current schemas of two connections),
    a name qualified with the source schema in a source view's definition is written with the target
    schema in the script's `CREATE VIEW`, quoted as the target's name needs. A quoted qualifier matches
    exactly and an unquoted one ignoring case; names qualified with any other schema, and text in
    literals and comments, stay as they are. Views are compared after the same rewrite, with a
    qualifier of the target schema counting as none on both sides, so a view the script created
    compares equal afterwards. With view definitions written relative to their schema (SPEC-0013 v5
    behaviour 17), a name in the source's own schema has no qualifier and resolves in the target
    schema, where the script runs (SPEC-0110 v7 behaviour 16). [STD, OBS: IMP-0415]
19. **The script names its target (v7, IMP-0415):** whenever the target schema is known (a schema
    chosen for the comparison, or the target connection's current schema read when the comparison
    starts, SPEC-0110 v11 behaviour 16), the script's first statement pins it, whatever schema the
    session that runs the script is in. So the script cannot create, change or drop objects in
    another schema — for example after the reverse script, an earlier script or the user's own
    `SET` left the console in the source schema. Every name the script writes without a schema
    (tables, indexes, constraints, foreign keys, view bodies of behaviour 18) then resolves in the
    target. Per dialect:
    - **PostgreSQL:** the first statement inside the script's transaction (behaviour 14), right
      after `BEGIN`, is `SET LOCAL search_path TO <target>, pg_temp`. `SET LOCAL` lasts until the
      transaction ends — by the script's `COMMIT`, by `ROLLBACK` after a failed statement, or when
      the connection closes — so the session's `search_path` is the same after the script as before
      it, and the statements the user runs later in that console, or the next script, resolve their
      names as before. This variant is chosen over a session-level `SET search_path` before
      `BEGIN`, which would stay in force after the script, also after a failed one, and send the
      user's later statements to the target without saying so — the same fault the other way
      round. In a console in Manual mode
      (SPEC-0093 v7) the script's `COMMIT` ends the console's transaction and the pin with it.
      `pg_temp` is named last so a temporary table of the session that has the name of a target
      table is not changed in its place (without it PostgreSQL searches the temporary schema
      first). Nothing else is on the path: definitions are read relative to their own schema
      (SPEC-0013 v5 behaviour 17), so a name in any other schema is written with that schema, and
      `pg_catalog` is always searched.
    - **DuckDB:** the first statement is `SET search_path TO <target>`. DuckDB has no `SET LOCAL`
      and its script is not one transaction, so the setting stays for the session; a comment line
      before it says so.
    - **MySQL, MariaDB:** the first statement is `` USE <target> ``. The default database belongs to
      the session and has no transaction-local form (and each DDL statement commits on its own), so
      it stays after the script; a comment line before it says so.
    - **Oracle:** the first statement is `ALTER SESSION SET CURRENT_SCHEMA = <target>`; it also
      stays for the session, with the same comment.
    - **SQL Server:** no statement makes a schema current: an unqualified name resolves in the
      database user's default schema, which a session cannot change and no generated script
      changes. The script's names are not schema-qualified (checked for v7); its target is the
      target connection's default schema (two schemas of one connection are not compared on SQL
      Server, SPEC-0110 v7 behaviour 16), and the window binds the script to that connection
      (SPEC-0110 v8 behaviour 5). So there is no pin; qualifying every name is out of scope.
    - **SQLite:** no pin; a connection's schema is `main`, and attached databases are not compared.
    The target is quoted as its name needs. A script that runs no statement (no differences, or
    every statement left out) has no pin, and a target whose name is empty (a MySQL connection
    without a default database) gets none. The MCP `script` (behaviour 10) and the window's script
    (SPEC-0110 behaviour 5) are the same text after the window's head line. [STD, OBS: IMP-0415]
20. **Two single objects (v8, OBS-0068 L8):** instead of whole schemas one table or view of the
    source can be compared with one of the target: of the same connection and schema under another
    name, of another schema, or of another connection of the same dialect. Both must exist and be
    of the same kind (table with table, view with view); otherwise the comparison is refused with a
    message naming the object and the side, and nothing else is read into the result. The source
    object is compared as if it had the target object's name, so the differences and the
    statements are those of behaviours 1–18 for that one object: they change the target object in
    place (its name is kept), never create, drop or touch another table or view, and a foreign key
    of the source object to itself is read as one to the target object. Destructive statements
    start unchecked as in behaviour 13, the script is one transaction where behaviour 14 allows it,
    and its first statement pins the target schema as behaviour 19 says. Without objects the
    comparison is the whole-schema one, unchanged. [GEN]
21. **Storage parameters (v9, OBS-0070 V5):** on PostgreSQL the schema model of SPEC-0013 carries each
    table's storage parameters as the database lists them (`autovacuum_enabled=false`, `fillfactor=70`;
    those of its TOAST table as `toast.autovacuum_enabled=false`), sorted; reading a schema reads them
    with the tables. Two tables differ when their parameters differ, and the difference is listed with
    the table's other differences ("storage (fillfactor=70) → (autovacuum_enabled=false)"). A new
    table's `CREATE TABLE` ends with `WITH (…)`; for a changed table one `ALTER TABLE … SET (…)` gives
    every parameter the source has with another value or the target lacks, and one `ALTER TABLE …
    RESET (…)` takes back the parameters only the target has. Neither loses data. The DDL a table shows
    (SPEC-0089 v5 behaviour 11) has its `WITH (…)` too. Table options of other databases (MySQL's engine
    and row format, SQL Server's compression) are not read yet; they are neither compared nor written.
    [STD]

## Acceptance criteria
- **AK-1** Table added, removed and changed (column added, removed, type, nullability, default, primary
  key, index, foreign key) are listed; equal schemas give no differences; renamed but identical indexes
  and foreign keys are not differences.
- **AK-2** The PostgreSQL, MySQL and SQLite migrations for such a change set match the expected
  statements in the order of behaviour 5, with data-loss flags.
- **AK-3** Round trip on SQLite (always) and PostgreSQL/MySQL (when configured): applying the migration
  from target to source to the target database and reading it again gives no differences.
- **AK-4** The `CREATE` script of the reference schema, run on an empty database, gives the same schema.
- **AK-5** The MCP tools return differences and statements, check `read-schema` on both connections and
  refuse different dialects.
- **AK-6 (v2)** Target with a view on tables that the source lacks: the script drops the view first,
  its destructive statements are not selected, and with `include_destructive` the script runs without
  error on SQLite and PostgreSQL (live, throwaway schemas).
- **AK-7 (v2)** A changed column used by a view: the view is dropped before the change and created
  after it; a changed view is dropped and created; views on views in dependency order.
- **AK-8 (v2)** Script text: transaction wrapping per dialect, left-out destructive statements as
  comments, `PRAGMA` outside the transaction on SQLite.
- **AK-9 (v3)** Dropping `item.discontinued` leaves `stock_value` (which uses `item` but not that
  column) alone; a view selecting `i.*` or naming the column is still re-created
  (`ddl::tests::views_that_do_not_use_a_dropped_column_stay`).
- **AK-11 (v5)** SQLite: adding `item.discontinued` in place gives only the `ADD COLUMN`; dropping it
  (a rebuild) re-creates `stock_value` (`ddl::tests::sqlite_keeps_views_when_a_column_is_added_in_place`);
  in a window of the test harness (`crates/workbench/tests/it/db_handtest_retest.rs`
  `hand_test_121_views_and_the_data_loss_question`); live on PostgreSQL and MySQL neither direction
  touches the unchanged view and both scripts leave no differences
  (`crates/database/tests/it/er_compare_live.rs` `hand_test_121_unchanged_view_stays_on_the_servers`).

- **AK-10 (v4)** The generated `CREATE TABLE` of the catalogue sample's `affinity` (defaults `1e3`,
  `x'00ff'`, `strftime('%s', 'now')`) and `child_actions` (a reference without columns) runs on SQLite
  and gives the same defaults (`crates/database/tests/it/sqlite_catalogue.rs`
  `the_shown_ddl_recreates_an_equivalent_object`).
- **AK-12 (v6, behaviour 16)** A new table gets `CONSTRAINT … CHECK (…)` and `CONSTRAINT … UNIQUE (…)`
  in its `CREATE TABLE` and no `CREATE UNIQUE INDEX` for the index behind the unique constraint; for a
  table in both, a check only in the source is added after the column changes, one only in the target
  dropped after the foreign keys; renamed but identical constraints are no difference
  (`ddl::tests::check_and_unique_constraints_stay_constraints`).
- **AK-13 (v6, behaviour 17)** Target tables `a` (foreign key → `item`) and `b` (foreign key → `a`), a
  view on `a` and a staying table `c` whose foreign key → `b` the source lacks: the drops of both
  tables, of their foreign keys, of `c`'s foreign key and of the view are one group, none selected; the
  script with the defaults holds them only as comment lines; toggling one chooses them all, foreign keys
  before tables (`ddl::tests::a_left_out_table_drop_keeps_its_foreign_keys`).
- **AK-14 (v6, behaviour 18)** `pd_b.item`, `"pd_b".item` and `PD_B.item` in a source view become
  `public.item` (or a quoted target name); `other.item`, `'pd_b.item'` and `-- pd_b.item` stay; the
  source view on `pd_b.item` and a target view on `item` are no difference, one on `other.item` is
  (`ddl::tests::view_bodies_move_to_the_target_schema`).
- **AK-15 (v6, live, behaviours 16–18)** On PostgreSQL in two throwaway schemas built like OBS-0068
  (`pd-setup.postgresql.sql`: tables `warehouse`, `item`, `stock`, `movement`, `pd_note` with its check,
  unique constraint and index, `pd_move`; views; the other schema with `item.pd_discontinued` and a
  view on its `item` written with the schema), one of them the session's current schema
  (`crates/database/tests/it/er_compare_live.rs` `obs_0068_schemas_migrate_both_ways`): towards the
  second schema the script with the defaults creates `pd_note` with both constraints as constraints;
  the other way the script with the defaults drops no foreign key of the four tables it leaves, which
  keep their foreign keys after it ran, and the new view reads the target's `item`; in both directions
  the script with every statement leaves no differences. Skipped without the server.
- **AK-16 (v7, behaviour 19)** The script for a target `old`: on PostgreSQL its first statement
  after `BEGIN` is `SET LOCAL search_path TO "old", pg_temp`, also for a target that is the
  connection's current schema and for one whose name needs quotes; DuckDB starts with
  `SET search_path TO "old"`, MySQL and MariaDB with `` USE `old` ``, Oracle with `ALTER SESSION SET
  CURRENT_SCHEMA = "old"`, each after a comment saying it stays for the session; SQL Server and
  SQLite have none; a script without a chosen statement and an empty target name have none; a
  dropped table whose name holds a line break leaves no statement outside its comments, chosen
  or left out (`ddl::tests::the_script_pins_its_target`).
- **AK-17 (v7, live, behaviour 19)** On PostgreSQL with throwaway schemas for source, target and a
  wrong one: the session's `search_path` is set to the wrong schema and a temporary table has the
  name of a target table; the script with every statement creates every table, index, constraint,
  foreign key and view in the target, nothing in the wrong schema, leaves the temporary table as
  it was, and afterwards the session's `search_path` is the wrong schema as before and no
  difference is left; a script that fails at a statement leaves the same `search_path` after its
  `ROLLBACK`. On MySQL and MariaDB the script run after `USE` of a wrong database creates its
  objects in the target, which is the session's database afterwards
  (`crates/database/tests/it/er_compare_live.rs` `the_script_pins_its_target_schema`). Skipped
  without the servers.
- **AK-18 (v8, behaviour 20)** Narrowing to two objects (`crates/database/src/ddl.rs` tests:
  `two_single_tables_are_compared`): a table under another name gets the statements that change
  it in place, a column only in the target is dropped as destructive, a self-referencing foreign
  key follows the name, other tables of both schemas are left out; views compared with views;
  a missing object and a table against a view are refused with the side named.
- **AK-19 (v9, behaviour 21)** Parameters read from the catalogue rows and sorted
  (`introspect::tests::storage_parameters_belong_to_their_table`); a new table with `WITH (…)`, a
  changed table with `SET (…)` and `RESET (…)`, equal parameters giving no difference, the listed line
  (`ddl::tests::storage_parameters_are_compared_and_written`,
  `db_compare_model::tests::diff_lines_list_tables_and_their_parts`).
- **AK-20 (v9, live, behaviour 21)** On PostgreSQL with two throwaway schemas: `pd_move` with
  `autovacuum_enabled = false` and `toast.autovacuum_enabled = false` in the source and without them in
  the target, `pd_note` with `fillfactor = 70` only in the target, `pd_new` with `fillfactor = 80` only
  in the source; the script has the `SET`, the `RESET` and the `CREATE TABLE … WITH (…)`, run on the
  target it leaves no difference, and `pd_new` keeps its parameter
  (`crates/database/tests/it/er_compare_live.rs::storage_parameters_reach_the_target`).

## Out of scope
Triggers, routines, sequences, exclusion constraints, check and unique constraints as constraints outside
PostgreSQL (there unique constraints are compared as unique indexes and checks are not compared; v6), renames (shown as remove + add), comparing across dialects, partitioning,
materialized views as their own kind (compared like views). v7: schema-qualified names in the
script (on SQL Server the only way to pin a schema, behaviour 19), and a transaction around the
DuckDB script.
v9: table options of databases other than PostgreSQL (behaviour 21), storage parameters of views and
indexes.

## Known deviations from the reference
- Statements are ordered by dependency: views are dropped before the tables they use and created after
  them; the reference drops tables before a view on them, so its script fails on PostgreSQL (IMP-0055).
- Destructive statements are not selected by default and are marked as data loss; the reference
  selects them without a warning. The script runs in one transaction where the database supports
  transactional DDL (IMP-0055).
- Matching indexes and foreign keys by content means that only differently named but identical objects
  are not reported.
- v6: names of the source schema in view bodies are written with the target schema (behaviour 18);
  the reference leaves view bodies as they are, so they resolve against the search path of whatever
  console runs the script (IMP-0415, OBS-0068 C3). On PostgreSQL a unique constraint is dropped as a
  constraint (behaviour 16), so the `DROP INDEX` failure below no longer occurs for it.
- v6: statements that only exist for a table drop are left out with it (behaviour 17); the reference
  selects destructive statements by default (IMP-0055), and its handling of a left-out drop with its
  foreign keys was not observed.
- MySQL reports defaults as values, not expressions; a default is written unquoted when it is a number,
  `NULL` or a `CURRENT_TIMESTAMP` form, and as a string literal otherwise.
- PostgreSQL: an index that backs a unique constraint cannot be dropped with `DROP INDEX`; the statement
  then fails and the error is shown.
- Both schemas are read from the database when a comparison starts; the reference compares its last
  introspection without saying how old it is, including schemas that no longer exist (IMP-0414,
  OBS-0068 C1), and compares a schema it never read (left out of its schema filter) as an empty one
  (OBS-0070 C1).
- Compared with a connection (OBS-0070 C2, C3): the reference drops a sequence and tables before the
  objects that depend on them, keeps a new table's `nextval('…'::regclass)` default unqualified and
  opens the script in a console of the source; Ferris IDEA's script creates the sequence in the target
  schema with the search path set there (IMP-0055, IMP-0415).
- Since v6 (IMP-0415): in view bodies, names qualified with the source schema are rewritten to the
  target schema (behaviour 18), which closes OBS-0068 L7; the reference leaves view bodies
  unqualified, so they resolve against the console's search path. Since v7 the script also pins
  its target before its first change (behaviour 19), on PostgreSQL only for its own transaction, so
  neither an unqualified name nor the console's earlier search path can send a statement to
  another schema, and the console's search path is unchanged afterwards (IMP-0415 complete). Before
  v7 the search path was set, for the whole session, only when the target was not the connection's
  current schema.
