# SPEC-0005: SQL script splitting

- **Version:** 5 · **Date:** 2026-11-15 (v5: the editor colours block comments by the same rules, behaviour 19 — OBS-0051 L7); 2026-11-15 (v4: every result set of one statement, behaviour 18 — OBS-0051 L2); 2026-11-14 (v3: `DELIMITER`, escape strings, nested comments, SQLite brackets, MySQL's `--` rule and properties of every split, behaviours 12–17 — F-050); 2026-11-13 (v2: the bodies of triggers, procedures and functions stay one
  statement — F-053, notes from the SQLite check in PR 650; v1 2026-10-30)
- **Sources:** GEN (running "the current statement" or a whole script in SQL tools), STD (SQL lexical rules; PostgreSQL, MySQL and SQLite manuals for dialect differences; SQLite `CREATE TRIGGER`, MySQL compound statements, PostgreSQL `CREATE FUNCTION … BEGIN ATOMIC`, T-SQL batches; v3: PostgreSQL manual on `E'…'` strings and nested block comments, MySQL manual on the `mysql` client's `delimiter` command and on `--` comments, SQLite documentation on `[identifier]`, Transact-SQL reference on nested comments)
- **Feature matrix rows:** F-050, F-053

## Purpose
Split an SQL script into individual statements so they can be executed one by one.

## Behaviour
1. A statement ends at `;` outside string literals, quoted identifiers and comments. [STD]
2. Doubled quotes inside a literal or quoted identifier do not end it. [STD]
3. Line comments (`--`) and block comments (`/* */`) are recognised in all dialects. [STD]
4. MySQL only: `#` starts a line comment, backticks quote identifiers, backslash escapes inside string literals. [STD]
5. PostgreSQL only: dollar-quoted bodies (`$$…$$`, `$tag$…$tag$`) are opaque; `$1` is a parameter, not a quote. `#` is an operator. [STD]
6. Statement text is trimmed, excludes the terminator and excludes comments before its first token. Statements consisting only of whitespace and comments are dropped. [GEN]
7. Each statement reports its byte range in the script so the editor can highlight it. [GEN]
8. An unterminated literal runs to the end of the script. [GEN]
9. **v2 – Bodies of stored units:** a statement that creates a stored unit with a body of `BEGIN … END`
   blocks does not end at a `;` while one of its blocks is open: SQLite `CREATE [TEMP] TRIGGER`; MySQL and
   MariaDB `CREATE … TRIGGER | PROCEDURE | FUNCTION | EVENT` (after modifiers such as `OR REPLACE`,
   `DEFINER = …`, `IF NOT EXISTS`, `AGGREGATE`); PostgreSQL and DuckDB `CREATE [OR REPLACE] FUNCTION |
   PROCEDURE` with a standard body (`BEGIN ATOMIC … END`). Blocks are counted on the code words outside
   literals and comments: `BEGIN` and `CASE` open one, `END` closes one (with its `IF`, `CASE`, `LOOP`,
   `WHILE` or `REPEAT` after it). The statement ends at the first `;` after its last block closed. A
   bare `BEGIN` (a transaction) and `CREATE TABLE`, `VIEW`, `INDEX` and the like are no stored units. [STD]
10. **v2 – MySQL control flow:** in MySQL and MariaDB `IF`, `WHILE`, `REPEAT` and `LOOP` open a block
    when they start a statement inside a body (after `;`, `BEGIN`, `THEN`, `ELSE`, `DO`, `LOOP`, `REPEAT`
    or a label's `:`), closed by `END IF`, `END WHILE`, `END REPEAT`, `END LOOP`. Called as functions —
    `IF(a, b, c)`, `REPEAT('x', 3)`, recognised by a `,` at the level of their parentheses — and in
    `DROP TABLE IF EXISTS` they open nothing. A condition in parentheses (`IF (a = 1) THEN`) is no call. [STD]
11. **v2 – T-SQL units:** in SQL Server `CREATE`, `ALTER` or `CREATE OR ALTER` of a `PROCEDURE` (`PROC`),
    `FUNCTION`, `TRIGGER` or `VIEW` must be the only statement of its batch, so it runs to the end of the
    batch (the next `GO` line or the end of the script); its `;` stay in its text. [STD]
12. **v3 – Escape strings:** in PostgreSQL and DuckDB a literal written `E'…'` (or `e'…'`, the `E` not
    being the end of a longer word) takes a backslash as an escape, so `\'` does not end it. Other
    literals keep behaviour 5. [STD]
13. **v3 – Nested block comments:** in PostgreSQL, DuckDB and SQL Server a `/*` inside a block comment
    opens another level and the comment ends when every level is closed. In MySQL, MariaDB, SQLite and
    Oracle the first `*/` ends it. [STD]
14. **v3 – SQLite brackets:** in SQLite `[…]` quotes an identifier (to the next `]`), as it does in
    SQL Server (there with `]]`). [STD]
15. **v3 – MySQL's `--`:** in MySQL and MariaDB `--` starts a comment only when a blank, a control
    character or the end of the script follows; `5--1` is an expression. [STD]
16. **v3 – `DELIMITER`:** in MySQL and MariaDB a line whose first word is `DELIMITER` (any case) is the
    client's command and no statement: it is never sent. The rest of the line, trimmed, is the new
    delimiter (an empty one changes nothing). From there a statement ends at that delimiter outside
    literals, quoted identifiers and comments; while it is not `;`, a `;` is text of the statement and
    behaviours 9 and 10 do not apply. `DELIMITER ;` brings behaviours 1, 9 and 10 back. The command is
    recognised only where a statement could start, so the word in a literal, a comment or the middle of
    a statement is text. A text split without the line (a selection that begins after it) is split by
    `;`. [STD]
17. **v3 – Properties of every split:** for every dialect and any text the statements are in script
    order, their ranges do not overlap and lie on character boundaries, each text is the script's slice
    of its range without blanks at either end and is never empty; splitting never fails. [GEN]

18. **v4 – Several result sets of one statement (OBS-0051 L2):** a statement that returns more than
    one result set — `CALL` of a MySQL or MariaDB procedure with several `SELECT`s — gives one
    outcome per result set, in the order the server sends them, all with the statement's range. The
    status the server sends after a procedure's result sets is no outcome of its own; a statement with
    no result set gives its update count as before. When a later result set fails, the sets before it
    are kept and the failure follows them. The SQL editor and the console show every result set as a
    result tab of its own (SPEC-0093), the run counts the statement once, and `run_sql` returns them
    in order. [STD]
19. **v5 – Comments coloured as split (OBS-0051 L7):** the SQL editor colours a block comment over the range
    this splitter finds for the file's dialect (behaviour 13): in PostgreSQL, DuckDB and SQL Server an
    inner `/*` opens a level, so `/* a /* b */ c */` is one comment to its last `*/`; in the other dialects
    it ends at the first `*/`. Quotes, dollar quotes and line comments are skipped as in splitting, so a
    `/*` in a string opens nothing. The dialect is that of the file's data source (SPEC-0192), else the
    one its path names (SPEC-0019), else PostgreSQL as for inspections. [STD]

## Acceptance criteria
- **AK-1** Basic splitting and trimming.
- **AK-2** Semicolons in literals, quoted identifiers and comments do not split.
- **AK-3** MySQL rules (backticks, `#`, backslash).
- **AK-4** PostgreSQL rules (`#` operator, backslash literal, dollar quotes, `$1`).
- **AK-5** Dollar quotes are not special outside PostgreSQL.
- **AK-6** Empty and comment-only statements are dropped.
- **AK-7** Ranges index into the script.
- **AK-8** Unterminated literal.
- **AK-9 (v2)** An SQLite trigger with a `CASE` expression and two statements in its body is one
  statement; `BEGIN; …; COMMIT;` still splits; against a real SQLite file the trigger is created in one
  statement and fires.
- **AK-10 (v2)** A MySQL/MariaDB procedure with labelled `WHILE`, `IF`/`ELSEIF`/`ELSE`, a nested
  `BEGIN … END`, a `CASE` statement, `REPEAT … UNTIL … END REPEAT`, `LOOP`, the functions `IF(…)` and
  `REPEAT(…)` and `DROP TABLE IF EXISTS` is one statement, as are a trigger and an event; a function
  without `BEGIN` ends at its `;`. A PostgreSQL/DuckDB function with `BEGIN ATOMIC` is one statement.
- **AK-11 (v2)** A T-SQL `CREATE OR ALTER PROCEDURE` runs to its `GO`; statements in other batches split
  at `;` as before.
- **AK-12 (v3)** `E'it\'s; x'` is one PostgreSQL statement and two in SQLite; a plain `'a\'` ends at
  its quote in PostgreSQL.
- **AK-13 (v3)** `/* a /* b */ ; */ select 1; select 2` is two statements in PostgreSQL, DuckDB and
  SQL Server; MySQL and SQLite end the comment at the first `*/`.
- **AK-14 (v3)** `select [a;b] from t; select 2` is two statements in SQLite.
- **AK-15 (v3)** `select 5--1; select 2` is two statements in MySQL and MariaDB, one in PostgreSQL.
- **AK-16 (v3)** A script with `DELIMITER //`, a procedure with `;` in its body ended by `//`,
  `DELIMITER ;` and a plain statement gives the procedure and the statement, no `DELIMITER` line; a
  delimiter of several characters (`$$`); the delimiter inside a literal does not end the statement;
  in PostgreSQL the line is text. The console finds no statement on the `DELIMITER` line and the
  procedure with the caret in its body.
- **AK-17 (v3)** Generated scripts (a fixed pseudo-random walk, 400 per dialect): statements joined
  with their terminator, comments and blank lines come back exactly; arbitrary text from an alphabet
  of quotes, comment marks, `$`, `;`, `/`, `GO`, `DELIMITER` and multi-byte characters never breaks
  behaviour 17.
- **AK-18 (v3)** The engines agree: scripts using behaviours 12 to 15 run statement by statement
  without an error on SQLite and DuckDB (in the test process) and, where the reference servers run,
  on PostgreSQL, MySQL and MariaDB (`crates/database/tests/it/split_live.rs`).
- **AK-19 (v4, behaviour 18)** Where the reference servers run (`split_live.rs`): on MySQL and MariaDB
  a procedure with two `SELECT`s gives two result sets in order from one `CALL`, a procedure without
  one gives its update count (`a_call_returns_every_result_set`); the console makes a result tab of
  each and counts the statement once (`sql_console_model::tests::every_result_set_of_a_statement_is_a_tab`).
- **AK-20 (v5, behaviour 19)** `script::tests::block_comments_follow_the_dialect` (nested in PostgreSQL,
  DuckDB and SQL Server, flat in MySQL, SQLite and Oracle; `/*` in a string, a dollar quote and after
  `--` opens none); `sql_editor_model::tests::nested_comments_are_coloured_per_dialect`: the spans of `/* a /* b */ c */ select 1` colour
  `c */` as comment for PostgreSQL and as code for MySQL.

## Out of scope
A count after `GO` (`GO 5`): the line ends the batch and the batch runs once. `psql` backslash commands,
SQL*Plus commands other than the `/` line, a `;` inside the parentheses of PostgreSQL's `CREATE RULE`.
SQL Server `GO` batches and Oracle PL/SQL blocks were added later in the same splitter (SPEC-0116, SPEC-0135).
PostgreSQL bodies in literals or dollar quotes are opaque already (behaviour 5).
