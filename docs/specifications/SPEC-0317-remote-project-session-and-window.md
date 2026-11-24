# SPEC-0317: Remote project — session and window

- **Version:** 9 · **Date:** 2026-11-15 (v9: closing is final when it meets a round of the supervisor or a reconnection, see "Details fixed in v9"); 2026-11-13 (v8: Disconnect stays when it meets a round of the supervisor, see "Details fixed in v8"; v7: the time the host may take for the backend's checksum follows the build's size, a simulated break returns once the session is disconnected, found on a busy Linux host; v6: the tree's root row and the path actions name the host folder; see "Details fixed in v6". v5: when an editor meets its replica, numbered notifications that overtake each other, host paths in popups; see "Details fixed in v5". v1 written before the observation of the reference's
  remote development; the open points at the end are compared after it. v2: the headless part of
  steps 2 and 3 is built — protocol in its own crate, acknowledgements in the event stream, one
  edit in flight per document and client, attach and state options of `serve`, MCP without a
  window; see "Protocol details fixed in v2". v3: the window integration of step 3 — entry
  points, stand-in folder, editor reconciliation, supervisor and reconnection schedule, recovery
  writer, MCP on window projects; see "Window details fixed in v3". v4: the SSH channel requires a
  post-quantum key exchange, ADR-0025 v3, PO-STATUS question 29)
- **Sources:** GEN (remote development: the code and the backend stay on a host, the window stays
  local and keeps typing fast; editors keep unsaved text across network breaks; F-083 "Remote"), STD
  (OpenSSH client as in SPEC-0121; JSON-RPC 2.0; operational transformation for a single sequencing
  server, as in the Jupiter collaboration system; POSIX `rename`, `getpeereid`/`SO_PEERCRED`, Unix
  domain sockets; SHA-256), ADR-0025 (network security), ADR-0026 (session protocol), ADR-0008,
  ADR-0012, SPEC-0003, SPEC-0044, SPEC-0053, SPEC-0058, SPEC-0059, SPEC-0094, SPEC-0121, SPEC-0158,
  SPEC-0220, SPEC-0262
- **Feature matrix rows:** F-083
- **Roadmap:** M6.12 steps 2 and 3 (session protocol, window); M10.3 (`ferris-idea serve`, first part)

## Purpose
Open a folder on an SSH host as a normal project: the project tree and the editor show the host's
files, the text is owned by Ferris IDEA's backend on the host, typing stays as fast as locally, and a
network break or a closed laptop lid does not lose a single unsaved character.

## Behaviour

### Opening a remote project
1. **Entry points:** "Remote Development…" on the welcome screen next to the other ways to open a
   project (SPEC-0094) and in File. The dialog lists the hosts of `~/.ssh/config` with user, host
   name and port as SPEC-0121 behaviour 1 resolves them, takes `[user@]host[:port]`, and lists the
   recent remote projects (host and folder, newest first). Recent remote projects are kept in the
   user's settings folder with the other recent projects, without credentials. [GEN, SPEC-0121]
2. **Folder on the host:** after a host is chosen, the dialog shows the host's home folder and lets
   the user go into folders or type a path (SPEC-0121 behaviour 5 rules); "Open" takes the folder
   shown. Browsing uses SPEC-0121's POSIX listing, because no backend runs yet. [GEN, SPEC-0121]
3. **Connecting** shows its steps with a cancel button: connect (10 s limit), check or bring the
   backend (step 4), start it, open the session. Each step's failure is said with what to do
   (SPEC-0121 behaviour 2 messages for host keys, login and reachability; ADR-0025 section 8). Cancel
   ends whatever was started on the host. [GEN, ADR-0025]
