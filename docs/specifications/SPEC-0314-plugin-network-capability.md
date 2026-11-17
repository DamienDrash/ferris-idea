# SPEC-0314: Network capability for WebAssembly connector plugins

- **Version:** 2 · **Date:** 2026-11-16 (v2: the connection's SSL mode decides whether a plugin
  gets a connection without TLS; elsewhere IDECraft makes the TLS — `net_connect_tls`, answers `-6`
  and `-7`, `ssl` in the request, `tls` in the audit; behaviours 3, 4, 9, 10, AK-6, SPEC-0016 v4,
  IMP-0427); 2026-11-05 (v1)
- **Sources:** GEN (plugin platforms of editors and browsers give extensions network access either
  not at all or broadly, with one install-time prompt), STD (TCP, RFC 3986 host syntax), the project owner's
  decision on PO-STATUS question 25 (2026-11-05), ADR-0007, ADR-0016, ADR-0020, ADR-0023 rule 7,
  ADR-0024, SPEC-0156, SPEC-0211, SPEC-0300, SPEC-0302; v2: STD (TLS 1.2, RFC 5246), SPEC-0016 v4,
  the coordinator's decision under rule 12 (PO-STATUS 2026-11-16: no plain text off the loopback
  without the user's explicit `disable`), IMP-0427
- **Feature matrix rows:** F-080, F-060

## Purpose
A database system that IDECraft cannot reach with its own drivers or ODBC can be added by a
WebAssembly plugin that speaks the system's protocol. The plugin may connect only to the server of
the connection the user picked. Before it does so for the first time, the user, or an agent, sees a
clear warning and decides.

## Behaviour
1. **Permission and action kind.** A manifest may ask for the permission `profile-network` (Plugins
   page: "connects to the host and port of a connection you pick"). An action of kind `connector` is
   a connector entry point. A manifest with a `connector` action that does not ask for
   `profile-network` is refused on loading. The manifest may name its `author` (one line, at most 80
   characters). As with every permission, the user grants `profile-network` at install or in
   Settings › Plugins. An update never grants it (SPEC-0211 behaviour 4). [ADR-0016, ADR-0024]
2. **Connector definition.** `family = "plugin"` in a connector definition (SPEC-0300 behaviour 3)
   needs a `[plugin]` table:
   - `module`, the plugin's file stem (`[a-z0-9][a-z0-9_-]{0,63}`);
   - `lexis`, optional: the SQL family whose lexis the editor uses (a native family, default
     `postgresql`).

   Without `[plugin]`, or with `[plugin]` under another family, the definition gets a problem with
   its key path. A plugin connector:
   - is not connectable through native sessions;
   - shows the note "Connects through the WebAssembly plugin `<module>`; IDECraft does not review
     its code.";
   - reads as User or Imported like every file (never Supported).

   Its URL schemes select it, and the profile's dialect is the `lexis` family. [ADR-0020, ADR-0024]
3. **Host functions** of module `idecraft`. Each answers `-1` unless the call was started with
   network access for this plugin (behaviour 5):
   - `net_connect(ptr, len: i32) -> i32` takes UTF-8 `host:port` (`[v6]:port` for IPv6 literals). It
     returns a handle (≥ 0) when host and port equal the granted target: the host name is compared
     case-insensitively without a trailing dot, the port exactly. Any other target returns `-2` and
     nothing is resolved. `-3` means the name did not resolve or no address accepted within 10 s,
     `-4` that 4 connections are open, `-5` a bad argument. Only the target's own name is resolved,
     by the host. **v2:** the connection has no TLS; where the connection's SSL mode does not allow
     that (behaviour 10) the answer is `-6` and nothing is resolved.
   - **v2:** `net_connect_tls(ptr, len: i32) -> i32` takes the same argument and gives the same
     answers, but the handle's bytes travel inside TLS that the host makes from the first byte on,
     with the checks of behaviour 10; `-7` means the TLS handshake failed (no TLS on that port, a
     version below TLS 1.2, a certificate or host name the mode does not accept). `net_send`,
     `net_recv` and `net_close` work on both kinds of handle alike.
   - `net_send(handle, ptr, len: i32) -> i32` writes all bytes and returns the count (`-3` error or
     30 s time limit, `-5` bad handle).
   - `net_recv(handle, max: i32) -> i64` reads up to `max` (at most 1 MiB) bytes. It returns them
     packed like other host answers; length 0 means the server closed the connection. Errors are
     `-3` and `-5`.
   - `net_close(handle) -> i32` returns 0.

   Connections left open close when the call ends. No other socket operation exists. [ADR-0024]
