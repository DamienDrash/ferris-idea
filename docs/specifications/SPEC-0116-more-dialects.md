# SPEC-0116: More dialects — MariaDB and SQL Server

- **Version:** 7 · **Date:** 2026-11-16 (v7: the CA file is the only trust anchor — IDECraft reads the
  server's certificate chain from the handshake and checks it against the file before the login
  leaves, behaviour 15, AK-11 — SPEC-0016 v4, IMP-0426); 2026-11-16 (v6: TLS with SQL Server works on macOS — the records of a
  handshake flight go to the server as one pre-login message; the server's TLS version is checked; a CA
  file; `prefer` falls back only on the loopback and only after TLS failed, behaviours 2, 13–15, AK-10 —
  SPEC-0016 v3, IMP-0421, finding of PR #869); 2026-11-16 (v5: every result set of a SQL Server batch, batches with variables and control of flow, the encryption named for a login without server rights, behaviours 3 and 13, AK-9 — F-060 run without a screen); 2026-11-15 (v4: acceptance criterion AK-8, hand test 43 in a window of the test harness against SQL Server; no change of behaviour); 2026-11-14 (v3: the test level in the connection dialog, sequences in the catalog, what is verified where, behaviours 10–12 — F-060; v2: 2026-11-03; v1: 2026-11-01)
- **Sources:** GEN (database IDEs connect to many systems, and every database feature of
  SPEC-0005–SPEC-0022 works the same way on each; F-060), STD (T-SQL lexical rules: `[identifier]`
  with `]]`, `"identifier"`, `N'…'` literals, `GO` as the client's batch separator; TDS protocol),
  DOC (MariaDB knowledge base, read for the rules, not quoted: `EXPLAIN FORMAT=JSON` and
  `ANALYZE FORMAT=JSON`, `information_schema` reporting `NULL` as a column's default, column-level
  `REFERENCES` creating foreign keys; Microsoft SQL Server documentation: `SET SHOWPLAN_XML`,
  `SET STATISTICS XML`, the showplan schema, `sys.*` catalog views, default constraints, `string_agg`
  from SQL Server 2017), SPEC-0008 (profiles), SPEC-0009/0010 (sessions), SPEC-0013 (introspection),
  SPEC-0017 (editing), SPEC-0018 (DDL), SPEC-0020 (plans), SPEC-0108 (connection dialog), ADR-0008,
  ADR-0011; v6: STD (TDS pre-login packets: type 0x12, status bit "end of message", 4096-byte packets
  before the login; TLS 1.2 record and ServerHello layout), DOC (Microsoft: `mssql-conf` settings
  `network.tlscert`, `network.tlskey`, `network.tlsprotocols`), SPEC-0016 v3; v7: STD (TLS 1.2
  Certificate message, RFC 5246; X.509 certification paths, RFC 5280), OSS (the driver's
  documentation: a CA file adds to the system's trust store and no setting removes the system's
  roots), SPEC-0016 v4, IMP-0426
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
   `require` encrypts without checking the certificate, `verify-full` checks it. **v6:** `prefer`
   falls back only for a server on the loopback and only when TLS itself failed; on any other host
   it is `require` (SPEC-0016 v3 behaviours 3 and 13). Errors name the
   system (`MariaDB error 1054: …`, `SQL Server error 208 (line 1): …`). v2: MariaDB syntax errors
   get a position as in SPEC-0010 v2; a SQL Server error in a statement of several lines gets the
   position of the first non-blank character of the line the server names. [STD, ADR-0011, IMP-0008]
3. **T-SQL scripts:** statements end at `;` or at a line holding only `GO` (optionally with a
   count); `[…]` and `"…"` are identifiers, `#` starts no comment. Each statement is its own batch;
   statements that change rows report their row count, `OUTPUT` clauses and `EXEC` return their
   rows. v5: a statement that declares a variable or uses control of flow takes the rest of its
   batch with it (SPEC-0005 v6 behaviour 11a), so variables and `TRY … CATCH` work as in `sqlcmd`.
   Every result set a batch returns is an outcome of its own, in order (SPEC-0005 behaviour 18):
   `EXEC` of a procedure with two `SELECT`s, two `SELECT`s of one batch without `;` between them,
   the `SELECT`s inside `IF … BEGIN … END`. A statement that changes rows reports its row count as
   before; a batch of SPEC-0005 behaviour 11a without a result set reports 0 rows, since the
   driver gives the counts of such a batch only without its rows. When a later part of a batch
   fails, the result sets before it are kept and the error follows them; the server sends the
   columns of a query that fails while it runs, so that query is an empty result before the
   error. [STD]
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
13. **v5 – Encryption of a SQL Server connection:** Test Connection says whether the connection
    is encrypted (SPEC-0108 v3). SQL Server tells this only to a login with the right to see the
    server's connections (`sys.dm_exec_connections`); an ordinary login gets no answer there, and
    `CONNECTIONPROPERTY('encrypt_option')` answers it with `NULL`. Then IDECraft names what its
    driver agreed with the server: encrypted for `require`, `verify-ca` and `verify-full`, and for
    `prefer` unless it fell back (v6: on the loopback only); not encrypted for `disable`. The
    server's own answer wins where it gives one. [DOC, SPEC-0016]
14. **v6 – TLS on macOS (finding of PR #869):** in TDS 7 the TLS handshake travels inside pre-login
    messages. The macOS TLS library hands over every record of a handshake flight on its own and
    asks for it to be sent at once; the driver made each of them a pre-login message of its own,
    marked as the end of the message. SQL Server takes one message per flight and closed the
    connection after the client's second flight (key exchange, cipher change, finished as three
    messages), so no TLS came about on macOS: `require` failed with "connection closed", `prefer`
    fell back to plain text. IDECraft now collects the records of a flight and sends them as one
    pre-login message when the client next waits for the server, split into packets of at most 4096
    bytes with only the last marked as the end. Messages without TLS records and everything after
    the handshake pass unchanged. [STD, GEN]
15. **v6 – TLS version and CA file:** before the handshake goes on, IDECraft reads the protocol
    version the server chose in its ServerHello; below TLS 1.2 the connection ends before any login
    data is sent, with an error naming the version and that TLS 1.2 or newer must be enabled on the
    server (SPEC-0016 v2 behaviour 9; the driver itself would accept TLS 1.0). A SQL Server profile
    may name a CA file (PEM, `~` allowed) for `verify-ca` and `verify-full`; the driver trusts its
    certificates in addition to the system's roots (it cannot replace them, unlike the PostgreSQL
    and MySQL drivers, SPEC-0016 v3 behaviour 10). No client certificate; a file that cannot be read
    or holds no certificate fails before anything is sent. The connection dialog shows the CA file
    field for SQL Server on its SSH/SSL page, without the tunnel and client certificate fields.
    [STD, DOC]
    **v7 – The CA file alone (IMP-0426):** the driver offers no way to leave the system's roots out
    and builds its TLS connection itself, so IDECraft checks the anchor in the same place where it
    checks the version: in TLS 1.2 the server sends its certificate chain in the clear, inside the
    pre-login packets of its first flight. IDECraft reads the Certificate message there and checks
    the chain against the CA file alone — the server's certificate must lead, through the
    intermediates it sent, to a certificate of the file (signature, validity period, CA rights,
    server use), or be a certificate of the file itself. A chain that does not passes no further:
    the handshake ends at once, with an error naming the file and that the server's certificate was
    not issued by a certificate in it. When the handshake is over and no chain could be read (TLS
    1.3 encrypts it), the login is held back and the connection ends with an error that says so
    and how to allow TLS 1.2 on the server (`mssql-conf set network.tlsprotocols 1.2`). SQL Server
    uses TLS 1.3 only for TDS 8, which IDECraft does not speak. The host name stays the driver's
    check, as before. Without a CA file nothing changes: the system's roots are the anchor.
    [STD, OSS, rule 12]

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
- **AK-9 (v5, behaviours 3 and 13)** Live against the SQL Server of the reference environment
  (`crates/database/tests/it/dialect_steps_live.rs::sql_server_steps`, skipped without
  `IDECRAFT_TEST_SQLSERVER`): a script with three `GO` batches gives five outcomes, the third
  batch's error with code 207 at its line; Test Connection names SQL Server 16, its version and the
  encryption for the ordinary login `idecraft`; the key is listed under the name `sys.indexes`
  has, a cell is changed and read back, Explain Plan names the key's index with costs, Explain
  Analyze measures one row; `EXEC` of a procedure with two `SELECT`s gives both result sets; a
  variable is read in its batch, `TRY … CATCH` answers from `CATCH`; two schemas of the database
  compare with the added column.
- **AK-10 (v6, behaviours 2, 14, 15)** Unit: a flight of three pre-login messages leaves as one
  with the payloads in order and one header; a flight above 4088 bytes as packets with only the last
  marked as the end; the first pre-login message and everything after the handshake unchanged
  (`tds_tls::tests::a_flight_leaves_as_one_message`, `…::a_long_flight_is_split_at_the_packet_size`,
  `…::other_messages_pass_unchanged`); a ServerHello with TLS 1.0 or 1.1 ends the handshake, TLS 1.2
  passes (`…::old_tls_versions_are_refused`); `prefer` retries without TLS only on the loopback and
  only after a TLS failure (`sqlserver::tests::prefer_retries_only_after_tls_failed`); a CA file is
  accepted for SQL Server and client certificates are not (`profile::tests::tunnel_tls_and_options_are_checked`),
  and its SSH/SSL page shows only the CA file (`connection_dialog::tests::sql_server_shows_only_the_ca_file`).
  Live against the SQL Server of the reference environment on macOS (SPEC-0016 v3 AK-13,
  `tests/tls.rs` `sql_server_modes`).
- **AK-11 (v7, behaviour 15)** Unit: a server certificate signed by the CA of the file passes, the
  file's own server certificate passes, a certificate of another authority — one the system might
  trust — is refused naming the file, an empty chain is refused (`tds_tls::tests::the_ca_file_is_the_only_anchor`);
  the chain is read from a server flight split over two pre-login packets, a refused chain ends the
  handshake on the read, and a handshake without a readable chain holds the first login bytes back
  (`…::the_chain_is_read_from_the_server_flight`, `…::a_chain_outside_the_ca_file_ends_the_handshake`,
  `…::no_login_without_a_checked_chain`); the refusal is the connection's error, whatever the driver
  made of the broken stream (`sqlserver::tests::the_ca_files_refusal_is_the_error`). Live against the SQL Server of the reference environment
  (SPEC-0016 v4 AK-14, `tests/tls.rs` `sql_server_modes`): `verify-full` with the test CA connects to
  `localhost`, with a CA file that did not sign the certificate it fails naming the file.

## Out of scope
Oracle (no pure-Rust driver without the proprietary client libraries), Windows and Azure AD
authentication, named instances via the SQL Browser service, several schemas per SQL Server
database, T-SQL procedural blocks in completion and inspections, `sp_rename` for renaming objects,
MariaDB system-versioned tables and sequences in the catalog; v6: TDS 8 ("strict", TLS before the
pre-login, which needs a certificate configured on the server), client certificates for SQL Server.

## Known deviations from the reference
Read-only SQL Server profiles are enforced by IDECraft, not by the server. The live SQL Server path
is tested where the VM of the reference environment runs (AK-8, AK-9; last run 2026-11-16), else by
unit tests of every generated statement and parser. Not compared side by side yet. Errors point
into the statement where the server names a line or the text it stopped at (IMP-0008). v7: a CA
file is the only trust anchor of a SQL Server connection, as for the other dialects (IMP-0426).
