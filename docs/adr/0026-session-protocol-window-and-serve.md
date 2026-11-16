# ADR-0026: Session protocol between the window and `idecraft serve`

- **Status:** accepted · 2026-11-07 · prepares M6.12 step 2 ([plan](../architecture/REMOTE_AND_MOBILE.md));
  the headless service of M10.3, moved before QG-1 by the Project Owner (PO-STATUS question 27,
  [ADR-0015](0015-quality-gate-ai-native.md) amendment 2026-11-07), takes this protocol over unchanged
- **Related:** [ADR-0025](0025-network-security-for-remote-connections.md) (channels and security),
  [ADR-0012](0012-headless-core-capability-api.md) (headless core, `domain.verb` names, cancellation,
  error codes), [ADR-0013](0013-protocol-adapters.md) (adapters), [ADR-0005](0005-language-services-via-protocols.md)
  (LSP/DAP), SPEC-0220 (document registry), SPEC-0158 (backend on a host), SPEC-0058 (saving with a
  change check), SPEC-0262 (atomic state files), SPEC-0044 (local history),
  [SPEC-0317](../specifications/SPEC-0317-remote-project-session-and-window.md) (first user)

## Context

The remote project (M6.12) moves documents, file trees, language servers, terminals, runs and
debugging to a host while the window stays local. SPEC-0158 already runs a backend on the host,
but it speaks MCP and the window edits local copies (SPEC-0121). MCP is a tool-call protocol for
agents: it has no notion of a document with revisions, no byte streams for terminals or ports, no
flow control and no resumable event stream. The readiness analysis requires one owner of open
documents (A2, SPEC-0220) and the headless service of ADR-0012 hosts workspaces, documents,
sessions and an event stream. This ADR defines the one protocol between a window (later also a
paired device, M16, or a mobile app, M17) and `idecraft serve`.

## Decision

### 1. Layers and transports

```
 session messages   (JSON-RPC 2.0 messages + binary stream chunks)        this ADR
 frames             (length, type, stream id, payload)                      this ADR
 byte stream        SSH channel stdio | Noise channel | in-process pipe     ADR-0025
```

- **SSH:** the window runs `ssh <options of SPEC-0121> <host> <path>/idecraft serve --stdio`; the
  protocol uses the process's standard input and output; standard error carries only log lines for
  the connection log, never protocol data. The project folder is sent in `session.hello`, not on the
  command line.
- **Paired:** the Noise channel of ADR-0025 carries the same byte stream.
- **In-process:** the window can host `serve` in its own process over a pair of in-memory pipes.
  This is how local projects can later use the same code path (M10.3) and how most protocol tests
  run.

The protocol never depends on the transport: no field says which one is used, and every
transport gives an ordered, reliable, already authenticated byte stream.

### 2. Framing

Each frame is

| Field | Size | Meaning |
|---|---|---|
| `length` | u32, big-endian | number of bytes after this field (type + stream id + payload) |
| `type` | u8 | `1` message (UTF-8 JSON), `2` stream data, `3` stream credit, `4` stream close |
| `stream` | u32, big-endian | `0` for messages; the stream id otherwise |
| `payload` | `length − 5` bytes | JSON text, raw bytes, a u32 credit, or a JSON close reason |

- A frame is at most **1 MiB** (payload ≤ 1 MiB − 5). A larger message is an error (`too_large`);
  bulk content (file text above 512 KiB, terminal output, port bytes, search result pages) goes
  through streams. The cap keeps a large transfer from blocking typing, heartbeats or cancellation:
  the sender interleaves frames of different streams and always sends message frames first.
- An unknown `type`, a malformed length or invalid UTF-8 in a message closes the connection with a
  logged reason (ADR-0025 SEC-14).

### 3. Messages: JSON-RPC 2.0, `domain.verb` names

- Messages are JSON-RPC 2.0 requests (with `id`), responses and notifications, as LSP, DAP-like
  tooling and MCP already use in IDECraft. Method names follow ADR-0012's `domain.verb` scheme so the
  capability layer of M10 can map them one to one: `session.*`, `doc.*`, `fs.*`, `lsp.*`, `dap.*`,
  `term.*`, `run.*`, `port.*`, `event.*`, `stream.*`.
- Either side may send requests (the backend asks the window, for example, to show a right question
  for an agent, SPEC-0053 behaviour 8).