4. **Connector calls.** The input of a `connector` action is JSON: `op` (`test`), and the profile's
   `host`, `port`, `database` and `user`, plus `password` when one is stored or typed. **v2:** also
   `ssl`, the profile's SSL mode (`disable`, `prefer`, `require`, `verify-ca`, `verify-full`), and
   `plain`, whether `net_connect` is allowed for this connection (behaviour 10). The answer is
   `{ok: true, note}` or `{ok: false, error}`. The call runs in a fresh instance with the normal
   instruction budget, off the UI thread. [ADR-0016]
5. **Grants.** A call gets network access only when all of these hold:
   - the plugin is installed and enabled;
   - its manifest asks for `profile-network` and the user granted it;
   - a network grant exists for this plugin, host, port and holder.

   Grants live in `plugins/network-grants.json` in the user's settings folder, as a list of
   `{plugin, host, port, holder, granted}` (`holder` is `user` or `agent`, `granted` an RFC 3339 UTC
   time). Granting again replaces the old entry. Revoking takes plugin and optionally host, port and
   holder. Withdrawing `profile-network` or removing the plugin removes all of that plugin's grants.
   A grant never lives in a workspace. [ADR-0024, IMP-0028]
6. **Warning.** One text and one machine-readable object, built from the manifest, the connector
   and the target:
   - title "Allow network access for plugin “<name>”?";
   - the plugin's name, version and author ("unknown author" when none is given);
   - the target `host:port` ("the host and port of the connection you pick" when none is known yet);
   - that it receives the connection's user name and password;
   - the connector's status (Beta, User or Imported) and what that status means;
   - "IDECraft does not review the code of plugins. Allow it only if you trust the author."

   The object has `plugin`, `version`, `author`, `connector`, `status`, `host`, `port`,
   `reviewed: false`, `receives_credentials: true`, `title` and `text`. [ADR-0024]
7. **Window.** Test Connection in the connection dialog checks the user's grant for a plugin
   connector first. Without a grant it shows the warning as a modal dialog over the connection
   dialog:
   - the title and each line of the text;
   - the buttons "Cancel" (selected) and "Allow and connect"; Tab or ←/→ move the selection,
     Enter confirms it, Escape cancels;
   - "Allow and connect" stores a `user` grant and runs the test;
   - Cancel leaves the note "Not connected: plugin “<name>” was not allowed to connect."

   With a grant the test runs at once. Settings › Plugins lists each plugin's network grants as
   `network: host:port (you|agents, <date>)`; ⇧Delete (⇧⌫) on the selected plugin revokes them.
   [ADR-0009]
8. **MCP.**
   - `list_connectors` returns `plugin` (the module or null), `warning` (the object of behaviour 6
     without a target, or null) and `requires_grant` (true for plugin connectors, false otherwise)
     for every system.
   - `test_connection` on a plugin connector returns `warning` and `requires_grant` too. Without an
     agent grant for the profile's host and port it connects nowhere. It returns `ok: false`,
     `requires_grant: true` and an error naming `grant_plugin_network`.
   - `grant_plugin_network {connection}` asks the user in the IDECraft window with the full warning.
     "Allow once" lasts for this MCP session, "Always" stores an `agent` grant, and a refusal or no
     window gives no grant.
   - `revoke_plugin_network {plugin, host?, port?}` removes grants.
   - `plugin_network {plugin?}` lists grants and the last 200 audit records.

   A `user` grant never lets an agent connect (ADR-0023 rule 7). [ADR-0008, ADR-0023]
9. **Audit.** Every `net_connect` adds a record to `plugins/network-audit.jsonl` with `time`,
   `plugin`, `holder`, `target`, `address` (the address connected to, if any), `outcome` (`opened`,
   `refused`, `failed`). Every close adds `closed` with `sent` and `received` byte counts. Nothing
   in the log leaves the machine. The file holds no passwords and no payload. **v2:** `opened` and
   `closed` records carry `tls` (true when the host made TLS for the connection); a connection
   refused for want of TLS is `refused` with `error` "needs TLS"; a failed TLS handshake is `failed`
   with `error`, the TLS library's reason. [ADR-0007, ADR-0023]
