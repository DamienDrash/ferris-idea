# SPEC-0012: Running SQL over MCP with explicit permissions

- **Version:** 3 · **Date:** 2026-11-14 (v1 2026-10-31; v2 2026-11-04: one check per capability for every database tool, the window asks HTTP clients, behaviours 8–10; v3: the user grants the permissions outside the project, the project file can only narrow them, nothing is granted by default, safe mode, preview before a change, files of file databases, a log line per call — security review 2026-11-14, rule 12, ADR-0031; behaviours 1, 3, 4, 7 and 9 changed, 11–21 new)
- **Sources:** STD (SQL statement classes: queries, data manipulation, data definition, control), GEN
  (agents query databases through IDE tools), OBS-0004 (weakness: permissions enforced unevenly, missing
  permission reported as "rejected by user" without asking → IMP-0007; OBS-0010: the settings text
  promises a confirmation, the request is still refused), ADR-0008, ADR-0031, SPEC-0008–0011, SPEC-0053 v8,
  SPEC-0221 v2, SPEC-0278 v2, SPEC-0310, DOC (SQLite: `ATTACH DATABASE`, `VACUUM INTO`; DuckDB: the
  settings `enable_external_access` and `lock_configuration`), security review of 2026-11-14 (finding
  A1: a cloned project could give an agent database access)
- **Feature matrix rows:** F-050, F-084

## Purpose
An agent can run SQL against a workspace connection through MCP. What it may do is decided by the
user, per connection, outside the project; it is checked before anything runs, and a refusal says
exactly which permission is missing and where the user can grant it. A project file can take
permissions away but never give one.

## Terms
- **Connection:** a profile of `.ferris-idea/connections.toml` (SPEC-0008) or a store of
  `.ferris-idea/stores.toml` (SPEC-0310). Both are files of the project: whoever wrote the repository
  wrote them.
- **Target:** what a connection reaches, as one line: its URL without password (and `via ssh
  <host>` for a tunnel), followed by `#` and
  16 hexadecimal digits of a SHA-256 over everything that decides where it connects and what runs on
  connecting — dialect and system, host, port, database, user, the database file with links
  resolved, SSL mode and TLS files, the keychain entry it names, the SSH tunnel, the startup script.
  For a store: its URL and the keychain entry.
- **Agent:** a client of the MCP server (stdio or HTTP). The person at the window is not an agent.

## Behaviour
1. **Four permissions, granted by the user (v3).** Each connection has four permissions for agents:
   `read-schema`, `read-data`, `modify-data`, `modify-schema`. An agent has a permission only when
   the user granted it for this workspace, this connection (by id) and this target. **Nothing is
   granted by default**, reading included. Grants are kept in the user's grant file of SPEC-0053
   behaviour 2 (`agent-permissions.toml` in the settings folder), as tables

   ```toml
   [[connection]]
   workspace = "/path/of/the/workspace"
   connection = "<id of the connection>"
   target = "<target line>"
   allow = ["read-schema", "read-data"]
   ```

   Workspace paths are compared after resolving links. An entry whose target differs from the
   connection's present target grants nothing: when the project file changes where the connection
   leads (another host, file, user, keychain entry, tunnel or startup script), the user is asked
   again. An unknown permission name makes the whole file grant nothing, as for rights. No MCP tool
   writes this file; only the user's answer in the window does (behaviour 9). [IMP-0007, IMP-0028,
   ADR-0031]
