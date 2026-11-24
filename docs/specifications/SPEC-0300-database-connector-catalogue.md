# SPEC-0300: Database connector catalogue and definition format

- **Version:** 3 · **Date:** 2026-11-05 (v2: details settled while implementing — schema rows, settings folder, MCP dialect argument, port hint; v3: family `plugin`, SPEC-0314)
- **Sources:** GEN (database IDEs offer a long list of data source types, grouped and searchable;
  many systems speak the PostgreSQL, MySQL or TDS wire protocol and differ in catalogue and lexis),
  STD (PostgreSQL frontend/backend protocol, MySQL client/server protocol, TDS, `information_schema`
  of SQL:2016, TOML 1.0), DOC (vendor documentation of the systems listed in behaviour 6, read for
  default ports, URL schemes, wire compatibility and catalogue differences, not quoted), ADR-0007,
  ADR-0008, ADR-0011, ADR-0016, ADR-0020, SPEC-0008 (profiles), SPEC-0013 (introspection),
  SPEC-0014 (completion), SPEC-0020 (plans), SPEC-0108 (connection dialog)
- **Feature matrix rows:** F-060, F-051

## Purpose
The user picks a database system from one searchable, grouped list and connects to it; each entry
says whether Ferris IDEA tests it (Supported) or only builds it (Beta). Systems that speak a protocol
Ferris IDEA already has are added as declarative connector definitions — the built-in ones and those
the user writes or imports.

## Behaviour
1. **Connector, family, profile.** A connector is one database system. It belongs to exactly one
   protocol family (`postgresql`, `mysql`, `mariadb`, `sqlserver`, `oracle`, `sqlite`, `duckdb`,
   later `odbc`, and `plugin` for connector plugins, SPEC-0314); the family decides driver, session, script splitting and base lexis. A
   connection profile keeps `dialect` (the family) and gains an optional `system` (connector id).
   Without `system` the profile is the family's own system. Opening a session for a profile whose
   connector is unknown fails with "unknown database system `<id>`; import its definition" before
   any network access. [ADR-0020, SPEC-0008]
2. **Catalogue.** The catalogue is the built-in definitions (compiled in) followed by the files in
   `connectors/*.toml` in the Ferris IDEA settings folder (`$FERRIS_IDEA_CONFIG_DIR`, else
   `$XDG_CONFIG_HOME/ferris-idea`, else `~/.config/ferris-idea`; `FERRIS_IDEA_CONNECTORS_DIR` overrides the
   folder for tests). A file whose id equals a built-in id is refused (built-ins cannot be shadowed); of
   two user files with the same id, the first in name order wins and the other is reported as a
   problem. Files that do not validate are listed as problems with their path and messages and do
   not appear as systems. [GEN]
