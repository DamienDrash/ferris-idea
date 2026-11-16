# SPEC-0007: Script results, error positions and stable result order

- **Version:** 1 · **Date:** 2026-10-31
- **Sources:** GEN (running SQL scripts in database tools), STD (SQLSTATE codes; PostgreSQL error position is a
  1-based character offset into the statement text; `ORDER BY` semantics), OBS-0004, OBS-0005 (weaknesses
  only, restated as improvements IMP-0008, IMP-0009, IMP-0011, IMP-0012)
- **Feature matrix rows:** F-050, F-054

## Purpose
When a user runs an SQL script, every statement produces a visible outcome in order, errors point at the
exact place in the script, and paged, sorted results never repeat or skip rows. Value summaries in the
result grid say which rows they were computed from.

## Behaviour
1. Running a script produces one outcome per statement from `SPEC-0005`, in script order. An outcome is a
   result set (columns and rows), an update count, or an error. No outcome is dropped. [GEN, IMP-0009]
2. Each outcome carries the statement's byte range in the script. [GEN]
3. After an error, execution stops by default; the outcomes list ends with that error and states how many
   statements were not run. A "continue on error" mode runs all statements. [GEN]
4. An error carries the database message, the SQLSTATE code if the driver reports one, and the position
   if the database reports one. [STD, IMP-0008]
5. A reported position is a 1-based character offset into the statement text. It is mapped to a byte offset
   in the script (statement start + offset of that character). A position outside the statement is
   ignored (no position) instead of pointing at the wrong place. [STD]
6. Errors are never reported as "unknown" when the database supplied a message. If no message exists at
   all, the outcome says so explicitly. [IMP-0008]
7. Sorting a result by one or more columns adds a tie-breaker when the sort columns do not contain a
   unique key of the source: the columns of the primary key, else of the first unique key, in ascending
   order, skipping those already sorted on. Without any known unique key the order is marked as not
   stable. [STD, IMP-0011]
8. Generated `ORDER BY` clauses quote identifiers per dialect: double quotes for PostgreSQL and SQLite,
   backticks for MySQL; quote characters inside names are doubled. [STD]
9. A value summary of a column (distinct values with counts) states its scope: either "loaded rows" with
   the number of loaded rows and, if known, the total, or "all rows" when computed by the database. [IMP-0012]
10. A summary over loaded rows sorts values by count (descending), then by value, and treats NULL as its own
    value. [GEN]

## Acceptance criteria
- **AK-1** Given a script with two SELECTs, when its outcomes are recorded, then there are two result-set
  outcomes in script order with the statements' ranges.
- **AK-2** Given three statements where the second fails, when run with stop-on-error, then the outcomes
  are result, error, and the run reports one statement not executed; with continue-on-error there are
  three outcomes.
- **AK-3** Given an error with position 8 in the second statement of a script, when mapped, then the
  script byte offset points at the 8th character of that statement, also when earlier text contains
  multi-byte characters.
- **AK-4** Given a position larger than the statement, when mapped, then no position is reported.
- **AK-5** Given an error with an empty message and no SQLSTATE, when displayed, then the text says that
  the database gave no message (never "unknown error").
- **AK-6** Given sort on `sku` and primary key `id`, then the order is `sku, id`; given sort on `id desc`,
  no tie-breaker is added; given a composite primary key `(warehouse_id, sku)` and sort on `sku`, the order
  is `sku, warehouse_id`.
- **AK-7** Given no primary or unique key, the order is returned unchanged and marked not stable.
- **AK-8** Identifiers are quoted per dialect, including embedded quote characters.
- **AK-9** Given 500 loaded rows of 10 000, a summary reports scope "500 loaded rows of 10000" and counts
  sorted by count, then value; NULL is counted as its own value.

## Out of scope
Executing statements against a database (drivers, M5), rendering the grid, server-side `GROUP BY`
summaries (the scope type exists; the query is built later).

## Known deviations from the reference
- Every statement's outcome is kept (reference: only the last result via MCP; none when running a file). IMP-0009.
- Errors keep message, SQLSTATE and position (reference: "unknown error" via MCP). IMP-0008.
- Sorting adds a unique tie-breaker (reference: sort column only). IMP-0011.
- Value summaries state their scope (reference: page-local counts without scope). IMP-0012.