10. **v2 – TLS by the connection's SSL mode (SPEC-0016 v4 behaviour 14, IMP-0427).** The host cannot
    see whether a plugin encrypts what it sends, so it decides by the connection the user picked.
    The caller hands the host the profile's SSL mode and CA file with the grant; the host itself
    decides whether the target is on the loopback (`localhost`, 127.0.0.0/8, `::1`, as SPEC-0016 v3
    behaviour 3):
    - `net_connect` (no TLS) is allowed for `disable` on any host and for `prefer` on the loopback;
      for `prefer` off the loopback and for `require`, `verify-ca` and `verify-full` on every host
      it answers `-6`.
    - `net_connect_tls` is allowed in every mode. It refuses anything below TLS 1.2. `require`,
      `prefer` and `disable` encrypt without checking the certificate; `verify-ca` checks the chain;
      `verify-full` checks the chain and the host name. A CA file (PEM) replaces the system's
      trusted roots; it is read when the call starts, and a file that cannot be read gives `-7`.
    A plugin learns from `plain` in its input which one to use. A protocol that turns TLS on inside
    its own messages (PostgreSQL's request for TLS, MySQL, SQL Server's TDS 7) cannot be spoken
    through `net_connect_tls`; such a plugin reaches a server off the loopback only when the user
    chose `disable`. [STD, GEN, rule 12]

## Acceptance criteria
- **AK-1** Grant logic: granting, re-granting, holder separation, revoking by host and by plugin,
  removal on uninstall and on withdrawing the permission (unit tests `network::tests`).
- **AK-2** A plugin granted `127.0.0.1:<port>` talks to a local server there; `127.0.0.1:<other>`,
  `localhost:<port>` and `example.com:80` are refused with `-2` and logged as `refused`; without a
  grant every call answers `-1` (unit test with a WAT plugin and a local listener).
- **AK-3** A `connector` action without `profile-network` is refused on loading; a `plugin`
  connector definition without `[plugin]` reports the key path (unit tests).
- **AK-4** MCP: `list_connectors` carries `warning` and `requires_grant`; `test_connection` without
  an agent grant refuses with both fields and opens no connection; with an agent grant it connects
  and the audit names holder `agent` (protocol test).
- **AK-6** (v2, behaviour 10) With a WAT plugin and local servers: `net_connect` answers `-6` and
  logs `refused` with "needs TLS" for `require`, `verify-ca` and `verify-full` on `127.0.0.1` and
  for `prefer` on a host off the loopback, without resolving it; it connects for `prefer` and
  `disable` on `127.0.0.1` (audit `tls: false`), and `disable` allows it off the loopback too
  (`network::tests::plain_connections_follow_the_ssl_mode`). `net_connect_tls` talks to a TLS
  server with `require` (audit `tls: true`), and with `verify-full` and the CA file that signed the
  server's certificate for `localhost`; it answers `-7` and logs `failed` with the reason for
  `verify-full` without that CA file, for a CA file of another authority, for a CA file without
  certificates (before anything is resolved) and for a server without TLS
  (`…::tls_made_by_the_host_checks_as_the_mode_says`). The request carries `ssl` and `plain`, never
  the CA file (`…::the_request_names_the_mode`).
- **AK-5** Window self-test steps (written, run in the nightly window test): Test Connection on a
  plugin connector opens the warning with plugin, author, host and port, status and the review
  sentence, with Cancel selected; Escape cancels with the note; Tab and Enter allow, store a user
  grant and run the test; a second test does not ask again.

## Out of scope
Database tool window sessions, the query console and introspection through plugins; UDP,
listening sockets and connections to other hosts, which will not be added. v1 left TLS to the
plugin; v2 makes it in the host (behaviour 10), and a plugin's own TLS over `net_connect` counts as
no TLS, since the host cannot check it. Switching an open connection to TLS (STARTTLS) is not
offered: the host could not tell the bytes before the switch from a password.

## Known deviations from the reference
The reference has no sandboxed connector plugins; its JDBC drivers run with the IDE's full rights.
IDECraft confines a connector plugin to the one host and port the user picked, warns people and
agents with the same content, and keeps a local audit of every connection (IMP-0300). v2: the
plugin's traffic follows the connection's SSL mode, with TLS made by IDECraft (IMP-0427).
