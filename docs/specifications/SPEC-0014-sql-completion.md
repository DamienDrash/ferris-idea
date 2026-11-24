# SPEC-0014: Schema-aware SQL completion

- **Version:** 4 · **Date:** 2026-11-13 (v4: a quoted table name is a table of the statement, behaviour 19;
  v3 2026-11-07: join clauses from foreign keys, schema in the detail, list
  opens by itself after table keywords, from OBS-0026; v2 2026-11-03: ranking, schemas and current
  metadata from OBS-0010 A5; IMP-0057, IMP-0059)
- **Sources:** GEN (SQL editors complete table and column names from the live schema; F-053), STD (SQL
  clause structure; identifier quoting per dialect; `JOIN … ON` and foreign key constraints),
  SPEC-0005 (lexer), SPEC-0013 (schema model), SPEC-0072 behaviour 8 (list opening by itself),
  OBS-0010 A5 (system catalog tables offered before the user's
  tables after `from`; hundreds of functions after the columns of `alias.`; a column renamed outside the IDE
  still completed with its old name), OBS-0026 (side-by-side with PostgreSQL: after `join` and a space
  the list opens without being asked and offers complete join clauses with alias and condition before
  the plain tables, also some built from equal column names; each table names its schema),
  IMPROVEMENTS IMP-0057, IMP-0059
- **Feature matrix rows:** F-053

## Purpose
While typing SQL, offer the table, view, column and keyword names that fit the cursor position, using the
connection's real schema.

## Behaviour
1. Completion works on the statement containing the cursor (SPEC-0005 ranges); other statements of the
   script are ignored. [GEN]
2. Inside a string literal, quoted identifier or comment, nothing is offered. [GEN]
3. The prefix is the identifier text directly before the cursor; candidates match it case-insensitively at
   the start. An empty prefix matches everything. [GEN]
4. After `qualifier.`, the columns of the table the qualifier names are offered; the qualifier may be a table
   name or an alias declared in the statement (`from stock s`, `join item as i`). [STD]
5. After `FROM`, `JOIN`, `INTO`, `UPDATE`, `TABLE`, or after a comma in a `FROM` list, tables and views are
   offered. [STD]
6. Elsewhere (select list, `WHERE`, `ON`, `GROUP BY`, `ORDER BY`, `SET`, `HAVING`), the columns of all tables
   referenced in the statement are offered, then keywords. If the statement references no table yet, columns
   of all tables are offered. [GEN]
7. Each candidate has a label, a kind (`table`, `view`, `column`, `keyword`) and a detail (column type and
   table, or `view`). Order: kind order of the context, then labels whose case matches the prefix exactly,
   then alphabetical. Duplicates are merged (same column in two tables: one candidate, details joined). [GEN]
8. The text to insert quotes identifiers that need it — not all lowercase letters, digits and `_`, or
   starting with a digit — with the dialect's quotes (SPEC-0007 behaviour 8). [STD]
9. MCP tool `complete_sql` (connection, sql, cursor as byte offset) returns the candidates; it needs the
   `read-schema` permission (SPEC-0012). [ADR-0008]

10. **v2 – Order of names (IMP-0059):** after `FROM` and the other table positions, the tables and views
    of the current schema come first, then the names of the other non-system schemas of the database (kind
    `schema`, detail `schema`). System schemas (PostgreSQL `pg_*` and `information_schema`, MySQL/MariaDB
    `mysql`, `information_schema`, `performance_schema`, `sys`, SQL Server `sys` and `INFORMATION_SCHEMA`,
    SQLite `temp`) and their tables are never offered unprompted. [GEN]
11. **v2 – Schema qualifier:** after `schema.` where `schema` is no alias or table of the statement but a
    schema of the database, the tables and views of that schema are offered (when its tables are known;
    the editor reads them on first use). Typing a system schema's name explicitly (`pg_catalog.`) is the
    only way its tables are offered. [GEN]
12. **v2 – After `alias.`:** only that table's columns are offered — no keywords, functions or other
    tables. [GEN]
