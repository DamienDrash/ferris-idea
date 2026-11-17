# SPEC-0108: Connection dialog

- **Version:** 12 · **Date:** 2026-11-17 (v12: a MariaDB server's version is its number, without the product's name
  and build suffix the server appends, behaviour 22, AK-23 — OBS-0076 C2); 2026-11-16 (v11: `test_connection` and `remove_connection` take the connection as `connection` too, behaviour 21, AK-22, OBS-0071 G6); 2026-11-14 (v10: the result of a test has its area from the start, so the buttons stay where they are, behaviour 20, AK-21; second window check of 2026-11-14; v9: the SSL button goes to the stricter modes first, a changed form marks the test result outdated, the labels keep to one line, behaviours 17 to 19); 2026-11-13 (v8: testing a SQLite file reads it and names what is wrong with it in
  words — not a database, locked, not readable, a folder; a relative file is found from the workspace; v7 2026-11-12: the SSH tunnel counts as open once ssh reports its listening socket, not when it names the forward, found by the server CI; v6: a long value stays inside its field; v5: the tunnel comes from the one start point of `ssh`, every
  jump host with the post-quantum key exchange, ADR-0025 v5; v4: pages General, SSH/SSL and Options; SSH tunnel through the
  system `ssh`, TLS files, connection options, colour, comment, driver line and URL field; keychain
  work off the UI thread and a redraw when the window comes back — OBS-0026 L22–L26; v3: what a
  successful test found out — server version, driver,
  round trip, TLS, file — with a Copy action, also over MCP; v2 2026-11-05: the cause of a failed test in
  plain words — hand test 33, D11)
- **Sources:** GEN (data source dialogs of database tools: database system, server or file, user,
  password kept by the system, SSL, read-only, test before saving), STD (PostgreSQL `SHOW
  server_version`, MySQL/MariaDB `version()`, SQLite `sqlite_version()`, `pg_stat_ssl`,
  `Ssl_cipher`), OBS-0026 (side-by-side comparison of Test Connection; v4: the reference's dialog has
  name, colour, comment, tabs General/Options/SSH-SSL, a driver choice and a URL field), SPEC-0008
  (profiles, keychain references), SPEC-0016 (TLS), SPEC-0089 (Database tool window), SPEC-0121
  (system `ssh`), STD (OpenSSH `ssh -L`, `BatchMode`, `StrictHostKeyChecking`, `ExitOnForwardFailure`;
  PostgreSQL `statement_timeout`, MySQL `max_execution_time`, MariaDB `max_statement_time`), ADR-0008,
  ADR-0012, ADR-0025 (network security), IMP-0370, IMP-0371, IMP-0373
- **Feature matrix rows:** F-051

## Purpose
The user adds, changes, tests and removes database connections in the window instead of editing
`.idecraft/connections.toml` by hand; passwords stay in the system keychain.

## Behaviour
1. **Opening:** in the Database tool window "+" or ⌘N (Ctrl+N) opens the dialog for a new connection,
   "✎" or F4 for the selected one. Tab and Shift+Tab move between the fields, Enter saves, Escape
   closes without saving. [GEN]
