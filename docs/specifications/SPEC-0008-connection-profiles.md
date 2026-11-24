# SPEC-0008: Database connection profiles

- **Version:** 9 · **Date:** 2026-11-19 (v9: a profile from a data source of the reference's `.idea/dataSources.xml`, only on an explicit action, without user, password or the URL's SSL mode, behaviour 19, AK-21 — SPEC-0049 v7, gap DS1 of OBS-0109); 2026-11-17 (v8: on Linux passwords live in the Secret Service and survive a restart; without one saving is refused, behaviour 18); 2026-11-15 (v7: a test program may name the test-only password folder without setting the variable, behaviour 14); 2026-11-14 (v6: a system keychain that cannot be reached is named as such, behaviour 17); 2026-11-13 (v5: a relative database file belongs to the workspace, behaviour
  16; v4 2026-11-07; v2: secret store, credential resolution; v3: test-only
  password folder, behaviour 14; v4: tunnel, TLS files, options, colour and comment in the profile,
  `verify-ca`, behaviour 15 — SPEC-0108 v4)
- **Sources:** GEN (database tools keep named connection profiles per project), STD (PostgreSQL libpq
  connection URI; MySQL URI syntax of Connector/C++ and MySQL Shell; SQLite file paths; RFC 3986 percent
  encoding; freedesktop.org Secret Service API, v8), ADR-0025 §3 (v8: the Secret Service on Linux), SPEC-0049 v7 behaviour 18 (v9: the data sources read from
  `.idea/dataSources.xml`), OBS-0004 (weakness: user name lost after restart, then a modal login prompt blocked all
  access → IMP-0004)
- **Feature matrix rows:** F-051

## Purpose
A workspace keeps its database connections as named profiles that survive any restart, can be shared
through version control without secrets, and can be created from or turned into a connection URL.

## Behaviour
1. A profile has a stable id, a unique name, a dialect, and either a server address (host, port,
   database, user) or, for SQLite, a file path. Optional: SSL mode (`disable`, `prefer`, `require`,
   `verify-full`; default `prefer`), read-only flag. [GEN, STD]
2. The port defaults to 5432 for PostgreSQL and 3306 for MySQL when not given. [STD]
3. Profiles never contain passwords. A profile may name a secret reference (service and account in the
   system keychain); the secret itself is stored and read only there. [GEN, F-051]
4. The user name is part of the profile, not of the secret, so it is never lost when secrets are
   unavailable. [IMP-0004]
5. Profiles are stored in `.ferris-idea/connections.toml` in the workspace. Saving writes a temporary file
   and renames it over the old one, so an interrupted save leaves either the old or the new file. [IMP-0004]
6. Loading a missing file gives no profiles; a damaged file is an error naming the file and does not
   delete it. [GEN]
7. Parsing a URL: `postgresql://` and `postgres://`, `mysql://`, and `sqlite:` followed by a path.
   User, host, port, database and the SSL query parameter (`sslmode` for PostgreSQL, `ssl-mode` for MySQL,
   case-insensitive values) are read; percent-encoded characters are decoded. [STD]
8. A URL that contains a password is rejected with an error asking to store the password in the
   keychain; the password is not echoed in the error. [GEN]
9. Rendering a profile as a URL gives the same form (without secrets); parsing the rendered URL gives the
   same profile fields. [STD]
10. Validation: names are unique within the file; server profiles need a host; ports are 1–65535; SQLite
    profiles need a non-empty path. Violations name the profile and the field. [GEN]
11. Secrets are read, written and deleted through a secret store. The default store is the operating
    system's credential store (macOS Keychain, Windows Credential Manager; on Linux the Secret Service
    since v8, behaviour 18 — before v8 the kernel keyring); tests use an in-memory store. [GEN]
12. A secret value never appears in debug output, logs or error messages; it is shown as `***`. [GEN]
13. Resolving the credentials of a profile never waits for input. It gives one of: no password needed
    (no secret reference), password available, or credentials missing — naming the profile and the
    keychain entry so a non-modal prompt can ask for it. [IMP-0004]