13. **v2 – Current metadata (IMP-0057):** an editor keeps the schema it read, but before it completes it
    checks a cheap fingerprint of the catalog (PostgreSQL: a hash over the current schema's relation and
    column names and types; MySQL/MariaDB: count and checksum of `information_schema.COLUMNS`; SQL Server:
    `checksum_agg` over `sys.columns`; SQLite: `schema_version`; Oracle: count and `ora_hash` over
    `user_tab_columns`). If it differs from the one taken with the cached schema, the schema is read again
    before the list is shown. The check runs at most once every 2 seconds per connection, so typing does not
    query the database on each key. MCP tools read the schema fresh on each call. [GEN]

14. **v3 – Join clauses:** directly after `JOIN` (any join kind: `inner`, `left [outer]`, `right`,
    `full`, `cross`), when at least one known table is named before the cursor in the statement's
    `FROM`/`JOIN` list, each foreign key between a table of the current schema and a table already
    named — in either direction — gives a candidate of kind `join`:
    `<table> <alias> on <left>.<column> = <alias>.<column>`, the column pairs of a composite key joined
    with `and`. `<left>` is the alias of the table already named, or its name when it has none. A
    table already named is offered again only for a key that references the table itself. A key
    without referenced columns references the primary key. Names that need it are quoted in the
    inserted text (behaviour 8). Only real foreign keys count; nothing is guessed from equal column
    names. [STD, OBS-0026]
15. **v3 – Alias:** the initials of the table name's parts split on `_` (and other non-alphanumeric
    characters), in lower case (`warehouse` → `w`, `stock_value` → `sv`); `t` is put in front when that
    would be empty or start with a digit. When the alias equals an alias or table name already in the
    statement, or an SQL keyword, `2`, `3` … is appended (`w2`). [GEN]
16. **v3 – Order and filter:** join clauses come before the plain tables and views, which stay
    available; typing a prefix filters both by the table name the label starts with. When the
    current schema is known, a table's detail is `table · <schema>`, a view's `view · <schema>`, a
    join clause's `foreign key · <schema>`; tables of another schema (behaviour 11) name that schema.
    Without a known schema, details stay as in v2 (`view`, or empty). [GEN, OBS-0026]
17. **v3 – Opening by itself:** in a `.sql` file with a connection, typing a space directly after the
    word `FROM`, `JOIN`, `INTO` or `UPDATE` (any case, not in a comment or string, not qualified like
    `x.from`) opens the list by itself after the usual short delay (SPEC-0072 behaviour 8); the
    schema is read first if it is not cached yet. A second space, other keywords (`select `,
    `where `) and other files do not open it this way. [GEN, OBS-0026]
18. **v3 – MCP:** `complete_sql` returns the same join clauses (kind `join`), since it shares the
    logic; its table details carry no schema, because the tool does not ask for the current schema.
    [ADR-0008]

19. **v4 – Quoted table names:** a table written in the dialect's identifier quotes (`"order items"`,
    `` `order items` ``, `[order items]` in T-SQL; doubled quotes inside undone) is a table of the
    statement like an unquoted one: its alias gives its columns after `alias.`, its columns are offered
    in the statement, and its foreign keys give join clauses. In MySQL and MariaDB a double quote opens
    a string, not a name. Behaviour 2 is unchanged: inside the quotes nothing is offered. [STD]

## Acceptance criteria
With the reference schema (`item`, `movement`, `stock`, view `stock_value`, `warehouse`):
- **AK-1** `select * from |` → `item`, `movement`, `stock`, `stock_value`, `warehouse` (alphabetical, kinds
  `table` and `view`), no columns or keywords.
