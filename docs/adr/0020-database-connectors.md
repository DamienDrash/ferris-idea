# ADR-0020: Database connectors — protocol families, declarative definitions, one generic route

- **Status:** accepted · 2026-11-05 · extends ADR-0011 (native driver per database) · decided on
  the project owner's request of 2026-11-05 ("far more database systems; untestable ones as Beta with a feedback
  function; users add and share their own")
- **Context:** Ferris IDEA speaks to six systems (PostgreSQL, MySQL, MariaDB, SQL Server, Oracle,
  SQLite; `Dialect` in `crates/database`), each through a native Rust driver behind one session
  interface (ADR-0011). Database IDEs list dozens of systems. Most of them do not need a new driver:
  they speak a wire protocol we already have. What differs is the catalogue (where tables, keys and
  indexes are listed), a few lexical rules, the plan statement and the default port. A small project
  cannot test paid or cloud-only systems, and must not send anything home (ADR-0007). Users must be
  able to add a system nobody listed and share it without writing Rust.

## Decision

**1. A system is a connector, a connector belongs to a protocol family.** `Dialect` stays what it
is today: the *protocol family* — driver, session, script splitter and base lexis. A **connector**
names one database system on top of a family: id, display name, category, status, URL schemes,
default port, fields of the connection form and, where the system differs from its family, its own
catalogue queries, keywords, quoting and plan statement. A connection profile keeps `dialect` (the
family) and gains an optional `system` (the connector id); a profile without `system` is the
family's own system, so every existing profile stays valid.

| Family | Driver | Systems added as connectors |
|---|---|---|
| PostgreSQL wire | `tokio-postgres` | CockroachDB, YugabyteDB (YSQL), TimescaleDB, Greenplum, Amazon Redshift, Materialize, AlloyDB, Aurora PostgreSQL, Neon, Supabase, CrateDB, QuestDB |
| MySQL wire | `mysql_async` | TiDB, SingleStore, Vitess/PlanetScale, Aurora MySQL, StarRocks, Doris, Manticore |
| TDS | `tiberius` | Azure SQL Database, Azure SQL Managed Instance, Azure Synapse (dedicated SQL pool) |
| Embedded | `rusqlite`, `duckdb` | SQLite variants (libSQL file, SQLCipher not), DuckDB (new family) |

**2. Embedded and file systems are families of their own** when a permissively licensed Rust crate
exists: DuckDB through the `duckdb` crate (MIT, bundled engine) as a new family `duckdb` with the
PostgreSQL-like lexis it shares. H2 and HSQLDB run only in a JVM; they are not embedded and stay out
(the generic route below can reach H2's PostgreSQL-compatible server mode as a PostgreSQL connector).

**3. Connectors are declarative files, the built-in ones included.** One format, versioned, in TOML
(`format = 1`), describes a connector completely (SPEC-0300 behaviour 3): family, URL template and
fields, default port, introspection queries for schemas, tables, columns, keys and indexes in the
column layout the catalogue reader already expects, splitter hints, completion keywords, quoting,
plan statement and a test query. Ferris IDEA's own connectors are such files, compiled in, so the
format is exercised by every built-in system. Users and the community add files to
`~/.config/ferris-idea/connectors/`; importing validates first and explains every problem with its
key path. **A definition never executes code**: it holds data and SQL text that runs on the user's
own connection exactly like a query they typed, under the profile's read-only and agent rules. Only
a WebAssembly plugin can contribute code, and only through the plugin permission model (ADR-0016).

**4. Generic route for systems with a protocol of their own: ODBC as the default.** Evaluated:

| Route | Reach | Cost and risk | Verdict |
|---|---|---|---|
| ODBC (driver manager on the user's machine: unixODBC, iODBC; Windows built in) | Widest: Snowflake, Databricks, BigQuery, Db2, Teradata, SAP HANA, Vertica, Exasol, Informix, Firebird and most vendors ship ODBC drivers | `odbc-api` (MIT); the driver manager is loaded from the user's system at run time, not shipped; values arrive as text, errors carry SQLSTATE — fits ADR-0011's session contract | **Default** generic route, next connector milestone |
| Arrow Flight SQL / ADBC | Growing (Dremio, DuckDB, Snowflake, BigQuery, Doris via ADBC drivers) | Flight SQL needs gRPC (`tonic`, `prost`) and Arrow in the build; ADBC needs a C driver manager and per-system native drivers the user installs; columnar results must be turned into text per type | Second route, for systems that offer only Flight SQL; reconsidered when ADBC drivers are packaged widely |
| WASM connector plugin (SPEC-0126, SPEC-0156, SPEC-0210) | Any protocol someone implements in Rust | The plugin ABI has no network host function by design; a connector plugin needs a new reviewed capability ("connect to the host and port of a profile the user chose") and an async socket bridge into the interpreter; slow for large results | Kept as the code route for community connectors; the capability is the project owner's decision before any implementation (open question) |

ODBC wins as the default because it reaches the most systems without adding a protocol, puts the
vendor's tested driver between Ferris IDEA and the server, and installs nothing behind the user's back:
a definition with `family = "odbc"` names the driver, and the connection dialog says when the driver
manager or driver is missing. Until the ODBC session exists, such definitions validate and are
listed but cannot connect (status "needs the ODBC route").

**5. Every system shows its status.** **Supported:** built-in, tested in CI or the local reference
environment against a real server of the stated version. **Beta:** built in but not testable here
(paid, cloud-only or licence-bound); the New Data Source list shows a badge and a one-line note of
what is unverified, and offers "Report a problem with this database" (SPEC-0301). **User** and
**Imported:** from the connectors folder, marked as such; Ferris IDEA makes no statement about them.

**6. Feedback without telemetry.** Nothing leaves the machine by itself (ADR-0007). A report is built
locally, shown and editable, then copied or opened as a prefilled issue in the browser; its
destination is configurable and its default is the project owner's decision (SPEC-0301, open question).

**7. Agents** list, validate, import and export connectors through MCP; import needs the `write`
right and shows the same validation (ADR-0008, SPEC-0302).

## Consequences

- One new family (DuckDB) adds a dependency with a C++ engine compiled from source (longer first
  build, larger binary); every other new system costs one TOML file and, where its catalogue
  differs, a few queries.
- Connectors of a family inherit the family's features (editing, DDL, plans, completion). A system
  that rejects a family feature (e.g. a catalogue view it lacks) needs override queries; until
  someone tests it, it is Beta and the note says so.
- Profiles gain one optional key; older Ferris IDEA versions refuse profiles that use it (the file has
  `deny_unknown_fields`), which is acceptable before the first release.
- The definition format is public and versioned; changes that break old files need `format = 2`
  and a reader for both.
- Systems whose licence needs an acceptance (CockroachDB since 2024 — accepted for tests 2026-11-05, SQL Server, Oracle) are tested
  only after the project owner accepts that licence for the test machine; until then they stay Beta.
