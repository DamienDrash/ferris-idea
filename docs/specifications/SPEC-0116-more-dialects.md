# SPEC-0116: More dialects — MariaDB and SQL Server

- **Version:** 4 · **Date:** 2026-11-15 (v4: acceptance criterion AK-8, hand test 43 in a window of the test harness against SQL Server; no change of behaviour); 2026-11-14 (v3: the test level in the connection dialog, sequences in the catalog, what is verified where, behaviours 10–12 — F-060; v2: 2026-11-03; v1: 2026-11-01)
- **Sources:** GEN (database IDEs connect to many systems, and every database feature of
  SPEC-0005–SPEC-0022 works the same way on each; F-060), STD (T-SQL lexical rules: `[identifier]`
  with `]]`, `"identifier"`, `N'…'` literals, `GO` as the client's batch separator; TDS protocol),
  DOC (MariaDB knowledge base, read for the rules, not quoted: `EXPLAIN FORMAT=JSON` and
  `ANALYZE FORMAT=JSON`, `information_schema` reporting `NULL` as a column's default, column-level
  `REFERENCES` creating foreign keys; Microsoft SQL Server documentation: `SET SHOWPLAN_XML`,
  `SET STATISTICS XML`, the showplan schema, `sys.*` catalog views, default constraints, `string_agg`
  from SQL Server 2017), SPEC-0008 (profiles), SPEC-0009/0010 (sessions), SPEC-0013 (introspection),
  SPEC-0017 (editing), SPEC-0018 (DDL), SPEC-0020 (plans), SPEC-0108 (connection dialog), ADR-0008,
  ADR-0011
- **Feature matrix rows:** F-060

## Purpose
Work with MariaDB and Microsoft SQL Server databases with the same tools as PostgreSQL, MySQL and
SQLite: connect, run scripts, browse, complete, edit results, compare schemas, explain.

## Behaviour
1. **Profiles:** dialects `mariadb` and `sqlserver`; URLs `mariadb://user@host:port/db?ssl-mode=…`
   and `sqlserver://user@host:port/db?sslmode=…` (`mssql://` is read too); default ports 3306 and
   1433. The connection dialog offers both next to the existing dialects. [GEN, SPEC-0008]
2. **Sessions:** MariaDB through the MySQL protocol driver, the profile deciding the dialect; SQL
   Server through the pure-Rust TDS driver `tiberius` with SQL Server authentication. SSL modes as
   in SPEC-0016: `disable` sends nothing encrypted, `prefer` falls back once to unencrypted,
   `require` encrypts without checking the certificate, `verify-full` checks it. Errors name the
   system (`MariaDB error 1054: …`, `SQL Server error 208 (line 1): …`). v2: MariaDB syntax errors
   get a position as in SPEC-0010 v2; a SQL Server error in a statement of several lines gets the
   position of the first non-blank character of the line the server names. [STD, ADR-0011, IMP-0008]
3. **T-SQL scripts:** statements end at `;` or at a line holding only `GO` (optionally with a
   count); `[…]` and `"…"` are identifiers, `#` starts no comment. Each statement is its own batch;
   statements that change rows report their row count, `OUTPUT` clauses and `EXEC` return their
   rows. [STD]
4. **Read-only SQL Server profiles:** SQL Server has no read-only session, so a read-only profile
   refuses every statement that is not known to only read (SPEC-0012 classes), except `SET`
   options. [DOC]
5. **Catalog:** MariaDB like MySQL, but a reported `NULL` default reads as none and functional
   indexes are not read; SQL Server from `sys.objects`, `sys.columns`, `sys.types`,
   `sys.key_constraints`, `sys.foreign_keys`, `sys.indexes` in the user's default schema, with
   lengths, precision and `IDENTITY(seed,increment)` in the type. [DOC]
6. **Quoting, literals, editing:** identifiers in backticks (MariaDB) or brackets (SQL Server);
   SQL Server literals as `N'…'`; edits compare the loaded value with `<=>` (MariaDB) or `=` /
   `IS NULL` (SQL Server, which only knows `IS NOT DISTINCT FROM` from 2022); transactions start
   with `START TRANSACTION` or `BEGIN TRANSACTION`. Completion offers dialect keywords (`TOP`,
   `OUTPUT`, `MERGE`, … for T-SQL; `RETURNING`, … for MariaDB) and brackets names that need it.
   [STD, SPEC-0014, SPEC-0017]
7. **DDL:** MariaDB like MySQL (defaults as reported); SQL Server `ADD` without `COLUMN`,
   `ALTER COLUMN name type [NOT] NULL`, `DROP INDEX name ON table`, defaults as constraints: setting
   one is `ADD DEFAULT … FOR column`, removing one (also before dropping its column) looks its name
   up in `sys.default_constraints` in a single batch. [DOC, SPEC-0018]
8. **Plans:** MariaDB `EXPLAIN FORMAT=JSON` / `ANALYZE FORMAT=JSON` read into the plan tree (each
   table a step with access type, key, estimated and measured rows, time and loops; blocks such as
   sort or nested loop above them). SQL Server `SET SHOWPLAN_XML ON` (estimate) or
   `SET STATISTICS XML ON` (measured), each switched off again afterwards; every `RelOp` is a step
   with its physical and logical operator, object, seek and filter predicates, estimates, cost and
   the measured rows per execution summed over threads. Data changes measured on either are rolled
   back as in SPEC-0020. [DOC, SPEC-0020]
9. **MCP:** `split_sql_statements` and `save_connection` accept `mariadb` and `sqlserver`; all other
   database tools follow the connection's dialect. [ADR-0008]
10. **v3 – Test level in the dialog:** under the system's name the connection dialog says how far
    IDECraft vouches for it (SPEC-0300 behaviour 4): for a supported system "Tested with" and the
    version of its definition; for a Beta system that IDECraft has not tested it against a
    server; for a user's or imported definition that it is not IDECraft's. The definition's own
    note follows. The definitions of SQL Server and Oracle say that their live tests run only on a
    machine where that server is started. [GEN, SPEC-0300]
11. **v3 – Sequences:** the catalog lists the sequences of the current schema also on MariaDB
    (`information_schema.tables` with type `SEQUENCE`), DuckDB (`duckdb_sequences()`), Oracle
    (`user_sequences`) and SQL Server (`sys.sequences` of the default schema); MySQL and SQLite
    have none. A failed listing leaves the group out and never fails the tree. [DOC, STD]
12. **v3 – What is verified where** (state of 2026-11-14; "live" = automated test against a real
    engine, "unit" = generated statements and parsers only):

    | | session, catalog, editing, DDL, plan | splitting, quoting, completion | refactorings | sequences | cancel |
    |---|---|---|---|---|---|
    | MariaDB 12.3 | live | unit, splitting live | live | live | as MySQL; the live test runs on MySQL |
    | DuckDB (bundled) | session, catalog and plan live; editing and DDL unit | unit, splitting live | unit | live | none (■ grey) |
    | SQL Server 2022 | live only where the VM runs; not run on 2026-11-14 | unit | unit | unit | none (■ grey) |
    | Oracle Free 23 | live only where the container runs; not run on 2026-11-14 | unit | unit | unit | none (■ grey) |
    | Db2, SAP ASE and other ODBC systems | not connectable: the dialog says so, Test Connection names the missing route | — | — | — | — |

    Where a cell says "unit", a refusal of the server reaches the user as the server's message
    (SPEC-0164 behaviour 7); IDECraft claims nothing more. [GEN]

## Acceptance criteria
- **AK-1** URLs, `GO` and brackets in splitting and completion, quoting, literals, edit SQL,
  SQL Server and MariaDB migrations including default constraints, MariaDB JSON and SQL Server XML
  plans, read-only refusal, value rendering (unit tests in `profile`, `script`, `completion`,
  `order`, `edit`, `ddl`, `plan`, `sqlserver`); v2: line of a SQL Server error to a position
  (`outcome::tests`); MariaDB and SQL Server positions live (`crates/database/tests/{mariadb,sqlserver}.rs`).
- **AK-2** Against the reference MariaDB 12.3 (`db.sh install-mariadb` of the reference environment):
  counts, errors and read-only, introspection with the foreign keys MariaDB creates from inline
  references, editing with rollback, DDL round trip, plans with measured values and rollback
  (`crates/database/tests/{mariadb,introspect,edit,ddl,plan}.rs`, `IDECRAFT_TEST_MARIADB`).
- **AK-3** MCP: splitting T-SQL and MariaDB scripts, profiles for both (`crates/mcp/tests/it/protocol.rs`:
  `more_dialects_tools`).
- **AK-4** Window self-test: the connection dialog tests a SQL Server profile against a closed port
  and reports the address; Escape closes it.
- **AK-5** Hand test (round 4): a real SQL Server (2017 or later) — connect, browse, run a script
  with `GO`, edit a cell, explain with and without measuring.
- **AK-6 (v3, behaviour 10)** `connector::tests::every_system_says_how_far_it_is_tested`;
  `connection_dialog::tests::the_test_level_line`.
- **AK-7 (v3, behaviour 11)** Live: a sequence created in MariaDB and in DuckDB is listed
  (`crates/database/tests/it/introspect.rs::sequences_are_listed`); the Oracle and SQL Server
  queries are covered by `introspect::tests::sequence_queries_per_dialect` only.
- **AK-8 (v4, AK-5 without a screen)** In a window of the test harness with keys against the SQL
  Server of the reference environment (`crates/workbench/tests/it/sqlserver_retest.rs`
  `hand_test_43_sql_server`; skipped without `IDECRAFT_TEST_SQLSERVER`): Test Connection names SQL
  Server; the tree lists the tables and their `PK__…` keys; a script with a `GO` line runs as two
  batches, the second's error with its code 207; a changed cell is submitted ("1 change(s)
  committed") and read back; Explain Plan shows costs and the key's index under its whole name, as
  `sys.indexes` has it, in a column wider than the text measured in the plan's font (the side
  finding of hand test 43, `PK__wa…` cut, SPEC-0110 v3 behaviour 2); Explain Analyze shows
  measured rows.

## Out of scope
Oracle (no pure-Rust driver without the proprietary client libraries), Windows and Azure AD
authentication, named instances via the SQL Browser service, several schemas per SQL Server
database, T-SQL procedural blocks in completion and inspections, `sp_rename` for renaming objects,
MariaDB system-versioned tables and sequences in the catalog.

## Known deviations from the reference
Read-only SQL Server profiles are enforced by IDECraft, not by the server. The live SQL Server path
has no automated test on this machine (no server runs here); it is covered by unit tests of every
generated statement and parser and by the hand test. Not compared side by side yet. Errors point
into the statement where the server names a line or the text it stopped at (IMP-0008).