2. Every statement is classified before execution, ignoring comments and literals (SPEC-0005 lexer):
   - read data: `SELECT`, `VALUES`, `TABLE`, `SHOW`, `EXPLAIN` (without `ANALYZE`), `DESCRIBE`/`DESC`,
     and `WITH` whose body contains no data-changing keyword;
   - modify data: `INSERT`, `UPDATE`, `DELETE`, `MERGE`, `REPLACE`, `UPSERT`, `COPY`, `CALL`, `EXEC`,
     `DO`, `LOCK`, `EXPLAIN ANALYZE`, and `WITH` containing `INSERT`/`UPDATE`/`DELETE`/`MERGE`;
   - modify schema: `CREATE`, `ALTER`, `DROP`, `TRUNCATE`, `RENAME`, `GRANT`, `REVOKE`, `COMMENT`, `VACUUM`,
     `REINDEX`, `ATTACH`, `DETACH`, and `PRAGMA` with `=` or a parenthesised argument (both forms can set a
     value; a bare `PRAGMA name` only reads);
   - everything else (including `SET`, `BEGIN`, unknown keywords) counts as modify schema — the strictest
     class — so nothing unknown slips through. `BEGIN`, `COMMIT`, `ROLLBACK`, `SAVEPOINT`, `RELEASE` are
     read data (transaction control alone changes nothing). [STD, GEN]

   **For agents (v3)** three more rules apply on top; the read-only guard of a connection in the
   window (SPEC-0008) keeps the classes above:
   - a statement of the read class that contains the word `INTO` outside literals (`SELECT … INTO
     table`, `INTO OUTFILE`, `INTO DUMPFILE`) is modify schema;
   - a statement of the read class that calls one of `set_config`, `setval`, `nextval`, `lo_import`,
     `lo_export`, `lo_unlink`, `dblink`, `dblink_exec`, `pg_read_file`, `pg_read_binary_file`,
     `pg_terminate_backend`, `pg_cancel_backend`, `load_file`, `load_extension`, `readfile`,
     `writefile` is modify schema (they change settings, sequences or large objects, or reach files
     and other servers);
   - a statement with text but no keyword the lexer can read (only punctuation, an unterminated
     literal) is modify schema; a script that cannot be split is one such statement.

   What the classification cannot see: a function or view of the database's own that changes data
   when a query reads it. The read-only session of behaviour 4 is the second line there. [STD, GEN,
   rule 12]
