# SPEC-0310: Non-relational stores — connections, browser tree, catalogue, MCP

- **Version:** 4 · **Date:** 2026-11-16 (v4: the dialog's SSL mode `prefer` gives TLS without certificate checks for a store off the loopback and plain text only on the loopback, behaviour 3, AK-10 — SPEC-0016 v3, IMP-0421); 2026-11-14 (v3: for agents the rights of a store are the user's grants of SPEC-0012 v3 — kept outside the project, nothing granted by default, `ai` only an upper limit, no store usable in safe mode, changing commands need `apply: true`, the log holds no command text; behaviour 8; v2 2026-11-05: v2: the connection dialog builds the store URL from its fields; MCP `run_store_command` takes the database; catalogue keys of the planned systems)
- **Sources:** GEN (database IDEs list document and key-value stores beside SQL databases and browse
  them as a tree; connection strings of MongoDB and Redis are URLs), STD (MongoDB connection string
  format incl. the `mongodb+srv` DNS seed list; Redis URL scheme `redis://` / `rediss://` with the
  database number as path; RESP), DOC (MongoDB and Redis server manuals: default ports 27017 and 6379,
  `authSource`, `tls`/`ssl` options, `SCAN` semantics — read, not quoted), OBS-0015 (the reference
  lists MongoDB, DocumentDB, DynamoDB, Cassandra, Couchbase and Redis side by side with SQL systems,
  without saying which are tested), ADR-0007, ADR-0008, ADR-0020, ADR-0022, SPEC-0008, SPEC-0012,
  SPEC-0016, SPEC-0089, SPEC-0300, ADR-0021
- **Feature matrix rows:** F-051, F-060

## Purpose
The user adds a MongoDB or Redis connection from a URL, sees its databases, collections and keys in
the Database tool window, and agents do the same through MCP — under the same keychain, read-only and
permission rules as SQL connections.

## Behaviour
1. **Store profiles** live in `.idecraft/stores.toml`, which starts with the ADR-0021 envelope
   `[idecraft] kind = "stores"`, `format = 1` (a wrong kind is refused; a newer format is refused
   with "written by a newer IDECraft, format n; this version reads up to 1"; a file without the
   envelope is read as format 1 and gets it on the next save), followed by `[[store]]` entries: `id`, `name`,
   `system` (`mongodb` | `redis`), `url` (never with a password), `read_only`, `secret` (keychain
   entry, same service as SQL profiles) and `ai` (SPEC-0012 permissions, reading only by default).
   Unknown keys are errors. Ids and names are unique within the file and must not equal an id of
   `connections.toml`. [ADR-0022, SPEC-0008]
2. **URLs.** `mongodb://[user@]host[:port][,host2…]/[db][?options]` and `mongodb+srv://user@cluster/…`
   select MongoDB; `redis://[user@]host[:port][/db]` and `rediss://…` select Redis. The scheme is
   case-insensitive. A password in the user info (`user:pw@`) or as a query option (`password=`) is
   refused with the SQL profile's message; the password is stored in the keychain. Port defaults are
   27017 and 6379; `mongodb+srv` takes no port. The Redis path is a database number 0–15 (default 0);
   anything else is an error naming the path. [STD]
3. **TLS.** `rediss://`, `mongodb+srv://` (TLS on unless `tls=false`) and `tls=true`/`ssl=true` on
   `mongodb://` mean TLS with certificate and host name checked. `tlsAllowInvalidCertificates=true`
   (MongoDB) and `rediss://…?insecure=true` (Redis) turn checking off and the tree row says
   "TLS, certificate not checked". TLS is never weakened silently: a TLS profile does not fall back to
   plain text. [STD, SPEC-0016] **v4:** in the connection dialog the SSL mode `prefer` becomes a URL
   with TLS without certificate checks (as `require`: `tls=true&tlsAllowInvalidCertificates=true`,
   `rediss://…?insecure=true`) when the host is off the loopback, and a plain URL only for a host on
   the loopback (`localhost`, 127.0.0.0/8, `::1`; SPEC-0016 v3 behaviour 3). Up to v3 `prefer` always
   became a plain URL, also for a store on another machine. [SPEC-0016, rule 12]
4. **Connect.** Opening a store session reads the password from the keychain without waiting for
   input; a missing entry fails before any network access with "password missing in keychain entry
   <service>/<account>". Connecting and the first command are bounded by 10 s; failures name the
   profile and the address. A store opened read-only refuses every command that is not a read
   (classification in SPEC-0311 behaviour 9 and SPEC-0312 behaviour 5). [SPEC-0009, SPEC-0016]
5. **Browser tree** (SPEC-0089 window, under the SQL connections, one row per store profile with
   "MongoDB · host:port" or "Redis · host:port/db"):
   - MongoDB: databases (name, size) → collections and views (name, estimated document count) →
     "indexes" (name, keys, unique, TTL) and "fields" (union of field paths from a sample of at most
     100 documents, with the BSON types seen and how many sampled documents have them).
   - Redis: the databases that hold keys (from the keyspace section of `INFO`) plus the profile's
     own → key groups by `:` → keys with type and remaining TTL. Keys are read with `SCAN` in pages of
     at most 500 names; when more exist a "more…" row loads the next page; `KEYS` is never used.
   Loading runs in the background ("Loading…"); errors are a warning row under the node, retried by
   expanding it again. [GEN, ADR-0022]
