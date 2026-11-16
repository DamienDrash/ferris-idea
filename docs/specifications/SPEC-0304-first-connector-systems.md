# SPEC-0304: First connector systems — tested family members and DuckDB

- **Version:** 2 · **Date:** 2026-11-05 (v2: CockroachDB Supported after PO-STATUS question 24)
- **Sources:** GEN, OBS-0015 (list of systems and entry points of the reference), DOC (vendor documentation of TimescaleDB, YugabyteDB, TiDB and DuckDB, read for
  default ports, catalogue support and plan statements, not quoted), OSS (`duckdb` crate, MIT, and
  the DuckDB engine, MIT), ADR-0011, ADR-0020, SPEC-0005, SPEC-0007, SPEC-0011, SPEC-0013,
  SPEC-0020, SPEC-0300
- **Feature matrix rows:** F-060

## Purpose
The first systems of the catalogue become Supported because they pass the same live test against a
real server; DuckDB, an embedded analytical database, joins as a family of its own.

## Behaviour
1. **Live test per family system.** One test (`crates/database/tests/it/connectors.rs`) connects with
   a profile from the system's URL, runs its test and version query, and in a schema (PostgreSQL
   family) or database (MySQL family) of its own creates two tables with a primary key, a named
   foreign key, a default, an index and a view, inserts rows, then checks: current schema, the
   schema list with the current one marked, introspection (columns in order, not-null, primary key,
   foreign key with referenced table, index), the view definition, an estimated plan with at least one
   step, and a measured plan of an `UPDATE` that is rolled back (the row is unchanged afterwards).
   It drops its schema at the end. [SPEC-0013, SPEC-0020]
2. **Servers.** `connectors.sh start|stop|status|env <system>` of the reference environment runs each server
   as a container in the Lima VM `idecraft-docker`, bound to 127.0.0.1, with the user `idecraft`
   whose generated password lives in a 0600 file and travels through standard input, never on a
   command line: TimescaleDB (`timescale/timescaledb:latest-pg17-oss`, the Apache-2.0 build) on
   54330, TiDB (`pingcap/tidb:v8.5.3`, single process) on 4000, YugabyteDB
   (`yugabytedb/yugabyte:2.25.2.0-b359`, YSQL authentication off in the container) on 5433,
   CockroachDB (`cockroachdb/cockroach:v26.3.5`) on 26257 under the conditions of behaviour 7.
   `connectors.sh test <system>` starts a fresh container, runs the live test and removes it. [GEN]
3. **Promotion.** A system becomes `status = "supported"` with `tested` naming the server version
   once behaviour 1 passes: TimescaleDB 2.31 on PostgreSQL 17, TiDB v8.5.3, YugabyteDB 2.25.2 (YSQL,
   PostgreSQL 15), CockroachDB v26.3.5 (behaviour 7). [ADR-0020]
4. **DuckDB family.** Dialect `duckdb`; profiles `duckdb:/path/file.duckdb` (like SQLite: a file,
   no server fields, never created implicitly; read-only profiles open the file read-only).
   Extensions are never installed or loaded automatically (autoload off). Lexis as PostgreSQL (double-quoted
   identifiers, `--` and `/* */` comments, `$tag$` strings). Values arrive as text (lists as
   `[a, b]`, blobs as hex, timestamps as ISO text). Catalogue through `information_schema` and
   `duckdb_constraints()`, `duckdb_indexes()` and `duckdb_views()`; plans with `EXPLAIN` /
   `EXPLAIN ANALYZE` read as text plans (one step per line of the drawn plan); editing, export and
   DDL follow the PostgreSQL rules, except: foreign keys only inside `CREATE TABLE` (DuckDB cannot add
   or drop them later), no renaming of indexes or constraints, and no savepoints, so the table
   editor's Manual mode is not available. A column of type `NULL` reads as NULL. The connector
   `duckdb` is Embedded and Supported (its tests need no server and run in CI). The `duckdb` crate is
   used without its download feature, so building never fetches anything. [OSS, DOC]
6. **Systems the reference lists (OBS-0015).** Added as Beta so the list is at least as broad:
   IBM Db2, SAP ASE (Sybase), Exasol, Vertica, Apache Hive, Snowflake and Google BigQuery with
   `family = "odbc"` (listed with their URL scheme and port, not connectable until the ODBC route
   exists; the note says so), and ClickHouse through its MySQL interface (MySQL family, port 9004,
   catalogue not verified). Not added: SQL Server LocalDB (Windows only), H2, HSQLDB and Derby (need
   a JVM). Entry points the reference has and IDECraft not yet — a data source from the clipboard, a
   DDL-only source without a server, demo databases, quick buttons for the most used systems in the
   empty Database tool window, cloud explorers — are follow-ups outside this spec. [OBS-0015]
5. **Licence.** `duckdb` and its dependencies are permissively licensed (MIT, Apache-2.0); `cargo
   deny check licenses` passes without a new exception. [OSS]

7. **CockroachDB under its licence.** The project owner accepted the CockroachDB Software License for
   development and tests on 2026-11-05 (PO-STATUS question 24, `docs/legal/COCKROACHDB_LICENCE.md`).
   The server runs only as the licence's Limited Non-Production Use: one node
   (`start-single-node`), no account, no licence key, insecure mode, user `idecraft` without a
   password, and a fresh container for every start. It is attached only to the internal Docker
   network `idc-crdb-internal`, which has no route out. Before each start a probe container on that
   network must fail to resolve a public name and to reach a public address, or the script refuses to
   start the server. A socat container on that network and the default bridge publishes the SQL port
   on 127.0.0.1. If a key ever becomes necessary or the server does not start without egress,
   CockroachDB goes back to Beta. [DOC, ADR-0007]

## Acceptance criteria
- **AK-1** Live tests pass against TimescaleDB, TiDB, YugabyteDB and CockroachDB (`connectors.rs` with
  `IDECRAFT_TEST_{TIMESCALE,TIDB,YUGABYTE,COCKROACH}`); without the variables they pass without doing
  anything.
- **AK-2** The built-in definitions of the four are `supported` with `tested`
  (`connector::tests::live_tested_family_systems_are_supported`).
- **AK-6** CockroachDB runs without outbound network: `connectors.sh test cockroachdb` checks the
  internal network before starting, and the server log reports no licence type and no telemetry
  requirement.
- **AK-3** DuckDB: a script with a table, a key, an index, a view, a list value and a measured
  plan runs; the file is not created when missing; read-only refuses a write; introspection reads
  the keys (`crates/database/tests/it/duckdb.rs`, a temporary file, runs everywhere).
- **AK-4** `cargo deny check licenses` green.
- **AK-5** The OBS-0015 systems are listed as Beta; the ODBC ones are not connectable and say so
  (`connector::tests::built_ins_are_valid_unique_and_honest`).

## Out of scope
ClickHouse's native and HTTP protocols, MotherDuck (cloud, needs extensions), DuckDB extensions, the
ODBC route itself (next connector spec), the entry points named in behaviour 6.

## Known deviations from the reference
Each listed system says whether it was tested against a real server and which version
(IMP-0284).