2. **Fields:** database system (PostgreSQL, MySQL, SQLite), name, and for servers host (default
   `localhost`), port (empty means the system's default), database, user and password, for SQLite the
   database file; SSL mode (`disable`, `prefer`, `require`, `verify-full`) and read-only. [GEN,
   SPEC-0008]
3. **Saving:** the form is checked before anything is written (name and host or file required, port
   1–65535, names unique). A new connection gets an id from its name, made unique, and read-only
   agent permissions; a changed one keeps its id and agent permissions. [SPEC-0008]
4. **Password:** shown as dots and never copied or cut. A typed password is stored in the system
   keychain (service `idecraft`, account `connection:<id>`) and the profile names that entry; an empty
   password field keeps the stored one. The profile file never contains a password. SQLite needs none.
   [SPEC-0008]
5. **Removing:** Delete on a connection asks in the tool window; a second Delete removes it and its
   keychain entry. [GEN]
6. **Testing:** Test Connection connects with the form as it is (typed password, else the stored one)
   and runs `SELECT 1`; the dialog shows "Connected to <name>" or the error, e.g. that a SQLite file
   does not exist (files are never created implicitly). Testing runs in the background. **v2:** the
   error names the cause, not only its kind: a server that wants a password the form left empty says
   so ("asks for a password and none was given"), other driver errors carry their cause after the
   kind (`postgres::tests::missing_password_is_named`). [GEN, ADR-0012]
   **v3:** a successful test also says what it found out, one line each under "Connected to <name>":
   the database product and the version the server reports (PostgreSQL `SHOW server_version`, MySQL
   and MariaDB `version()`, a MySQL server whose version names MariaDB is shown as MariaDB, SQLite
   `sqlite_version()`, SQL Server `SERVERPROPERTY('ProductVersion')`, Oracle
   `product_component_version`, a connector's own version query first; the version is left out
   when it cannot be read), the client library used, the round trip of the test query in
   milliseconds (a tenth below 10 ms), whether the connection is encrypted ("TLS: yes/no", as the
   server reports it, SPEC-0016; the line is left out for files and when the server does not
   tell) and, for SQLite and DuckDB, the database file. A **Copy** link next to the lines puts the
   note and the lines on the clipboard as plain text. Neither the lines nor the copied text ever
   contain the password or the user name. The lines belong to that result: a new test, an error or
   any other note replaces them. Stores tested through their own driver (SPEC-0310) and connector
   plugins (SPEC-0314) keep their note without lines. [GEN, STD, OBS-0026]
7. **MCP:** `save_connection` (dry run by default), `remove_connection` and `test_connection` do the
   same for agents. They never take a password: the user types it into the dialog. **v3:** a
   successful `test_connection` returns `note`, `product`, `server_version` (null when unknown),
   `driver`, `ping_ms`, `tls` (null when unknown), `file` and `details` (the dialog's lines).
   [ADR-0008]

8. **v4 – SSH tunnel (page SSH/SSL):** fields SSH host (an alias of `~/.ssh/config` or a host name),
   SSH port and SSH user (both from the config when empty). With a host, connecting first starts the
   system `ssh` with the shared options of every IDECraft SSH path (from the start point
   `idecraft_remote::SshCommand`, which checks the client and holds every `ProxyJump` hop to the same
   options, SPEC-0121 v4, ADR-0025 v5): the post-quantum key exchange only — a host or a
   local client without one is refused before anything is sent, and the dialog says in plain words
   which side to update (OpenSSH 9.0 or later, ML-KEM from 9.9) — `BatchMode=yes` (no password or passphrase is
   ever asked for or kept; keys, the SSH agent and the config apply), `StrictHostKeyChecking=yes` (an
   unknown or changed host key is refused with what to do, never accepted from inside IDECraft),
   `ExitOnForwardFailure=yes`, no agent or X11 forwarding, no shared master connection — with a
   forward from a free port on `127.0.0.1` (never another interface) to the profile's host and port as
   the SSH host sees them. The tunnel counts as ready only when `ssh` reports that it listens on that
   port, so a program that took the port first is never mistaken for it (IMP-0373). The driver
   connects to the local end; TLS still checks the profile's host name. The tunnel ends with the
   session. Tunnels exist for the PostgreSQL, MySQL and MariaDB families (and connectors on them); the
   page says so for others and the profile refuses a tunnel there. The test result names the tunnel
   ("SSH tunnel: bastion (local end 127.0.0.1)"). Host and user must not start with `-` or contain
   spaces, so `ssh` never reads them as options; the profile file holds no other `ssh` arguments. In
   builds with debug assertions only, the environment variable `IDECRAFT_TEST_SSH_CONFIG` names an
   `ssh` config file for automated tests; release builds ignore it. [STD, SPEC-0121, ADR-0025]
9. **v4 – Colour, comment, driver, URL (page General):** a Colour button cycles through red, orange,
   yellow, green, blue, violet, grey and none; the colour marks the connection in the Database tree. A
   Comment field keeps a note (shown in the connection's quick documentation, SPEC-0089 v6). A Driver
   line names the client library of the chosen family ("tokio-postgres (built in)"): IDECraft brings
   one driver per family and never downloads one (IMP-0371). A URL line shows the profile as a URL,
   never with a password (SPEC-0008 behaviour 9); the URL field takes a pasted URL and Enter fills
   system, host, port, database, user and SSL mode from it; a URL with a password is refused, changes
   nothing and the field is emptied, and the message never repeats the password (SPEC-0008 behaviour
   8). [GEN, OBS-0026]
10. **v4 – Options (page Options):** Auto-commit (on by default; off starts the connection's consoles
    and data editors in Manual, SPEC-0093 v3, SPEC-0190 v4), a startup script (SQL run after every
    connect before anything else; a failing script fails the connection with its message; a script
    read from a project that is not trusted is project code and does not run in safe mode, SPEC-0278 —
    the connection's documentation says so — until the user changes it in the dialog), connect
    timeout (seconds, 10 when empty), query timeout (seconds; the server ends longer statements:
    PostgreSQL `statement_timeout`, MySQL `max_execution_time` for SELECT, MariaDB
    `max_statement_time`), keep-alive (seconds between TCP probes) and a schema filter
    (comma-separated names, `*` for any characters, ignoring case; empty shows all schemas, SPEC-0089
    v6 behaviour 16). [GEN, STD, OBS-0026]
11. **v4 – Checks:** before anything is saved: SSH port 1–65535, SSH port or user only with a host;
    time limits whole numbers 1–86400; no empty schema pattern; certificate files only with an SSL mode
    other than `disable`, a CA file only with `verify-ca` or `verify-full`, a PEM client certificate
    only with its key file, a `.p12` bundle without a key file, for MySQL and MariaDB a `.p12` bundle
    only (SPEC-0016 v2). The note names the field. [GEN]
12. **v4 – Keychain off the UI thread, redraw (OBS-0026 L26):** saving and removing a connection write
    the keychain on a background thread, as testing already did, so a system prompt for keychain access
    never blocks drawing; when the answer is back the window is drawn again. Whenever the window becomes
    active again (after any system prompt) it asks for a frame at once and once more a quarter second
    later. [GEN]
13. **v4 – Pages and SSL default:** the dialog has the pages General (system, name, server, user,
    password, file, comment, colour, URL, driver, read-only, SSL mode), SSH/SSL (tunnel, SSL mode,
    certificate files) and Options; Tab moves between the fields of the shown page. The SSL mode of a new server
    connection is `verify-full` (SPEC-0016 v2 behaviour 8, IMP-0370); the SSL button goes from the most
    to the least secure mode (v9: see behaviour 17). [GEN]
14. **v4 – MCP:** `save_connection` takes `ssh_host`, `ssh_port`, `ssh_user`, `ca_file`, `cert_file`,
    `key_file`, `auto_commit`, `startup_script`, `connect_timeout_secs`, `query_timeout_secs`,
    `keep_alive_secs`, `schemas`, `color` and `comment` (on a changed connection what is left out is
    kept, an empty text removes it, `0` removes a time limit) and returns them with `ssl`; a new
    connection defaults to `verify-full`. Still no password, passphrase or key content is taken.
    [ADR-0008]

15. **v6 – Clipped:** a text longer than its field is cut at the field's edge and does not run over the
    dialog; the full text stays editable. [OBS-0028 window check 2026-11-09]

16. **v8 – Testing a SQLite file:** the test (and every connect) reads the file's schema table once,
    so a file that is no database fails the test instead of the first use. The error says what is
    wrong and what to do, then SQLite's result code in brackets: the file is not a SQLite database (or
    is encrypted); the database is locked because another program is writing to it (after SQLite's
    wait of five seconds; try again when that program has finished); the file or its folder cannot be
    read or written; the path is a folder; the file does not exist (never created, behaviour 6). The
    message names the file with its full path. A relative file is looked for in the workspace folder
    (SPEC-0008 v5 behaviour 16), also for a connection that is not saved yet. A test never changes the
    file and creates none (SPEC-0011 v2). [GEN, STD: SQLite result codes]
17. **v9 – SSL button (window check 2026-11-14):** from `prefer` the button goes first to the
    stricter modes — `require`, `verify-ca`, `verify-full` — then to `disable`, and back to
    `prefer`. Until v9 the first press from `prefer` turned encryption off. [GEN, rule 12]
18. **v9 – Outdated test result:** once a field differs from the form the last test was made with,
    the result under the fields is marked "Outdated — the fields changed since the test" in the
    muted colour, its details and Copy are not shown; testing again shows the new result. [GEN]
19. **v9 – Labels on one line:** the field labels keep to one line; the label column is wide
    enough for "Connect timeout". [GEN]
20. **v10 – The buttons stay in place (second window check 2026-11-14):** between the fields and
    the row of buttons (Test Connection, Cancel, Save) is the result area, 112 px high from the
    moment the dialog opens: room for the note's line and the five detail lines of a test. The
    note ("Connecting…", the result, "Saving…", why saving failed), the detail lines and Copy
    appear inside it; a text that needs more — an error of many lines — scrolls inside the area.
    Nothing the dialog shows by itself (a test ending, the result going out of date, a save
    failing) moves a button. Until v10 the note and the details were inserted above the buttons
    when a test ended and pushed them down: a click aimed at Save hit Copy. The dialog's height
    still follows what the user chooses (the page, the database system and its fields, the
    system list). [GEN]

21. **v11 – One name for the connection:** every other database tool names the connection
    `connection`; `test_connection` and `remove_connection` take it as `connection` as well as by
    their older name `id`. Giving neither is an error that names both; giving both with different
    connections is an error too. [GEN; OBS-0071 G6]
22. **v12 – MariaDB's version number (OBS-0076 C2):** a MariaDB server reports its version with the
    product's name and the build's suffix appended (`12.3.3-MariaDB`, `10.11.6-MariaDB-0+deb12u1-log`).
    The test result (behaviour 6 v3, MCP `server_version`) gives the number before `-MariaDB`, so the
    line reads `Server: MariaDB 12.3.3` and no longer names the product twice. [STD, GEN]

## Acceptance criteria
- **AK-1** Form to profile and back; port, host and name checks; SQLite drops server fields and
  password; unique ids; upsert keeps names unique; save keeps the password in the store only and keeps
  id, permissions and password when edited; remove deletes the password too (unit tests with an
  in-memory store).
- **AK-2** Testing a missing SQLite file fails with its reason, an empty file connects, a server that
  is not there fails (integration test).
- **AK-3** `save_connection` dry run and save, `test_connection` before and after the file exists,
  unknown dialect refused, `remove_connection` (protocol test).
- **AK-4** Window self-test: a new SQLite connection is tested, saved with Enter, listed in the tool
  window and removed with two presses of Delete.
- **AK-5** Hand test (round 4): a PostgreSQL connection with password, edited and tested again.
- **AK-6** (v3) The detail lines and the copied text: server with and without version, round trip
  to a tenth and whole, TLS line only when known, file line, no secrets
  (`connection_form::tests::test_details_are_formatted_without_secrets`).
- **AK-7** (v3) Testing a SQLite file names SQLite 3.x, the file and a measured round trip without a
  TLS line (`crates/database/tests/it/connection_form.rs`: `test_connection_reports_success_and_failure`);
  live against the reference PostgreSQL and MySQL (`IDECRAFT_TEST_PG` / `IDECRAFT_TEST_MYSQL`, skipped
  without them): version not empty, round trip measured, TLS known and "no" for `sslmode=disable`
  (`live_postgres_test_names_version_ping_and_tls`, `live_mysql_test_names_version_ping_and_tls`).
- **AK-8** (v3) `test_connection` over MCP returns `product`, `server_version`, `ping_ms`, `tls`,
  `file` and `details` for a SQLite connection (protocol test `connection_tools`).
- **AK-9** (v3) Window self-test: after testing the new SQLite connection the dialog shows the
  version, round trip and file lines and Copy's text starts with the note (`connection_steps`).

- **AK-10** (v4) Tunnel, TLS files, options, colour and comment from the form into the profile and the
  file and back; a plain profile writes none of them; SQLite drops tunnel and files
  (`connection_form::tests::extras_round_trip`, `profile::tests::tunnel_tls_and_options_round_trip`);
  the refusals of behaviour 11 (`profile::tests::tunnel_tls_and_options_are_checked`).
- **AK-11** (v4) Security: the `ssh` arguments hold batch mode, strict host keys, the forward bound to
  127.0.0.1 and nothing secret (`tunnel::tests::arguments_are_strict_and_loopback_only`); against the
  test's own `sshd`: data goes through the tunnel, its port is not reachable over the machine's network
  address and closes with the tunnel, an unknown and a changed host key are refused
  (`tests/tunnel.rs`); live PostgreSQL and MySQL through the tunnel, a refused host key stops the
  connection with the tunnel error (`sessions_go_through_the_tunnel`).
- **AK-12** (v4) Secure default, URL field, driver line, the hint for a local server without TLS
  (`connection_form::tests::secure_default_and_url_field`).
- **AK-13** (v4) Live: the startup script runs first and a failing one fails the connection; the query
  timeout ends a long statement on PostgreSQL and MySQL (`tests/cancel.rs`).
- **AK-14** (v4) MCP `save_connection` with tunnel, files, options, colour and comment, its default
  `verify-full` and refusals (protocol test `grid_values_object_doc_history_and_connection_extras`).
- **AK-15** (v4) Window self-test (`connection_steps`, written, not run yet): a URL with a password is
  refused and not kept, a query timeout that is not a number stops saving with the reason, options,
  comment and colour are saved; saving and removing finish in the background.
- **AK-15a** (v5) The tunnel's command line comes from the start point: shared options first, the
  route pinned (`ProxyCommand=none`) or a jump host through IDECraft's hop stand-in (`tunnel::tests`;
  jump hosts against test `sshd`s: SPEC-0121 AK-7).
- **AK-16** (v4) In the window next to the reference: the three pages, a tunnel to a real SSH host, the
  redraw after a keychain prompt (OBS-0026 L26).

- **AK-17 (v6)** By hand: a long File path ends at the field's right edge.

- **AK-18 (v7)** The tunnel is open only when `ssh -v` reports the channel of the listening socket
  (`channel N: new port-listener [port listener]`, written after `listen`); OpenSSH's line `Local forwarding
  listening on … port N` comes before `bind` and `listen`, and a connection right after it was refused
  under load (`crates/database/tests/it/tunnel.rs::tunnel_forwards_on_loopback_only_and_refuses_unknown_and_changed_keys`,
  80 runs eight at a time: 8 and 5 failures before, none after; unit test
  `tunnel::tests::the_listening_channel_marks_the_tunnel_ready`). [STD: OpenSSH's own messages]

- **AK-19 (v8)** Connecting to a text file, a missing file, a folder and a database another connection
  holds an exclusive transaction on fails with those words and leaves the files as they were; after the
  lock is released the same connection reads the schema
  (`crates/database/tests/it/sqlite_catalogue.rs` `files_that_cannot_be_opened_say_why`).
- **AK-20 (v9, behaviours 17 to 19)** `connection_dialog::tests::ssl_goes_stricter_first_and_a_changed_form_outdates_the_test`:
  the order of the SSL modes, the outdated test, the label width. By hand: test, change the host —
  the result reads Outdated.
- **AK-21 (v10, behaviour 20)** In a window of the test harness
  (`window_check_1114b.rs::the_connection_dialog_keeps_its_buttons_in_place`): the painted places of
  Test Connection, Cancel and Save, read from the semantic tree by their test ids, are the same
  before a test, while it runs, after a result with five detail lines and after an error of many
  lines; the result area is 112 px high and above the buttons, Copy is inside it.
- **AK-22 (v11, behaviour 21)** `protocol.rs::test_connection_takes_the_connection_by_either_name`:
  on a SQLite profile with the `read-schema` grant, `test_connection` connects with `connection`,
  with `id` and with both given alike; with neither the error names `connection`, with two different
  ones it says so; `remove_connection` with `connection` removes the profile.
- **AK-23 (v12, behaviour 22)** `connection_form::tests::a_refused_certificate_points_to_the_ca_file`:
  `12.3.3-MariaDB` and `10.11.6-MariaDB-0+deb12u1-log` give `12.3.3` and `10.11.6`, a MySQL version
  stays as it is; live headless against MariaDB 12.3 (OBS-0076): `Server: MariaDB 12.3.3`.

## Out of scope
Driver properties, other database systems (F-060), importing data sources of the reference, a choice of
how long a password is kept, SSH tunnels for SQL Server and Oracle, jump hosts other than through
`~/.ssh/config` (`ProxyJump` there works, each hop with the post-quantum key exchange, v5), Kubernetes port forwarding from the dialog (SPEC-0241 has
it), a time zone option.

## Known deviations from the reference
The password is always kept in the system keychain (no "until restart" or "never" choice). Removing asks
by a second Delete instead of a confirmation dialog. **v4:** three pages (General, SSH/SSL, Options)
instead of the reference's six tabs; no driver choice — one built-in driver per family, nothing
downloaded (IMP-0371); SSH tunnels never take a password or passphrase and refuse unknown host keys
instead of asking (IMP-0373, ADR-0025); new connections start at `verify-full` (IMP-0370). **v3:** the
test result stays in the dialog under the note instead of a separate small panel, so there is nothing to
close and no second place repeating the version next to the Test button; the same fields are available to
agents over MCP.
