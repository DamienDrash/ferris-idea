# SPEC-0019: Renaming tables and columns in SQL

- **Version:** 5 · **Date:** 2026-11-16 (v5: `WITH` queries and subqueries in `FROM` keep their result
  columns, behaviour 13 — OBS-0068 L3); 2026-11-13 (v1 2026-10-31; v2: every project SQL file in its own dialect,
  derived constraint and index names — IMP-0056; v3: column definitions in `CREATE TABLE`, files follow
  derived names — hand tests 122 and 78, D45; v4: only the project's own SQL files are changed,
  behaviour 12)
- **Sources:** GEN (database refactoring "rename" changes a table or column and every SQL place that refers
  to it; F-057), STD (`ALTER TABLE … RENAME TO`, `ALTER TABLE … RENAME COLUMN … TO` in PostgreSQL,
  MySQL 8 and SQLite ≥ 3.25; identifier quoting), SPEC-0005 (statement splitting), SPEC-0013 (schema),
  SPEC-0014 (quoting of inserted names), OBS (OBS-0010, A3; v5: OBS-0068 L3, the outer place of a `WITH`
  query pointed at nothing after the rename), SPEC-0013 v2 (constraint names), STD (v5: PostgreSQL and
  MySQL name a result column without an alias after the column it selects, also through a `::` cast)
- **Feature matrix rows:** F-057

## Purpose
Rename a table or a column: produce the `ALTER TABLE` statement and the text edits for every place in SQL
scripts that refers to the object, and name the places that could not be decided instead of guessing.

## Behaviour
1. Input: SQL text, dialect, schema (SPEC-0013), the object (table, or table and column) and the new name.
   Unknown object, a new name that already exists (table in the schema, column in the table) or an empty
   new name are errors. [GEN]
2. Analysis is per statement (SPEC-0005). Literals, comments and dollar-quoted bodies are never changed;
   quoted identifiers are recognised (`"x"`; in MySQL `` `x` ``) and compared exactly, unquoted ones
   case-insensitively. [STD]
3. Table references are the names after `FROM`, `JOIN`, `INTO`, `UPDATE`, `TABLE`, `REFERENCES`, the
   names in a `FROM` list, and the name after `ON` in `CREATE INDEX`; the last part of a qualified name
   counts. A qualifier `t.` whose name is the table (and not an alias of another table) is a reference too.
   [GEN]
4. Column references of table T: `q.col` where `q` is T or an alias of T; an unqualified `col` in a
   statement that references T, unless another table referenced in that statement also has a column of
   that name (then the place is reported as ambiguous and not changed); the column list after
   `REFERENCES T`. Words directly after `AS`, function names and names inside `CREATE TABLE` of another
   table (except after `REFERENCES T`) are not column references. [GEN]
5. The new name is written unquoted when it is a plain lower-case identifier that is not a keyword,
   otherwise quoted (SPEC-0014); a place that was quoted stays quoted. [STD]
6. The result: the `ALTER TABLE` statement, the edits (byte range and replacement, in order), the
   ambiguous places with the reason, and the rewritten text. [GEN]
7. MCP tool `rename_sql_object` (connection, table, optional column, new name, optional SQL text): with
   SQL text it rewrites that text; without, it reports edits for every `.sql` file of the workspace. It
   needs `read-schema`, never runs the `ALTER TABLE` and never writes files. [ADR-0008]
8. **All project SQL files (v2):** without SQL text, every `.sql` file of the project is searched,
   whether or not it is bound to a connection. Each file is analysed in its own dialect: a file name
   ending in `.<dialect>.sql` or a folder on its path named after a dialect (`postgres`, `postgresql`,
   `pg`, `mysql`, `mariadb`, `sqlite`, `sqlserver`, `mssql`, `oracle`) sets it; otherwise the
   connection's dialect. Files of another dialect are listed with their hits but marked as such and are
   left unchanged unless the caller includes them. [GEN, OBS: IMP-0056]
9. **Derived names (v2):** the named constraints (SPEC-0013 v2 behaviour 7) and indexes of the renamed
   table whose names contain the old name as a whole `_`-separated part or run of parts (case
   ignored), e.g. `item_unit_price_cents_check` for column `unit_price_cents` or `item_pkey` for table
   `item`, are offered for renaming with that part replaced by the new name. Each offer carries the
   statement of its dialect — PostgreSQL and Oracle `ALTER TABLE … RENAME CONSTRAINT` (constraints) or
   `ALTER INDEX … RENAME TO` (indexes; on PostgreSQL an index with the name of a constraint is renamed
   with it), MySQL/MariaDB `ALTER TABLE … RENAME INDEX` (indexes and unique keys), SQL Server
   `sp_rename` — or the reason it cannot be renamed (MySQL/MariaDB foreign keys and checks, SQLite).
   An offer whose new name is taken is dropped. Offers run after the main statement and refer to the
   table by its new name. [STD, OBS: IMP-0056]
