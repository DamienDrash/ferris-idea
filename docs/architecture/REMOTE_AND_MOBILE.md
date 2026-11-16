# Remote development and mobile apps: plan

- **Status:** planned · **Date:** 2026-11-07 · **Requested by:** the Project Owner (PO-STATUS
  2026-11-07) · **Tracking:** ROADMAP.md M6.12–M6.15, M16, M17, M18
- **Builds on:** remote host over SSH ([SPEC-0121](../specifications/SPEC-0121-remote-host.md)),
  backend on a host ([SPEC-0158](../specifications/SPEC-0158-remote-backend.md)), file operations
  ([SPEC-0253](../specifications/SPEC-0253-remote-file-operations.md)), MCP over HTTP
  ([SPEC-0221](../specifications/SPEC-0221-mcp-http-transport.md)), agent rights (SPEC-0053), the
  headless service of M10.3 ([ADR-0012](../adr/0012-headless-core-capability-api.md))

No dates are given. Effort is given in relative sizes (S < M < L < XL).

## Requested areas

| Area | Today | Goal | Milestone | Phase |
|---|---|---|---|---|
| Remote project | Remote Host window edits single files through a local copy; backend searches, lists problems and runs configurations on the host | A folder on a host opens as a normal project: tree, editor, code insight, Git, terminal, run and search all run on the host, the window stays local | M6.12 (L) | classical, after the current comparison queue |
| Remote debugging | Run configurations on the host run in an SSH terminal; no debugger | Debug sessions on the host (adapter runs there), attach to a remote JVM, debugpy or lldb through a forwarded port | M6.13 (M) | classical |
| Every host | POSIX hosts over SSH; the backend is uploaded only to a host of the same platform | Linux, macOS and Windows hosts (x86_64 and arm64), containers, WSL and hosts without SSH | M6.14 (L) | classical |
| Remote control of the local app | MCP over HTTP on `127.0.0.1` with a token | A running IDECraft on a PC can be used from a paired device (another PC, phone, tablet) without exposing it to the network | M16 (L) | after M6.12, before QG-1 (PO decision 2026-11-07) |
| Working together | none | Several people (and agents) work live in one project as smoothly as in a shared spreadsheet: a project room owned by the server, link sharing with roles, presence, follow mode, no conflicts, shared history; a short peer session from a desktop window as a special case | M6.15 (L) | classical (reference: Code With Me), room after M6.12, peer case after M16 |
| Mobile apps | none | iOS and Android apps: client for a desktop app or a server backend, and a reduced local editor ("IDECraft Lite") | M17 (XL) | future, after QG-1 |
| Web IDE | none | IDECraft in the browser without a desktop app, against `idecraft serve`: self-hosted as one binary, later offered by a service provider | M18 (XL) | transport and spike before QG-1, product after QG-1 |

M6.12–M6.14 belong to the classical phase because the reference IDE offers remote development
(feature matrix row F-083); they are compared side by side like other rows. The Project Owner
decided on 2026-11-07 (PO-STATUS, question 27) that the headless service `idecraft serve` (M10.3)
is built as the backend of M6.12 and so starts before quality gate QG-1
([ADR-0015](../adr/0015-quality-gate-ai-native.md)); M16 follows M6.12, also before QG-1. M17 and
the rest of M10–M15 stay after QG-1.

## Network security (binding for every remote path)

The Project Owner decided on 2026-11-07 that remote work is always secured on the network side.
These rules apply to M6.12–M6.15, M16, M17 and M18 and to every later remote feature;
[ADR-0025](../adr/0025-network-security-for-remote-connections.md) makes them testable (procedures,
threat model, mandatory security tests SEC-1–SEC-16), and each spec names how it meets them.
Since ADR-0025 v2 (Project Owner, 2026-11-07, PO-STATUS question 28) paired channels are
post-quantum hybrid from their first version (Noise with X25519 plus ML-KEM-1024), and a host
without a keychain keeps the key of `idecraft serve --pair` in a 0600 file that refuses the start
when others can read it.

1. **Encrypted and authenticated, always:** every connection between window, backend, paired
   device and relay is encrypted and mutually authenticated (SSH with the user's keys and strict
   host keys, or a hybrid post-quantum Noise handshake with keys pinned at pairing; ADR-0025
   compared TLS 1.3 and chose Noise). There is no plaintext mode, no downgrade and no "accept any
   certificate" switch.
2. **Nothing listens openly:** no service listens on a network interface other than loopback unless
   the user switches it on for a paired device; such a listener accepts only pinned keys and closes
   when remote access is switched off.