14. Test-only password folder (v3). In builds with debug assertions, and only while the environment
    variable `FERRIS_IDEA_TEST_SECRETS_DIR` names a folder, that folder replaces the system store for
    every read, write and delete: the password of service `S` and account `A` is the file `<dir>/S/A`
    without a trailing line break. The folder and `<dir>/S` must have mode 0700 and the file 0600, and
    none may be a symbolic link; otherwise the operation fails with an error naming the path, never the
    value. Service and account must be plain names (no `/`, `\`, `.`, `..`). A missing folder or
    file means "no password". Release builds contain neither the variable nor the folder store and
    always use the system store. Purpose: automated tests (the functional tests through MCP) run
    against password-protected servers without touching or prompting for the keychain. **v7:** a
    test program may name the folder in-process instead (`test_store::use_dir`, debug builds only;
    the first folder named stays for the process and takes precedence over the variable): its tests
    run on several threads, where setting an environment variable is not safe. [GEN]

15. **v4 – More fields:** a profile may also hold an SSH tunnel (`[connection.ssh]`: host, port, user),
    TLS files (`[connection.tls]`: `ca_file`, `cert_file`, `key_file`), options (`[connection.options]`:
    `auto_commit`, `startup_script`, `connect_timeout_secs`, `query_timeout_secs`, `keep_alive_secs`,
    `schemas`), a `color` and a `comment` (SPEC-0108 v4, SPEC-0016 v2). None of them is written while
    it has its default, so older files and files of profiles without them stay as they were. None of
    them is ever a secret: the tunnel uses the SSH agent and keys, the key file stays where it is. The
    SSL mode `verify-ca` is read as `sslmode=verify-ca` / `ssl-mode=VERIFY_CA` and rendered back. [GEN,
    STD]

16. **v5 – Relative files:** the path of a file-based profile (SQLite, DuckDB) may be relative; it is
    then taken from the workspace folder whose `.ferris-idea/connections.toml` holds the profile, not from
    the folder Ferris IDEA was started in, so a profile shared through version control opens the same
    file for everyone. The file keeps the path as it was written; the workspace folder is never
    written into it. An absolute path is used as it is. A path inside a startup script (`ATTACH`) is
    the database's own business and is not rewritten. [GEN]
17. **v6 – Keychain not reachable:** when the session has no usable credential store (on Linux no
    session keyring of the user, e.g. an expired one inherited through `sudo`), reading, storing and
    deleting a password fail with "the system keychain cannot be reached in this session", never
    with "no matching entry". Removing a connection that names a keychain entry is then refused and
    the profile stays, so no password is left behind without its profile (rule 12). [GEN]
18. **v8 – Linux: the Secret Service:** on Linux the credential store is the freedesktop.org Secret
    Service on the session bus (GNOME Keyring, KWallet and others), as for the remote keys
    (ADR-0025 §3): only over an encrypted Secret Service session, only in the default collection,
    which keeps the password encrypted at rest under the user's login keyring, so it survives a
    restart. Entries carry the attributes `application = ferris-idea`, `service` and `account`; saving
    replaces an entry of the same service and account. Nothing is unlocked by Ferris IDEA: a locked
    default collection counts as not reachable, so no prompt ever waits for input (behaviour 13).
    - *Saving* goes only to the Secret Service. Without one (no session bus, no service, no default
      collection, or a locked one — typical for a headless machine) saving fails with an error that
      names the Secret Service, the reason, and says "cannot be reached in this session" and that
      passwords are never kept in a file; nothing is written anywhere else — no file, no kernel
      keyring.
    - *Reading* asks the Secret Service. A password found only in the kernel keyring (stored there
      by a version before v8 during the current login session) is given and moved once: written to
      the Secret Service and, only after that worked, removed from the kernel keyring. A move that
      fails still gives the password and leaves the kernel keyring as it was. Without a Secret
      Service reading stays as before v8: the kernel keyring is read.
    - *Removing* removes the entry from the Secret Service and any copy in the kernel keyring.
      Without a Secret Service the removal fails with the reason (behaviour 17).
    - Passwords of versions before v8 that were lost with a restart cannot be recovered; the user
      enters them again once and they are then kept in the Secret Service. [STD, GEN; IMP-0004]

19. **v9 – From the reference's data sources:** a data source offered by SPEC-0049 v7 behaviour 18 becomes a
    profile only when the user adds it in the Database tool window (SPEC-0089 v16 behaviour 28, through the
    connection dialog and its Save) or an agent adds it with MCP `add_idea_data_source` (SPEC-0049 v7
    behaviour 19); reading the project never does. The profile holds the data source's name, the dialect of
    its kind (`sqlite`, `postgresql`, `mysql`, `mariadb`, `sqlserver`) and its target: for SQLite the file,
    project-relative as behaviour 16 keeps it; for a server host, port and database. It holds no user, no
    password and no secret reference: the user enters them in the dialog, and the password goes to the
    keychain as for any connection. The SSL mode is the one of every new connection (`verify-full` for
    servers, IMP-0370), whatever the URL says; the URL's mode is not taken over (IMP-0530). The profile gets
    a new id and, like every new connection, the read-only agent permissions. A data source counts as added
    when a profile of the workspace has its name, or its dialect and the same file or the same host, port and
    database; such a data source is no longer offered, and adding it again is refused (behaviour 10).
    [GEN, CLAUDE.md rule 12]

## Acceptance criteria
- **AK-1** `postgresql://idecraft@127.0.0.1:54329/inventory?sslmode=disable` parses to dialect PostgreSQL,
  user `idecraft`, host `127.0.0.1`, port 54329, database `inventory`, SSL `disable`.
- **AK-2** `mysql://root@localhost/inventory` parses with default port 3306 and SSL `prefer`;
  `sqlite:/tmp/inv.sqlite` parses to a SQLite profile with that path.
- **AK-3** Percent-encoded user and database names are decoded (`us%40er` → `us@er`).
- **AK-4** `postgresql://u:secret@h/db` is rejected; the error mentions the keychain and not `secret`.
- **AK-5** Rendering and parsing again round-trip for all three dialects, including characters that need
  encoding.
- **AK-6** Saving and loading `.ferris-idea/connections.toml` keeps every field; no temporary file remains;
  the file contains no password field.
- **AK-7** A missing file loads as empty; a damaged file is an error and stays on disk unchanged.
- **AK-8** Duplicate names, missing host, port 0 and empty SQLite path are each rejected with the
  profile name and field.
- **AK-9** With an in-memory store: a profile without secret reference resolves to "not needed"; with a
  stored password to "available"; with a reference but no stored value to "missing", naming profile,
  service and account; deleting removes the value.
- **AK-10** Debug and display output of a secret show `***`, never the value.
- **AK-11** (manual, macOS) Storing, reading and deleting a throw-away entry in the real keychain works.
- **AK-12** Test folder store: set, get and delete round-trip with folder 0700 and file 0600; a trailing
  line break is dropped; a missing folder reads as no password.
- **AK-13** A file with mode 0644 or a folder with mode 0755 is refused with an error that names the
  mode and not the value; `..`, `a/b` and an empty name are refused.
- **AK-14** A release build with `FERRIS_IDEA_TEST_SECRETS_DIR` set ignores it (run by
  `scripts/ci-local.sh`).
- **AK-15** (v4) The fields of behaviour 15 survive saving and loading, a profile without them writes
  none of their keys, `verify-ca` round-trips (`profile::tests::tunnel_tls_and_options_round_trip`);
  refusals (`tunnel_tls_and_options_are_checked`), schema patterns and colours
  (`schema_filter_and_colours`).

- **AK-16 (v5)** A profile with `path = "catalogue.db"` read from a workspace opens the file in that
  workspace whatever the working directory is; saving the profiles writes the relative path again and
  no folder (`crates/database/tests/it/sqlite_catalogue.rs`
  `a_relative_file_is_found_from_the_workspace_and_attached_databases_are_read`).
- **AK-17 (v6, behaviour 17)** `crates/mcp/tests/it/protocol.rs`
  `agents_use_only_idecrafts_own_keychain_entry`: with a reachable keychain the removal succeeds;
  without one it is refused with the reason and the profile stays. The test checks the one that
  holds in the session it runs in.
- **AK-18 (v8, behaviour 18)** The rules of behaviour 18, with in-memory stand-ins for the Secret
  Service and the kernel keyring, on every platform: saving goes only to the Secret Service and drops
  a kernel keyring copy; without a Secret Service saving fails naming the Secret Service, the reason,
  "cannot be reached in this session" and "never in a file", without the value, and the kernel
  keyring stays empty; reading moves a kernel keyring password once and the Secret Service copy wins
  afterwards; a failed move gives the password and keeps the old copy; without a Secret Service
  reading takes the kernel keyring; removing clears both and fails without a Secret Service.
- **AK-19 (v8, Linux)** Against a real Secret Service on a private session bus (`dbus-daemon` and
  `gnome-keyring-daemon` started by the test, never the user's bus): the system store saves,
  replaces, reads and removes a password; a password in the legacy store is moved on first read and
  is then read from the Secret Service alone. Skipped where the programs are missing, except in CI.
- **AK-20 (v8, Linux)** With a session bus nobody serves, saving through the system store fails
  naming the Secret Service and "cannot be reached in this session", without the value, and the
  legacy store stays empty; the store reports itself not reachable.
- **AK-21 (v9, behaviour 19)** `crates/database/src/idea_sources.rs` `tests`: a SQLite data source becomes a
  form with the relative file and no user; a PostgreSQL one with `sslmode=disable` in its URL gets
  `verify-full`, host, port and database and no user; MariaDB, MySQL and SQL Server get their dialect; a
  data source with a reason gives no form; a profile with the same name or the same target counts as added.

## Out of scope
Connecting (drivers, M5.3), unlocking a locked keyring or creating a collection (v8), opening SSH tunnels (SPEC-0108 v4), JDBC-style
URLs, the connection dialog.

## Known deviations from the reference
- User name lives in the shared profile and survives any exit (reference: lost after `SIGTERM`, then a
  modal prompt blocked access). IMP-0004.
- Passwords in URLs are refused instead of stored.
- Deviation (v3, decided by the project owner 2026-11-04, "Test-Passwortspeicher"): debug builds can take
  passwords from a private test folder instead of the keychain (behaviour 14). The reference has no
  such mode; here it lets the functional tests reach password-protected databases unattended. Release
  builds are unchanged: passwords live only in the system keychain.
- Deviation (v8, rule 12): on Linux a password is never kept in a weaker place when the Secret
  Service is missing — saving is refused with the reason instead of falling back to a file or to the
  session-only kernel keyring.
- Deviation (v9, rule 12): a data source of the reference's shared project file is never added on its own and
  its URL's SSL mode is not taken over; a server profile from it starts at `verify-full` (IMP-0530).