10. **Column definitions (v3):** inside `CREATE TABLE T (…)` of the renamed table, the column's own
    definition and its unqualified uses in checks, defaults and key lists (`PRIMARY KEY (…)`,
    `UNIQUE (…)`, `FOREIGN KEY (…)`) are column references; a name right after `CONSTRAINT` is a
    constraint name, and the column list after `REFERENCES other (…)` belongs to the other table. A
    script that created the table therefore still matches the database after the rename. [GEN]
11. **Files follow derived names (v3):** when a derived name (behaviour 9) is renamed too, its places in
    `CREATE`, `ALTER` or `DROP` statements about an index, key or constraint are renamed in the files as
    well; a derived name left out stays everywhere; literals and comments never change. [GEN]
12. **Only the project's own sources (v4; decision of the coordinator of 2026-11-13 under the Project Owner's general authority: the rule the Project Owner chose for Rename, Find in Files, Search Everywhere, Inspect Code and Endpoints):** "every `.sql` file of the project"
    (behaviours 7 and 8) means the `.sql` files that are the project's own by the rule of SPEC-0148
    v4 behaviour 7 (`OwnSources`). A `.sql` file with places that lies in a virtual environment,
    in build output, in a folder the project model excludes, behind a symbolic link or is matched
    by an ignore rule of Git without being tracked is not part of `files`: it is listed apart
    with the same facts (dialect, edits, ambiguous places) and the reason, and is not changed.
    Before v4 a rename with `apply` (SPEC-0128) wrote into such files, for example the migration
    scripts of an installed package. Folders the search never read (`node_modules`, `target`,
    `__pycache__`, version control and IDECraft data) are still not read. [GEN]
    - **MCP:** `rename_sql_object` returns these files as `outside_project` (numbered `index`,
      `path`, `dialect`, `other_dialect`, `edits`, `ambiguous`, `why`, `reason`, `included`);
      with `apply`, `include_outside: [indices]` changes the given ones too. A file of another
      dialect also needs `include_other_dialects`, as before. An unknown index is refused before
      the statement runs. SQL text given with `sql` is rewritten as given.
    - When it is unsure whether a file is the project's own, it is not changed.