6. **Catalogue** (SPEC-0300): categories *Document* and *Key-value* follow *Other*. Built-ins:
   MongoDB (family `mongodb`, Supported once AK-5 passes, `tested` names the image version), Redis
   (family `redis`, same rule), Valkey (family `redis`, Beta, "Redis protocol; not tested yet"),
   Amazon DocumentDB (family `mongodb`, Beta, "cloud only; MongoDB wire subset"), and as Beta
   "planned (ADR-0022), not connectable yet": Amazon DynamoDB, Apache Cassandra, Couchbase Server.
   Picking a store connector in New Data Source keeps the host, port, database, user, password and
   SSL fields; saving builds the store URL from them (`require` = TLS without certificate check,
   `verify-full` = TLS with check, MongoDB users authenticate against `admin`) and writes a store
   profile, the password into the keychain. Test Connection answers "Connected: Redis 8.2.11".
   Planned systems have the families `cassandra`, `dynamodb`, `couchbase` and the category
   *Wide column* for Cassandra. [ADR-0020, ADR-0022, OBS-0015]
7. **Export** of a result (documents or key values) to a file the user chose: JSON (array, pretty),
   JSON Lines, CSV (MongoDB: top-level fields of the exported documents as columns, nested values as
   compact JSON; Redis: key, type, ttl, value). Existing files are not overwritten without asking.
   [SPEC-0021]
8. **MCP** (ADR-0008), all under the user's grants for the store, the profile's `ai` line as their
   upper limit and the read-only flag (v3: SPEC-0012 v3 behaviours 1, 11–17):
   - `list_stores` — every store profile with id, name, system, url, read_only, rights.
   - `save_store` — add or change a store from `name` and `url` (dry run by default, `write` right to
     save; no password argument — the user types it into the dialog).
   - `browse_store` — children of a tree path (`[]` databases, `[db]` collections or key groups,
     `[db, collection]` indexes and fields; Redis `pattern` and `cursor` for key pages).
   - `run_store_command` — one console command (SPEC-0311 / SPEC-0312) with an optional MongoDB
     `database` (a `use` does not outlive the call), `page`, `page_size` and `confirm`; refused
     with the missing right named (`read-data`, `modify-data`, `modify-schema`) before anything is
     sent, even before connecting; logged as an agent action.
   - `export_store_result` — run a read command and write its result as JSON, JSON Lines or CSV into
     the workspace (`write` right).
   [ADR-0008, SPEC-0012]

## Acceptance criteria
- **AK-1** URLs: `mongodb://`, `mongodb+srv://`, `redis://h/3`, `rediss://` parse to profiles and
  back; passwords in user info and query are refused; Redis path `x` and `16` are errors; TLS flags
  as in behaviour 3 (unit tests `profile::tests`).
- **AK-2** Store file: load/save round trip with the envelope, wrong kind and newer format refused, unknown key refused, duplicate id refused, id clash with
  `connections.toml` refused (unit tests).
- **AK-3** Missing keychain entry fails before any network access (unit test with an unroutable
  host and a memory store).
- **AK-4** Tree models: MongoDB sample → field paths with types and counts; Redis key names →
  groups by `:` (unit tests `tree::tests`).
- **AK-5** Live: MongoDB and Redis containers with passwords (`nosql.sh` of the reference environment,
  `IDECRAFT_TEST_MONGO`, `IDECRAFT_TEST_REDIS`): connect, list databases/collections/indexes/fields,
  scan key pages with "more", refuse writes on a read-only profile (`crates/nosql/tests/live.rs`).
- **AK-6** Export to JSON, JSON Lines and CSV (unit tests `export::tests`).
- **AK-7** MCP `list_stores`, `save_store` (dry run), `browse_store` and `run_store_command` refusal
  without the right (protocol test `store_tools`).
- **AK-8** Catalogue entries of behaviour 6 validate and carry the stated statuses (unit test in the
  connector catalogue, once SPEC-0300 is merged).
- **AK-9** Window self-test: a store profile appears in the Database tool window with its system and
  address.
- **AK-10 (v4, behaviour 3)** `form::tests::prefer_needs_tls_off_the_loopback`: `prefer` for
  `db.local` gives the TLS URL of `require` for MongoDB and Redis, for `localhost` and `127.0.0.1`
  the plain URL.

## Out of scope
SSH tunnels; Redis Cluster and Sentinel topologies (single endpoint only); MongoDB Kerberos/LDAP/X.509
authentication; change streams and Pub/Sub; GridFS browsing; the later families of ADR-0022.

## Known deviations from the reference
Each store says whether it is tested (Supported/Beta) and the later systems are listed as planned
instead of looking equally available (IMP-0284). Key spaces are browsed with `SCAN` pages and never
with `KEYS` (IMP-0290). Agents get the same rights model as for SQL (IMP-0293).
