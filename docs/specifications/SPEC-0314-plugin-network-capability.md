# SPEC-0314: Network capability for WebAssembly connector plugins

- **Version:** 1 · **Date:** 2026-11-05
- **Sources:** GEN (plugin platforms of editors and browsers give extensions network access either
  not at all or broadly, with one install-time prompt), STD (TCP, RFC 3986 host syntax), the project owner's
  decision on PO-STATUS question 25 (2026-11-05), ADR-0007, ADR-0016, ADR-0020, ADR-0023 rule 7,
  ADR-0024, SPEC-0156, SPEC-0211, SPEC-0300, SPEC-0302
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
     by the host.
   - `net_send(handle, ptr, len: i32) -> i32` writes all bytes and returns the count (`-3` error or
     30 s time limit, `-5` bad handle).
   - `net_recv(handle, max: i32) -> i64` reads up to `max` (at most 1 MiB) bytes. It returns them
     packed like other host answers; length 0 means the server closed the connection. Errors are
     `-3` and `-5`.
   - `net_close(handle) -> i32` returns 0.

   Connections left open close when the call ends. No other socket operation exists. [ADR-0024]
4. **Connector calls.** The input of a `connector` action is JSON: `op` (`test`), and the profile's
   `host`, `port`, `database` and `user`, plus `password` when one is stored or typed. The answer is
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
   in the log leaves the machine. The file holds no passwords and no payload. [ADR-0007, ADR-0023]

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
- **AK-5** Window self-test steps (written, run in the nightly window test): Test Connection on a
  plugin connector opens the warning with plugin, author, host and port, status and the review
  sentence, with Cancel selected; Escape cancels with the note; Tab and Enter allow, store a user
  grant and run the test; a second test does not ask again.

## Out of scope
Database tool window sessions, the query console and introspection through plugins; TLS inside
the host (a plugin can implement it itself); UDP, listening sockets and connections to other hosts,
which will not be added.

## Known deviations from the reference
The reference has no sandboxed connector plugins; its JDBC drivers run with the IDE's full rights.
IDECraft confines a connector plugin to the one host and port the user picked, warns people and
agents with the same content, and keeps a local audit of every connection (IMP-0300).
