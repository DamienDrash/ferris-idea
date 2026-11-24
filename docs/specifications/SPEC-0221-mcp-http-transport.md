# SPEC-0221: MCP over HTTP for the running window

- **Version:** 3 · **Date:** 2026-11-16 (v1 2026-11-03, behaviours 9 and 10 made precise before the implementation; v2 2026-11-04: a call that needs a missing right waits for the user's answer in the window, behaviours 7 and 11, SPEC-0053 v4; quitting, behaviour 12, SPEC-0261 v2; v3: off by default, only the user switches it on in Settings, an agent can only switch it off — security decision of the project owner of 2026-11-16, PO-STATUS question 46 (c), rule 12; behaviours 1 and 9, AK-4, AK-5, new AK-8)
- **Sources:** STD (Model Context Protocol, transport "Streamable HTTP", revision `2026-07-28` with the
  earlier session-based revisions still accepted; its security notes: validate `Origin`, bind to
  localhost, authenticate), STD (RFC 6750 bearer tokens, RFC 6454 origins), GEN (local servers of desktop
  apps are reachable by every process and every user of the machine, and by web pages through DNS
  rebinding), OBS-0002 (the reference's built-in MCP server on a fixed port), OBS-0004 (MCP calls time out
  while a modal dialog waits), ADR-0008, SPEC-0006, SPEC-0053, SPEC-0220
- **Feature matrix rows:** F-084

## Purpose
MCP clients reach the Ferris IDEA window that is already running — its open files, unsaved buffers and
settings — instead of starting a separate headless process, without letting other local users, other
programs without the token, or web pages drive the IDE.

## Behaviour
1. When the window starts and the user has switched the setting on (behaviour 9), it serves MCP over Streamable HTTP on `127.0.0.1`
   only, on a port the system picks; it never listens on another interface. The endpoint of the first
   project is `http://127.0.0.1:<port>/mcp`; every open project also has
   `http://127.0.0.1:<port>/mcp/<id>`, where `<id>` is derived from the project path and stays the same
   across starts. Projects opened later are added while the server runs. [STD, improvement IMP-0100]
2. Each start creates a new random token of 32 bytes (64 hex digits). Every request must carry
   `Authorization: Bearer <token>`; a missing or wrong token is answered with 401 and
   `WWW-Authenticate: Bearer` before the request reaches MCP. The comparison takes the same time for every
   wrong token of that length. [STD, GEN, IMP-0100]
3. Against DNS rebinding the `Host` header must name `127.0.0.1`, `localhost` or `[::1]`, and an
   `Origin` header, when present, must be `http://127.0.0.1:<port>` or `http://localhost:<port>`; anything
   else is answered with 403 before the token is looked at. Requests without `Origin` (ordinary MCP
   clients) pass this check. [STD]
4. Discovery: the server writes `mcp-http/<pid>.json` in the user's settings folder (SPEC-0053
   behaviour 2), never into a project. The folder has mode 0700 and the file 0600, written to a new file
   and renamed into place. It holds the process id, the start time, the token, the main URL and every
   project with its root and URL. The file is removed when the server stops; files of processes that no
   longer run are removed at the next start. [GEN, IMP-0100]
5. The tools, their results and their rights are those of the stdio server (SPEC-0006): the same
   per-workspace grants of SPEC-0053 apply, dry runs stay the default and actions are logged as there.
   The grant file is read again for every MCP session or stateless request, so a grant the user adds
   while the window runs takes effect without a restart. [STD, SPEC-0053]
6. Tools see the running window's state: `read_file` and `open_documents` read the document registry
   (SPEC-0220), so an unsaved buffer is visible to a client. [GEN, A2]
7. The server runs on its own threads and never waits for the window; a tool call is answered while a
   dialog is open in the window. Exceptions: a call that needs a right the user has not granted waits for
   the user's answer to the question of behaviour 11 (at most 2 minutes), and `quit_ide` waits for the
   window to report and save its work (behaviour 12). [GEN, improvement IMP-0101]