3. If any statement needs a permission the agent does not have, nothing runs. The tool reports an
   error listing each refused statement with its index, class and the missing permission, plus the
   connection name, the grant file and the exact `[[connection]]` entry to add. It starts with
   `not granted:` and never says the user rejected anything when the user was not asked. (v3: it
   names the user's grant file, not the project file.) [IMP-0007]
4. A session an agent's call opens is read-only (SPEC-0009–0011) unless the call applies a change
   with a modify permission (behaviours 10 and 14), so the database itself enforces reading where
   it can. [GEN]
5. The tool `run_sql` takes a connection name, the SQL, optional `continue_on_error` and `max_rows`
   (default 100, at most 10 000 per result set). It returns one entry per statement: columns and rows,
   the number of rows returned and whether rows were cut off; or the update count; or the error with
   SQLSTATE, message and script offset. [IMP-0008, IMP-0009]
6. Passwords come from the keychain (SPEC-0008); missing credentials are an error naming the entry. [IMP-0004]
7. The tool `list_connections` reports for each connection `ai` (what the agent has now: granted
   and not taken away by the project file), `requested` (what the project file allows at most,
   behaviour 11), `target` and `usable` (false in safe mode, behaviour 12). `list_stores` does the
   same. Listing needs no permission: it reads a file of the project. [GEN]

8. **One check per capability.** Every MCP tool that touches a connection checks the permission of
   the capability it exposes, in one place, before it connects for that purpose: structure (schemas,
   tables, columns, keys, DDL, completion, inspection, diagrams, schema comparison, plans) needs
   `read-schema`; rows need `read-data`; changing rows needs `modify-data`; changing the schema needs
   `modify-schema`; statements are checked per statement as in behaviour 3. No tool returns structure
   without `read-schema`, and no tool lists less than another tool returns for the same permission.
   v3: `test_connection` connects and so needs `read-schema`; `sql_history` returns statements that
   ran and so needs `read-data`; `cancel_sql` needs any one permission of the connection (else
   `read-data` is asked for). [IMP-0007]
9. **Asking over HTTP.** For a client of the running window (SPEC-0221 v2) a missing permission is
   asked in the window instead of refused, as SPEC-0053 behaviour 8 describes, with the connection as
   the scope. The question names the connection and its target, and says so when the database file
   lies outside the workspace. Answers: "Deny" (default), "Allow Once" and "Always for This
   Connection" (v3: adds a `[[connection]]` entry with the permission, the workspace, the connection
   id and the target to the user's grant file; the project file is not written). For a script, every
   missing permission is asked in one question. A denial is refused with `denied:` and not asked
   again in that session. Over stdio, without a window, or without an answer within 2 minutes,
   behaviour 3 applies. [IMP-0007]
10. Allowing once does not change the read-only session rule of behaviour 4 for that
    call: the session is opened writable only when the statement's permission was allowed. [GEN]

11. **The project file only narrows (v3).** What a connection's entry in the project file says
    about agents can take a permission away and never give one:
    - `ai = [...]` is the most the project lets an agent have on this connection. Without the line
      that is the two read permissions; so a change by an agent needs both the line in the project
      file and the user's grant. A permission outside `ai` is refused without asking, with a message
      that names the line.
    - `read_only = true` takes both modify permissions away, whatever `ai` and the user's grants say.
    The agent has exactly the permissions that are granted by the user (behaviour 1) and not taken
    away here. Changing `ai` (for example with `save_connection`, which needs `write`) therefore
    grants nothing by itself. [ADR-0031, rule 12]
12. **Safe mode (v3).** In a workspace that is not trusted (SPEC-0278) no connection of the project
    is usable by an agent: every tool that names a connection or store — to read, to change, to
    test, to cancel, to bind a file — is refused with the safe-mode message before anything
    connects, whatever is granted. Listing the connections and editing the project file
    (`save_connection`, `remove_connection`, with `write`) stay possible; they connect to nothing.
    [SPEC-0278 v2, rule 12]
13. **Migration (v3).** Permissions written in project files before v3 (`ai` with or without
    modify permissions, and the former default "reading is allowed") are requests, not grants: the
    first call of an agent on such a connection is refused as in behaviour 3 or asked as in
    behaviour 9, and the user's answer is kept in the user's grant file. `ai` lines stay valid as
    the upper limit of behaviour 11; nothing in a project file has to be rewritten. [ADR-0031]
14. **Preview before a change (v3).** A call that can change data or schema does nothing unless it
    says `apply: true`, the convention of the database tools (`edit_table_data`, `copy_table`,
    `change_column_type`, `rename_sql_object`):
    - `run_sql`: when at least one statement of the script is not of the read class, the call
      without `apply: true` connects to nothing and returns `applied: false` and, per statement, its
      index, the permission it needs (which names its class), whether it changes anything and its
      length. With `apply: true` the permissions are
      checked (behaviour 3), then the question for unsafe changes (`confirm_unsafe`, SPEC-0020) is
      asked as before, then the script runs. A script of reading statements runs at once.
    - `run_store_command`: a command that is not a read does nothing without `apply: true` and
      returns its class and permission; `confirm: true` for the dangerous ones is asked in addition.
    - `explain_sql` runs a statement only with `analyze: true`, which is that switch.
    The preview needs no permission, because it does not reach the database; it is refused in safe
    mode like every use of a connection. [GEN, rule 12]
15. **Files of file databases (v3).** For agents the rules of `read_file` and `write_file` hold for
    what a file database (SQLite, DuckDB) opens:
    - The database file, with links resolved, lies inside the workspace, or the user's grant is for
      exactly this target — the target names the file, so a grant for a file outside the workspace
      is always an explicit one, and question and refusal say that the file lies outside.
    - SQLite: `ATTACH` is allowed only with a quoted file name of an existing file inside the
      workspace (or `:memory:` or the empty name); a file name that is an expression, a `file:` URI,
      a relative name or a file outside is refused. `VACUUM INTO` is refused. This holds for the
      agent's statements and for the connection's startup script: a session for an agent whose
      startup script attaches or opens a file outside the workspace is not opened.
    - DuckDB: a session for an agent is opened with `enable_external_access = false` and
      `lock_configuration = true` before the startup script runs. The engine then refuses every
      file but the database itself: `ATTACH`, `COPY … TO/FROM`, `read_csv`, `read_text` and the
      other file functions, `INSTALL` and `LOAD`, inside the workspace as well.
    Server databases: files the server reads or writes on its own machine (`COPY … FROM '/file'`,
    `LOAD DATA INFILE`) are not files of this computer; such statements are of a modify class
    (behaviour 2). [DOC, rule 12]
16. **Agent log (v3).** Every call of an agent that names a connection or store writes exactly one
    line to `.ferris-idea/agent-log.jsonl`: the call's line of SPEC-0053 v7 behaviour 11, also for a
    tool that only reads. Each decision on the permissions is an entry of its `database` list:
    `connection` (the name), `permissions` (the ones the call needs), `dry_run` (true for a
    preview of behaviour 14), `ok` (true: allowed and handed to the database; false: refused),
    and for a refusal `refused` with the reason in one word (`not-granted`, `denied`, `safe-mode`,
    `project-file`, `read-only`, `keychain`). For SQL and store commands it adds `chars` (the
    length of the text) and `kinds` (the class of each statement) — never the text, never a value,
    never the target. A tool that checks twice (reading the structure, then changing it) has two
    entries in its one line. The line's `result` says how the call ended (`ok`, `refused`,
    `error`). The line is written when the call ends; an allowed call is refused before it
    reaches the database when the log cannot be written, so no database call runs unlogged. v3: `run_store_command`
    no longer logs the command's text. [IMP-0028, rule 12]
