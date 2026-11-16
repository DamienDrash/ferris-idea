# SPEC-0010: MySQL session — connect and run scripts

- **Version:** 2 · **Date:** 2026-11-03 (v1: 2026-10-31)
- **Sources:** STD (MySQL client/server protocol: text protocol result sets, affected rows, error packets
  with error number and SQLSTATE; `SET SESSION TRANSACTION READ ONLY`), GEN, OBS-0004 (IMP-0004,
  IMP-0008, IMP-0009), ADR-0011, SPEC-0009 (same session contract)
- **Feature matrix rows:** F-050, F-051

## Purpose
Connect to MySQL from a connection profile and run an SQL script with the same contract as the
PostgreSQL session (SPEC-0009).

## Behaviour
1. Behaviour 1–4 and 8 of SPEC-0009 apply unchanged, with MySQL defaults (port 3306) and MySQL lexical
   rules for splitting (SPEC-0005). [STD]
2. A read-only profile starts the session with `SET SESSION TRANSACTION READ ONLY`; changes are refused
   by the server (SQLSTATE 25006). [STD]
3. Statements run with the text protocol; each yields a result set (column names, values as text, NULL as
   none) when the server returns columns, otherwise the affected row count. Binary values that are not
   valid UTF-8 are shown as `0x` followed by hex digits. [STD, GEN]
4. Server errors carry the message, SQLSTATE and the MySQL error number (as `MySQL error <n>` in the
   message prefix). MySQL reports no character position, but a syntax error names the text where
   parsing stopped and its line (`… near 'frm movement' at line 2`): the position is where that text
   starts on that line (at the end of the statement when the text is empty). Other errors carry no
   position; none is guessed. The same holds for MariaDB (SPEC-0116). [STD, IMP-0008]

## Acceptance criteria
Integration criteria run against a MySQL server given by `IDECRAFT_TEST_MYSQL` (a SPEC-0008 URL);
without it they are skipped.
- **AK-1** `select count(*), sum(delta) from movement` gives `10000` and `-30`.
- **AK-2** Two SELECTs give two result sets in order.
- **AK-3** `select no_such_column from movement` gives SQLSTATE `42S22` and error number 1054 in the message.
  v2: `select *\n  frm movement` gives a position at `frm` (unit test `mysql::tests` for the
  message forms, live in `crates/database/tests/it/mysql.rs`).
- **AK-4** Creating a temporary table and inserting three rows gives update count 3.
- **AK-5** With a read-only profile, `update movement set delta = delta where id = 1` fails with SQLSTATE
  `25006`.
- **AK-6** `select unhex('ff00')` returns `0xff00`.
- **AK-7** Missing-credential profiles are refused before connecting (shared with SPEC-0009); TLS follows SPEC-0016.

## Out of scope
`caching_sha2_password` with a password over an unencrypted connection, multiple result sets from
stored procedures, cancelling statements.

## Known deviations from the reference
Same as SPEC-0009 (IMP-0004, IMP-0008, IMP-0009); since v2 syntax errors also point at the
position the server named by text and line (IMP-0008).
