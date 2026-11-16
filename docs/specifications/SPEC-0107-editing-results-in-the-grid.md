# SPEC-0107: Editing query results in the grid

- **Version:** 2 · **Date:** 2026-11-03 (v1 2026-11-01; v2: order of statements, preview and modes in SPEC-0190)
- **Sources:** GEN (editable result grids of database tools: change cells, add and delete rows, submit or
  revert, pending changes highlighted), SPEC-0017 (the edit set: row identity, conflict check, one
  transaction), SPEC-0093, SPEC-0099, ADR-0008, ADR-0012
- **Feature matrix rows:** F-055

## Purpose
The user changes the rows a query returned right in the Services grid and submits all changes at once;
nothing is written until they submit, and a row changed by someone else meanwhile is not overwritten.

## Behaviour
1. **Editable results:** a result can be edited when its statement is a plain `SELECT` from one table
   (optional schema, alias, `WHERE`, `ORDER BY`, `LIMIT`; no joins, several tables, subqueries in
   `FROM`, grouping, `DISTINCT` or set operations), the table has a row identity (primary key, else a
   unique index over NOT NULL columns) and every identity column is in the result. Other results stay
   read-only and say so when an edit is attempted. Columns that are not columns of the table (computed
   values) stay read-only. [GEN, SPEC-0017]
2. **Cells:** a cell is selected with a click or the arrow keys; a double-click, Enter or F2 edits it in
   place with its text, Enter keeps the new value, Escape cancels; Delete/Backspace sets it to NULL.
   Setting a cell back to its loaded value drops the change. Changed cells are highlighted. [GEN]
3. **Adding rows:** "+ Row", ⌘N (Ctrl+N) or Alt+Insert adds an empty row below the loaded rows;
   columns left empty get their defaults. [GEN]
4. **Deleting rows:** "− Row" or ⌘⌫ (Ctrl+Delete) marks the selected row for deletion (struck through)
   or unmarks it; on an added row it removes that row. [GEN]
5. **Submit and revert:** "Submit (n)" or ⌘↩ (Ctrl+Enter) applies all pending changes in one
   transaction (deletes, updates, inserts — v2, SPEC-0190 behaviour 7), matching updates and deletes against the loaded values;
   when one fails or matches no row, nothing is changed and the note says why. After a commit the query
   runs again. "Revert" or ⌘⌥Z (Ctrl+Alt+Z) drops all pending changes. [GEN, SPEC-0017]
6. **MCP:** the agent counterpart is `edit_table_data` (SPEC-0017), which takes the same changes and
   previews or applies them. [ADR-0008]

## Acceptance criteria
- **AK-1** The source table is found for plain selects, also schema-qualified, aliased and quoted, and
  refused for joins, several tables, subqueries, grouping, `DISTINCT`, set operations, `WITH` and
  non-selects (unit test).
- **AK-2** Pending edits: a result without the key is refused, computed columns are read-only, changed,
  reverted, NULL, deleted and added cells have the right states, and the changes become one edit set of
  update, delete and insert statements (unit test).
- **AK-3** Window self-test: a query on `customers` is editable; an edited cell and an added row are
  pending; ⌘↩ commits both and the query shows the table again.
- **AK-4** Hand test (round 4): highlighting, double-click editing, a conflicting change rolled back.

## Out of scope
Editing results of joins or views, editing several cells at once, pasting ranges, value editors for
dates, JSON or binary values, a preview of the SQL before submitting (available through
`edit_table_data` with a dry run), automatic submit.

## Known deviations from the reference
Changes are always collected and submitted explicitly (no automatic submit mode). Rows of an editable
result are identified by the table's key, so results that leave out the key stay read-only instead of
being matched by all their values.
