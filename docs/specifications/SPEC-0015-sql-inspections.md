# SPEC-0015: SQL inspections

- **Version:** 5 · **Date:** 2026-11-17 (v5: `DUAL` is a known table on Oracle, MySQL and MariaDB, behaviour 13,
  AK-12 — OBS-0083 L4); 2026-11-16 (v4: unknown columns of a `WITH` query, behaviour 12 — OBS-0068
  L3); 2026-11-13 (v3: hidden columns of virtual tables are known — notes from the
  SQLite check in PR 650; v2 2026-11-03: closest-name fixes, unqualified columns, table named in the
  unsafe-change warning; IMP-0060, IMP-0062)
- **Sources:** GEN (SQL editors flag unresolved names and unsafe statements; F-053 "inspections"), STD (SQLite virtual
  tables: `pragma table_xinfo`, `hidden = 1`; FTS5 `rank` and the column named after the table), STD
  (MySQL reference manual: column-level `REFERENCES` clauses are parsed but ignored by InnoDB; only
  table-level `FOREIGN KEY` definitions create constraints), SPEC-0013 (schema), SPEC-0014 (lexer, table
  references), observation during SPEC-0013 (reference MySQL schema silently lost its foreign keys), OBS-0010 A5 (for the
  unknown column `nme` the fixes start with choosing a schema and adding the column; the similar `name` is
  not offered; the `DELETE` warning text needs the mouse), IMPROVEMENTS IMP-0060, IMP-0062, OBS-0068 L3
  (v4: after a rename the outer `b.body` of a `WITH` query named a column the query no longer had,
  and the editor did not mark it)
- **Feature matrix rows:** F-053

## Purpose
Point out mistakes in SQL before it runs: names that do not exist, statements that change every row by
accident, and definitions the database will silently ignore.

## Behaviour
1. Inspections run per statement (SPEC-0005) and report problems with a severity (`error`, `warning`), a
   message and the byte range in the script. Literals and comments are never inspected. [GEN]
2. **Unknown table** (error): a name after `FROM`, `JOIN`, `INTO`, `UPDATE` or in a `FROM` list that is no
   table or view of the schema and no CTE of the same statement. Function calls (`from f(…)`) and DDL
   statements (`CREATE`, `ALTER`, `DROP`) are not checked. Runs only when a schema is known. [GEN]
3. **Unknown column** (error): `q.name` where `q` resolves to a table or alias and that table has no column
   `name`. `q.*` is fine. [GEN]
4. **Unsafe change** (warning): `UPDATE` or `DELETE` without `WHERE` changes every row; the message says so
   and suggests adding `WHERE` (or `WHERE true` to make the intent explicit). [GEN]
5. **Ignored foreign key** (warning, MySQL only): in `CREATE TABLE`, a column definition containing
   `REFERENCES` creates no constraint; the message names the column and suggests a table-level
   `FOREIGN KEY (column) REFERENCES …`. [STD]
6. Names compare case-insensitively. Problems are sorted by position. [GEN]
7. MCP tool `inspect_sql` (connection, sql) returns the problems; it needs `read-schema` (SPEC-0012). [ADR-0008]

8. **v2 – Unqualified columns:** in a `SELECT` whose tables all resolve (no subquery, CTE or table
   function in its `FROM`), a bare name in the select list, `WHERE`, `GROUP BY`, `HAVING` or `ORDER BY` at
   the statement's top level that is no column of those tables, no alias defined in the statement, no
   keyword and no function call is an error "unknown column `x`". Names inside parentheses, after `.` or
   `::`, and implicit aliases (a name directly after an expression) are not checked. [GEN]
9. **v2 – Fixes (IMP-0060):** an unknown table or column carries up to three fixes "Replace with `name`",
   one per existing name of the same kind (tables and views; the columns of the resolved tables) whose
   case-insensitive edit distance is at most a third of the longer name's length (at least 1, at most 3),
   closest first, then alphabetical. Fixes are edits of the problem's range, with the name quoted as in
   SPEC-0014 behaviour 8. They come before any other fix the editor offers. [GEN]
10. **v2 – Unsafe change names the table (IMP-0062):** the warning reads "DELETE without WHERE removes
    every row of `t`" / "UPDATE without WHERE changes every row of `t`", followed by the suggestion; the
    problem carries the table name so the editor can count its rows (SPEC-0192 behaviour 8). [GEN]
