# ADR-0024: Network for WebAssembly connector plugins — only to the picked profile's host and port

- **Status:** accepted · 2026-11-05 · amends ADR-0016 (plugins as sandboxed WebAssembly modules),
  completes the WASM route of ADR-0020 decision 4 · decided by the project owner on PO-STATUS question 25
  ("Ja aber mit Warnhinweis für den User und die spätere KI damit diese selber entscheiden können")
- **Context:** ADR-0016 gives plugins no network: a plugin is a pure function over text with two
  reviewed host functions for reading the open project. ADR-0020 keeps WebAssembly plugins as the
  code route for database systems that have neither a wire protocol IDECraft speaks nor an ODBC
  driver. Such a connector plugin implements the system's protocol itself and must open a TCP
  connection to the server. Plugins come from third parties; IDECraft does not review their code. A
  plugin with general network access could send project files, passwords or anything it computes to
  any host. Agents will configure and use connections too (ADR-0023), so the risk must be visible to
  them in a form they can read, and an agent must not inherit a user's decision.

## Decision

**1. One narrow capability.** A plugin whose manifest asks for the permission `profile-network` may
open TCP connections, and only to the host and port of the connection profile the user picked for
it. There is no other network access:
- no listening sockets and no UDP;
- no name resolution of other names (the host resolves only the profile's host name itself);
- no other port, also not on the same host.

The plugin gets four host functions of module `idecraft`: `net_connect(target)`, `net_send`,
`net_recv` and `net_close`. `net_connect` takes the target as text `host:port`. The host compares it
with the profile's host (case-insensitive) and port (equal). Anything else is refused with code `-2`
and written to the audit log. The plugin never sees an address it did not get from the profile.
Calls block with time limits (10 s to connect, 30 s per read or write), at most 4 connections per
call. Every connection closes when the call ends.

**2. A connector plugin is a connector definition plus a module.** A definition with
`family = "plugin"` names the module (`[plugin] module = "<file stem>"`) and the SQL family whose
lexis the editor uses (`lexis`, default `postgresql`). The definition is data like any other
(ADR-0020 decision 3); the module is code. Plugin connectors come only from the connectors folder or
from import, so their status is always **User** or **Imported** (or **Beta** if one is ever built
in). They are never Supported, because IDECraft does not test third-party code.

**3. A warning before the first use, for people and agents.** Before a plugin first connects to a
host, IDECraft shows a warning. It names:
- the plugin, its version and its author as the manifest states them (or "unknown author");
- that the plugin gets network access to that host and port, and receives the connection's user
  name and password;
- the connector's status (Beta, User or Imported);
- that IDECraft does not review the plugin's code.

The window shows this in a dialog with "Allow and connect" and "Cancel" (Cancel is the default).
MCP returns the same content as a `warning` object, with `requires_grant`, in `list_connectors` and
on the connect path (`test_connection`). An agent can therefore see the risk and decide, or ask its
user.

**4. Grants per plugin and host, local and revocable.** A grant names plugin, host, port, holder
(`user` or `agent`) and date. Grants are kept in `plugins/network-grants.json` in the user's
settings folder, never in a workspace (like ADR-0016's permissions). They are listed and revoked in
Settings › Plugins and with MCP `revoke_plugin_network`. Removing a plugin, or withdrawing its
`profile-network` permission, deletes its grants.

**5. Agents need their own grant (ADR-0023 rule 7).** A user's grant covers the window only. An agent
connects through a plugin only with a grant whose holder is `agent`. It gets one only by asking:
MCP `grant_plugin_network` puts the full warning before the user in the IDECraft window. The user
answers once (this session only) or always (stored as an agent grant); without a window nobody can
answer and the agent gets nothing.

**6. Local audit.** Every connection a plugin opens or is refused is appended to
`plugins/network-audit.jsonl`: time, plugin, holder, target, the address connected to, outcome, and
bytes sent and received. The log stays on the machine, is never sent anywhere (ADR-0007), and is
readable in Settings › Plugins and through MCP `plugin_network`.

## Consequences
- Systems without a wire protocol IDECraft speaks and without ODBC become reachable through
  community plugins, at interpreter speed.
- The plugin still cannot read files beyond `project-read`, start programs, or reach anything but
  the server the user chose. What it does with the data on that one connection is its own business,
  and the warning says so.
- The rest of the host-function ABI and the sandbox (fuel, memory, fresh instance per call) is
  unchanged. Blocking network calls run off the UI thread; the time limits bound them.
- Connecting through a plugin is offered first in Test Connection and to agents. The Database tool
  window's sessions, introspection and the query console keep using native drivers. A plugin
  session there is a follow-up (SPEC-0314 out of scope).