- **Errors** use JSON-RPC error objects whose `data.code` is a stable string (ADR-0012 rule 2,
  readiness A4): `not_granted`, `denied`, `not_found`, `conflict`, `stale_revision`, `cancelled`,
  `deadline_exceeded`, `too_large`, `unsupported_version`, `unavailable`, `invalid`, `internal`.
  `data.hint` says what to do.
- Every notification from the backend carries `seq`, a per-session counter that grows by one
  (section 8).

### 4. Versioning

- The first exchange is `session.hello` from the window: protocol name `idecraft-session`, the
  majors it speaks (`[1]`), its build, its client kind (`window`, `device`, `mobile`), the project
  folder and the capabilities it wants. The backend answers with the chosen major, its build, the
  session id, a **resume token** (256 random bits; kept only in memory on both ends, ADR-0025
  section 3), the granted rights and its capability list (for example `doc.ops`, `fs.watch`,
  `lsp`, `dap`, `term`, `run`, `port`).
- **Minor changes** add methods, fields or capabilities; receivers ignore unknown fields and answer
  unknown methods with JSON-RPC "method not found". The backend answers its `minor` in
  `session.hello` (raised with every added method, or new field a window relies on; a test compares
  the bound methods with it). A window refuses a backend whose `minor` is lower than its own or
  absent, with what to update: otherwise the difference appears only at the first missing method,
  long after connecting (SPEC-0321 behaviour 5a, 2026-11-09). The `minor` only ever refuses, so it
  is no way to downgrade. **Major changes** (removing or changing the
  meaning of anything) raise the major, which is also part of the Noise prologue (ADR-0025 section 1).
- Over SSH the backend is the window's own build (SPEC-0158 behaviour 2), so both sides agree.
  Paired devices and mobile apps may lag: a backend supports the current and the previous major;
  anything else is refused with `unsupported_version` naming both versions and what to update.

### 5. Documents: the backend owns them, the window works optimistically

- The backend hosts the **document registry** of SPEC-0220 for its workspace: path → text, revision,
  modified flag, plus (new) the log of operations since the oldest revision a connected client still
  builds on. Revisions keep SPEC-0220's meaning: start at 1, grow by one with every change; a save
  without a change only clears the modified flag.
- `doc.open {path}` returns the document id (the path relative to the project root), revision,
  modified flag, line ending and text (inline up to 512 KiB, else as a stream).
- **Edits** are operations: `doc.edit {doc, base, client, cseq, ops}`, where `ops` is a list of
  `{at, delete, insert}` in **Unicode scalar offsets** of the text at revision `base` (the unit of the
  editor buffer, SPEC-0003), `client` is the session's client id and `cseq` grows by one per edit of
  that client.
- **Ordering is the backend's (server-sequenced operational transformation, as in the Jupiter
  system):** if `base` is the current revision, the backend applies the edit; otherwise it
  transforms the ops against the operations applied since `base` (inserts at the same offset: the
  operation sequenced earlier goes first), applies them, and the result gets the next revision. The
  backend acknowledges with the new revision and a 64-bit hash of the resulting text, and announces
  every applied edit to the other clients as `doc.changed {doc, revision, ops, author}`.
- **The window applies its own edits immediately** and keeps unacknowledged edits in a pending
  queue; it transforms incoming `doc.changed` ops against that queue (the client half of the same
  algorithm). Typing never waits for a round trip.
- **Exactly once:** the backend remembers the last applied `cseq` per client and document, so a
  resent edit (after reconnection) is acknowledged again without being applied twice (ADR-0025
  section 8).
- **Divergence check:** if the hash in an acknowledgement differs from the window's text at that
  revision, the window asks for `doc.snapshot`, keeps its own text as an unsaved local version and
  shows the difference; it never silently discards either side.
- **Saving:** `doc.save {doc, revision}` writes on the host through a temporary file renamed into
  place, keeps the file's mode, records local history on the host (SPEC-0044) and refuses with
  `conflict` when the file changed on disk since the last load (SPEC-0058); the window then offers
  the usual compare/overwrite choice. Files changed on disk by others arrive as `doc.changed` with
  `author: "disk"` when the document is unmodified, or as a `doc.disk_changed` notification when it
  has unsaved changes.