11. **v3 – Hidden columns:** a virtual table's hidden columns (SQLite `pragma table_xinfo` with
    `hidden = 1`, such as FTS5's `rank` and the column named after the table, or FTS4's `docid`) are
    no columns of its structure — the tree, `describe_table` and generated DDL leave them out — but a
    query may name them: behaviours 3 and 8 report no unknown column for them. `describe_table` lists
    them as `hidden_columns` when a table has any. [STD]
12. **v4 – Columns of a `WITH` query (OBS-0068 L3):** `q.name` where `q` is a `WITH` query of the
    statement, by its name or by an alias of it, is an error "unknown column `name` in `q`" when the
    query's result columns are known and `name` is none of them; the fixes of behaviour 9 come from
    those columns. The result columns are known when the query has a column list (`big(id, body)`),
    or when every item of its first select list has a name: an alias (`expr as x`, `expr x`) or a
    column reference (`x`, `t.x`, maybe with a `::type` cast) whose name it takes. A query with `*`
    or `t.*`, with an item that is an expression without an alias, or that is no `SELECT` (`VALUES`)
    is not checked. Names compare as in behaviour 6. [GEN, STD]
13. **v5 – `DUAL` (OBS-0083 L4):** on Oracle, MySQL and MariaDB `DUAL` (any case) is the
    system's one-row table, never "unknown table", and its columns are not checked; before, `select
    sysdate from dual` on Oracle was an error. On the other dialects it is checked like any name.
    [STD]

## Acceptance criteria
With the reference schema of SPEC-0014:
- **AK-1** `select * from stok` → one error "unknown table `stok`" covering `stok`.
- **AK-2** `select s.qtty from stock s` → error "unknown column `qtty` in `stock`"; `select s.* from stock s` → none.
- **AK-3** `with x as (select 1 as a) select * from x` → none; `select * from generate_series(1, 3)` → none.
- **AK-4** `delete from stock` and `update stock set qty = 0` → one warning each; with `where` → none.
- **AK-5** MySQL `create table t (a int references w (id), b int, foreign key (b) references w (id))` →
  one warning naming column `a`; the same in PostgreSQL → none.
- **AK-6** `select 'from stok' -- from stok` → none.
- **AK-7** Over MCP, `inspect_sql` returns AK-1's error with its range; refused without `read-schema`.
- **AK-8 (v2)** `select nme from item` → error "unknown column `nme`" whose first fix replaces it with
  `name`; `select * from stok` → first fix `stock`; `select qty q, count(*) as n from stock group by q order
  by n`, `select now()`, `select x.a from (select 1 a) x` → none (unit tests).
- **AK-9 (v2)** `delete from stock` → warning naming `stock`, with the table in the problem (unit test).
- **AK-10 (v3)** For an FTS5 table `docs(title, body)` with the hidden columns `docs` and `rank`,
  `select title, rank from docs where docs match 'x' order by rank` and `select d.rank, d.Docs from docs d`
  → none; `select d.rang from docs d` and `select score from docs` → unknown column (unit test); against
  a real SQLite file the introspection reports the hidden columns apart from the columns.
- **AK-11 (v4)** `with big as (select id, text_body from pd_note) select b.body from big b` → error
  "unknown column `body` in `big`" covering `body`; the same with `big.body` and no alias; with
  `text_body as body`, with `big(id, body)` and with `select *` → none; `with big as (select id,
  text_bodi from pd_note) select b.text_body from big b` → the fix `text_bodi` (unit test
  `inspect::tests::columns_of_with_queries`).

- **AK-12 (v5)** `select sysdate, dummy from dual` and `SELECT 1 FROM DUAL` → none on Oracle, MySQL
  and MariaDB; on PostgreSQL "unknown table `dual`"; `select * from stok, dual` on Oracle → only
  `stok` (unit test `inspect::tests::dual_is_known_where_the_dialect_has_it`).

## Out of scope
Type checking, ambiguity of unqualified columns, unused aliases, dialect-specific syntax validation, fixes
that change the schema (adding a column).

## Known deviations from the reference
- The MySQL ignored-foreign-key warning goes beyond what was seen in the reference.
- Unknown names offer the closest existing names first instead of schema and DDL fixes (IMP-0060).
- The unsafe-change warning names the table (IMP-0062); the editor shows it inline (SPEC-0192).
