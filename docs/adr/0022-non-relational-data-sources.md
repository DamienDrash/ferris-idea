# ADR-0022: Non-relational data sources — store families beside the SQL dialects

- **Status:** accepted · 2026-11-05 · extends ADR-0011 (native driver per system) and ADR-0020
  (connectors, catalogue, Supported/Beta), follows ADR-0021 (file envelope) · decided on the project owner's request of 2026-11-05 ("offer NoSQL,
  worked out by a dedicated agent"; feedback for Beta systems and shared definitions through GitHub
  Issues)
- **Context:** IDECraft's database tooling is built around SQL: a `Dialect` decides driver, script
  splitting, lexis, introspection into tables/columns/keys, plans, DDL and refactorings, and every
  result is a grid of text cells (SPEC-0007). The reference lists six non-relational systems
  (OBS-0015): MongoDB, Amazon DocumentDB, Amazon DynamoDB, Apache Cassandra, Couchbase Server and
  Redis. None of them has tables with a fixed column list, none speaks SQL in the SQL sense, and
  their values are nested (documents) or typed per key (strings, hashes, lists, sets, sorted sets,
  streams). Squeezing them into `Dialect` would make every SQL feature carry "does not apply" arms
  and would hide nesting behind text cells.

## Decision

**1. Non-relational systems are *store families*, not SQL dialects.** A store family owns its driver,
session, browser tree, query language, completion and result model. Two families now, more later:

| Family | Data model | First system | Driver | Later systems of the family |
|---|---|---|---|---|
| `document` | databases → collections → documents (BSON/JSON trees) | MongoDB | official `mongodb` crate (Apache-2.0) | Amazon DocumentDB (MongoDB wire, cloud only → Beta), Azure Cosmos DB for MongoDB (Beta), FerretDB (MongoDB wire on PostgreSQL, free → Supported once tested) |
| `key-value` | numbered databases → keys (grouped by a separator) → typed values | Redis | `redis` crate (BSD-3-Clause) | Valkey and KeyDB (Redis protocol, free → Supported once tested), Amazon ElastiCache/MemoryDB (Beta), DragonflyDB (source-available, Beta) |
| `wide-column` (later spec) | keyspaces → tables with partition and clustering keys; CQL | Apache Cassandra | `scylla` crate (Apache-2.0/MIT) | ScyllaDB, Amazon Keyspaces (Beta) |
| `document` over HTTP (later spec) | buckets → scopes → collections; SQL++ | Couchbase Server | REST/N1QL over HTTP, no official Rust SDK | — (Beta until a client exists) |
| `key-value` over HTTP (later spec) | tables → items; PartiQL | Amazon DynamoDB | AWS SDK for Rust (Apache-2.0) | DynamoDB Local (free container → Supported possible) |

The SQL `Dialect` enum stays SQL-only. A crate `idecraft-nosql` holds the families, beside
`idecraft-database`, and reuses from it what is not SQL-specific: the keychain store and credential
resolution (SPEC-0016), the agent permissions (SPEC-0012), the export file writer conventions
(SPEC-0021) and the TLS rule (no TLS mode silently weakened).

**2. Profiles.** A non-relational connection is a *store profile* in its own workspace file
`.idecraft/stores.toml` (array `[[store]]`), a project file with the envelope of ADR-0021
(`[idecraft] kind = "stores"`, `format = 1`), shared like `connections.toml` and never holding a
password. A separate file keeps the SQL profile reader (`deny_unknown_fields`) and every older
IDECraft untouched; the Database tool window and MCP list both files together. A store profile has `id`, `name`,
`family` (`document` | `key-value`), `system` (connector id, `mongodb` | `redis` now), the URL it was
made from minus any password, `tls`, `read_only`, `secret` (keychain entry) and `ai` permissions.
URL schemes: `mongodb://`, `mongodb+srv://` (DNS seed list), `redis://`, `rediss://` (TLS). A
password in a URL is refused with the same message as for SQL profiles; it goes to the keychain.

