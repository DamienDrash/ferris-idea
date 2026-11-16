# SPEC-0322: Remote debugging — debug adapters on the host, attach, containers and pods

- **Version:** 5 · **Date:** 2026-11-09 (v5: where a stop is shown in a remote project, behaviour
  14; v4: Java debugging back, on private listeners only; v3:
  refused until then; v2: Delve on a private Unix socket; an adapter's port is refused by
  `port.forward`)
- **Sources:** GEN (debugging where the code runs; F-083 "Remote"), STD (DAP; JDWP; gdb remote
  protocol; `docker`/`podman inspect` JSON; `kubectl port-forward`), OSS (`vscode-js-debug`:
  `dapDebugServer.js [port|socket path] [host]`; Delve: `dlv dap --listen=unix:<path>`, tried with
  Delve 1.27.2; `debugpy`; `lldb-dap`; `microsoft/java-debug`: `JavaDebugServer` opens its port
  without a bind address, confirmed on 0.53.1), ADR-0008, ADR-0025 §5 and §6, ADR-0026 `dap` family, SPEC-0039, SPEC-0091,
  SPEC-0097, SPEC-0119, SPEC-0132, SPEC-0147, SPEC-0168, SPEC-0278, SPEC-0317, SPEC-0318
  (behaviour 28 is its foundation)
- **Feature matrix rows:** F-083
- **Roadmap:** M6.13

## Purpose
In a remote project, Debug works as for a local project although nothing of the program or the
debugger runs on this computer: the debug adapter starts on the host through the backend, its DAP
messages travel over the encrypted, authenticated session, and breakpoints, stepping, variables,
evaluation and hot swap behave as locally. The user can also attach to a JVM, debugpy, lldb-server
or Node.js inspector on the host, or in a container or Kubernetes pod the host can reach. No debug
port is opened on the window's machine, ever.

## Behaviour