17. **Stores (v3).** Behaviours 1, 3, 7, 9 and 11–16 hold for MongoDB and Redis stores (SPEC-0310)
    with their own classification of commands (SPEC-0310 behaviour 8). [SPEC-0310]
18. **The window is not an agent (v3).** The Database tool window, consoles and data editors of the
    person at the window do not read the grant file and are unchanged: they use the connection as
    the project file describes it, with the startup-script rule of SPEC-0278. [GEN]
19. **Plugin connections (v3).** A connection that goes through a WebAssembly plugin (SPEC-0314)
    keeps its own grant per plugin, host and port; behaviour 12 holds for it too. [SPEC-0314]
20. **Keychain entry (v3).** The project file names the keychain entry of a connection's
    password. For agents a connection or store is usable only when it names none or the entry
    Ferris IDEA keeps for it (service `ferris-idea`, account `connection:<id>`, SPEC-0008); with any
    other entry every use is refused before anything connects and logged as `keychain`. So a
    repository cannot have an agent's call send another password of the user to a host it names.
    `remove_connection` removes only Ferris IDEA's own entry and refuses a connection that names
    another one. The window is unchanged. [rule 12]
21. **What runs or connects besides the call's own statements (v3).** The project file can make
    a connection do more than the agent's statement says; for an agent's call it counts:
    - **Startup script.** It runs on connecting when the project is trusted (SPEC-0278), in an
      agent's session under the file rules of behaviour 15. It is part of the target: a grant is
      for the connection with exactly this script, and the question and the refusal say that the
      connection has a startup script and whether it holds statements that are not of the read
      class. It is not held to the permissions of the call: a script with `SET search_path`
      would otherwise make users grant schema changes for reading.
    - **SSH tunnel.** A connection through a tunnel starts `ssh` to a host the project file
      names: besides the database permission it needs the right `ssh` (SPEC-0053), and the
      target line shows the host.
    - `read_table_data` builds its statement from the caller's `where` and `order_by`; the
      statement is classified like any other and refused unless it is of the read class. [rule 12]

## Acceptance criteria
- **AK-1** Classification: `select 1`, `with x as (select 1) select * from x`, `explain select 1`,
  `begin` → read data; `with x as (delete from t returning *) select * from x`, `insert …`,
  `explain analyze select 1` → modify data; `create table …`, `drop …`, `set x = 1`, `frobnicate` → modify
  schema. Keywords inside literals or comments do not count. v3, for agents: `select * into t2 from
  t`, `select set_config('a','b',false)`, `'` and `;;(` → modify schema; `select 'into'` and a
  comment alone → read data.
- **AK-2 (v3)** A profile without `ai` and without a grant: `list_connections` shows `ai` empty and
  `requested` with the two read permissions; `run_sql` with `select 1` is refused with
  `not granted:`, names `read-data`, the grant file and a `[[connection]]` entry, and does not
  contain "rejected".
- **AK-3** `run_sql` on a SQLite connection with `read-data` granted returns rows of a SELECT, with
  `truncated` when `max_rows` is smaller than the result.
- **AK-4** With the read permissions granted, a script `select 1; delete from t` with `apply: true`
  runs nothing and the error names statement 2, class modify data and the missing permission
  `modify-data`; the error text does not contain "rejected".
