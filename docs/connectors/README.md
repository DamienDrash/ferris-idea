# Database connectors

IDECraft connects to a database system through a *connector*: a small TOML file that names the
protocol family IDECraft already speaks and what this system does differently. The built-in
systems are such files too ([`crates/database/connectors/`](../../crates/database/connectors/)).
Decisions: [ADR-0020](../adr/0020-database-connectors.md); format:
[SPEC-0300](../specifications/SPEC-0300-database-connector-catalogue.md); sharing:
[SPEC-0302](../specifications/SPEC-0302-connector-import-export.md).

## Status words in the system list

| Badge | Meaning |
|---|---|
| none | **Supported**: tested against a real server of the version the definition names |
| `Beta` | built in, not testable here (cloud only, paid or licence-bound); the note says what is unverified |
| `User` | a file you put into the connectors folder |
| `Imported` | a file brought in with "Import database system…" |

User and imported files never count as Supported, whatever they say.

## Add a system that is not listed

A system that speaks the PostgreSQL, MySQL, MariaDB or SQL Server (TDS) wire protocol needs no code.
Write a file `acme.idcconnector`:

```toml
[idecraft]
kind = "connector"
format = 1
id = "acme"                 # lowercase letters, digits and -
name = "Acme DB"

[connector]
family = "postgresql"       # postgresql, mysql, mariadb, sqlserver, oracle, sqlite, duckdb, odbc
category = "cloud"          # relational, distributed, cloud, analytics, time-series, embedded, other
note = "Speaks the PostgreSQL protocol; catalogue not verified."
aliases = ["acme cloud"]

[connection]
schemes = ["acme"]          # acme://user@host/db selects this system
default_port = 6543
ssl = "require"

[sql]                       # optional; absent keys use the family's
keywords = ["upsert"]
explain = "EXPLAIN {statement}"
plan_format = "text"

[introspection]             # optional catalogue queries; {schema} becomes a quoted literal
schemas = "select nspname, case when nspname = current_schema() then '1' else '0' end from pg_namespace"
```

Then use **Import database system…** at the end of the system list in the connection dialog (or
copy the file into `connectors/` in the IDECraft settings folder, `~/.config/idecraft/connectors/`
by default). IDECraft shows every problem with its key and every query the file would run before
it stores anything. A definition is data: it never names a program, a library or an address
IDECraft would run or fetch, and its queries run on your connection like queries you type, under
the connection's read-only and agent rules.

Column layouts of the `[introspection]` queries:

| Key | Columns |
|---|---|
| `schemas` | name, is current (`1`, `t` or `true`) |
| `columns` | table, `table`/`view`, column, type, `not null`/`null`, default, position, (unused) |
| `keys` | table, constraint, `p`/`f`, columns (comma-separated), referenced table, referenced columns |
| `indexes` | table, index, unique (`1`/`t`), columns (comma-separated) |
| `views` | view, definition |

Systems with a protocol of their own (Snowflake, BigQuery, Db2, …) will use the ODBC route
(`family = "odbc"`). Such files are accepted and listed today, but cannot connect yet.

## Connector plugins

A system with neither a protocol IDECraft speaks nor an ODBC driver can be reached through a
WebAssembly plugin that implements its protocol. The definition says `family = "plugin"` and names
the plugin under `[plugin]`:

```toml
format = 1
id = "acmedb"
name = "AcmeDB"
family = "plugin"
category = "other"

[plugin]
module = "acmedb"      # acmedb.wasm in the plugins folder
lexis = "postgresql"   # SQL family the editor uses for this system (default postgresql)

[connection]
schemes = ["acmedb"]
default_port = 7000
```

The plugin asks for the permission `profile-network`. It can only connect to the host and port of
the connection you pick; IDECraft refuses every other host and port. Before the plugin first
connects to a host, IDECraft warns you:
- which plugin it is and who made it;
- that it gets network access to that host and receives the connection's user name and password;
- its status;
- that IDECraft does not review its code.

Allowing is kept per plugin and host. Revoke it in Settings › Plugins (⇧Delete). Agents see the
same warning through MCP and need a grant of their own. Every connection a plugin opens is written
to a local log (`network-audit.jsonl` in the plugins folder) that never leaves your machine
([SPEC-0314](../specifications/SPEC-0314-plugin-network-capability.md)).

## Share a definition

**Share…** under the chosen system copies the definition and opens a prefilled GitHub issue from the
`connector-definition` template; nothing is sent until you submit it. **Export definition…** saves
the `.idcconnector` file; built-in systems are exported without their tested status, so a shared copy
never claims to be tested. Tools › Database › **Export Connection…** writes a connection as a
`.idcsources` file without password references, agent rights, user name or absolute paths.

When a system misbehaves, **Report a problem with this database…** builds a report you can read and
edit first ([SPEC-0301](../specifications/SPEC-0301-database-problem-report.md)).