3. **Unauthenticated protocols stay local:** debug ports (JDWP, debugpy, lldb-server, Node
   inspector), language servers and database ports on a host are reached only through the
   encrypted session or an SSH tunnel bound to `127.0.0.1` on both ends, never exposed directly.
   Forwarded ports bind to loopback by default.
4. **Least privilege:** each paired device and each remote project has its own rights (view, edit,
   run, debug, push, agents); deny by default; every remote action is logged; access is revoked at
   once.
5. **Relays see nothing:** traffic through a relay is end-to-end encrypted between the two
   endpoints; the relay only forwards bytes and holds no keys.
6. **Secrets stay in the keychains:** keys and tokens live in the operating system's keychain (or
   the SSH agent; on a host without a keychain, `idecraft serve --pair` keeps its key in a 0600 file
   in a 0700 folder, ADR-0025 section 3a); they are never written to project files, logs or the
   session protocol.
7. **Robust by default:** replay protection, time limits, rate limits on pairing and login, and
   clear refusals that say what to do; no silent fallbacks.
8. **Checked:** every remote spec has security tests (wrong key, changed host key, missing right,
   plaintext attempt, exposed port), and the network paths are reviewed before a release.

## M6.12 Remote project

1. **Observation first:** side-by-side observation of the reference IDE's remote development
   (connection over SSH, backend download, project on the host, latency, reconnect, port
   forwarding) as an OBS document; behaviour only (RESEARCH_BOUNDARIES).
2. **Session protocol ([ADR-0026](../adr/0026-session-protocol-window-and-serve.md); window and
   session: [SPEC-0317](../specifications/SPEC-0317-remote-project-session-and-window.md)):** one
   channel between window and backend for documents, file tree
   events, language server traffic, terminals, run and debug sessions. The backend owns the
   documents (readiness item A2); the window sends edits as operations and keeps working while
   the round trip is pending. The protocol is written so that the headless service of M10.3 can
   take it over unchanged.
3. **Window:** "Remote Development" on the welcome screen and in File: host from `~/.ssh/config`,
   folder on the host, recent remote projects. The project window shows the host in the title and
   the status bar, a connection indicator, and reconnects after a network break without losing
   unsaved edits.
4. **Functions on the host:** project tree with file watching, editor and local history, language
   servers, search and index, Git, terminal, run configurations and test results, build tools,
   database connections from the host.
5. **Port forwarding:** ports a program opens on the host are offered for forwarding (dev servers,
   debuggers); forwarded ports are listed and can be closed.
6. **Agents:** MCP tools act on the remote project as on a local one; rights stay those of the
   host's project.

## M6.13 Remote debugging

1. Debug adapters (java-debug, debugpy, lldb-dap and the others of F-070) start on the host through
   the backend; DAP traffic runs over the session channel; breakpoints, stepping, variables,
   evaluation and hot swap behave as locally.
2. Remote attach run configurations: JVM (JDWP), Python (debugpy listen), native (lldb-server or
   gdbserver), Node.js inspector; the port is forwarded through SSH when the target is not reachable
   directly.
3. Source mapping between host and local paths where code is mounted or containerised.
4. Containers and Kubernetes pods as debug targets (F-083 container functions).

## M6.14 Every host

1. **Backend everywhere:** release builds of the headless backend for Linux, macOS and Windows on
   x86_64 and arm64, built and tested in CI; checksums verify every upload.
2. **No shell assumptions:** file operations, search and process control move from POSIX tools on
   the host into the backend, so the host's shell no longer matters once the backend runs.
   Before it runs, a minimal bootstrap detects the host (`uname`, or PowerShell on Windows) and
   picks the build.