- **AK-2** `select s.| from stock s` → `warehouse_id`, `sku`, `qty` (stock's columns only).
- **AK-3** `select * from stock s join item i on i.|` → item's columns.
- **AK-4** `select * from stock where q|` → `qty` first.
- **AK-5** `select * from item; select | from stock` → stock's columns, not item's `name`.
- **AK-6** `select 'fro|` and `-- from |` → nothing.
- **AK-7** A table `Order Items` inserts `"Order Items"` for PostgreSQL and `` `Order Items` `` for MySQL.
- **AK-8** Over MCP, `complete_sql` returns stock's columns for AK-2 and is refused without `read-schema`.

- **AK-9 (v2)** With other schemas `audit` and `pg_catalog`: `select * from |` → the five reference names
  first, then `audit` (kind `schema`), never `pg_catalog`; `select * from audit.|` → audit's tables;
  `select * from pg_catalog.|` → its tables when known; `select i.| from item i` → only item's columns
  (unit tests).
- **AK-10 (v2)** Fingerprint: the throttle (no check within 2 s, a check after) and the decision to re-read
  (unit tests); live against PostgreSQL, MySQL and SQLite: renaming a column on a separate connection
  changes the fingerprint, and the re-read schema completes the new name (integration tests).

With the reference schema and its foreign keys (`movement` and `stock` → `item(sku)`, `warehouse(id)`),
plus `stock_check(warehouse_id, sku)` → `stock(warehouse_id, sku)`:
- **AK-11 (v3)** `select * from movement m join |` → `item i on m.sku = i.sku`,
  `warehouse w on m.warehouse_id = w.id`, and no clause for `stock` (equal column names only);
  `select * from warehouse w left outer join |` → `movement m on w.id = m.warehouse_id`,
  `stock s on w.id = s.warehouse_id`; a key without referenced columns uses the primary key; none
  after `from |`, after a comma, or after a join with no known table before it
  (`completion::tests::join_clauses_from_foreign_keys_both_directions`).
- **AK-12 (v3)** `select * from stock s join |` → `stock_check sc on s.warehouse_id = sc.warehouse_id
  and s.sku = sc.sku` (`completion::tests::composite_keys_are_joined_with_and`).
- **AK-13 (v3)** Aliases: `stock_value` → `sv`, `item` with `i`, `i2` taken → `i3`, `order_note` →
  `on2`; `from movement w join |` → `warehouse w2 on w.warehouse_id = w2.id`; `from movement as mv` keeps
  `mv`, `from movement` uses `movement.`; a table already joined is not offered again
  (`completion::tests::join_aliases_are_unique_and_existing_ones_kept`).
- **AK-14 (v3)** Join clauses first, then all plain tables and views; `… join wa|` → the warehouse
  clause, then `warehouse` (`completion::tests::join_clauses_rank_first_and_filter_by_table_name`);
  quoted names in the inserted clause (`completion::tests::join_clauses_quote_names_that_need_it`).
- **AK-15 (v3)** With current schema `public`: `item` → `table · public`, `stock_value` →
  `view · public`, a join clause → `foreign key · public`; `audit.|` → `table · audit`; without a schema
  the v2 details (`completion::tests::tables_show_their_schema`).
- **AK-16 (v3)** The list opens by itself after `from `, `join `, `LEFT JOIN `, `into `, `UPDATE `,
  `delete from `, not after `select `, `from  `, `stock `, `where `, `x.from `, `fromage `
  (`completion_model::tests::sql_list_opens_after_table_keywords`); in the window, completing after
  `select * from orders o join ` on the self-test SQLite shows `customers c on o.customer_id = c.id`
  first (self-test step of `sql_console_steps`).
- **AK-17 (v3)** Over MCP, `complete_sql` after `select * from s x join ` returns
  `w w on x.w_id = w.id` (kind `join`) first and the plain table `w`
  (`protocol.rs`: `introspection_tools_share_the_read_schema_check`).

- **AK-18 (v4)** `select * from "order items" o where o.|` → the columns of `order items`, also with
  `as`, backticks (MySQL), brackets (T-SQL) and a name with a doubled quote; not with double quotes in
  MySQL; `select * from "order items" oi join |` → `orders o on oi."order id" = o.id` first; nothing
  inside an open quote (`completion::tests::quoted_tables_give_columns_and_join_clauses`; against the
  catalogue sample: `crates/database/tests/it/sqlite_catalogue.rs`).

## Out of scope
Function and parameter completion, subquery and CTE column inference, ranking by usage, completing
columns of `schema.table.` (only tables of a schema are offered).

## Known deviations from the reference
- User objects of the current schema rank first; system catalogs only with their schema typed; after
  `alias.` only columns (IMP-0059).
- Cached metadata is checked against the database before completing, so changes made outside the IDE show
  up at once (IMP-0057).
- **v3 – No guessed joins:** the reference also offers join clauses for tables that merely share a
  column name with a table in the statement (e.g. `stock` after `movement` over `warehouse_id`).
  Ferris IDEA deliberately offers join clauses only for real foreign keys: a guessed condition can be
  wrong without any hint (equal names, different meaning) and quietly produce wrong results. The plain
  table stays available for writing the condition by hand.
- **v3 – Detail text:** the schema is shown with the kind (`table · public`) instead of in parentheses,
  in the same `kind · owner` form the column details already use.