4. **Backend:** the backend is `ferris-idea serve --stdio` of the window's own version and build, found
   or uploaded as SPEC-0158 behaviour 2 says. After an upload, and before each start of an uploaded
   copy, the window compares the copy's SHA-256 with its own binary's (computed by the window, not
   taken from the host); a mismatch deletes the copy and refuses with "the backend on the host does
   not match this build". A host of another platform is refused as in SPEC-0158 (other platforms:
   M6.14). [GEN, STD]
   **(v7)** The host may take as long for the checksum as its size needs: the usual 30 s of a call
   and one second for each started 16 MiB of the build (a 2.5 GiB debug build needed 39 s on a
   busy host, so the open failed as timed out). The window computes its own build's checksum
   reading it in blocks, not all of it into memory at once. [GEN]
5. **Session:** the window sends `session.hello` (ADR-0026 section 4) with the project folder; the
   backend opens the folder as its workspace, answers with session id, resume token, rights and
   capabilities. A folder that does not exist or cannot be read is refused with `not_found` or
   `denied` and the dialog stays open. [ADR-0026]

### The project window
6. **Title and status bar:** the window's title shows the folder name and `— host` (the alias, or
   `user@host`); the status bar shows a connection widget with the host and a state: *Connected*
   (with the round-trip time of the last heartbeat), *Reconnecting* (with the attempt and the time
   left in the reattach period), *Disconnected*. Clicking it opens a menu: Reconnect, Disconnect,
   Show Connection Log, Backend Info (version, build, host platform, folder, session id — never the
   resume token). [GEN]
7. **Project tree:** the tree shows the host folder through `fs.list` and follows `fs.watch` events
   (ADR-0026 section 6): files created, changed, renamed or deleted on the host appear within 1 s of
   the event. Excluded folders follow the project settings as for local projects (SPEC-0059).
   Creating, renaming and deleting through the tree go through `fs.*` on the backend. [GEN]
8. **Editor:** opening a file opens a remote document (`doc.open`, ADR-0026 section 5). There is no
   local copy as the source of truth (unlike SPEC-0121 behaviour 4). Highlighting, search in the
   file, undo and redo work on the window's replica as for local files; local history is kept on
   the host. Files larger than the editor's limit for local files are refused in the same way.
   [GEN, ADR-0026]
9. **Typing is local:** every keystroke changes the window's text at once; the edit goes to the
   backend as an operation and waits in the pending queue for its acknowledgement. Edits of other
   clients and agents (`doc.changed`) are merged into the text without moving the caret except where
   text before it changed. The tab of a document with unacknowledged edits shows the usual unsaved
   marker; the connection widget's tooltip shows the number of pending edits. [GEN, ADR-0026]
10. **Saving** (⌘S, Save All, saving on frame deactivation if set) sends `doc.save`; the status bar
    says "Saved on host". A file changed on the host since it was loaded is not overwritten: the
    window offers compare, overwrite or reload as SPEC-0058 does for local files. A file changed on
    the host while unmodified in the window is reloaded silently, as locally. [GEN, SPEC-0058]
11. **Closing the project** asks about unsaved remote files as for local files (SPEC-0261), then
    sends `session.bye`; the backend ends when no client is left. Quitting Ferris IDEA does the same
    for every remote project window. [GEN]

### Network breaks and reconnection
12. **While disconnected** every editor stays editable; new edits join the pending queue. Saving
    says "Not saved — reconnecting to <host>; the file is saved when the connection is back" and
    keeps the file marked unsaved; a save requested while disconnected is carried out after the
    resume. Tree operations and other host functions are refused with the same reason. Nothing that
    needs the host fails silently. [GEN, ADR-0026 section 8]
13. **Recovery copy:** while a remote document has unsaved or unacknowledged edits, the window keeps
    a recovery copy (host, folder, path, base revision, base hash, pending edits, current text) in
    its own state folder (mode 0700, files 0600), written by SPEC-0262's write-then-rename method,
    and removes it when the edits are saved on the host. After a crash of the window, opening the
    remote project again offers to restore each copy (behaviour 15 decides how). [GEN, SPEC-0262]
