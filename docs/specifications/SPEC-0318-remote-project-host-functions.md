# SPEC-0318: Remote project — language servers, inspections, search, Git, terminal and running on the host

- **Version:** 8 · **Date:** 2026-11-12 (v8: `search.text` searches the project's own sources, `include_non_project`, `files`, `not_searched`; behaviour 13, decision of the Project Owner of 2026-11-12; v7: `search.files` and `search.everywhere` list the project's own files, `include_non_project`, `recent`, `not_listed`; behaviour 14); 2026-11-09 (v6: merge, rebase, rename and delete of the Branches popup and the log's path filter on the host; section "Branches and the log's path filter (v6)"; v5: the remaining host actions — network Git, rollback, stash, the log's commit actions, the comments/strings filter, the preview of file and symbol hits, the Ports node; section "Remaining host actions (v5)". v4: `run.stop` asks the program to end first, as SPEC-0087 v4; v1 written before the observation of the reference's
  remote development, like SPEC-0317 v1; the open points at the end are compared after it. v2: the
  window side of step 4 — the tool windows of a remote project and four protocol precisions; see
  "Window details fixed in v2" and "Protocol details" 5–8. v3: the window check of details 2–12,
  section "Window check (v3)")
- **Sources:** GEN (remote development: code insight, search, version control, terminals and
  programs run where the code is, the window stays local; F-083 "Remote"), STD (LSP 3.17; DAP;
  POSIX pseudo terminals; `lsof`/`/proc/net/tcp` listening sockets), ADR-0025 (network security, in
  particular rule 3 / section 6: unauthenticated protocols only through the session or a loopback
  tunnel), ADR-0026 (session protocol, families `lsp`, `dap`, `term`, `run`, `port`), ADR-0008,
  ADR-0012, SPEC-0024, SPEC-0029, SPEC-0031, SPEC-0034, SPEC-0038, SPEC-0042, SPEC-0045, SPEC-0047,
  SPEC-0053, SPEC-0062, SPEC-0064, SPEC-0072, SPEC-0074, SPEC-0076, SPEC-0087, SPEC-0090, SPEC-0091,
  SPEC-0097, SPEC-0158, SPEC-0278, SPEC-0290, SPEC-0317
- **Feature matrix rows:** F-083
- **Roadmap:** M6.12 step 4 (functions on the host) with the port forwarding of step 5 and the
  agent part of step 6; the foundation of M6.13 (debugging on the host)

## Purpose
A remote project (SPEC-0317) is more than a remote editor: the language servers, the inspections,
Find in Files and Search Everywhere, Git, the terminal and the run configurations all work on the
host, where the code, the toolchains and the repository are. The window shows their results for the
session's documents as it does for local files, programs that open a port on the host can be reached
from the local browser through the session, and agents use the same tools on the remote project as
on a local one — with the host project's rights. Nothing of the project runs on the window's
machine, and nothing unauthenticated is exposed on either machine.

## Behaviour

### General
1. **Families:** the backend (`ferris-idea serve`, in process or over SSH) binds the families `lsp`,
   `search`, `git`, `term`, `run`, `port`, `dap` and `mcp` (methods below) and lists each in the
   capabilities of `session.hello` (SPEC-0317 v2 detail 10: until now they answered "method not
   found"). A window that finds a family missing (an older backend) leaves its functions off and
   says "not available on this backend". [ADR-0026 §6]
2. **Rights** (ADR-0025 §4, deny by default): `view` for `lsp.*` queries, `search.*`, `git.status`,
   `git.diff`, `git.base`, `git.log`, `git.branches`, `run.configs`, `run.list`, `term.list`,
   `port.list`; `edit` for `lsp.rename` and `lsp.apply_action` with `apply: true`; `vcs-write` for
   `git.commit`, `git.create_branch`, `git.checkout`; `run` for `term.*` (except `list`), `run.start`,
   `run.stop`, `run.attach`, `run.trust`; `ports` for `port.forward`, `port.connect`, `port.close`;
   `debug` for `dap.*`; `agents` for `mcp.*`. A missing right is answered `not_granted` naming it and
   audited; an SSH session has them all. [ADR-0025 §4]
3. **Paths on the wire** are relative to the project folder in both directions (SPEC-0317 behaviour
   18). Results of language servers, search and Git that name a host file inside the project carry
   its relative path (`doc`); a place outside the project (a library source, the standard library)
   carries only a display name (`outside`), never a host path, so the window cannot be made to open
   or write a path of its own machine. Any path or `file:` URI from the window that leaves the
   project — `..`, absolute, a link out of the folder, a URI of another scheme or host — is refused
   with `invalid` before a server sees it. [ADR-0025 T5, SEC-14]
4. **Revisions:** every request that names a position in a document carries the revision of the
   document the window computed it on; the backend answers `stale_revision` when the document moved
   on, and the window asks again on the new revision (once) or drops the answer. The window sends
   such a request only once its own edits of that document are acknowledged (at most one round trip;
   typing never waits for it). [ADR-0026 §6]
5. **Trust on the host:** `serve` runs in safe mode (SPEC-0278): a host project that the host's
   trust list does not trust starts its language servers in their safe configuration and runs
   none of its code (run configurations, terminals in its folder still open, Git hooks are skipped
   with a note). The window shows the trust banner for an untrusted host project; "Trust Project"
   sends `run.trust` (right `run`, audited), which adds the folder to the host's trust list. The
   window's own trust list is never consulted for host folders. [SPEC-0278, ADR-0025 §4]
6. **Audit:** every request that changes something or runs something — `lsp.rename`/`apply_action`
   with `apply`, `git.commit`/`create_branch`/`checkout`, `term.open`/`close`, `run.start`/`stop`/
   `trust`, `port.forward`/`close`, `dap.start`, `mcp.call` — is written to the host's audit log
   (time, peer, method, target, outcome; never content). [ADR-0025 §4]

### Language servers and inspections
7. **Servers on the host:** the backend uses the pool of SPEC-0090 on the host: one server per
   language and project, started on first use with the host's toolchains (SPEC-0042 resolution on
   the host). **Document synchronisation is the backend's job:** whenever a document of its registry
   changes revision, the backend sends its text to the server within 150 ms (`didOpen`, then
   `didChange`); the window never sends document text to a server. [ADR-0026 §6, SPEC-0090]