8. Clients of revision `2026-07-28` are served without sessions; older clients get a session id as their
   revision requires. A request body larger than 4 MiB is refused. Simple calls are answered as JSON,
   calls that send progress first as an event stream. [STD]
9. Setting `mcp.http` (Settings → AI Assistant → "Serve MCP to local agents over HTTP") switches the
   server off and on while the window runs, without a restart. Switching it off stops the listener, ends
   open streams and removes the discovery file; switching it on starts a new server with a new port and
   token. **v3:** the setting is **off by default**: without a settings file, and with one that does not
   name it, the window opens no port and writes no discovery file. Only the user switches it on, in
   Settings. `set_ide_settings` with `mcp_http` can only switch it off (`false`), which windows read at
   their next start like the other settings of that tool (SPEC-0129); `mcp_http: true` while it is off is
   refused as invalid, names the Settings row and changes nothing, also as a dry run. [GEN, rule 12]
10. The server stops when the app quits; a failure to start (port refused, settings folder not
    writable) is reported on stderr and leaves the window working. [GEN]

11. A call over HTTP that needs a right the workspace does not grant (SPEC-0053) or a database
    permission the connection does not grant (SPEC-0012 v2) asks the user in the window as SPEC-0053
    v4 behaviours 7–9 describe, naming the client from its `initialize` request. [GEN, IMP-0007]
12. The window serves `quit_ide` (SPEC-0261 v2): it reports the unsaved files and running programs of
    every project window and quits as the client chose. A server without a window (stdio) answers
    `quit_ide` with an error that names `ferris-idea quit`. [GEN, IMP-0002]

## Acceptance criteria
- **AK-1** Protocol over HTTP: without `Authorization` and with a wrong token → 401, nothing reaches MCP;
  with `Host: evil.example` or `Origin: http://evil.example` → 403; `initialize`, `tools/list` and a tool
  call with the token behave as over stdio (same tool names, same `read_file` result).
- **AK-2** A buffer registered as unsaved is returned by `read_file` over HTTP.
- **AK-3** The discovery file has mode 0600 in a 0700 folder, names port, token and URLs, lies outside
  the project, and is removed on stop; a stale file of a dead process is removed at the next start.
- **AK-4** The listener is bound to 127.0.0.1 only; turning the setting off closes the port.
- **AK-5** Window self-test: switched on in Settings › AI Assistant, the running window's server answers
  `tools/call read_file` with the text of an unsaved editor, and turning `mcp.http` off closes the port.
- **AK-6** Hand test: a real MCP client (Claude Code) connects with the URL and token of the discovery file.
- **AK-7** Over HTTP, a server with a window that asks: the client's name from `initialize` reaches the
  question; "Allow Once" lets the call through (protocol test with a stand-in window).
- **AK-8** *(v3)* Off by default, on via Settings: the defaults and a settings file without the key give
  `mcp.http = false`, `http = true` in `[mcp]` gives on (unit test); a window self-test starts without a
  server, switching the Settings row on starts one at once; `set_ide_settings` refuses `mcp_http: true`
  as invalid, with and without dry run, and accepts `false` (protocol test).

## Out of scope
A Unix-socket transport (the token covers the same threat on loopback; considered again for Linux
multi-user hosts), remote access, OAuth, server-to-client requests (sampling, elicitation), a stdio
bridge that reads the discovery file for clients that cannot send headers.

## Security decision (rule 12)
The project owner decided on 2026-11-16 (PO-STATUS question 46 (c)) that the server is off by default
and switched on in Settings. A listening port is attack surface even on loopback with a token: the
token file is readable by every process of the same user, and a client that holds it can call every
tool the workspace grants. A user who does not use an MCP client over HTTP gets no port at all. An
agent can close the port but not open it, as with the other security switches agents may only turn
towards the safer side (ADR-0025 section 1a, SPEC-0319 behaviour 32).

## Known deviations from the reference
- Random port and a per-start token with a 0600 discovery file instead of a fixed port without
  credential (IMP-0100, OBS-0002).
- Tool calls are answered while the window shows a modal dialog (IMP-0101, OBS-0004).