13. **`WITH` queries and subqueries in `FROM` (v5, OBS-0068 L3):** the query of `WITH name AS (…)`
    and a subquery in `FROM` or `JOIN` with an alias are scopes of their own. Inside one, behaviour 4
    applies to the tables named inside it. Outside it, the query counts as a table named by its name
    or alias whose columns are its result columns. [GEN, STD]
    - A result column that is the renamed column itself — a select-list item that is only `col` or
      `q.col` (maybe with a `::type` cast) without an alias, in a query without a column list — would
      take the new name with the rename. Inside, it is written `new_name AS col` instead (the old name
      as it was written, `AS` in the case of that `SELECT`). So the query keeps its result columns,
      and every place outside that reads `big.col`, `b.col` or `col` stays correct and unchanged.
    - This variant is chosen over renaming the outer places because it keeps the statement's result
      columns stable: `select b.body from big b` still returns a column `body` to whatever reads the
      result (a program, a report, an export), and other readers of the query (further `WITH`
      queries, joins, a view's columns) need no change. Renaming outward would change the column
      the statement returns, which no SQL file can show.
    - A query that passes the table's columns through with `*` or `t.*` (no column list) has the new
      name as its result column; there is no item to alias. Its readers' places `q.col`, and an
      unqualified `col` in a scope that reads it, are renamed like references of the table.
    - With a column list (`big(id, body)`, `(…) s(id, body)`) or an alias of its own (`body as b`),
      the result columns do not change; only the inner reference is renamed.
    - An unqualified `col` outside such a query refers to the table only where its scope names the
      table itself (behaviour 4) or a query of the `*` kind. The ambiguity check of behaviour 4 looks
      at the tables of the same scope.
    - Other subqueries (in `WHERE`, in the select list, after `EXISTS` or `IN`) stay part of the scope
      around them, as before.

## Acceptance criteria
- **AK-1** Renaming table `item` changes `from item`, `join item i`, `insert into item`, `update item`,
  `references item`, `item.sku`, `"item"`, but not `'item'`, `-- item`, `item_name` or an alias `item`
  of another table.
- **AK-2** Renaming column `item.sku` changes `i.sku` (alias of item), `item.sku`, unqualified `sku` in a
  statement on item only, `insert into item (sku, …)`, `references item (sku)`, `create index … on item
  (sku)`; it does not change `s.sku` (alias of another table) or `select 1 as sku`.
- **AK-3** An unqualified `sku` in `select sku from item join stock …` (both have `sku`) is reported as
  ambiguous and not changed.
- **AK-4** The `ALTER TABLE` statements per dialect; quoting of the new name; errors for unknown objects
  and existing names.
- **AK-5** On SQLite the `ALTER TABLE` plus the rewritten queries run against the reference schema and
  return the same rows as before the rename.
- **AK-6** The MCP tool rewrites given SQL, reports edits for workspace `.sql` files and checks
  `read-schema`.
- **AK-7 (v2)** Dialect of a file from its name and folders; on `sql-basic`, renaming
  `item.unit_price_cents` finds hits in `schema.sql`, `seed.sql` and `mysql/schema.sql`, the last one
  marked as another dialect for a PostgreSQL connection.
- **AK-8 (v2)** Derived names per dialect: `item_unit_price_cents_check` → `item_price_cents_check` with
  `ALTER TABLE … RENAME CONSTRAINT` on PostgreSQL, refusals with reasons on MySQL and SQLite; no offer
  for names that only contain the old name inside a word (`items_x` for `item`).
- **AK-9 (v2)** Live (PostgreSQL, throwaway schema): rename a column and its check constraint, read the
  catalog again.
- **AK-10 (v3)** Renaming `item.unit_price_cents` in `create table item (… unit_price_cents integer
  check (unit_price_cents >= 0), unique (unit_price_cents, sku))` changes the definition, the check and
  the key list; the same column name in another table's `CREATE TABLE` and `references item (sku)` for
  another column stay (`refactor::tests::create_table_column_definitions_are_renamed`).
- **AK-11 (v3)** Renaming `ht_b.menge` with derived `ht_b_menge_idx` and `ht_b_menge_check` changes
  `create index ht_b_menge_idx` and `constraint ht_b_menge_check` in the file, not the literal
  `'ht_b_menge_idx'`; a derived name left out stays in the file
  (`db_rename_dialog::tests::files_follow_derived_index_and_constraint_names`; MCP `rename_sql_object`
  likewise).
- **AK-12 (v4)** MCP on SQLite in a temporary project with the column used in an own file, in
  `.venv/lib/python3.12/site-packages/x.sql`, `node_modules/y.sql`, a file an ignore rule matches
  and one Git tracks although the rule matches it: `files` holds the own and the tracked file,
  `outside_project` the environment's and the ignored one with the reason; an unknown index is
  refused and nothing runs; `apply` with the ignored file's index changes the own files and that
  file, not the environment's and the dependency's
  (`crates/mcp/tests/it/protocol.rs::rename_sql_object_leaves_files_outside_the_own_sources_alone`).

- **AK-13 (v5)** Renaming `pd_note.body` to `text_body` in `with big as (select id, body from pd_note
  where qty > 10) select b.body from big b` gives `with big as (select id, text_body as body from
  pd_note where qty > 10) select b.body from big b`; `SELECT` in capitals gives `AS`; `body::text`
  keeps its cast before the alias; with `select *` in the query the outer place becomes `b.text_body`;
  with `big(id, body)` or `body as b` only the inner name changes; an unqualified `body` outside a
  query of the first kind stays; a subquery `(select body from pd_note) s` behaves like the `WITH`
  query; `select body from pd_note` alone still becomes `select text_body from pd_note`
  (`refactor::tests::with_queries_keep_their_result_columns`).
- **AK-14 (v5)** Live on PostgreSQL in a throwaway schema (`crates/database/tests/it/refactor_live.rs`
  `a_renamed_with_query_still_runs`): the query of AK-13, its `*` variant and a subquery in `FROM`
  return the same rows before and after the `ALTER TABLE` and the rewrite of the text, the first and
  the subquery under the column `body`, the `*` variant under `text_body`; skipped without the server.

## Out of scope
Renaming schemas; renaming constraints on their own; scopes of subqueries outside `FROM` (analysis per
statement); renaming inside view or routine bodies stored in the database; sequence names. Running the
statement and writing files: SPEC-0128.

## Known deviations from the reference
- Usages are searched in every project SQL file in its own dialect, bound to a connection or not; the
  reference found none in unbound files even with text search on (IMP-0056).
- Constraint and index names that embed the old name are offered for renaming; the reference keeps the
  old name (IMP-0056).
- Ambiguous places are reported instead of being changed or silently skipped.
- v4: SQL files outside the project's own sources are listed and left unchanged (behaviour 12); the
  reference was not compared for this.
- v5: a column that a `WITH` query or a subquery in `FROM` selects is aliased to its old name inside
  that query (behaviour 13); the reference was not compared for it, its connection was missing in
  OBS-0068 (L3 was seen in IDECraft alone).