**3. Catalogue.** The connector catalogue of ADR-0020 gains two categories at its end, *Document*
and *Key-value* (and later *Wide column*). MongoDB and Redis are built-in connectors with
`family = "mongodb"` / `"redis"` (the store family's driver), status **Supported** once a live test
against the official container image passes, with the tested version in `tested`. DocumentDB,
DynamoDB, Cassandra and Couchbase are listed as **Beta** with a note "not connectable yet — planned
(ADR-0022)" until their spec lands; Valkey is a Redis connector (Beta until tested). The report
function for Beta systems and shared definitions goes to GitHub Issues (project owner, 2026-11-05;
SPEC-0301).

**4. Browser tree** (Database tool window, SPEC-0089): for a document store
*connection → databases → collections → (indexes, sample-derived fields)*; for a key-value store
*connection → databases db0…dbN (only non-empty ones, plus the profile's own) → key groups by the
`:` separator → keys with their type and TTL*. Nothing is listed eagerly beyond one level: key spaces
are scanned with cursors (`SCAN`), never `KEYS *`, in pages, with a visible "more…" row.

**5. Query languages.**
- *Document:* a console that accepts the common shell forms
  `db.<collection>.find(<filter>, <projection>)` with `.sort()`, `.limit()`, `.skip()`,
  `countDocuments`, `aggregate([…])`, `insertOne/Many`, `updateOne/Many`, `deleteOne/Many`,
  `distinct`, `createIndex`, `drop`, plus `show dbs`, `show collections`, `use <db>`, and
  `db.runCommand({…})`. Arguments are relaxed JSON / Extended JSON (`ObjectId("…")`, `ISODate("…")`,
  unquoted keys, single quotes, trailing commas). It is **not** a JavaScript interpreter: no
  variables, loops or functions; anything else is refused with what is accepted. This keeps the
  console safe for agents (ADR-0008) and needs no JS engine.
- *Key-value:* Redis commands as typed in `redis-cli` (quoting by its rules), one per line.
  Commands are classified read / write / admin by a table; admin and dangerous commands
  (`FLUSHALL`, `FLUSHDB`, `CONFIG SET`, `SHUTDOWN`, `DEBUG`, `KEYS` on large spaces) need
  confirmation or the `write`/`admin` right.
- Both consoles complete: collection names, operators (`$gt`, `$in`, `$set`, aggregation stages),
  field names sampled from the collection; Redis command names with argument hints, and key names
  by prefix (cursor scan, bounded).

**6. Result display.** Results are not forced into SPEC-0007 text grids. A *document result* is a
list of documents shown either as a **tree** (expandable fields, type per value) or as a **grid**
whose columns are the union of top-level fields of the page (nested values as compact JSON), or as
raw **JSON**. A *key-value result* shows the value by type: string (text, JSON pretty-printed when it
parses), hash (field/value grid), list (index/value), set (members), sorted set (member/score),
stream (id/fields); every command reply also has its raw RESP form. Editing goes through explicit
operations (replace one document by `_id`, set a field; set/expire/rename/delete a key; add/remove
hash fields and list/set members), each previewed as the command that will run.

**7. Agents.** Every capability has MCP tools (ADR-0008): list stores, browse, run a console
command, read and write documents and keys, export — under the profile's `ai` rights and read-only
flag, with refusals that name the missing right.

## Consequences

- Two drivers with their own trees of dependencies (the `mongodb` crate brings BSON, a DNS resolver
  for `mongodb+srv`, and rustls; `redis` is small). Builds take longer once; both are permissively
  licensed.
- SQL features (plans, DDL, refactorings, ER diagrams) do not exist for stores, by design. Features
  that both have (tree, console, export, MCP) are implemented per family behind small shared types.
- Data sources live in two files (`connections.toml`, `stores.toml`); ids are unique across both,
  which the savers check.
- The Mongo console accepts a well-defined subset of the shell; users who need scripts use the
  shell itself. The subset grows only through the spec.
- Systems of later families are visible in the catalogue as Beta "planned" entries, so the list is
  honest about what connects.