8. **Diagnostics follow typing:** the backend sends `lsp.diagnostics {doc, revision, items}` to
   every client that has the document open, whenever a server publishes for it; a newer publication
   for the same document replaces an unsent older one (coalesced, bulk priority). Items are
   SPEC-0024 diagnostics (severity, range in the server's units, message, source, code,
   unnecessary). `lsp.diagnostics {doc}` as a request returns the current list. The window shows them
   as for local files (underlines, gutter, Problems tool window, the file's problem count).
   [SPEC-0024, SPEC-0090]
9. **Inspections:** `lsp.inspect {doc, revision}` runs Ferris IDEA's own inspections (SPEC-0092
   catalogue, the host project's inspection profile SPEC-0315 and dictionary SPEC-0163) on the
   document's text on the host and returns the findings with their ranges; the window shows them
   with the diagnostics. [SPEC-0092]
10. **Code insight** on the document's text at the given revision:
    - `lsp.complete {doc, revision, line, character, limit}` → completion items (SPEC-0072);
    - `lsp.hover {doc, revision, line, character}` → hover text or `null` (SPEC-0024);
    - `lsp.signature {doc, revision, line, character}` → signature help or `null` (SPEC-0080);
    - `lsp.locations {doc, revision, line, character, kind}` with `kind` = `definition`,
      `references`, `type_definition`, `implementation` → `{locations: [{doc, range}], outside:
      [names]}` (behaviour 3);
    - `lsp.symbols {doc}` → the file's symbols (structure, SPEC-0024);
    - `lsp.semantic {doc, revision}` → semantic tokens (SPEC-0290);
    - `lsp.code_actions {doc, revision, range}` → actions (SPEC-0076);
      `lsp.apply_action {doc, revision, range, title}` applies the one with that title (behaviour 12);
    - `lsp.rename {doc, revision, line, character, new_name, apply}` → the plan: per file its
      relative path and number of edits (SPEC-0074); with `apply: true` it is carried out (behaviour
      12).
    `lsp.status {}` lists the host's servers (language, program, running, ready, safe mode).
    [SPEC-0024, SPEC-0072, SPEC-0074, SPEC-0076, SPEC-0080, SPEC-0290]
11. **In the window** a remote document gets what a local one gets from these answers: diagnostics
    and inspection findings, completion while typing and on Ctrl+Space, hover and quick
    documentation, parameter info, ⌘B and ⌘-click to the definition (a target inside the project
    opens its remote document at the place; an `outside` target is named in the status bar),
    Find Usages, semantic colours, the code-action bulb (⌥⏎) and Rename (⇧F6). Run markers are
    computed in the window from the replica's text (they read nothing else) and run on the host
    (behaviour 22). [GEN]
12. **Applying edits:** a rename or code action with `apply: true` changes documents open in the
    backend's registry through an edit by author `lsp` (every client receives `doc.changed`, the
    window's undo is one step per document) and writes files not open as documents on the host
    through a temporary file renamed into place, recording local history on the host. Files outside
    the project in a plan are refused and nothing is changed (behaviour 3). The answer lists the
    changed files. [SPEC-0074, ADR-0026 §5]

### Search and index
13. **Find in Files:** `search.text {query, regex, case_sensitive, whole_words, mask, folder,
    max}` searches the host project (SPEC-0029 rules: binary and files over 10 MiB skipped, excluded
    folders left out) and uses the backend's text for documents with unsaved changes, so results
    match what the editors show. It returns files with their relative paths and per match line,
    column, length and the line's text; at most `max` matches (default 1000, then `truncated`).
    The window's Find in Files dialog shows them, opening a result opens its remote document.
    `$/cancel` stops a running search. (v8, decision of the Project Owner of 2026-11-12) The files
    searched are those of SPEC-0029 v5 behaviours 7 and 8, from the same listing as the local search:
    the project's own sources by default; `folder` wins when it names a folder that is not the
    project's own (it is searched completely); `include_non_project: true` searches every file
    except version control data and `.ferris-idea`; `files` (relative paths, each checked as a path
    inside the project like `folder`) searches those files whatever they are, for the Open Files
    scope. A folder or file behind a symbolic link is not read and finds nothing. When the search
    left something out, the answer has `not_searched`, the line of SPEC-0029 behaviour 10. There is
    no replacing call. [SPEC-0029, SPEC-0065]
14. **Search Everywhere:** `search.everywhere {query, categories, limit}` runs SPEC-0031 on the host
    (files, symbols from the host's index, text) and returns hits with relative paths; actions stay
    the window's own. `search.files {query, limit}` ranks file names only (Go to File). Results
    arrive in the popup as they do for local projects, the host's hits after a round trip. (v7) Both
    list the project's own files (SPEC-0031 v5 behaviour 12) and return `not_listed`, the line on what
    was left out; `include_non_project: true` lists every file, `recent` (relative paths, each checked
    as a path inside the project) names the window's open and recent files, which are listed in any
    case. [SPEC-0031, SPEC-0067]

### Git
15. **Git on the host** through the backend's own Git library (no `git` process needed for these):
    `git.status` (per path staged and unstaged change), `git.diff {path}` (side-by-side rows),
    `git.base {path}` (the file's text at HEAD, for the change markers), `git.log {limit, branch}`,
    `git.branches`, `git.commit {message, paths, amend}`, `git.create_branch {name, start}`,
    `git.checkout {name, create}`; the further methods are in "Remaining host actions (v5)". Commit hooks run on the host only for a trusted project
    (behaviour 5). `git.changed {branch, head}` is sent when HEAD, the index or a ref changes on the
    host (checked every second while a client watches). [SPEC-0034, SPEC-0064]
16. **In the window** the Commit and Git tool windows show the host repository, the status bar
    shows the host branch, and the gutter of a remote document shows change markers against
    `git.base` (refreshed on `git.changed` and after each save). Fetch, pull, push and the other
    actions: "Remaining host actions (v5)". [SPEC-0064]

### Terminal
17. **A terminal on the host** is a pseudo terminal in the backend: `term.open {cwd, cols, rows,
    env}` starts the user's login shell on the host (from the host's account, `$SHELL` else
    `/bin/sh`) in `cwd` (relative, default the project folder) and returns `{term, stream}`. The
    shell's output arrives as data on the stream, keystrokes go back as data on the same stream (both
    directions with credit, ADR-0026 §9: a terminal the window does not read stops its program as a
    slow terminal would). `env` takes names of the form `[A-Z_][A-Z0-9_]*` only; `TERM` is
    `xterm-256color`. No second `ssh` process is started for a terminal. [STD, ADR-0026 §6]
18. `term.resize {term, cols, rows}` changes the size (the program gets `SIGWINCH`); `term.close
    {term}` ends the shell and its process group; `term.exited {term, code}` reports the end;
    `term.list {}` lists the session's terminals with size, folder, whether running, exit code.
    [SPEC-0038]
19. **Breaks:** a terminal keeps running while the connection is broken and keeps its last 1 MiB
    of output; after the resume the window calls `term.attach {term}` and gets a new stream that
    starts with that output. Terminals end with their session (`session.bye`, or the reattach period
    over). [ADR-0026 §8]
20. **In the window** the Terminal tool window opens host terminals for a remote project: the tab is
    named after the host, typing, pasting, mouse reporting, resizing and the screen model work as for
    a local terminal (SPEC-0062), and the tab shows "Reconnecting" while the connection is broken.
    [SPEC-0062]

### Running and testing
21. **Run configurations of the host project:** `run.configs {}` returns the configurations of the
    host's `.ferris-idea/run.toml` and the detected ones (SPEC-0045, SPEC-0097) with their problems.
    `run.start {name}` starts one with its before-launch chain on the host (SPEC-0087) and returns
    `{run, stdout, stderr}` (two streams); `run.event {run, kind, …}` reports `started` (name,
    command), `finished` (exit code, timed out, stopped, duration), `tests` (the parsed test report
    of a test configuration, SPEC-0047) and `chain_stopped`. `run.stop {run}` ends it with its
    process group as a local Stop does (SPEC-0087 v4 behaviour 5: SIGTERM, SIGKILL after 3 s); `run.list {}` lists the session's runs. An untrusted host project refuses with
    the trust message (behaviour 5). [SPEC-0045, SPEC-0047, SPEC-0087]
22. `run.start {context: {doc, line}}` runs what a run marker or ⌃⇧R stands for at that line of the
    document (SPEC-0097 context configuration, computed on the host from the backend's text).
    [SPEC-0097]
23. **Breaks:** a run survives a break like a terminal (output kept, 1 MiB per stream; `run.attach
    {run}` gives new streams starting with it; events are in the event stream and replayed). Runs end
    with their session. [ADR-0026 §8]
24. **In the window** the Run tool window shows a host run as a tab with its output (standard error
    in the error colour), the test tree from `tests` events, Stop and Rerun; the Run widget lists the
    host's configurations. [SPEC-0087]

### Port forwarding
25. **Offered ports:** the backend watches the TCP ports that programs started by its terminals and
    runs (and their child processes) listen on, every second, and announces each new one with
    `port.detected {port, address, pid, command, kind}` and a closed one with `port.gone {port}`;
    `port.list {}` returns the detected ports and the open forwards. `kind` is `debug` when the
    program is a debug adapter or debuggee listening for a debugger (JDWP `-agentlib:jdwp`,
    `debugpy`, `lldb-server`, `gdbserver`, Node `--inspect`, or the usual ports 5005, 5678, 9229
    of such a program), `database` for the usual database ports (5432, 3306, 1433, 1521, 6379,
    27017) and `service` otherwise. The window shows offered ports in a notification ("Port 8080 on
    host: Forward") and in the Ports list of the Services tool window. [GEN, ADR-0025 §6]
26. **Forwarding** starts only from a user action or an agent with the `ports` right — never because
    the backend asked (ADR-0025 T5). `port.forward {port, target_host, confirm}` registers the
    target on the backend and returns `{forward}`; the target is `127.0.0.1` on the host unless the
    user typed another `target_host`, which the list then shows. A `debug` port is refused
    (`denied`, "debug ports are reached by the debugger through the session") and offered no
    Forward button; a `database` port needs `confirm: true` from the user after the warning that
    other local users can reach a forwarded port (agents cannot confirm: their call is refused with
    the warning). [ADR-0025 §6]
27. **The local end** listens on `127.0.0.1` (and `[::1]` when available) only, on the same port
    number when it is free, else on a free one (the list shows both); there is no setting for another
    interface. Each accepted local connection opens `port.connect {forward}` → `{stream}`, and the
    backend connects to the target on the host and relays the bytes in both directions with credit.
    A forward's connections close with the forward (`port.close {forward}`), the local listener
    closes with it, and every forward closes with the session or the window. [ADR-0025 §6, SEC-7]

### Debugging (foundation for M6.13)
28. `dap.adapters {}` lists the debug adapters installed on the host (SPEC-0091 adapters, resolved
    on the host). `dap.start {adapter}` starts an installed adapter on the host with its standard
    input and output joined to a new stream (DAP messages travel unchanged; no TCP port of the
    adapter is opened on either machine) and returns `{session, stream}`; `dap.stop {session}` ends
    it, `dap.exited {session, code}` reports its end. Launch and attach configurations, breakpoints
    and the window's debugger on these streams are SPEC-0322 (M6.13), which also adds the other
    transports. [ADR-0026 §6, ADR-0025 §6]

### Agents
29. **The same tools on the host:** `mcp.tools {}` returns the tools of Ferris IDEA's MCP server for the
    host project and `mcp.call {tool, arguments}` calls one there — with the agent rights,
    permissions file and safe mode of the host project (SPEC-0053, SPEC-0158 behaviour 3), never
    the window's. The window's MCP HTTP server (SPEC-0221) serves each remote project window under a
    route of its own, `/mcp/<id>` of the project's stand-in folder, whose tools are the host's; so
    `problems`, `search_text`, `git_status`, `git_commit`, `run_configuration`, `terminal_*` and the
    other tools act on the remote project as they do on a local one (ADR-0008). The stdio MCP server
    reaches a headless remote project the same way with `remote_backend_call {host, folder, tool,
    arguments}` naming an open remote project (no second backend is started). [ADR-0008, SPEC-0053]
30. **MCP tools for the new functions:** `remote_ports` (detected ports and forwards of an open
    remote project), `remote_port_forward` (`dry_run` default true; a real forward needs the `ports`
    right of SPEC-0053; debug ports refused, database ports refused for agents) and
    `remote_port_close`. [ADR-0008]

## Acceptance criteria
- **AK-1** (`crates/host`, in-process backend) Each family is in the capabilities; every method
  behind a right is refused with `not_granted` naming it on a session without that right, nothing is
  done, and the refusal is audited (SEC-4).
- **AK-2** Language server against a real server where installed (clangd, as in CI): diagnostics
  follow an edit made through `doc.edit` without the window sending any text (`lsp.diagnostics`
  notification with the new revision); completion, hover, definition (relative `doc`), semantic
  tokens, rename plan and applied rename (open document edited by author `lsp`, closed file written),
  `stale_revision` for an old revision; a `file:` URI or path outside the project is refused with
  `invalid` before a server sees it, and a location outside the project comes back as `outside` only
  (SEC-14). Skipped without clangd, failing under `FERRIS_IDEA_REQUIRE_TOOLS`.
- **AK-3** Search: `search.text` finds unsaved text of an open document and saved text of others,
  respects `mask`, `max` and `truncated`; `search.everywhere` and `search.files` return relative
  paths only. (v8) `search.text` searches the project's own sources, the rest with
  `include_non_project`, a named `folder` or named `files`, and says what it left out (SPEC-0029
  AK-11).
- **AK-4** Git on a test repository: status, base, diff, log, branches; commit and branch creation
  with `vcs-write`; `git.changed` after a commit by another process.
- **AK-5** Terminal: a shell on the host echoes input sent as stream data, follows a resize
  (`stty size`), survives a break (output written meanwhile arrives after `term.attach`), reports
  `term.exited`, ends with `session.bye`; `env` with a bad name is refused.
- **AK-6** Run: configurations listed, a run's standard output and error on their streams, a test
  configuration's report as a `tests` event, stop ends the process group, a run survives a break and
  `run.attach` replays its output; an untrusted project is refused until `run.trust`.
- **AK-7** Ports: a program started in a host terminal that listens on a port is announced with
  `port.detected`; a forward relays an HTTP request from a local client to it; the local listener is
  bound to loopback only (SEC-7: the listening sockets of the test process during the forward are
  all loopback); a JDWP-style debug port is announced as `debug` and refused for forwarding; a
  database port needs `confirm`; closing the forward closes listener and connections.
- **AK-8** Dap foundation: `dap.adapters` lists the host's adapters; `dap.start` of a test adapter
  joins its standard input and output to the stream (bytes in, bytes out), no TCP listener is opened,
  `dap.stop` ends it.
- **AK-9** MCP: `mcp.tools` lists the host's tools; `mcp.call` runs `search_text` and `git_status`
  on the host project with the host's rights (a tool needing an ungranted right is refused there);
  `remote_backend_call` with an open remote project goes through the session; `remote_ports`,
  `remote_port_forward` (dry run, refusal without `ports`, debug port refused).
- **AK-10** Against the test `sshd` (as SPEC-0317 AK-3): the uploaded backend binds the families;
  diagnostics of a C file (clangd), a terminal and a run on the host, and a forward, through the SSH
  channel; SEC-7 for the backend process (no TCP listener) and the window side (only loopback);
  SEC-10: no resume token, key or other secret in the arguments and environment of the shell, the
  run, the adapter and the language server started on the host.
- **AK-11** Window self-test (written, run with the window check): diagnostics and completion in a
  remote C document, ⌘B to a definition in another remote file, rename, a host terminal tab, a run
  with its output, an offered port with Forward, the trust banner of an untrusted host project.
- **AK-12** Hand test and side-by-side comparison with the reference's remote development after the
  observation (open points below).

## Remaining host actions (v5)
Numbers go on from behaviour 30. A remote project keeps the dialogs of a local one; only the work
moves to the host. [GEN]

### Git over the network
31. **Methods.** `git.fetch {remote}` (default: every remote; `remote` must be a configured remote
    of the repository, so a URL or an option can never be passed), `git.pull {rebase, dry_run}`,
    `git.reset_upstream {}` (Update Project's reset, SPEC-0144 v4 behaviour 9), `git.outgoing
    {branch, up_to}` (what Push would send, SPEC-0109) and `git.push {branch, target, set_upstream,
    tags, force_with_lease, up_to}`. Results are the structures of SPEC-0055 and SPEC-0144. A plain
    force push does not exist on the host either (IMP-0029). Rights: `vcs-write` for fetch, pull and
    reset, `view` for outgoing, **`push`** for push; all but `git.outgoing` are audited.
    [SPEC-0055, SPEC-0109, SPEC-0144]
32. **Credentials stay on the host.** Ferris IDEA neither transports nor stores a Git credential:
    nothing about them is in any request or answer, and the backend adds nothing to the environment
    of `git`. `git` runs on the host with the host's own `ssh-agent`, keys and credential helpers
    (the `git` executable of SPEC-0055). It never prompts (no terminal, no askpass program, `ssh` in
    batch mode). A missing credential comes back as `auth_required: true`, and the window says
    "… the remote needs credentials; set up an SSH key or a credential helper on <host>". The agent
    of the window's machine is not forwarded for this. [ADR-0025, SPEC-0055]
33. **Post-quantum rules.** Git over SSH on the host uses the options of ADR-0025 through
    `GIT_SSH_COMMAND` as a local operation does: strict host keys, the required post-quantum key
    exchange on every hop, the user's `core.sshCommand` replaced. A host or client without it comes
    back as `ssh_refusal` and nothing is transferred. The Azure DevOps exception of ADR-0025 section
    1a follows the settings **of the host** (read by `ferris-idea serve` at the start); its use is
    reported with `pq: false` and `pq_notice`, which the window shows once per session, and logged
    on the host. [ADR-0025 §1a]
34. **Nothing secret in answers.** `output`, `messages` and `ssh_refusal` have any `user:password@`
    of a URL removed and the project's path replaced by `.`. [ADR-0025, SEC-10]

### Rollback, stash and the log's actions
35. `git.rollback {paths, delete_added, dry_run}` (right `vcs-write`, audited) rolls project-relative
    paths back as SPEC-0063 v5 behaviour 10. An open document without unsaved changes follows through
    the session; one with unsaved changes asks like every other change on the host (SPEC-0317), so
    no typed text is dropped silently. [SPEC-0063]
36. `git.stashes {}`, `git.stash_files {index}` (`view`) and `git.stash_push {message,
    include_untracked, keep_index}`, `git.stash_apply {index, pop}`, `git.stash_drop {index}`
    (`vcs-write`, audited). The stash popup, the stash dialog and the stash's diff use them.
    [SPEC-0106]
37. **The log's commit menu.** `git.commit_action {action, id, mode, name, message}` (`vcs-write`,
    audited) with the action ids of SPEC-0086: `cherry-pick`, `checkout`, `reset` (`mode`, the hard
    reset keeping changes in a stash as IMP-0320), `revert`, `undo`, `drop`, `new-branch`, `new-tag`,
    `edit-message` (`message`). Reading (`view`): `git.patch {id}`, `git.commit_diff {id, path}`,
    `git.fixup_message {id, squash}`, `git.files_at {id}`, `git.text_at {id, path}`,
    `git.worktree_changes {id}`, `git.worktree_text {path}` (the document's text, else the saved file). Rebase from Here and Edit Commit Message in the interactive dialog
    use `git.rebase_todo {onto}` and `git.rebase {onto, entries}` (`vcs-write`, audited); the dialog
    shows no commit details on the side for a remote project. Show Repository at Revision and Compare
    with Local show the host's files and texts; Compare with Local compares with the saved files on
    the host. [SPEC-0086, SPEC-0145]
38. **Stops with conflicts.** An operation that stops with conflicts reports them; the merge view
    of a remote project does not exist yet, so the window names the files and continuing is done in
    a host terminal. [SPEC-0101]
39. **Trust (extends behaviour 5).** Everything in behaviours 31 and 36–37 that starts a `git`
    process reads the repository's configuration, and that may name programs (hooks, filter and
    credential helpers, `core.fsmonitor`, an `ext::` remote). For a host project that is not trusted
    these methods are refused with the trust message and start nothing; the Git library's
    `git.status`, `git.diff`, `git.log`, `git.show`, `git.branches` and `git.rollback` stay
    available, because they run no process. Deviation from a local project in the
    more secure direction: AGENTS.md rule 12. [SPEC-0278]

### Search
40. **Comments/strings filter.** `search.text {context}` with `context` one of `anywhere`,
    `in_comments`, `in_strings`, `except_comments`, `except_strings`,
    `except_comments_and_strings` (default `anywhere`; another value is `invalid`) keeps the matches
    that lie where it allows, on the text searched (unsaved text included), before `max` is counted.
    The Find in Files panel sends its choice. [SPEC-0066 v3 behaviour 11]
41. **Preview of hits.** `search.excerpt {path, line, context}` (`view`, `context` at most 20)
    returns `{first, lines}`: the lines around `line`, the backend's text of a document with unsaved
    changes first, `invalid` for a path that leaves the project, `not_found` for a binary, too large
    or missing file. The Search Everywhere preview shows it for every hit with a path, with the same
    context as locally; until it arrives a text hit shows the line the host sent. [SPEC-0031 v5
    behaviour 12]

### Services
42. **Ports node.** In a remote project the Services tool window has a tab **Ports** next to Output:
    one row per detected port and per open forward, with the rules of v2 details 10–11 and the same
    registry as the list: Forward (none for a debug port; a database port asks first), Open in
    Browser and Stop Forwarding. It follows `port.detected`, `port.gone` and the window's own
    forwards. [ADR-0025 §6]

### Agents
43. The host's MCP tools `git_fetch`, `git_pull`, `git_push`, `git_rollback`, `git_stash`,
    `git_cherry_pick` and `search_in_files` reach a remote project through `mcp.call` and
    `remote_backend_call` (behaviour 29). They keep the agent rights of the host project. [ADR-0008]

### Branches and the log's path filter (v6)
44. **Merge and rebase.** `git.merge {source}` merges into the current branch with the `git` program;
    `git.rebase_onto {onto, branch?}` replays the current branch, or checks out local branch `branch`
    first (Checkout and Rebase of the popup). Both `vcs-write`, audited; they stop with the conflicts
    as behaviour 38 and refuse while another operation is in progress. Local changes that would be
    overwritten are refused with the files named. [SPEC-0068, SPEC-0106, SPEC-0036]
45. **Rename.** `git.rename_branch {old, new}` (`vcs-write`, audited); an existing `new` is refused.
    [SPEC-0068]
46. **Delete.** `git.delete_branch {name}` (`vcs-write`, audited) deletes a merged local branch; never
    forced, never the current branch. Behaviours 44–46 start a `git` process and fall under
    behaviour 39; names and revisions may not look like options, as behaviour 34. [SPEC-0068]
47. **Log path filter.** `git.commits_touching {ids, paths}` (`view`) answers `{ids}`, the commits of
    `ids` that change one of the project-relative `paths`, in the order of `ids`, at most 5000 commits.
    Paths are checked as paths only (`..` and absolute paths `invalid`), since a deleted file's history
    is what the filter is for. Git library only: it needs no trust (behaviour 39). The log's path
    filter uses it. [SPEC-0086 v5 behaviour 10]

### Acceptance (v5)
- **AK-15** (`crates/host`, in-process backend): fetch, pull and push against a bare repository in
  the test's folder; a rejected ref update reported; a credential the host lacks (a URL needing a
  password) comes back `auth_required` without prompting and without the password anywhere in the
  answer; `remote` that is no configured remote is `invalid`; `git.push` needs `push`, the others
  `vcs-write` (SEC-4).
- **AK-16** Rollback (a modified and an added file, a path leaving the project refused), stash
  push/list/files/apply/drop, cherry-pick, revert, hard reset keeping a stash, new tag and branch, a
  revision starting with `-` refused.
- **AK-17** An untrusted project: every process-starting method refused, a hook never runs, no
  `git` started; `git.status` and `git.log` still answer.
- **AK-18** `search.text` with each `context` over a Rust file with the word in code, a comment and
  a string, unsaved text counted, an unknown `context` refused; `search.excerpt` gives the lines
  around a line, the unsaved text first, refuses a path outside.
- **AK-19** Window (written, run with the window check): the steps of behaviours 31–42 in
  `remote_tool_window_steps`.
- **AK-20** Against the test `sshd`: the uploaded backend answers `git.fetch` for a repository on
  the host and reports `ssh_refusal`/`auth_required` over SSH remotes; run with the window check.

- **AK-21 (v6)** (`crates/host`, in-process backend): merge, rebase with a branch, rename and delete of a
  merged branch; a merge stopping on conflicts reports them; a name or revision like an option refused.
- **AK-22 (v6)** The four methods need `vcs-write`; an untrusted project refuses them and starts nothing;
  `git.commits_touching` still answers.
- **AK-23 (v6)** `git.commits_touching` for a project in a sub-folder of its repository: a file, a folder and
  a deleted file; the path outside refused; `ids` missing is `invalid`.

## Security (ADR-0025 section 9)
- **SEC-1, SEC-3, SEC-6, SEC-12, SEC-13, SEC-15, SEC-16:** not applicable — this spec adds no
  channel; it runs inside the SSH channel of SPEC-0317 (paired channels and relays are M6.14/M16).
- **SEC-2, SEC-5, SEC-8, SEC-9, SEC-11, SEC-17:** unchanged from SPEC-0317 (same channel, same
  backend start; covered by its AK-3–AK-5 and `pq_kex.rs`).
- **SEC-4** missing right per method: AK-1, AK-9, AK-15.
- **SEC-7** exposed ports: AK-7, AK-8, AK-10 — the backend opens no TCP listener (language servers
  and adapters on standard input and output, forwarded targets reached by connecting on the host),
  the window's forwards listen on loopback only, debug ports are refused.
- **SEC-10** secrets: AK-10 (process arguments and environments on the host), plus the audit log
  holding no content; Git credentials never cross the session (behaviours 32 and 34, AK-15).
- **SEC-14** host cannot escalate: AK-2 (URIs and paths outside the project refused in both
  directions, outside locations without host paths), AK-7 (the backend cannot open a local
  listener: `port.detected` is an offer only).

## Protocol details (deviations from ADR-0026, with reasons)
1. **`lsp` methods are structured, not raw LSP pass-through.** ADR-0026 §6 lets LSP payloads pass
   unchanged with `lsp.request {server, method, params}`. This spec binds named methods
   (`lsp.complete`, `lsp.hover`, …) that run on the host with the backend's text and answer with
   Ferris IDEA's parsed results, because (a) every host path and URI is translated to a relative path
   or refused in one place on the host instead of in each window feature (behaviour 3, SEC-14 —
   security first); (b) position units, encodings and server quirks (SPEC-0042) are handled where the
   server runs; (c) the window's editor already consumes these parsed results. A raw `lsp.request`
   may be added later as a minor change if a client needs it; it would get the same URI checks.
2. **New families `search`, `git` and `mcp`** beside ADR-0026's list: they are capabilities like the
   others; `event.subscribe` kinds stay for later (diagnostics and Git state arrive as family
   notifications). [ADR-0026 §6 "a spec adds methods within these families"]
3. **Run output on two streams** (`stdout`, `stderr`) instead of one, so the window keeps the error
   colour without framing inside the stream.
4. **Terminal and run streams belong to one connection**; after a resume `term.attach`/`run.attach`
   open new streams (with the kept output) instead of carrying stream ids across connections, so
   credit stays per connection (ADR-0026 §9).

5. **Reattaching at an offset (v2):** `term.attach {term, from}` and `run.attach {run,
   stdout_from, stderr_from}` take the number of bytes of the output the window already has; the
   new stream starts there when it is still kept, else at the oldest kept byte, and the answer
   names the start it took (`from`, `stdout_from`, `stderr_from`). Without them the stream starts
   with everything kept, as in v1. Attaching loses nothing that is produced while the kept output
   is sent. Reason: a terminal screen and a run console go on without repeating or losing output.
6. **Match offsets (v2):** each match of `search.text` also carries `start`, its character offset
   in the file's text, so the window selects it exactly in the document.
7. **`git.show {id}` (v2, right `view`):** the commit's details — info, message, parents, changed
   files with relative paths (behaviour 3) — for the Git tool window. Reason: the details pane of
   the log is a view function the host already has.
8. **Highlight positions (v2):** the hits of `search.everywhere` and `search.files` carry their
   matched character `positions`, as the window's own search does.

## Window details fixed in v2 (with reasons)
Connecting the tool windows fixed these points; behaviours 1–30 stay the rule otherwise. The rule
of all of them: a remote project is used like a local one — the same tool windows, keys and look —
only the data comes from the session. Where a host function does not exist yet the control stays
where it is and says why, instead of acting on this machine. [GEN]

1. **No local language services for remote text:** the window's language server pool, ESLint,
   usage counts, code authors and parameter hints never get the text of a remote document (the
   stand-in folder has no project, and the text is the host's); the window's own inspections are
   not shown for it. What a local file gets from them comes from the host (behaviour 11, details
   8–9). The syntax tree for highlighting and folding is still built in the window. [ADR-0025 T5]
2. **Terminal tabs:** the Terminal tool window of a remote project opens host terminals
   (`term.open`, folder: the project folder) for ⌥F12, ⌘T and the splits; the tabs are named after
   the host (`build`, `build (2)` …, renamable). The shell's folder is not known to the window, so
   a split starts in the project folder too. While the connection is broken a tab shows
   "(Reconnecting)" after its name and keeps its screen; after the resume the window reattaches
   (`term.attach` with the offset it has, protocol detail 5) and the screen goes on where it
   stopped; when output was lost meanwhile the screen is redrawn from the kept output. Typing
   during the break is kept (up to 64 KiB) and sent after the resume. `exit` closes the tab as for
   a local shell; a terminal that ended with its session keeps its text with "Ended with the
   session". The saved terminal tabs of SPEC-0205 are not restored for a remote project: host
   terminals live with their session. [SPEC-0062, SPEC-0205]
3. **Run configurations and the Run window:** the run widget and Run… list `run.configs`, read
   when the window opens, after `run.trust` and each time Run or the chooser opens; Run, Stop and
   Rerun work on host runs as on local ones, standard error in the error colour, the test tree from
   the `tests` event. After a break the window reattaches each running run (`run.attach` with the
   bytes it has per stream); lost output is noted in the console ("… N bytes of output were lost
   during the break"). Debugging on the host comes with M6.13: Debug says so instead of starting
   anything on this machine. [SPEC-0087, SPEC-0088]
4. **Run markers and ⌃⇧R:** the ▶ markers are computed in the window from the document's text as
   for local files (Java, Kotlin, `package.json`); their Run item and ⌃⇧R send `run.start
   {context: {doc, line}}` with the marker's or the caret's line, so the host decides what runs
   (behaviour 22). [SPEC-0097]
5. **Find in Files:** the panel searches the host (`search.text` with query, mask, options, the
   Directory scope as `folder`); results, preview (the match's line from the host) and opening
   (exact selection, protocol detail 6) as for a local project; the Open Files scope keeps the
   host's matches of the open documents (v8: it sends them as `files`, and the toggle "Libraries
   and ignored files" as `include_non_project`; the host's `not_searched` line is shown in the
   head, SPEC-0066 v6 behaviour 18); the comments/strings filter is applied on the host
   (v5 behaviour 40); Replace in Files is out of scope and says so. [SPEC-0029,
   SPEC-0066]
6. **Search Everywhere, Go to File / Class / Symbol:** files, classes, symbols and text are the
   host's (`search.everywhere`), with highlighted positions (protocol detail 8); actions, settings
   pages and the calculator stay the window's own. The preview: v5 behaviour 41. [SPEC-0031]
7. **Commit and Git tool windows:** the Commit window lists `git.status` and commits with
   `git.commit` after saving the remote documents on the host; Amend works; hooks follow the
   host's trust (behaviour 5; the note is shown). The Git window shows `git.log` and
   `git.branches`, the selected commit's details from `git.show` (protocol detail 7); Checkout and
   New Branch in the Branches popup and Checkout in the Git window's branches use `git.checkout`
   (with `create`). The status bar shows the host branch (`git.status`, `git.changed`). Rollback, Push, Fetch, Pull, stash and the commit actions of the log run
   on the host since v5, merging, rebasing, renaming and deleting branches from the Branches popup
   since v6; the merge view for conflicts, Compare with, Show Diff with Working Tree and Checkout
   Revision are not on the host yet and say "Not available for a remote project yet". [SPEC-0063, SPEC-0064, SPEC-0068, SPEC-0086]
8. **Language functions:** Rename (⇧F6) plans and applies on the host (`lsp.rename` with `apply:
   true`, behaviour 12): the editors take the host's edits through the session. ⌥⏎ lists
   Ferris IDEA's own fixes at the caret (computed in the window on the document's text, applied as
   editor edits) and the host server's actions (`lsp.code_actions`); an action's preview lists the
   files it changes (`apply: false`), applying sends `lsp.apply_action` with `apply: true`.
   Parameter info (⌘P, and after `(`) asks `lsp.signature`. [SPEC-0074, SPEC-0076, SPEC-0080]
9. **Semantic colours and inspections:** after each changed revision (at most one question per
   document at a time, asked again when the text moved on meanwhile) the window asks
   `lsp.semantic` and `lsp.inspect` and shows the answers as for a local file; the colour-scheme
   keys are computed in the window from the token types. The host's findings replace the window's
   own inspections for a remote document (the host's profile and dictionary count, behaviour 9).
   [SPEC-0290, SPEC-0092]
10. **Port offers:** a `port.detected` of kind `service` or `database` shows a banner over the
    editors: "Port 8080 on <host> is listening (<command>)" with Forward and Ignore; a `debug` port
    shows "Port 5005 on <host> is a debug port: the debugger reaches it through the session" with
    Ignore only. A port is offered once per session. Forward of a `database` port asks first with
    the warning of behaviour 26 (Forward Anyway, Cancel). [ADR-0025 §6]
11. **Ports list:** "Ports…" in the connection widget's menu and the action `Remote.Ports` list the
    detected ports (port, kind, command) and the open forwards (host port → local address);
    choosing a port forwards it (or says why not), choosing a forward offers Open in Browser
    (`http://127.0.0.1:<local port>`) and Stop Forwarding. The Services tool window shows them too (v5 behaviour 42); the list is the same
    `ferris_idea_remote::ports` registry agents use. Forwards close with the window. [ADR-0025 §6]
12. **Trust banner:** when `run.configs` says the host project is not trusted, the window shows the
    safe-mode banner: "<folder> on <host> is not trusted: nothing of it runs on the host — run
    configurations and Git hooks are skipped, language servers run in their safe configuration"
    with "Trust Project…", which asks once more and sends `run.trust`; then the configurations are
    read again and the banner goes. Run and the markers give the host's trust message until then.
    The window's own trust list is never consulted for the host folder (behaviour 5). [SPEC-0278]
13. **Agents:** `remote_projects` lists for each project of a window its host terminals (tab name,
    `term` id, state), its runs (name, `run` id, running, exit code, failed tests) and its forwards;
    `terminal_read` and `run_output` take `host` and `folder` to read the tabs of a remote project's
    window as they read local ones (same rights, same masking); `remote_ports` and
    `remote_backend_call` stay as in v1. [ADR-0008]
14. **Acceptance (v2):** AK-11 grows by the window steps for details 2–12 (written, run with the
    window check); **AK-13** (`crates/remote`, in-process backend): a host terminal link echoes
    input, reattaches after a break at its offset without repeating output and keeps typing made
    during the break; a host run handle delivers start, output of both streams, the test report and
    the end as a local run handle does, stops, and reattaches after a break; **AK-14**
    (`crates/host`): attaching at an offset sends exactly the bytes after it and nothing is lost
    while the kept output is sent; `search.text` matches carry `start`; `git.show` answers with
    relative paths.

## Window check (v3)
The window check of 2026-11-07 ran details 2–12 in the window (self-test group
`FERRIS_IDEA_SELFTEST_ONLY=remote-project` with `FERRIS_IDEA_SELFTEST_HOLD_MS`, a screenshot of each
point). It fixed these points; details 2–12 stay the rule otherwise. [GEN]

1. **No local safe mode for a remote project (detail 12):** the window's own trust check of the
   stand-in folder never runs for a remote project — neither when the window opens nor when it
   becomes active again — so the local banner "Safe mode: this project's run configurations …"
   never shows there; only the host's banner does.
2. **Database ports (detail 10):** the warning before forwarding a database port has Cancel
   preselected; ⏎ alone never exposes a database port (security first, AGENTS.md rule 12).
3. **Ports list (detail 11):** a forward's row reads "Port <host port> → 127.0.0.1:<local port>
   (forwarded)"; the host is named in the list's title, so the loopback address is never cut off.
4. **Git window (detail 7):** the Git window of a remote project shows only the host's history: a
   read of this machine's stand-in folder that ends after the host's read is dropped, and the
   window never reads again because this machine's refs moved (the host's change event does it).
5. **Run (detail 3):** a host run started from the window leaves no "Running … on <host>" status
   line behind; the Run window shows it running and ending, as for a local run.
6. **Acceptance:** AK-11 grows by the window steps of the trust banner with its confirmation, the
   test tree of a host test run (TAP), Stop of a host run, a fix of the host's server applied with
   ⌥⏎, the Ports list and the database-port warning.

## Out of scope
Replace in files, the merge view and branch merge/rebase/rename/delete on the host (v2 detail 7), build
tool sync windows and database connections from the host (later versions), the window's debugger
on host adapters, remote attach and source mapping (M6.13), several windows on one session, hosts
of another platform (M6.14), paired devices (M16).

## Known deviations from the reference
Not compared yet. Decided independently of the reference and kept unless the comparison shows a
better behaviour: no port of a language server, debug adapter or database is ever exposed on either
machine; debug ports are not offered as forwards; forwards listen on loopback only; agents act with
the host project's rights.

## Open points — compare after the OBS on the reference's remote development
1. Which functions the reference runs on the host and which in the window; latency of completion and
   diagnostics over a slow connection.
2. How the reference offers and lists forwarded ports, and what it does with debug and database
   ports.
3. Terminals and running programs during a network break and after it.
4. Git on the host: which actions, and how hooks and credentials are handled.