- **AK-5** With `modify-data` in `ai` and granted, an `insert` with `apply: true` returns its update
  count.
- **AK-6** An unknown connection name is an error naming it.
- **AK-7** Without `read-schema`, `list_schemas`, `list_tables`, `describe_table`,
  `generate_ddl`, `er_diagram`, `complete_sql` and `inspect_sql` are all refused with `not granted:`
  naming `read-schema`, and none returns a table name.
- **AK-8 (v3)** With a stand-in window and `modify-data` in `ai`: "Allow Once" runs a `delete` and
  neither the grant file nor the project file changes; "Always for This Connection" adds a
  `[[connection]]` entry with `modify-data` to the grant file and leaves the project file as it
  was; "Deny" refuses with `denied:`.
- **AK-9 (v3) Cloned project.** A workspace whose `connections.toml` gives all four permissions on
  a SQLite file, an empty grant file: `run_sql` with `create table …; insert …` and `apply: true` is
  refused with `not granted:`, the file has no new table, and the agent log has one line with
  `"ok":false` and no SQL text. The same call without `apply` returns the preview and changes
  nothing. After the user's entry for this target the call with `apply: true` creates the table.
  When the project file then points the connection at another file, the entry no longer applies.
- **AK-10 (v3) Absolute path.** A profile whose `path` is an absolute path to a SQLite file beside
  the workspace: `run_sql` with a SELECT is refused and the message says the file lies outside the
  workspace; nothing of the file is returned. With a grant for exactly this target the rows come.
- **AK-11 (v3) Safe mode.** In a workspace that is not trusted, with every permission granted:
  `run_sql`, `list_tables`, `test_connection` and `browse_store` are refused with the safe-mode
  message; `list_connections` answers with `usable: false`.
- **AK-12 (v3) Narrowing.** With all four permissions granted by the user: a profile without `ai`
  refuses an `insert` naming the `ai` line of the project file; a profile with `ai` holding
  `modify-data` and `read_only = true` refuses it as read-only; neither asks the window.
- **AK-13 (v3) Files.** With all permissions: `attach database '<file beside the workspace>' as x`
  is refused on SQLite and a file inside the workspace attaches; `vacuum into` is refused; a
  startup script that attaches a file outside keeps the session from opening. On DuckDB
  `select * from read_text('<file>')` and `attach` fail for an agent's session and work for a
  session of the window.
- **AK-14 (v3) Log.** Reading, a preview, a change and a refusal each leave exactly one line with
  the tool and, in `database`, the connection, permissions, `dry_run` and `ok`; no line contains
  the SQL text or a value of it.
- **AK-15 (v3) Stores.** A store without a grant refuses `browse_store`; a changing command returns
  the preview without `apply: true`.
- **AK-16 (v3) Keychain.** A connection whose project file names a keychain entry that is not
  Ferris IDEA's own for it is refused for `list_tables` and `remove_connection`, with every
  permission granted; the profile stays in the file.
- **AK-17 (v3) Startup script and tunnel.** The refusal for a connection with a startup script
  says that it has one and that it changes something (`create table …`) or only reads; a changed
  script makes the grant no longer apply. A connection with an SSH tunnel is refused without the
  right `ssh`, and its target names the host. `read_table_data` with `where: "v = nextval('s')"`
  is refused.

## Out of scope
Per-table permissions, row limits on the server side, a permissions section in the connection
dialog (the upper limit is edited in `.ferris-idea/connections.toml`, the grant is given through the
question or in the grant file), a list of the grants in the settings with a way to revoke them
(the file is edited by hand), functions and views of a database that change data when read
(behaviour 2), files a database server reads on its own machine, attaching files outside the
workspace for agents (no grant exists for that), a grant that survives a change of the target.

## Known deviations from the reference
- One permission model applied to every tool, checked up front with a precise refusal (reference: listing
  blocked while table structure returned; queries silently "rejected by user"). IMP-0007.
- Read-only default for agents is enforced by the database session as well.
- v3: the permissions are the user's, kept outside the project and bound to what the connection
  reaches; a project that is cloned brings no database access for agents with it, and reading is
  not on by default (the reference keeps the permissions with the data source). IMP-0028.