3. **Definition format** (`format = 1`, TOML; unknown keys are errors so typos surface):
   ```toml
   format = 1
   id = "cockroachdb"                 # [a-z0-9][a-z0-9-]{1,40}
   name = "CockroachDB"
   family = "postgresql"
   category = "distributed"           # behaviour 5
   status = "beta"                    # built-ins only: supported | beta; files: ignored, see 4
   note = "Catalogue and plans not verified against a server."   # one line, ≤ 160 chars
   aliases = ["crdb", "cockroach"]    # search terms
   homepage = "https://www.cockroachlabs.com/"   # optional, shown, never fetched
report_url = "https://…?title={title}&body={body}"   # optional, SPEC-0301
   tested = "v25.2 (local container)" # built-ins with status supported: version tested

   [connection]
   schemes = ["cockroachdb"]          # URL schemes that select this connector
   default_port = 26257
   default_database = "defaultdb"     # optional
   default_user = "root"              # optional
   ssl = "require"                    # optional default SSL mode for new profiles
   fields = ["host", "port", "database", "user", "password", "ssl"]   # form fields shown

   [sql]                              # all optional; absent keys inherit from the family
   identifier_quote = "\""            # one of " ` [
   keywords = ["UPSERT", "INTERLEAVE"]   # added to the family's completion keywords
   explain = "EXPLAIN {statement}"    # estimate; {statement} is replaced
   explain_analyze = "EXPLAIN ANALYZE {statement}"
   plan_format = "text"               # family (family parser) | text (shown as lines)
   test_query = "SELECT 1"
   version_query = "SELECT version()"
   current_schema = "SELECT current_schema()"

   [introspection]                    # all optional; absent queries inherit from the family
   schemas = "SELECT …"               # name, is_current (1, t or true)
   columns = "SELECT …"               # table, kind, column, type, not null, default, position, pk position
   keys = "SELECT …"                  # table, constraint, p|f, columns, referenced table, referenced columns
   indexes = "SELECT …"               # table, index, unique, columns
   views = "SELECT …"                 # view name, definition
   ```
   `{schema}` in an introspection query is replaced by the schema as a quoted SQL literal (single
   quotes doubled); a query without `{schema}` reads only the current schema. Columns are read by
   position; a result with fewer columns than listed is an introspection error naming the query.
   [GEN, SPEC-0013]
4. **Validation** (on load and on import) reports every problem at once, each with its key path:
   `format` missing or not 1; id pattern; `name` empty or over 60 characters; unknown `family`,
   `category`, `plan_format`; `family = "odbc"` is accepted and marked "needs the ODBC route" (not
   connectable yet, ADR-0020); `default_port` outside 1–65535; a scheme that is not lowercase
   letters, digits, `+`, `-`, `.`, or that is already taken by a family (`postgresql`, `postgres`,
   `mysql`, `mariadb`, `sqlserver`, `mssql`, `oracle`, `sqlite`, `duckdb`) or by another connector;
   `fields` with unknown names or without `host` for a server family; `identifier_quote` other than
   the three; `homepage` or `report_url` not `https://`; `explain…` without `{statement}`; any SQL text over 16 KiB; an introspection query
   that is not a single statement starting with `SELECT`, `WITH`, `SHOW` or `PRAGMA` (SPEC-0005
   splitter); keys outside the format. Files over 64 KiB are refused unread. A definition is data:
   no key names a program, library, script or URL that Ferris IDEA would run or fetch. The `status`
   of a user or imported file is never `supported`: it reads as **User** (written here) or
   **Imported** (brought in by import, SPEC-0302), whatever the file says. [ADR-0020, ADR-0007]
5. **Categories**, in this order: Relational, Distributed SQL, Cloud, Analytics, Time series,
   Embedded, Other. Each connector has one. [GEN]
6. **Initial catalogue.** Family systems (Supported, tested before this spec): PostgreSQL, MySQL,
   MariaDB, SQL Server, Oracle, SQLite. Added by this spec:
   | System | Family | Category | Target status | Why |
   |---|---|---|---|---|
   | DuckDB | duckdb | Embedded | Supported | embedded, tested in CI |
   | TimescaleDB | postgresql | Time series | Supported | free container, Apache-2.0 core |
   | YugabyteDB (YSQL) | postgresql | Distributed SQL | Supported | free container, Apache-2.0 |
   | TiDB | mysql | Distributed SQL | Supported | free container, Apache-2.0 |
   | CockroachDB | postgresql | Distributed SQL | Supported | licence accepted for tests (question 24, SPEC-0304) |
   | Greenplum | postgresql | Analytics | Beta | no maintained free build to test |
   | Amazon Redshift | postgresql | Cloud | Beta | cloud only |
   | Amazon Aurora PostgreSQL | postgresql | Cloud | Beta | cloud only |
   | Google AlloyDB | postgresql | Cloud | Beta | cloud only |
   | Neon | postgresql | Cloud | Beta | cloud only |
   | Supabase | postgresql | Cloud | Beta | cloud service (PostgreSQL inside) |
   | Materialize | postgresql | Analytics | Beta | source-available licence, not tested |
   | Amazon Aurora MySQL | mysql | Cloud | Beta | cloud only |
   | PlanetScale (Vitess) | mysql | Cloud | Beta | cloud only |
   | SingleStore | mysql | Analytics | Beta | licence key needed |
   | StarRocks | mysql | Analytics | Beta | not tested yet |
   | Apache Doris | mysql | Analytics | Beta | not tested yet |
   | Azure SQL Database | sqlserver | Cloud | Beta | cloud only |
   | Azure SQL Managed Instance | sqlserver | Cloud | Beta | cloud only |
   | Azure Synapse (dedicated SQL pool) | sqlserver | Cloud | Beta | cloud only |
   A "Supported" target becomes `status = "supported"` only with a passing live test against a
   real server; until then it ships as Beta. [GEN, DOC]
7. **New Data Source list** (connection dialog, SPEC-0108): the "database system" field opens a
   list of all connectors, grouped by category in the order of behaviour 5, alphabetical inside a
   group; the family systems first in Relational. Typing filters by name, id and aliases
   (case-insensitive, substring). Each row shows the name and, right-aligned, a badge: none for
   Supported, `Beta`, `User` or `Imported`; a Beta or User row shows its note as the row's tooltip
   and as a line under the form when selected. Up/Down move, Enter picks, Escape closes the list.
   Picking a connector sets the family, and database, user and SSL mode where the form is still
   empty; the port hint names the connector's default port; only its fields are shown. A profile saved from it has `system` set. [GEN, ADR-0009]
8. **URLs.** A connector's schemes select it in `ConnectionProfile::from_url` (`tidb://user@h/db`
   gives family `mysql`, system `tidb`); `to_url` writes the connector's first scheme. The family's
   SSL query key is used. [SPEC-0008]
