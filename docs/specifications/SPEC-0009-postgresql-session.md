# SPEC-0009: PostgreSQL session — connect and run scripts

- **Version:** 2 · **Date:** 2026-10-31 (v2: TLS per SPEC-0016 instead of refusing TLS profiles)
- **Sources:** STD (PostgreSQL frontend/backend protocol: simple query, CommandComplete row counts, error
  fields SQLSTATE and position; `default_transaction_read_only`), GEN (database tools connect from a
  profile and run scripts), OBS-0004 (weaknesses → IMP-0004, IMP-0008, IMP-0009), ADR-0011
- **Feature matrix rows:** F-050, F-051

## Purpose
Connect to PostgreSQL from a connection profile and run an SQL script, getting one outcome per statement
with values as text and precise errors.

## Behaviour
1. A session is opened from a SPEC-0008 profile and its resolved credentials. Host, port (default 5432),
   database and user come from the profile; the password only from the credentials. [STD]
2. Missing credentials (SPEC-0008 behaviour 13) are an error before any network access, naming the
   profile and the keychain entry; nothing waits for input. [IMP-0004]
3. SSL modes follow SPEC-0016 (TLS); a mode that cannot be met is an error, never a silent downgrade.
   [ADR-0011, SPEC-0016]
4. Connecting gives up after 10 seconds; the error names the profile and `host:port`. [GEN]
5. A read-only profile starts the session with `default_transaction_read_only = on`; changes are refused
   by the server (SQLSTATE 25006). [STD]
6. Running a script splits it with SPEC-0005 (PostgreSQL rules) and executes each statement with the
   simple query protocol. Each statement yields a SPEC-0007 outcome: a result set when the server
   described rows (column names in order, values as text, NULL as none), otherwise the affected row
   count. [STD, IMP-0009]
7. Server errors carry message, SQLSTATE and the error position; the position is mapped into the script
   (SPEC-0007 behaviour 5). Connection failures during a run become errors with the driver's message.
   [STD, IMP-0008]
8. Stop-on-error and continue-on-error follow SPEC-0007 behaviour 3. [GEN]

## Acceptance criteria
Integration criteria run against a PostgreSQL server given by the environment variable
`IDECRAFT_TEST_PG` (a SPEC-0008 URL, e.g. the reference database); without it they are skipped.
- **AK-1** `select count(*), sum(delta) from movement` on the reference database gives one result set
  with values `10000` and `-30`.
- **AK-2** A script with two SELECTs gives two result sets in order.
- **AK-3** `select no_such_column from movement` gives SQLSTATE `42703`, and the script offset of the
  error points at `no_such_column`.
- **AK-4** Creating a temporary table and inserting three rows gives update count 3 for the insert.
- **AK-5** With a read-only profile, `update movement set delta = delta where id = 1` fails with SQLSTATE
  `25006`.
- **AK-6** A profile with SSL `require` against a server that cannot provide TLS fails with an error naming `require` (SPEC-0016).
- **AK-7** A profile whose keychain entry is missing is refused before connecting, naming the entry.
- **AK-8** An unreachable port fails with an error naming profile and `host:port`.

## Out of scope
Cancelling a running statement, server-side cursors/paging for very large results, LISTEN/NOTIFY,
COPY, introspection (own spec).

## Known deviations from the reference
- All statement outcomes are kept and errors keep SQLSTATE and position (reference via MCP: last result
  only, "unknown error"). IMP-0008, IMP-0009.
- Missing credentials never open a blocking dialog. IMP-0004.