- **Undo** of the user's typing stays in the window: undoing sends the inverse ops as a normal edit,
  so edits of agents or other clients in between are kept. Agent edits on the backend keep their own
  undo groups (SPEC-0193).
- Agents use the same registry through MCP on the backend, so an agent edit appears in the window
  as `doc.changed` with the agent as `author` (A2: humans and agents on the same document).

### 6. The other message families

| Family | Methods (requests → notifications/streams) | Notes |
|---|---|---|
| `fs` | `fs.list`, `fs.stat`, `fs.read` (stream), `fs.write`, `fs.create`, `fs.rename`, `fs.delete`, `fs.watch {root, exclude}` → `fs.changed {seq, events}` | paths relative to the project root; `..`, absolute paths and links leaving the root are refused (`invalid`); events are coalesced over 100 ms and per path (create+change → create) |
| `lsp` | `lsp.start {language, root}` → server id; `lsp.request {server, method, params}`, `lsp.notify`; server → window: `lsp.message` | language servers run on the host in the backend's pool (SPEC-0090). **Document synchronisation is the backend's job**: it sends `didOpen`/`didChange` from the registry, so the window never sends them. Other LSP payloads pass unchanged; position-bearing requests name the revision they were computed on and the backend answers `stale_revision` when it moved on |
| `dap` | `dap.start {configuration}` → stream id; DAP messages travel unchanged on the stream | debug adapters run on the host (M6.13); no local debug port (ADR-0025 section 6) |
| `term` | `term.open {cwd, cols, rows, env}` → stream id; input and output as stream data; `term.resize`; `term.exited {code}` | the PTY lives in the backend; scrollback of 1 MiB is kept for reattachment |
| `run` | `run.start {configuration}` → run id and output stream; `run.stop`; `run.event` (started, test result, exited) | run configurations and before-launch chains resolved on the host (SPEC-0047, SPEC-0158 behaviour 9) |
| `port` | `port.detected` (a program on the host listens on a port), `port.forward {host_port, target}` → stream per accepted local connection, `port.close`, `port.list` | local listener on loopback only; rules of ADR-0025 section 6 |
| `event` | `event.subscribe {kinds}` → diagnostics, task state, index progress, git state | the event stream of ADR-0012 decision 5 / readiness B3 |
| `session` | `session.hello`, `session.resume`, `session.ping`, `session.ack {seq}`, `session.resync`, `session.bye` | sections 4, 7, 8 |
| `stream` | frame types 3 and 4 | section 9 |

Each family is a capability in `session.hello`; a spec adds methods within these families as it
needs them (a minor version change) and lists them.

### 7. Rights

Every request is checked against the session's rights before anything happens: an SSH session has
the user's own rights on the host; a paired device has its grants (ADR-0025 section 4). A missing
right answers `not_granted` naming it. Agent rights on the host stay those of SPEC-0053 for the
host's project. Every request that changes something or runs something is written to the audit log
of ADR-0025 section 4.

### 8. Event stream, reconnection without losing unsaved edits

- The backend keeps the last notifications in a ring buffer (at least 10 000 or 8 MiB, whichever is
  reached first). The window acknowledges the highest `seq` it has handled with `session.ack`
  (piggybacked at most every second), so the backend can drop older entries.
- **Network break:** requests in flight fail on the window side with `unavailable`; terminals, runs,
  debug sessions and language servers keep running on the host; the backend keeps the session for
  the reattach period (ADR-0025 section 8: default 10 minutes). The window keeps every editor
  editable; new edits join the pending queue; saving and host functions say "reconnecting" instead
  of failing silently.