9. **Sessions and features.** A connector session is its family's session; `Session::system()`
   names the connector. Introspection, view definitions, current schema, completion keywords,
   quoting, plans and the connection test use the connector's overrides where given, else the
   family's. Every other feature (editing, DDL, export, refactoring) is the family's. [ADR-0020]
10. **MCP.** `list_connectors` returns every connector with id, name, family, category, status,
    note, schemes, default port and origin (built-in, user, imported) plus the problems of invalid
    files; `save_connection` accepts `system`, or a connector id as `dialect`, and refuses a system
    of another family; `list_connections` names each profile's `system`. [ADR-0008]

## Acceptance criteria
- **AK-1** Every built-in definition validates; ids and schemes are unique; categories are known;
  statuses are supported or beta, supported ones name `tested` (unit tests `connector::tests`).
- **AK-2** Validation reports all problems of a broken file at once, with key paths: unknown key,
  wrong family, bad port, taken scheme, explain without `{statement}`, a write statement as an
  introspection query, a 70 KiB file; user files never read as supported; built-in ids cannot be
  shadowed (unit tests).
- **AK-3** Profiles: `tidb://`, `cockroachdb://`, `duckdb:` URLs to profiles and back; profiles
  without `system` load as before; unknown system refused before connecting (unit tests).
- **AK-4** Catalogue from a folder: valid, invalid and duplicate files; search by alias; grouping
  order (unit tests with a temporary folder).
- **AK-5** Introspection through override queries with `{schema}` against a live server
  (TimescaleDB container, `crates/database/tests/it/connectors.rs`, `FERRIS_IDEA_TEST_TIMESCALE`).
- **AK-6** MCP `list_connectors` and `save_connection` with `system` (protocol test).
- **AK-7** Window self-test steps: the system list opens, typing "tidb" leaves TiDB, Enter picks
  it (MySQL family, port hint 4000), Redshift shows the Beta badge, Escape closes list and dialog.

## Out of scope
The ODBC, Flight SQL and WASM routes (ADR-0020, later specs); driver properties and SSH tunnels;
importing data sources of the reference; downloading definitions from a registry.

## Known deviations from the reference
Every system shows whether it is tested (Supported/Beta) instead of all looking equally supported
(IMP-0284). Systems are plain, shareable text files instead of driver bundles (IMP-0285).
