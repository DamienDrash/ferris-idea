# SPEC-0020: Query plans

- **Version:** 2 · **Date:** 2026-11-16 (v2: every field of a step, its start-up cost, its share of
  the plan's cost and its hints; every measured statement rolled back; behaviours 4 and 7–9, AK-7 and
  AK-8 — F-059, OBS-0070 V6) · 2026-10-31 (v1)
- **Sources:** GEN (database tools show the execution plan of a statement as a tree with costs and row
  estimates, optionally measured; F-059), STD (PostgreSQL `EXPLAIN (FORMAT JSON [, ANALYZE])`; MySQL 8
  `EXPLAIN FORMAT=TREE` and `EXPLAIN ANALYZE`; SQLite `EXPLAIN QUERY PLAN`), SPEC-0012 (statement classes
  and permissions)
- **Feature matrix rows:** F-059

## Purpose
Show how the database will run (or ran) one statement, in one tree model for all dialects, and point out
the parts that usually explain a slow query.

## Behaviour
1. Input: exactly one statement and whether to measure (`analyze`). Several statements are an error. [GEN]
2. The plan is a tree of nodes: operation, object (table/alias or index, when the database names one),
   details (conditions, keys), estimated rows and cost when reported, and with `analyze` the actual rows,
   time in milliseconds and loops. The database's raw plan text is kept. [STD]
3. PostgreSQL: from the JSON plan (`Node Type`, `Relation Name`/`Alias`/`Index Name`, conditions,
   `Plan Rows`, `Total Cost`, `Actual …`). MySQL: from the tree text (one `->` line per node, indentation
   gives the depth, `(cost=… rows=…)` and `(actual time=…..… rows=… loops=…)`). SQLite: from the
   `EXPLAIN QUERY PLAN` rows (id, parent, detail); SQLite reports neither costs nor measurements, so
   `analyze` is refused there with that reason. [STD]
4. Measuring runs the statement. A statement that changes data is measured inside a transaction that is
   rolled back, and the result says so; a statement that changes the schema is never measured. [GEN]
   **v2:** every measured statement runs inside a transaction that is rolled back, also one that
   counts as a read (SPEC-0012): a `SELECT` can call a function that writes, so a read is no
   promise that nothing changes. The plan says that it was rolled back. On MySQL and MariaDB a
   table without transactions (MyISAM, MEMORY) keeps what the statement changed; for a statement
   that changes data there the plan's hints say so. A statement that changes the schema is still
   never measured. Before v2 only statements classed as changing data were rolled back. [GEN]
5. A full scan is a sequential scan (PostgreSQL `Seq Scan`, MySQL `Table scan`, SQLite `SCAN` without an
   index) and a PostgreSQL index scan without an index condition, which also reads every row.
   Hints: a full scan of an object with at least 1 000 estimated (or actual) rows; with `analyze`, a
   node whose estimate is off from the actual rows by a factor of 10 or more (stale statistics). [GEN]
6. MCP tool `explain_sql` (connection, sql, analyze): needs `read-schema`; with `analyze` also the
   permission of the statement's class (SPEC-0012). [ADR-0008]

7. **Every field of a step (v2):** each step keeps every field the database reports for it, by its
   own name and in its order, as `properties` (name and value as text; a list joined by `, `):
   PostgreSQL every key of the JSON node except `Plans` (`Node Type`, `Relation Name`, `Alias`,
   `Startup Cost`, `Total Cost`, `Plan Rows`, `Plan Width`, the `Actual …` values, buffers, `Filter`,
   `Index Cond`, `Join Type` …); MariaDB every value of the table or block; SQL Server the
   attributes of the `RelOp`; MySQL the parts of the tree line (operation, object, condition,
   `cost`, `rows`, `actual time` first and last, `actual rows`, `loops`); SQLite, Oracle and text
   plans the values they have. Nothing is renamed: a step is named as the database names it. [STD]
8. **Start-up cost and share (v2):** a step also has its start-up cost (PostgreSQL `Startup Cost`)
   and measured start-up time (PostgreSQL `Actual Startup Time`, MySQL the first `actual time`)
   where the database reports them, and `cost_share`: its total cost as a part (0 to 1) of the
   cost of the whole plan (the top-level steps together). Without costs there is no share. [GEN]
9. **Hints at their step (v2):** each hint of behaviour 5 is also kept with the step it is about
   (`hints` of the step), so a view can show it there; the plan's list of hints stays. [GEN]

## Acceptance criteria
- **AK-1** The PostgreSQL JSON, the MySQL tree text and the SQLite rows of a join with a filter give the
  expected trees (operations, objects, details, rows, costs, nesting).
- **AK-2** With `analyze`, actual rows, time and loops are filled (PostgreSQL, MySQL); SQLite refuses.
- **AK-3** A full scan of the 10 000-row `movement` table gives a hint; a misestimate by ≥ 10× gives a hint.
- **AK-4** Measuring an `UPDATE` leaves the table unchanged and reports the rollback; measuring DDL is
  refused; two statements are refused.
- **AK-5** Against the reference databases (SQLite always, PostgreSQL/MySQL when configured), plans of
  reference queries are read without error.
- **AK-6** The MCP tool returns the tree and checks the permissions.
- **AK-7 (v2, behaviours 7–9)** Unit tests `plan::tests::steps_keep_every_field`: a PostgreSQL JSON
  node keeps all its keys but `Plans` in order, with start-up cost and time; the MySQL line gives
  its parts and first actual time; shares add up for the top step (1.0) and its children; a full
  scan and a misestimate carry their hint at the step. MCP (`crates/mcp/tests/it/protocol.rs`:
  `explain_sql_returns_step_properties_and_shares`): on SQLite the steps have `properties` and
  `hints`.
- **AK-8 (v2, behaviour 4)** `plan::tests::every_measured_statement_is_rolled_back`: the statements
  run for a measured read and for a measured change are wrapped in `BEGIN` … `ROLLBACK` (MySQL:
  `START TRANSACTION`), and the MySQL hint for tables without transactions.

## Out of scope
Comparing plans. v2: graphical rendering is SPEC-0110 v13; buffer statistics are kept as fields
(behaviour 7), not interpreted.

## Known deviations from the reference
Not yet compared (guided session). Measured plans of data-changing statements are always rolled back.