- **Resume:** the window reconnects (SSH: `idecraft serve --attach`, which bridges standard input and
  output to the backend's reattach socket; paired: a new Noise handshake) and sends `session.resume
  {session, token, last_seq}` as the first message — the token never appears on a command line. The
  backend replays notifications after `last_seq`, the window resends every pending edit with its
  original `cseq` (applied once, section 5), and terminals replay their scrollback. If `last_seq` is
  older than the ring buffer, the backend answers `session.resync` and the window re-reads document
  revisions, the file tree and running sessions before resending its pending edits.
- **Backend gone** (reattach period over, host rebooted): the window starts a new backend and, per
  document with unsaved state, compares the host file with the text its edits were based on: equal →
  it reopens and sends its changes as edits; different → the window's text stays an unsaved local
  version and the user gets the compare/merge choice of SPEC-0058. While a remote document has
  unsaved or unacknowledged edits, the window also keeps a recovery copy (base revision, base hash,
  pending edits, current text) in its own state folder, written by SPEC-0262's write-then-rename
  method, so a window crash during a break loses nothing either; the copy is removed once the edits
  are saved on the host.

### 9. Backpressure

- **Streams use credit:** each stream starts with 256 KiB of credit per direction; the receiver
  grants more with a credit frame once it has consumed data. A sender without credit stops: the
  backend stops reading the PTY or the forwarded socket (the program on the host blocks as it would
  on a slow terminal) instead of buffering without bound.
- **Messages** have two priorities: interactive (`doc.*`, `lsp.request` for completion, hover and
  similar, `session.*`, cancellation) before bulk (search pages, `fs.changed` batches, diagnostics,
  index progress). Notifications whose newer value replaces the older (diagnostics per file, progress
  per task, `fs.changed` per path) are coalesced while queued.
- Each side caps its outgoing queue (default 32 MiB); a peer that stops reading for 45 s is treated
  as a network break (section 8).

### 10. Cancellation and deadlines

- `$/cancel {id}` (the LSP convention) cancels a request; the handler receives a cancellation handle
  (ADR-0012 rule 2) and answers `cancelled`. Requests may carry `deadline_ms`; on expiry the backend
  cancels and answers `deadline_exceeded`.
- Closing a stream (frame type 4) with a reason ends a terminal input, a forwarded connection or a
  file transfer; `term.close`, `run.stop` and `dap` disconnect end the process.
- `session.bye` ends the session deliberately: the backend saves nothing on its own, reports
  unsaved documents in the answer, ends terminals and runs that the window asked to end, and exits
  when no client is left.

### 11. Relation to MCP and to SPEC-0220

- **MCP stays the adapter for agents** (ADR-0012, ADR-0013): an agent talks MCP to the backend (stdio
  over SSH as in SPEC-0158, or through the window's HTTP server, SPEC-0221). MCP tool bodies and the
  session protocol's handlers call the same domain functions, so a human edit in the window and an
  agent edit through MCP meet in one registry. The session protocol is not offered to agents as a
  second API.
- **SPEC-0220's registry becomes the backend's document store.** Its read semantics stay; this ADR
  adds the operation log, `(client, cseq)` deduplication and the write path through operations. In a
  local project the registry stays in-process; the in-process transport (section 1) lets local and
  remote projects later share the window's code path.
- **M10.3** (`idecraft serve`) implements exactly this protocol as its native interface; adding
  workspaces per process (M10.4) adds a `workspace` field to `session.hello`, a minor change.

## Consequences

- One protocol for SSH backends, paired hosts, remote control and mobile clients.
- The window gains a client-side transformation step and a pending queue; tests must cover
  convergence (random concurrent edits from two clients and an agent end in the same text and hash).
- MCP-based remote calls of SPEC-0158 (`remote_backend_call`) keep working for agents; the window's
  remote project switches to the session protocol.
- JSON keeps the protocol inspectable in logs and tests; bulk data avoids JSON escaping through
  binary streams.

## Alternatives considered

- **MCP as the window protocol** — rejected: no documents with revisions, no streams, no flow
  control, no resumable events; and it would bind the window to an agent protocol's revisions
  (ADR-0012 alternative a).
- **A binary encoding (CBOR, MessagePack, protobuf) for messages** — rejected for now: JSON matches
  LSP, DAP and MCP, which pass through unchanged; bulk bytes already avoid JSON. A later major may
  switch if measurements demand it.
- **CRDT documents** — rejected: there is one owner (the backend) that orders all edits, which is
  what server-sequenced OT needs; CRDT metadata and tombstones buy nothing here.
- **Locking instead of optimistic edits** (window waits for the backend) — rejected: typing latency
  would equal network latency (the 16 ms budget of ADR-0015 cannot hold over a network).
- **The window as an LSP client sending `didChange` itself** — rejected: two sources of truth for the
  server; agents' edits would not reach it.
- **`ssh -L` port forwards per port** — rejected for debug and language servers: every forward is a
  local listener reachable by other local users; streams inside the session need none.