14. **Reconnecting:** after a break the window reconnects by itself, after 1, 2, 4, 8, 16 and then
    every 30 s, as long as the backend's reattach period lasts (ADR-0025 section 8, default 10
    minutes); Reconnect in the widget's menu tries at once. Over SSH it runs `ferris-idea serve --attach
    <session id>` on the host, which bridges standard input and output to the backend's reattach
    socket, and sends `session.resume` with the token as the first message. The backend replays the
    missed notifications, the window resends its pending edits (applied once), and the documents,
    tree and widget show the current state again. A wrong token, another user on the socket or an
    expired session is refused (ADR-0025). [ADR-0025, ADR-0026]
15. **Backend gone:** if the session cannot be resumed (reattach period over, host restarted), the
    window starts a new backend and, for each document with unsaved state, compares the host file
    with the base hash: equal → it reopens the document and sends its edits; different → the
    window's text stays as an unsaved version and the user gets the compare choice of behaviour 10.
    The connection log records which documents went which way. No text is discarded without the
    user's choice. [GEN, ADR-0026]
16. **Sleep and wake:** a laptop that sleeps is a network break: after waking the window reconnects
    as in behaviour 14 without a message unless the reattach period passed. [GEN]

### Security (ADR-0025)
17. **Channel:** only the SSH channel of ADR-0025 section 1a in this spec, with SPEC-0121's options
    (batch mode, strict host keys, the user's keys and agent, no passwords through Ferris IDEA, a
    hybrid post-quantum key exchange only — `serve` and `--attach` on a host without one are
    refused with SPEC-0121's message, also on reconnection). The
    backend opens no TCP listener; its only socket is the reattach socket in a 0700 folder with the
    peer user check. The resume token is never on a command line,
    in the environment, in logs or in files; the token is sent only inside the channel. [ADR-0025]
18. **Paths stay in the project:** every path the backend receives or sends is relative to the
    project folder; the window refuses paths from the backend that leave it (`..`, absolute, links
    out of the folder) and writes nothing on the window's machine except its own state and recovery
    files. The backend refuses the same for paths from the window. [ADR-0025 T5]
19. **Rights:** the SSH session has the user's own rights on the host. Agent rights on the host
    stay those of the host project (SPEC-0053, SPEC-0158 behaviour 3). Opening, resuming, saving,
    tree changes and ending the session are written to `remote/audit.jsonl` in the user state
    folder on the host (time, peer `ssh:<user>@<window host>`, method, path, outcome; never
    content). [ADR-0025 section 4]

### Agents
20. **MCP:** new tools in the window's MCP server (SPEC-0221) and the stdio server where a window is
    not needed:
    - `remote_project_open` — host and folder; `dry_run` default true returns the plan (host as
      resolved, platform, backend found/upload needed, checksum, folder). A real open needs the
      user-granted rights `ssh` and `run` (SPEC-0053) and is logged; it opens a project window.
    - `remote_projects` — the open remote projects: host, folder, state (connected, reconnecting
      with time left, disconnected), round trip, backend version, number of pending edits,
      documents with unsaved state. Never the token.
    - `remote_project_reconnect` — tries to resume at once; the result says what happened
      (resumed, new backend with documents reconciled or left for the user, refused with reason).
    - `remote_project_close` — `dry_run` default true lists unsaved remote files and pending
      edits; a real close takes `unsaved` = `refuse` (default: nothing happens while files are
      unsaved), `save` (needs `write`) or `discard` (needs `quit`, as losing unsaved work does in
      SPEC-0261 v2).
    - `remote_documents` — the open documents of a remote project with revision, pending edits,
      modified flag and whether a recovery copy exists.
    The existing file and editor tools act on a remote project's documents through the session when
    they are called on that project's route (M6.12 step 6, a later spec); until then they answer
    with an error that names `remote_backend_call`. [ADR-0008, SPEC-0053]

## Acceptance criteria
- **AK-1** Unit (`crates/session`, in-process transport): frame encoding and the
  1 MiB cap, malformed frames close the connection; `session.hello` version choice and
  `unsupported_version`; JSON-RPC errors with stable codes; `$/cancel` and deadlines.
- **AK-2** Unit and property tests (`crates/session`; seeded random runs, no extra test crate) of
  the document operations: transformation of concurrent inserts
  and deletes (same offset, overlapping ranges, Unicode scalars beyond the BMP); random sequences
  from two clients and an agent converge to the same text and hash; a resent `(client, cseq)` is
  applied once; a hash mismatch leads to `doc.snapshot` without losing either text.
- **AK-3** Against a real `sshd` started as the user with keys made for the test (as SPEC-0121
  AK-2): open a folder as a remote project with upload and checksum check, list the tree, open,
  edit and save a file (mode kept, local history on the host), a host-side change while unmodified
  reloads, while modified is refused for compare; a host-side create appears through `fs.watch`.
- **AK-4** Reconnection against the same `sshd`: the SSH client process is killed during typing;
  edits made while disconnected are kept, the window resumes through `--attach`, the pending edits
  are applied once and the host file after saving equals the window's text; resumption after the
  ring buffer overflowed goes through `session.resync`; after the reattach period (shortened in the
  test) the new backend reconciles one unchanged and one changed file as behaviour 15 says; a
  recovery copy is written while edits are pending and removed after saving.
- **AK-5** Security tests of ADR-0025 section 9 for this spec's channel:
  - SEC-2 unknown and changed host key refused without prompt (`sshd` with a new host key);
  - SEC-4 a method called on a session without the right (paired rights are M16; here: agent tools
    without `ssh`/`run`) refused with `not_granted`;
  - SEC-5 plain JSON and an HTTP request sent to the reattach socket: closed without content;
  - SEC-7 the listening sockets of the backend and the window during a session: none on a network
    interface, only the reattach socket;
  - SEC-8 a replayed `doc.edit` (same `cseq`) applied once; a malformed or oversized frame closes
    the connection;
  - SEC-9 three wrong resume tokens end the waiting session; a stalled `session.hello` is closed
    after 10 s;
  - SEC-10 no token in the arguments or environment of `ssh`, `serve` and `--attach`, in the
    connection log, the audit log, the recovery copies or the recent-projects file; state folder
    0700, files 0600;
  - SEC-11 the reattach socket refuses a peer with another user id (peer check unit test) and its
    folder is 0700;
  - SEC-14 paths with `..`, absolute paths and links leaving the folder are refused in both
    directions.
  SEC-1, SEC-3, SEC-6, SEC-12, SEC-13 and (ADR-0025 v2) SEC-15 (hybrid post-quantum handshake)
  and SEC-16 (key file on hosts without a keychain) concern paired channels and relays and are not
  applicable here (SSH channel only); they become mandatory with M6.14's `--pair` and M16.
  SEC-17 (ADR-0025 v3, post-quantum key exchange on the SSH channel) is covered by SPEC-0121 AK-6:
  `serve` starts through the same `Remote` and its shared options, so a classical host is refused
  before anything runs (`crates/remote/tests/it/pq_kex.rs`).
- **AK-6** MCP (`crates/mcp/tests/it/protocol.rs`; the state of an open project through
  `ferris_idea_remote::RemoteProject::describe` against the test `sshd`): `remote_project_open` plan
  without rights and refused without `ssh`/`run`; `remote_projects` and `remote_documents` show state and pending
  edits but no token; `remote_project_close` dry run lists unsaved files.
- **AK-7** Window self-test (v3: with a backend in the self-test's process, so the window steps
  do not depend on the machine's `sshd`; the SSH path of the same handle is tested headless
  against the test `sshd` in AK-3/AK-4): Remote Development in the project popup with the recent
  remote projects, title and status widget, tree, edit and save, connection broken and resumed
  with the widget passing through *Reconnecting*, typing and a pending save during the break, no
  text lost, a host change and a host-side create, a crash of the window with the recovery copy
  offered and restored, closing by an agent.
- **AK-8** Typing latency in a remote document stays within the local budget of SPEC-0260 with
  50 ms and with 300 ms of added round-trip time (test transport with delay), because typing never
  waits for the backend (v3: `crates/remote/tests/it/live.rs`, the synchronous work of a keystroke
  in a debug build against the painted-frame budget 8 / 16 ms at p50 / p95, and below a third of
  the added round trip). **(v7)** The work is the typing thread's CPU time, so a busy machine that
  preempts the thread does not count against the budget (the Linux CI had a wall-time p95 of 17 ms
  with an equally fast keystroke); not waiting is shown by the median wall time staying below half
  the added round trip.
- **AK-9** Hand test and side-by-side comparison with the reference's remote development after the
  observation (open points below).

## Protocol details fixed in v2 (deviations from ADR-0026, with reasons)
Building the protocol without a window fixed these points; ADR-0026 stays the rule otherwise.
1. **Crate:** the protocol, the backend and the client half live in `crates/session`
   (`ferris-idea-session`), not in `crates/remote`: the in-process transport and M10.3 use them without
   SSH. `crates/remote` adds only the SSH half (start, attach, checksum, `RemoteProject`). [GEN]
2. **Acknowledgements in the event stream:** besides the answer to `doc.edit`, the backend sends the
   author `doc.acked {doc, cseq, revision, hash}` as a numbered notification. A window that resumes
   after a break gets its own acknowledgements in order with the other clients' `doc.changed`, so
   replay never applies a change twice or out of order. `doc.changed` also carries the hash of the
   new revision, so a window checks every revision, not only its own. [STD: Jupiter, ADR-0026 §5]
3. **One edit in flight per document and client** (as in Jupiter-style editors): while an edit
   waits for its acknowledgement, later typing is composed into one buffered edit that is sent when
   the acknowledgement arrives. The pending queue is therefore at most two edits per document;
   typing still never waits. A resent edit keeps its `cseq`; a buffered edit gets the next one. [STD]
4. **Edit form:** `ops` are sorted, non-overlapping ranges of the base text (offsets never refer to
   a previous op of the same edit); anything else is `invalid`. The hash is the first 64 bits of
   SHA-256 over the UTF-8 text, written as 16 hexadecimal digits (JSON numbers lose bits above
   2^53). A `base` older than the backend's operation log (10 000 operations per document) is
   answered with `stale_revision`; the window then reads `doc.snapshot`. [GEN]
5. **Resume refusals:** an unknown or expired session is `not_found`, a wrong token `denied`; the
   third wrong token ends the waiting session. When the replay is impossible the answer has
   `resync: true` and the backend also sends `session.resync`; the window then reads each
   document's missed changes with the new request `doc.changes {doc, since}` (revision, ops,
   author, `cseq` from the operation log) and merges them as it would have live, counting its own
   edit among them as acknowledged; only when the log no longer reaches back does it take
   `doc.snapshot` and keep its own text as a local version. The backend also sends
   `doc.saved {doc, revision}` to the other clients of a saved document. [ADR-0025 §8]
6. **`serve` options:** `ferris-idea serve [<folder>] --stdio [--state-dir <dir>] [--run-dir <dir>]
   [--reattach-minutes <n>]` and `ferris-idea serve --attach <session id> [--run-dir <dir>]`. A folder
   on the command line restricts the session to that folder; `session.hello` still names it. The
   state folder (audit log) defaults to the settings folder of SPEC-0053, the run folder (reattach
   sockets) to `$XDG_RUNTIME_DIR/ferris-idea` or `~/.cache/ferris-idea/run`; both are created 0700 and
   refused when another user owns them or others may read them. After its standard input ends,
   the stdio backend keeps running for the reattach period and then exits. [ADR-0025 §5]
7. **Idle and queue limits:** a connection without any frame for 45 s is treated as a break (the
   session waits for reattachment). Queue-level coalescing (ADR-0026 §9) is left for the families
   that need it (diagnostics, progress); `fs.changed` is coalesced at its source over 100 ms per
   path. [ADR-0026 §9]
8. **Rights:** methods are grouped as `view` (`fs.list`, `fs.stat`, `fs.read`, `fs.watch`,
   `doc.open`, `doc.snapshot`, `doc.close`, `event.subscribe`) and `edit` (`doc.edit`, `doc.save`,
   `fs.write`, `fs.create`, `fs.rename`, `fs.delete`); an SSH session has both. A session without
   one is answered `not_granted` naming it, and the refusal is audited. [ADR-0025 §4]
9. **MCP without a window:** in the stdio MCP server a real `remote_project_open` keeps a headless
   session (documents, tree, reconnection) instead of opening a window; the window integration
   (step 3) opens a project window on it. The dry run does not connect: it names the host as
   resolved from the SSH configuration, this build's platform and SHA-256, where the backend is
   looked for and that it is uploaded when missing. [ADR-0008, SPEC-0053]
10. **Families not yet bound:** `lsp`, `dap`, `term`, `run` and `port` are reserved names; until a
    spec binds them the backend answers "method not found" and leaves them out of its capability
    list. Further families register handlers with a right and a priority. [ADR-0026 §6]

## Window details fixed in v3 (with reasons)
Building the window integration fixed these points; behaviours 1–20 stay the rule otherwise.
1. **Entry points:** Ferris IDEA has no welcome screen yet (SPEC-0094, out of scope there), so
   "Remote Development…" is the second row of the project popup (after "Open…") and an item of
   File (`Remote.OpenProject`, also found by Search Everywhere); a welcome screen gets it when it
   comes. The dialog is the Remote Host tool window of SPEC-0121 in a Remote Development mode: the
   recent remote projects come before the hosts, and a folder shown has an "Open … as a remote
   project" row first. Escape while opening stops it; a session that was already started is
   ended (`session.bye`). [GEN]
2. **Recent remote projects:** `remote/recent-remote-projects.json` in the settings folder, host
   (as typed) and folder only, newest first, at most 25, written 0600 (SEC-10). [GEN, ADR-0025]
3. **Stand-in folder:** the window stands for the host folder with a folder of its own state,
   `remote/projects/<key>/<folder name>` (0700; key: hash of host and folder). It takes the
   window's own files (layout, recent files) and nothing of the project: editors of remote
   documents never read or write this machine's disk, are not in this machine's registry of open
   documents (the backend keeps its own, SPEC-0220), and saving goes to the host. The window is
   not a local project: it is not in the recent projects, not served by the local MCP workspace,
   and asks no trust question (nothing of it runs here). Language servers, inspections and run
   markers are left out until step 4 brings them from the host. [GEN, ADR-0025 T5]
4. **Editor and replica:** the editor keeps its own text; at each keystroke and every 50 ms the
   window meets it with the session's replica only if the session is free at once. The editor's
   change since they last agreed and the replica's (other clients, agents, the host) are
   transformed against each other, the replica's first (it was sequenced by the backend); the
   editor takes the replica's part as ranges without moving the caret off its text. While the
   session is busy (reconnecting, saving, opening), the editor's newest text goes to the recovery
   copy instead, so typing never waits for a lock or the network. After a new backend or a
   divergence the editor still shows the window's text; it is sent again as an edit and the next
   save asks first (behaviour 15). Taking a change from elsewhere is an undo step of the editor
   (known limit). [STD: Jupiter, GEN]
5. **Saving:** "Saved on host" on success. A conflict, or a host file that changed under the
   window's text, writes nothing and says so; saving again overwrites (the host keeps the old text
   in its local history). Compare and reload of SPEC-0058 come with a later version. While
   disconnected the save waits ("Not saved — reconnecting …"), the widget shows "save pending",
   and the save is carried out after the resume. [GEN, SPEC-0058]
6. **Supervisor:** a thread per remote project pumps the session, sends a heartbeat every 5 s
   (no answer within 10 s is a break), and after a break tries again after 1, 2, 4, 8, 16, then
   every 30 s, while the host keeps the session (10 minutes); after that the widget shows
   *Disconnected* and Reconnect starts a new backend. Disconnect in the widget's menu stops the
   attempts until Reconnect. The widget shows host and round trip, *Reconnecting (attempt n)* or
   *Disconnected*; its tooltip the time left and the pending edits. [GEN, ADR-0025 §8]
7. **Recovery copies** are written by a thread of their own, newest text per file (a durable write
   takes several milliseconds on macOS; typing never waits for it). After a crash, opening the
   project again asks once: restore (each file is opened and takes the copy's text as an unsaved
   edit; a host file changed since asks before saving), discard the copies, or decide later.
   [GEN, SPEC-0262]
8. **Closing:** closing the window asks about unsaved remote files (Save All, Don't Save,
   Cancel), then ends the session; quitting names them as `host:folder/file` with the local
   files. A window that goes without closing (a crash) still ends its session; the copies stay.
9. **Agents (behaviour 20):** a remote project open in a window is published for the window's MCP
   server: `remote_projects` lists it with `window: true` and its connection state,
   `remote_documents`, `remote_project_reconnect` and `remote_project_close` act on it (a close
   ends the session and the window closes itself); `remote_project_open` with a window opens a
   project window on it. [ADR-0008]

## Details fixed in v5 (with reasons)
Found by the window self-test (AK-7, about 4 of 20 runs: a host change did not reach the open
editor although the tree showed it) and the window check of 2026-11-07. [GEN]
10. **Popups show host paths:** the footers of Search Everywhere, Go to File and Recent Files show
    a remote project's file with its path on the host, never the stand-in folder.
11. **When an editor meets its replica (behaviour 9):** a meeting is due while the replica changed
    since the last meeting (a stamp that changes with every change of the replica's text not typed
    by the user, unique per process, so that documents of an old and a new backend cannot be
    confused by equal revision numbers), the unsaved state differs, or the editor changed. A
    session that is busy at that moment is **no meeting**: nothing is noted as met and the next
    frame tries again. Otherwise a folder listing that holds the session just then (a host change
    starts both) leaves the editor behind until the next change.
12. **12a. Numbered notifications can overtake each other** (`fs.changed` and other bulk
    notifications travel behind interactive ones). The window resumes and acknowledges from the
    end of the part without a gap, not from the highest number seen, and ignores a replay of what
    already arrived; otherwise a break loses the overtaken notification.

## Details fixed in v6 (with reasons)
Found by a window check of M6.13: the tree showed the stand-in folder beside the project name. [GEN]
11. **The stand-in folder is never shown.** The tree's root row shows the folder on the host. Copy
    Path gives the host path, Copy Relative Path the path below the host folder, and the list of
    files that could not be saved before a launch names them on the host. Reveal in Finder (or the
    file manager) says that the file is on the host and opens nothing, because the stand-in folder
    is the window's own state.

- **AK-10 (v7)** `project::tests::checksum_limit_and_streamed_hash` (the limit for 0, 1 byte and 2.5 GiB, the
  hash read in blocks equals the hash of the whole); against a test `sshd` with the debug build
  (`crates/app/tests/it/remote_project.rs::a_folder_on_an_ssh_host_opens_survives_breaks_and_stays_private`,
  20 runs on Linux): opening checks the uploaded copy, and right after the simulated break the
  session says it is disconnected (`RemoteProject::break_connection` returns once the session's
  reader has seen the end of `ssh`, at most 5 s).

## Details fixed in v8 (with reasons)

1. **Disconnect wins over a running round of the supervisor:** a round that had found the connection
   up, or an attempt that was still connecting, wrote *Connected* after the user's Disconnect had
   written *Disconnected*. The widget then said *Connected* while the connection was broken and no
   attempt followed (server CI on a busy machine, 2026-11-13, `live.rs` test of behaviour 6).
   Disconnect marks the hold before it breaks the connection and sets its state; the supervisor
   checks the hold again under the state's lock before it writes *Connected*, and an attempt that
   connects after a Disconnect leaves the state alone (its line in the log says so).

- **AK-11 (v8)** `crates/remote/tests/it/live.rs::the_reattach_period_ends_automatic_attempts_and_reconnect_starts_a_new_backend`:
  after Disconnect the state stays *Disconnected* until Reconnect; 20 of 20 runs with the test held to
  one core shared with two busy loops.

## Details fixed in v9 (with reasons)

2. **Closing is final:** a round of the supervisor that had found the connection up before a close
   wrote *Connected* after the close had written *Closed*; the project stayed in `remote_projects`
   as connected, with its unsaved document, and a later round could have reconnected it (server
   CI on a busy machine, 2026-11-15, `protocol::remote_projects_of_windows_are_seen_and_acted_on_by_agents`).
   Close now sets the supervisor's stop while it still holds the project's lock, so no
   reconnection starts after it; nothing replaces *Closed* any more — not the end of a round, not
   an attempt, not a Disconnect; and Reconnect (the widget's, `remote_project_reconnect`) of a
   closed project is refused. A closed project leaves `remote_projects` as soon as the close
   returns. The backend's line "connection lost; waiting 600 s for reattachment" in that run
   belongs to the Reconnect step before the close, which breaks the connection on purpose and
   resumes it.

- **AK-12 (v9)** `live::tests::a_closed_project_stays_closed_whatever_a_round_found_before` plays the
  race in order (fails without the fix): the round finds the connection up, the project is closed,
  the round ends — the state stays *Closed*, the project is unlisted, later rounds and Reconnect do
  not reconnect it; `live::tests::closing_during_running_rounds_ends_closed` closes while a
  supervisor runs round after round (20 times); the MCP test reads `remote_projects` once right
  after the close.

## Out of scope
Language servers, search, index, Git, terminal, run configurations, build tools and database
connections on the host (M6.12 step 4, next spec; the protocol families exist in ADR-0026), port
forwarding (step 5), MCP tools acting on remote documents through the project route (step 6),
remote debugging (M6.13), hosts of another platform, Windows hosts, containers, WSL and paired
`--pair` channels (M6.14), several windows on one session, remote control (M16).

## Known deviations from the reference
Not compared yet. Decided independently of the reference and kept unless the comparison shows a
better behaviour: the backend is the window's own build checked by SHA-256 (no download from a
vendor server); host keys are only accepted outside the IDE (SPEC-0121); typing never waits for the
network; edits made during a break are kept and replayed.

## Open points — compare after the OBS on the reference's remote development
Nothing below is taken from the reference; each point is checked in the observation (M6.12 step 1)
and then confirmed, changed in a new version of this spec, or recorded as an improvement
(`IMP-nnnn`) when the reference is weaker.
1. Entry points and the steps of the connect dialog (host list, folder choice, recent projects,
   progress, cancel).
2. How the reference brings its backend to the host (download source, version matching, checksum),
   and how long the first and later connections take.
3. What the window shows about the connection (title, status bar, indicator, latency) and which
   actions it offers there.
4. Behaviour during a network break: are editors editable, what happens to unsaved text, to saves,
   to running programs and terminals; how long the host side waits; how reconnection is shown.
5. Behaviour after the host side was restarted, and after the window crashed during a break.
6. Typing latency over a slow connection (same delays as AK-8).
7. Closing the project: what happens to the backend and to running programs on the host.
8. Concurrent edits of the same file by an agent or a second client during a session.
9. Security behaviour visible to the user: host key handling, what listens on the host and on the
   local machine during a session (observed from outside with OS tools only, RESEARCH_BOUNDARIES
   section 3).
