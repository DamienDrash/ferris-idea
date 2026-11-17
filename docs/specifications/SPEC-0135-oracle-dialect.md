# SPEC-0135: Oracle dialect without a connection

- **Version:** 2 · **Date:** 2026-11-17 (v2: a column's default and identity clause before `NOT NULL`,
  behaviour 5, AK-5 — OBS-0083 L2); 2026-11-02 (v1)
- **Sources:** GEN (database IDEs support Oracle next to the other systems: scripts with PL/SQL
  blocks, completion, quoting, DDL for schema changes; F-060), STD (Oracle SQL and PL/SQL lexical
  rules: `"identifier"`, `'…'` with `''`, alternative quoting `q'[…]'`/`nq'…'` with paired
  delimiters, PL/SQL units ending at a line holding only `/` as SQL*Plus and SQLcl run them; DDL:
  `ALTER TABLE t ADD c …`, `MODIFY (c type DEFAULT … NULL|NOT NULL)`, `DEFAULT NULL` to remove a
  default, `RENAME COLUMN`/`RENAME TO`; `DECODE` comparing two NULLs as equal), SPEC-0005 (scripts),
  SPEC-0008 (profiles), SPEC-0014 (completion), SPEC-0017 (editing), SPEC-0018 (DDL), SPEC-0116
  (more dialects), SPEC-0108 (connection dialog), ADR-0008
- **Feature matrix rows:** F-060

## Purpose
Work with Oracle SQL and PL/SQL files in IDECraft — split, complete, inspect, generate DDL — and
keep Oracle connections in the workspace, while a driver is not included yet. Oracle's own
client libraries are proprietary downloads, and no mature pure-Rust driver for its network
protocol is known; the choice is the project owner's (PO-STATUS open question).

## Behaviour
1. **Profiles:** dialect `oracle`, URL `oracle://user@host:port/SERVICE` (the database part is the
   service name), default port 1521; the connection dialog offers Oracle. [SPEC-0008]
2. **Session:** connecting (also Test Connection) fails at once with "IDECraft has no Oracle driver
   yet; scripts, completion, inspections and DDL work without a connection". Nothing is contacted.
3. **Scripts:** a statement ends at `;`, except inside a PL/SQL block (a statement starting with
   `DECLARE`, `BEGIN` or `CREATE [OR REPLACE] [EDITIONABLE|NONEDITIONABLE] PROCEDURE|FUNCTION|PACKAGE|
   TRIGGER|TYPE`), which ends at a line holding only `/`; such a line also ends any other statement.
   `q'X…X'` and `nq'X…X'` are literals, with `[]`, `{}`, `()`, `<>` closing as pairs. [STD]
4. **Quoting, literals, editing:** identifiers in double quotes, literals with doubled quotes and no
   backslash escapes; editing compares a loaded value with `DECODE(column, value, 1, 0) = 1`, which
   holds for two NULLs; changes start with `SET TRANSACTION READ WRITE`. [STD, SPEC-0017]
5. **DDL:** `ADD` without `COLUMN`; a changed column is one `MODIFY (…)` restating only what changed
   (type, `DEFAULT …`/`DEFAULT NULL`, `NULL`/`NOT NULL`), because restating an unchanged `NOT NULL`
   is an error; constraints and indexes as in PostgreSQL. Renames use `RENAME COLUMN … TO` and
   `RENAME TO`. [STD]
   **v2 (OBS-0083 L2):** in a column definition the default and the identity clause come before
   `NOT NULL` (`"STATE" VARCHAR2(10) DEFAULT 'new' NOT NULL`); Oracle refuses `NOT NULL DEFAULT …`
   with ORA-03076, so generated DDL for a `NOT NULL` column with a default did not run. [STD]
6. **Completion:** the common keywords plus `dual`, `rownum`, `sysdate`, `systimestamp`, `nvl`,
   `decode`, `fetch first … rows only`, `connect by prior`, `start with`, `merge`, `varchar2`,
   `number`, `clob`, PL/SQL `begin`/`declare`/`exception`, `returning`, `minus`. [STD]
7. **MCP:** `split_sql_statements` and `save_connection` accept `oracle`. [ADR-0008]

## Acceptance criteria
- **AK-1** Scripts with PL/SQL blocks, `/` lines and `q'…'` literals (unit test
  `script::tests::oracle_plsql_blocks_slash_lines_and_q_quotes`).
- **AK-2** URLs, `DECODE` comparison, migration DDL, completion keywords and quotes (unit tests
  `parses_oracle_urls`, `oracle_compares_with_decode`, `oracle_migration`,
  `oracle_keywords_and_quotes`).
- **AK-3** Test Connection reports the missing driver (`crates/database/tests/it/connection_form.rs`);
  MCP splits an Oracle script (`crates/mcp/tests/it/protocol.rs`: `language_and_sql_tools`).
- **AK-4** Window self-test: Oracle in the connection dialog, Test Connection names the missing
  driver.
- **AK-5 (v2)** `ddl::tests::oracle_ddl_puts_default_and_identity_before_not_null`: identity and
  default before `NOT NULL`, never `NOT NULL DEFAULT`, comments as `COMMENT ON`; live in SPEC-0142 AK-7.

## Out of scope
A session, catalog and plans (they need the driver), PL/SQL language intelligence, packages and
synonyms in completion, `@script` and other SQL*Plus commands, `ROWID` editing.

## Known deviations from the reference
The reference connects through its own Oracle driver; IDECraft does not connect yet (behaviour 2).
