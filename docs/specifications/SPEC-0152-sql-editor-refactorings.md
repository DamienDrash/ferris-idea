# SPEC-0152: Refactorings in the SQL editor

- **Version:** 4 · **Date:** 2026-11-16 (v4: the message for a refactoring SQL files do not offer reads as a whole sentence, behaviour 6a — OBS-0068 N1); 2026-11-16 (v3: every way to Inline and Refactor This work in SQL files, other refactorings name no language server, behaviour 6a — OBS-0068 L4); 2026-11-15 (v2: Inline of a common table expression, behaviours 6–7 —
  the "Inline fehlt" gap of F-057); 2026-11-03
- **Sources:** GEN (database IDEs rename a table or column from its name in a SQL script with
  ⇧F6 and offer "Expand column list" on `*` of a select list with ⌥↩; F-057), DOC (action ids
  `RenameElement`, `ShowIntentionActions`), SPEC-0019 (analysis of references), SPEC-0093 (SQL
  console connection), SPEC-0128 (rename dialog), ADR-0008
- **Feature matrix rows:** F-057

## Purpose
Refactor the schema where the SQL is written, not only from the Database tool window.

## Behaviour
1. **Finding the object:** in a `.sql` editor the name at the caret is looked up in the schema of
   the console's connection (SPEC-0093): a table where a statement introduces it (`FROM`, `JOIN`,
   `INTO`, `UPDATE`, `TABLE`, `REFERENCES`) or where it qualifies a column; `q.c` is the column `c`
   of the table `q` names or aliases; an unqualified name is a column when exactly one table of the
   statement has it. [GEN, SPEC-0019]
2. **Rename:** ⇧F6 / Shift+F6 opens the rename dialog of SPEC-0128 for that table or column, with
   its preview, refusal on read-only connections and changes of the database and the SQL files.
   Closing it returns to the editor. [GEN, SPEC-0128]
3. **Refusals:** without a connection, on an alias, on a name of several tables of the statement or
   on anything else the status bar says why and no dialog opens. The schema is read once per
   connection and read again after a rename. [GEN]
4. **Expand column list:** ⌥↩ / Alt+Enter on `*` of a select list (not `count(*)`) offers "Expand
   column list", which replaces it with the columns of the statement's table, or each joined
   table's columns qualified by its alias or name; `q.*` becomes `q.` and the columns of `q`'s
   table. It is one undo step. [GEN, DOC]
5. **MCP:** `sql_refactorings_at` returns the object at a line and column of a SQL text (or why
   there is none) and the "Expand column list" edit with the resulting SQL; `rename_sql_object`
   renames the object. Needs the `read-schema` permission. [ADR-0008]
6. **v2 – Inline a common table expression:** Inline (⌥⌘N / Ctrl+Alt+N, Refactor menu and the
   editor's context menu) on the name of a `WITH` query, where it is defined or where a statement
   reads it, replaces every reference of the statement by the query in parentheses and removes the
   definition, and `WITH` with it when it was the only one; `⌥↩` offers the same as "Inline common
   table expression `name`". A reference keeps its alias; one without an alias gets the query's
   name as alias, so qualified columns (`name.col`) stay valid and every dialect accepts the derived
   table. Later `WITH` queries that read it are changed too. It is one undo step and needs no
   connection. [GEN]
6a. **v3 – Every way there (OBS-0068 L4):** in a `.sql` file the action Inline does behaviour 6
   whether it comes from the Refactor menu, the context menu, ⌥⌘N or Find Action; Refactor This
   opens the list of `⌥↩` at the caret, which holds the Inline of behaviour 6 and the expansion of
   `*`. Refactorings SQL files do not have (Extract Method, Introduce Variable and the like) say in
   the status bar that they are not offered in SQL files and name the ones that are; no message
   names a language server, which SQL files never have. [GEN]
   **v4 (OBS-0068 N1):** the message reads "`<refactoring>` is not offered in SQL files. Offered:
   Rename (⇧F6), Inline (⌥⌘N) and the actions of ⌥↩." — in v3 it ended in "… and the actions of
   ⌥↩ are" and read as cut off. [GEN]
7. **v2 – Refused, with the reason in the status bar:** a name that is no `WITH` query of the
   statement; a query that reads itself (recursive); one with a column list (`name (a, b) AS …`) or
   marked `MATERIALIZED` / `NOT MATERIALIZED`, whose meaning a derived table does not keep; one
   the statement writes to (`INSERT INTO`, `UPDATE`); one that nothing reads. Nothing changes then.
   [GEN]
8. **v2 – MCP:** `sql_refactorings_at` also returns `inline` (the edits and the resulting SQL) when
   the caret is on a `WITH` query's name, else `inline_error` with the reason of behaviour 7, or
   null when the caret is on no name. [ADR-0008]

## Acceptance criteria
- **AK-1** Object at the caret: aliased and unqualified columns, a table, refusals for an alias and
  for a column of two tables; expansion for one table, joined tables, `q.*`, not `count(*)`
  (`refactor::tests`).
- **AK-2** MCP: object, table, expansion and refusal on SQLite
  (`sql_refactorings_at_name_the_object_and_expand_the_wildcard`).
- **AK-3** Window self-test: ⇧F6 on `o.note` opens the dialog previewing
  `ALTER TABLE orders RENAME COLUMN note TO remark` with the place in the editor's file; Escape
  closes it; ⌥↩ on `*` offers "Expand column list", Enter writes `id, customer_id, note`.
- **AK-4** Hand test (round 4, no. 78): rename a column from a script and expand `*` in a join on
  PostgreSQL next to the reference.

- **AK-5 (v2, behaviours 6–7)** `refactor::tests::inline_a_common_table_expression` (from the
  definition and from a reference; alias kept and added; one of two `WITH` queries, a later query
  reading it, the only one removing `WITH`; refusals for recursive, column list, `MATERIALIZED`,
  written to, unused and no `WITH` query) and
  `sql_editor_model::tests::inline_is_offered_at_a_with_query` (the `⌥↩` entry and its edits).
- **AK-6 (v2, behaviour 8)** `sql_refactorings_at_name_the_object_and_expand_the_wildcard` also
  checks `inline` and `inline_error`.
- **AK-7 (v3, behaviour 6a)** The menu's Inline inlines the `WITH` query at the caret and
  Extract Method names no language server (window test of the harness,
  `crates/workbench/tests/it/db_gaps_obs0068.rs`: `the_menus_inline_inlines_a_with_query`); the
  status line (`sql_editor_model::tests::sql_files_name_their_own_refactorings`).
- **AK-8 (v4, behaviour 6a)** The whole sentence
  (`sql_editor_model::tests::sql_files_name_their_own_refactorings`).

## Out of scope
Renaming aliases, views, routines, indexes and constraints; other database refactorings (change
column type, introduce alias; v2: inlining a view's definition, which needs the view's text from the
database, and inlining a subquery's alias); qualified names with a schema prefix for the lookup.

## Known deviations from the reference
v2: Inline of a `WITH` query is specified from the generic refactoring; it has not been compared
with the reference's Inline in SQL files (which objects it offers, whether it asks first).
A name that could be a column of several tables is refused with the tables named instead of being
guessed.