### Adapters on the host
1. **Transports.** `dap.adapters {}` answers `{root, adapters: [{adapter, installed, transport}]}`,
   `root` being the host project folder and `transport` one of `stdio` (lldb-dap, debugpy,
   kotlin-debug-adapter), `socket` (js-debug, and dlv on a Unix host), `tcp` (dlv on a Windows
   host), `jdtls` (java-debug inside the host's jdtls). Whatever the transport, the window gets one session stream carrying DAP. [ADR-0026 §6]
2. **Socket adapters** listen on a Unix socket in a new folder of mode 0700 below the host's
   temporary folder; other host users cannot reach it, and no TCP port is opened. js-debug takes the
   path as its argument, Delve `--listen=unix:<path>`. When a path below the temporary folder would
   not fit a socket address (104 bytes on macOS), the folder is made in `/tmp`; it is created with
   mode 0700 and creating it fails on anything already there. It is removed with the session.
   (Windows hosts of M6.14 have no such socket; there the adapter takes a free port on the host's
   loopback.)
   js-debug asks for one more connection per debugged process (SPEC-0168):
   `dap.connect {session}` opens a further stream to the same server. [OSS, ADR-0025 §6]
3. **TCP adapters** bind to `127.0.0.1:0` on the host and print the port; the backend connects to
   it on the host's loopback and relays. This remains for dlv on a Windows host, which keeps its
   check that the connecting process belongs to the same user (`--only-same-user`, its default).
   While an adapter's port is open, `port.forward` refuses it as a debug port, whatever its command
   line says and whether or not the port list has seen it; `dap.start` names it as `adapter_port`.
   [OSS, ADR-0025 §6]
4. **java-debug** runs in a jdtls that the backend starts for the project (its own data folder, as
   SPEC-0097), with IDECraft's small bundle beside it. java-debug's own listener is never started:
   the bundle serves the debug protocol on a Unix socket in a new folder of mode 0700, removed with
   the session, and on a host without such sockets (Windows, M6.14) on the loopback address. It also
   replaces the wait for the launched program, which the platform opens on every interface, by a
   port on the loopback address that closes as soon as the program has connected. No listener exists
   on another interface, locally or on a host, also during hot swap. Only the java-debug version the
   bundle was built for (0.53) starts; another one is refused with a reason. `dap.start {adapter:
   "java-debug", main_class}` also answers `classpath: {modules, classes, project}` for the launch.
   `dap.host {session, action: "settings"|"sources"|"exception_types", value}` passes debug settings,
   changed files (hot swap, SPEC-0091 v12) and exception type filters to that jdtls. The same applies
   to a local Java session (SPEC-0097). [SPEC-0097, SPEC-0091, ADR-0025 §5 and §6, PO-STATUS question 35]
5. **Starting.** `dap.start {adapter, mode, attach?, main_class?, prepare?}` answers `{session,
   stream, adapter}`. `mode` is `launch` (default) or `attach`. A launch runs project code and
   needs a trusted host project; an untrusted one gets the refusal of SPEC-0318 behaviour 5.
   Attaching needs no trust, like local attach (SPEC-0147), except for java-debug whose jdtls loads
   the project. `prepare` (Kotlin's build command, trusted only) runs first, in the host project,
   and its output comes back as `prepared`. All `dap.*` that change something are audited, under the
   `debug` right. [SPEC-0278, SPEC-0318]
6. **Planning.** `dap.plan {name | context: {doc, line}}` answers what a run configuration or the
   run marker at a line stands for, resolved on the host with the host's `PATH`: `{name, kind,
   adapter, interpreter, program, args, cwd, env, prepare}`, the same plan as a local Debug
   (SPEC-0091). A configuration opening a page in a browser is refused with a reason. [SPEC-0091]
7. **Unchanged messages, exactly once.** The backend keeps the last 1 MiB of what each adapter
   connection sent. After a break the window calls `dap.attach {session, from}` with the bytes it
   has and gets `{stream, start, received}`; `received` counts the bytes the adapter got from the
   window, which resends only what follows. When `start` is later than `from` (more than the kept
   amount came during the break) the session ends with a message instead of guessing. [SPEC-0317]
8. **End.** `dap.stop {session}` ends the adapter, its socket folder, the forwards of the attach
   target and all connections of that adapter. So does the end of the session (the reattach
   period is over); `dap.exited {session, code}` reports it. [SPEC-0318]
9. **Host processes.** `dap.processes {}` lists the attachable processes of the host account
   (name, command's program only, runtime, debug port) as the local dialog does. [SPEC-0147, SEC-10]

### Attaching
10. **Targets.** `dap.start {mode: "attach", attach}` takes `{kind: "pid", pid}`,
    `{kind: "address", host, port}`, `{kind: "docker", container, port}` or
    `{kind: "pod", pod, namespace?, context?, port}`; each answer has `address: {host, port}` (or
    `pid`) for the attach request, and `mappings: [[host_path, target_path]]` when known. The
    adapter runs on the host and connects from there, so JVM (java-debug), debugpy (a
    `debugpy --listen` program is talked to directly), lldb (`gdb-remote-port`) and Node.js
    inspector targets need no port on the window's machine. [ADR-0025 §6, SPEC-0147]
11. **Other addresses.** An address other than the host's loopback is the user's explicit entry
    only; the Debug console says that the traffic between the host and that address is not
    protected by IDECraft. Agents may name only loopback addresses, as for forwards. [ADR-0025 §6]
12. **Containers.** For `docker` the backend reads `docker inspect` (or `podman inspect` when
    Docker is absent) on the host: a port published on the host's loopback is used, else the
    container's own address on the host's bridge network. A port published on all interfaces is
    not used, and the answer says how to publish it on `127.0.0.1` instead (we do not build on an
    open debug port). Bind mounts covering the host project give the `mappings`. Names outside
    `[A-Za-z0-9_.-]` are refused. [GEN, ADR-0025 §5]
13. **Pods.** For `pod` the backend starts `kubectl port-forward --address 127.0.0.1 pod/<name>
    :<port>` on the host (namespace and context when given), waits for its forwarding line and uses
    that loopback port; the tunnel to the API server is `kubectl`'s encrypted one. It ends with the
    debug session. A pod has no bind-mount information; the user gives the mapping. [STD, SPEC-0119]

### Source mapping
14. **Window and host.** Breakpoints and frames in the window use the project's stand-in folder
    (SPEC-0317); the DAP link rewrites `path` values between the stand-in folder and the host
    project folder in both directions, on the wire, so the debugger code stays the same. A frame
    outside the project keeps its host path and opens nothing here; the console and the call stack
    show it. No file of the project exists on this computer, so whether a source belongs to the
    project is decided by its path, never by looking at the local disk: a stop or a selected frame
    below the stand-in folder highlights the line and moves the caret there as locally, and such a
    frame is a project frame in the call stack, not a library frame. A file not open yet is opened
    through the session and gets the highlight and the caret when it has arrived; a later stop
    replaces that, in the same file without opening it a second time, and the end of the session
    drops it. [SPEC-0317]
15. **Host and target.** `mappings` become the adapter's own mapping in the attach request:
    debugpy `pathMappings`, js-debug `localRoot`/`remoteRoot`, lldb `sourceMap`; java-debug finds
    sources through jdtls by class and needs none. The user can name a target root in the dialog,
    which replaces the detected mapping. [STD]

### Window
16. **Debug** in a remote project (Debug button, ⌃⇧D, run markers' Debug item) calls `dap.plan`,
    then starts as behaviour 5 and opens the normal Debug tool window; Stop, Rerun, stepping,
    variables, evaluation, Set Value, watches and hot swap work as locally. The stand-in folder
    never leaves the window. [SPEC-0091]
17. **Attach to Process** (⌥⇧F5) in a remote project lists the host's processes and accepts
    `host:port`, `docker:<container>:<port>` and `pod:[<namespace>/]<pod>:<port>`, each optionally
    followed by `=<folder in the target>` for the source mapping. These two prefixes are refused
    with a reason in a local project (out of scope below). [SPEC-0147]
18. **A break** shows "Reconnecting…" in the Debug console; the session continues after the
    reattach. Requests that time out meanwhile fail as locally; messages are neither lost nor
    repeated (behaviour 7). A session whose adapter ended during the break ends with a message.
    [SPEC-0317]

### Agents (ADR-0008)
19. `remote_debug_start {host, folder, name | doc+line, breakpoints?, wait_seconds?, dry_run?}` and
    `remote_debug_attach {host, folder, runtime, target…, breakpoints?, wait_seconds?, dry_run?}`
    start a session on an open remote project, with `dry_run` defaulting to true and the `debug`
    right required otherwise. The session then answers to the existing `debug_*` tools by its id. [ADR-0008]

## Acceptance criteria
- **AK-1** `dap.adapters` names the transport of every adapter and the host root.
- **AK-2** A real debugpy on the host stops at a breakpoint set in the window's stand-in path;
  frames come back with stand-in paths; variables and stepping work over the session stream.
- **AK-3** A real js-debug and a real Delve each start on a Unix socket in a 0700 folder and no TCP
  port is opened by them; a breakpoint stops the Delve session; a program with a child process is
  debugged through `dap.connect`.
- **AK-4** A TCP adapter (`dlv`'s argument shape, fake program) is relayed; the backend's own
  listeners stay none, and the port is refused by `port.forward` while the adapter runs and no
  longer afterwards (SEC-7).
- **AK-5** An untrusted host project refuses `mode: launch` and `prepare`, not `mode: attach`.
- **AK-6** A break during a debug session and `dap.attach` lose and repeat no byte in either direction.
- **AK-7** More than the kept amount during a break ends the session with a message.
- **AK-8** Attaching to a `debugpy --listen` program on the host stops at a breakpoint.
- **AK-9** Docker targets: published loopback port used, wildcard refused with a hint, bridge
  address otherwise, bind mounts give mappings, bad names refused.
- **AK-10** Pod targets: `kubectl` arguments, forwarding line, tunnel ends with the session.
- **AK-11** `mappings` become each adapter's mapping argument.
- **AK-12** `dap.processes` hides arguments; `dap.plan` resolves on the host and refuses a browser configuration.
- **AK-13** MCP: `remote_debug_start` and `remote_debug_attach` plan by default, need the `debug` right, and hand a session to `debug_*`.
- **AK-14** Window: Debug and Attach to Process of a remote project (behaviours 16–18). Window check.
- **AK-15** A real java-debug in jdtls, on a host and locally, stops at a breakpoint; the adapter has
  no TCP port; while it runs, including the program's start and a hot swap, `lsof` shows no listener
  of its processes on anything but the loopback address.

- **AK-16** A stop in a remote project shows without any local file: marked at once in an open
  editor, after arrival in a new one, not at all outside the project; its frames are project frames.
  Window: the line is highlighted and the caret moves (self-test step).

## Out of scope
Containers and pods as attach targets of a local project (the same resolution would run locally;
it was ordered for remote work), Deployments and other workload kinds instead of a pod, debugging
in a page on the host's browser (no browser there), opening sources outside the project from a
frame, Windows hosts (M6.14), debug port forwarding to this computer (never, ADR-0025 §6).

## Known deviations from the reference
The comparison with the reference's remote debugging (F-083) has not been done. Deliberately not
offered regardless of it: a debug port reachable on this computer.

## Known deviations from ADR-0025 and the security-first rule
None for the network. java-debug 0.53.1 alone is not private: measured on 2026-11-09 with `lsof`, a
real session has the adapter's port and the wait for the program on every interface without
authentication, which is why its listeners are replaced (behaviour 4). What remains is on the
loopback address only: while a program starts (normally well under a second), another local account
could connect to the port the debugger waits on and pose as the program; it would reach the
debugger, not the program, and the handshake rejects anything that is not JDWP. On a Windows host
the adapter's port is on the loopback address too, without the per-account protection of a socket
folder.