3. **Transports:** SSH (OpenSSH on all three systems, including Windows' built-in server); Docker
   `exec` and Kubernetes `exec` for containers; WSL from a Windows window; and for hosts without
   SSH a backend started by hand with `idecraft serve --pair` that connects to the window over an
   encrypted, paired channel (shared with M16).
4. **Refusals stay explicit:** an unknown host key, a missing build for an exotic platform or a
   blocked port is named with what to do, never worked around.

## M6.15 Working together (collaboration)

Requested by the Project Owner on 2026-11-07. The reference IDE offers this as Code With Me, so it
is a feature matrix row (F-089) and is compared side by side; signing in to the reference's own
services is done by the project owner only.

**Not a copy of the reference.** The Project Owner noted on 2026-11-07 that Code With Me is seen
critically in the community. Its weaknesses are recorded as improvements (IMP entries when the spec
is written, rule 9) instead of being reproduced:

| Weakness of the reference (documented or reported) | IDECraft instead |
|---|---|
| End-to-end protection holds only if host and guest compare a security code, which is optional | rooms authenticate every person (OIDC, passkeys) over PQ TLS; on a peer channel the SAS comparison is mandatory before anything is shared (ADR-0025) |
| Local IP addresses, project name and user name travel unencrypted to set up the session | no metadata in plain text; the invitation carries only what the handshake needs |
| Relays run by the vendor are the fallback path | no vendor relay; direct, SSH or the user's own relay holding no keys |
| Calls with more than two participants were not end-to-end encrypted | no calls in the first version; any later call is end-to-end encrypted |
| Trouble in high-latency environments and with refactorings for guests (Thoughtworks Technology Radar) | optimistic local editing with server ordering (ADR-0026), latency acceptance criteria with 50 and 300 ms delay, refactorings run in the backend and arrive as one change |
| A session exists only while the host is online and approves each guest | the project room lives on the server; anyone with the right joins from a link, nobody has to host |
| Guests get a reduced client and limited features | a guest uses the normal IDECraft window with the host's language services; only rights limit it |

The side-by-side comparison observes the reference to find further gaps and weaknesses; it does not
set the target.

**Project Owner decision (2026-11-07):** working together has to feel like editing a shared
spreadsheet: changes appear at once, everyone sees the others, nobody has to host or approve, there
are no conflicts, a link with a role is enough to join, and the history is kept. M6.15 is therefore
built around a **project room owned by the server**, not around a host's window. The design follows a
research round on web IDEs and cloud development environments on 2026-11-07 (PO-STATUS).

1. **Project room on the server:** a room is a project in a running (or woken on demand)
   `idecraft serve` on the user's own server or at a service provider (M18). It has a stable URL
   and lives without any participant; when someone leaves, the work goes on. Desktop windows, the
   web client (M18) and later the mobile apps (M17) join the same room as clients of the session
   protocol (ADR-0026).
2. **One ordering authority, no conflicts:** the backend orders every change as an operation:
   keystrokes, refactorings, formatter runs, file tree operations (create, rename, move, delete),
   `git pull` and checkout, agent edits and changes from outside (terminal, build generators,
   through a file watcher). A refactoring over many files arrives as one atomic change; a
   concurrent keystroke is transformed against it, never overwritten. Clients apply their own
   input locally first. Undo works per person. A client that loses the network keeps its pending
   operations and sends them after reconnecting, without a dialog, for up to 60 seconds.
3. **Sharing by link, bound to identity:** "Share → copy link → choose role". Roles: viewer,
   commenter, editor, runner (terminal, run, debug), maintainer (rights, secrets, Git push).
   Default: anyone in the organisation with the link after sign-in (OIDC, passkeys), role viewer.
   A link without sign-in exists only when chosen explicitly, is read-only and expires; it never
   grants running code. No present person has to approve a joiner who has the right.
4. **Peer session (special case):** to show a local project briefly, a desktop window can open a
   room in its own `idecraft serve` and invite guests over the paired channel of M16 (link, code or
   QR code with the pairing secret of ADR-0025, hybrid post-quantum Noise handshake, mandatory SAS
   check, approval in the window, one use, 15 minutes by default, withdrawable). It ends when the
   window closes; the room model above is the normal case.
5. **Rights (deny by default):** the roles of point 3, changeable at any time; removing a right
   takes effect within one second without restarting the room (the participant's channel closes);
   every action is logged with the participant's name.
6. **Presence without setup:** avatars in the title bar, names and colours, carets and selections
   of every participant, marks in the project tree (who is in which file), scroll bar marks for
   other carets, follow mode (click an avatar), "summon" (bring everyone to my position). Presence
   is volatile and never part of the ordered document stream.
7. **Runtime state belongs to a person:** terminals, runs and debug sessions are owned by whoever
   started them, who can share them for viewing or control (by role); test results and problems are
   shared. Ports are forwarded only by the owner's decision and only to the participant's loopback
   or, in M18, to the private preview domain.
8. **History next to Git:** an automatic, fine-grained history of the shared working state (every
   editing phase with author and time, named versions, restore a file or a range to any state of
   the last 30 days) — the reference's Local History, but shared and on the server. Git stays the
   deliberate publication: a commit from the live state credits every author (`Co-authored-by`).
   Anyone can open a separate branch working state from the room to work in isolation. A named
   version is created before every agent run.
9. **Communication:** text chat and comments on lines inside the session; audio and video are not
   planned in the first version (external tools cover them); a later decision may add them with
   end-to-end encryption only.
10. **Agents as participants:** an AI agent can join with its own rights like a guest (link to
   SPEC-0053 and M12); its edits show with its name.
11. **Network paths and honest encryption:** rooms are reached over TLS 1.3 with the hybrid
    post-quantum key exchange X25519MLKEM768 (ADR-0025), peer sessions over direct, SSH or the
    user's own relay (M16), end-to-end encrypted, the relay holds no keys; no third-party service is
    required. Language services, indexing and builds need plain text on the server, so a room is
    end-to-end encrypted only from the clients to a server the user runs. At a service provider the
    protection is PQ TLS, encryption at rest with the customer's key, isolation (M18) and the audit
    log; the product says so instead of promising end-to-end encryption.
12. **Acceptance (target values, not market figures):** own input in the same frame; a
    participant's change visible within 150 ms at 50 ms round trip and within 500 ms at 300 ms;
    joining from a link to editable within 3 s for a warm room without anyone's approval; zero
    conflict dialogs under concurrent edits, renames, refactorings and `git pull` (convergence tests
    with n clients plus an agent); 60 s network loss without a dialog; a web participant uses the
    same language services as the desktop.
13. **Security tests:** SEC-1 to SEC-17 of ADR-0025 where they apply, plus: a person without a
    right gets nothing; a viewer cannot edit, run or reach the terminal; an anonymous link never
    runs code and expires; a removed right closes the channel within one second; an expired or
    reused peer invitation is refused; no file content reaches a relay in plain text.

## M16 Remote control of the local app

1. **Pairing instead of open ports:** a device is paired once (QR code or short code shown in the
   window, public keys exchanged, the user confirms on both sides). Nothing listens on a network
   interface other than `127.0.0.1` unless the user switches remote access on.
2. **Connection paths:** direct in the local network, through an SSH tunnel, or through a relay the
   user runs (for example on their own server); traffic is end-to-end encrypted between the paired
   devices, so a relay sees no content.
3. **Rights per device:** view, edit, run, debug, Git push and agent control as separate grants
   (agent rights, SPEC-0053); every action is logged; a device can be revoked at once.
4. **What a client can do:** list and open projects, edit with the window's unsaved buffers, run
   and debug, read terminals, test results and problems, commit and push, start and watch agent
   tasks, receive notifications. Screen mirroring is not planned; the operating system's own tools
   cover it.
5. **Base:** the headless service `idecraft serve` (M10.3, built with M6.12); the HTTP transport of
   SPEC-0221 stays for local agents.

## M17 Mobile apps (iOS, Android)

1. **Architecture (ADR before work starts):** the Rust core (buffer, syntax, Git, database
   drivers, protocol client) is compiled for iOS and Android and bound to a native interface
   (SwiftUI, Jetpack Compose). GPUI has no mobile target, so the desktop UI is not reused; design
   tokens and behaviour follow ADR-0009 where they fit a touch interface.
2. **Remote mode:** connect to a paired desktop app (M16) or a server backend (M6.12/M6.14);
   project tree, editor with highlighting and completion from the host, search, run and debug
   controls, terminal, test results, problems, Git, agent tasks and notifications.
3. **Local mode "IDECraft Lite"** with the essentials: projects in the app's storage and the
   system's file providers; editor with highlighting, search and replace, undo, local history;
   Git clone, commit, pull and push over HTTPS (tokens in the system keychain, entered by the user);
   Markdown preview; SQLite browser and SQL console for remote databases; offline use.
4. **Running code locally:** Android can bundle interpreters and language servers as processes.
   iOS allows no processes and no JIT for apps, and the App Store rules limit running downloaded
   code; local running on iOS is therefore limited to interpreters built into the app (for example
   Python and JavaScript, run in-process) and is checked against the current review guidelines
   before work starts. Everything else runs remotely.
5. **Testing:** simulators and emulators (at most two per session, by the project's rules for its own machines), the
   window self-test pattern ported to the apps, and agent-run hand tests on devices.
6. **Distribution:** developer accounts for Apple and Google are the Project Owner's (sign-in by
   the project owner); internal builds through TestFlight and Play internal testing; any public release needs
   the Project Owner's explicit approval, as GO-4 does for the repository.

## M18 Web IDE (browser, self-hosted or by a service provider)

Requested by the Project Owner on 2026-11-07 together with the apps: IDECraft in the browser without
a desktop app, like the cloud development environments on the market, first self-hosted, later
offered by a service provider. The market lesson (2024–2026: Cloud9, Glitch, Gitpod Classic, Fleet,
Ona Cloud and Firebase Studio closed, some with weeks of notice) is built in: the same protocol and
binary self-hosted and at a provider, a full export at any time, and a fixed notice period.

1. **Transport first (before QG-1, with M6.12/M16):** `idecraft serve` gets a WebSocket endpoint
   (wss only, TLS 1.3 with X25519MLKEM768, ADR-0025) and serves the web client's assets itself, so
   client and server versions always match; COOP/COEP/CORP and a strict CSP on the IDE origin;
   bound to `127.0.0.1` unless switched on, always authenticated (no token-less mode).
2. **Rendering spike (4–6 weeks, before the decision):** GPUI on `wasm32` with WebGPU and a WebGL2
   fallback, as a hybrid: GPUI draws editor, trees, terminal grid and tool windows; a thin DOM layer
   provides a hidden text area (IME, mobile keyboards, clipboard), an ARIA mirror generated from the
   semantic tree of ADR-0027 and browser menus; syntax highlighting comes from the backend. Abort
   criteria: bundle size and first frame, IME with German, Japanese and Chinese input methods,
   VoiceOver and NVDA over editor and project tree, WebGL2 fallback, memory with 50 open files. If
   accessibility fails, the fallback is a lean TypeScript client (editor, tree, terminal, review,
   working together) and the full interface stays in the desktop app. Theia/OpenVSCode front ends
   and pixel streaming are not used.
3. **Web client:** the same window as the desktop where the spike allows it, with browser keys that
   must not close the session (Ctrl+W, Ctrl+N) handled; joins project rooms of M6.15.
4. **Workspaces:** defined by `devcontainer.json` (portable, no own format); self-hosted on the
   user's server, VM or container (gVisor inside one organisation); at a service provider one
   microVM per workspace (Firecracker, Cloud Hypervisor or Kata), freshly patched on every start,
   snapshots never shared between tenants without re-seeding; no Kubernetes as the default base for
   interactive workspaces.
5. **Network and previews:** outgoing traffic denied by default with an allow list per organisation,
   the cloud metadata address blocked; forwarded ports private and authenticated by default;
   previews on a separate registrable domain with one subdomain per workspace and port; the proxy
   overwrites `X-Forwarded-*` and checks `Host` against an allow list (DNS rebinding).
6. **Secrets and extensions:** secrets never as environment variables for every process, not
   carried into forks; short-lived credentials (OIDC) instead of fixed tokens; plugins only from an
   allow list or own registry with pinned versions, updates in quarantine, plugins sandboxed with
   rights.
7. **Identity and audit in the base product:** OIDC and passkeys, roles of M6.15, audit log and
   encryption at rest without a premium tier (ADR-0007).
8. **Operation:** stage 1 self-hosting as one binary (backend, web client, endpoint) with TLS and
   OIDC; stage 2 a lean declarative control plane at a provider (microVMs from `devcontainer.json`,
   warm snapshots with a built index, idle and runtime timeouts measured on real activity including
   builds, runs and agents), same protocol, so a customer can move to self-hosting at any time.
   Provider terms: per seat plus transparent compute with budget limits, AI with the customer's own
   key or a capped quota with an estimate up front, AI switchable off per organisation, public URLs
   only for verified accounts (abuse defence), 12 months' notice, full export at any time; EU-only
   regions and customer-managed keys as options.
9. **Security tests:** SEC-1 to SEC-17 of ADR-0025 where they apply, plus: no endpoint without
   authentication; a preview cannot read the IDE origin; forged `X-Forwarded-Host` and rebinding are
   refused; a workspace cannot reach another or the metadata address; an unlisted plugin does not
   load.

## Order and dependencies

1. Current comparison queue (F-070 window check, database rows F-051–F-055, further rows by weight).
2. M6.14 step 1 (backend builds for every platform) and M6.12 step 1 (observation) can start in
   parallel; then M6.12 (session protocol, window), M6.13, the rest of M6.14.
3. M6.12 builds the backend as the headless service `idecraft serve` (M10.3); M16 follows M6.12.
4. M6.15 working together: the project room follows M6.12 (it needs only `idecraft serve` and the
   session protocol); desktop windows join first, the web client with M18; the peer case follows
   M16 (it reuses pairing, rights per device and the relay).
5. M18 step 1 (wss transport, served assets) and step 2 (rendering spike) before QG-1; the web
   client, workspaces and the provider stage after QG-1.
6. After QG-1: M17 remote mode → M17 local mode, alongside M18 steps 3–8.

## Open items for the Project Owner

- Apple Developer Program and Google Play Console accounts, when M17 starts.
- Whether a relay for M16 runs on the user's own server or only direct and SSH paths are
  offered (default: direct and SSH first, relay as an option).
- Whether SSH channels must use a post-quantum key exchange and refuse hosts without one
  (PO-STATUS question 29, ADR-0025 question 4; default: required, from the next change of the SSH
  options).
