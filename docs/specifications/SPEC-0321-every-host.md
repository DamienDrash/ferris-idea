# SPEC-0321: Every host

- **Version:** 5 · **Date:** 2026-11-11 (v5: a used pairing code closes the pairing window however the confirmation ends, and each side still reads the other's `no` when its own answer can no longer be sent, behaviour 17, AK-13; v4, Project Owner's decisions of 2026-11-09 evening: the host confirms storing a paired device before the window pins it, behaviour 17, AK-21; the Secret Service on Linux, debug builds on Linux keep keys in files too, behaviour 20, AK-22; v3: debug builds on macOS keep keys in the file store; v2: a backend older than the window is refused when connecting, behaviour 5a)
- **Sources:** [`ADR-0025`](../adr/0025-network-security-for-remote-connections.md) (`GEN`, `STD`: Noise rev. 34, FIPS 203),
  [`ADR-0026`](../adr/0026-session-protocol-window-and-serve.md) (`GEN`),
  [`REMOTE_AND_MOBILE.md`](../architecture/REMOTE_AND_MOBILE.md) section M6.14 (`GEN`),
  SPEC-0317, SPEC-0318. Docker CLI, `kubectl` and `wsl.exe` command lines: public documentation of
  those tools (`DOC`, facts only). No observation of the reference is needed: remote hosts beyond
  SSH are IDECraft's own scope.
- **Feature matrix rows:** F-083

## Purpose

A folder on any machine the user can reach opens as a remote project: over SSH on Linux, macOS and
Windows; inside a Docker container or a Kubernetes pod; in a WSL distribution; or on a machine
without SSH, by starting `idecraft serve --pair` there and pairing it with the window.

## Behaviour

### Backend builds

1. There is a headless backend program `idecraft-serve` (crate `crates/serve`) without any windowing
   toolkit. `idecraft-serve serve …` and `idecraft-serve mcp <folder>` behave as `idecraft serve …`
   and `idecraft mcp <folder>`; `idecraft-serve --version` starts with `idecraft <version>`. [GEN]
2. Targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`,
   `aarch64-apple-darwin`, `x86_64-pc-windows-gnu`, `aarch64-pc-windows-gnullvm`. `scripts/build-backends.sh`
   builds them into `dist/backends/<target>/` and writes `dist/backends/SHA256SUMS`; the CI workflow
   has a job per target. The result table of the last run on real platforms is kept in
   the status file; a target that was only compiled is named as such. Windows builds leave out the
   feature `mcp` (the host's `mcp.*` family and `idecraft-serve mcp`), because the database stack
   below it does not build there yet; such a backend does not advertise that family. Linux builds
   compile OpenSSL in, so they need no `libssl` on the host. [GEN]
3. Backends are looked up on the window's machine in this order: `IDECRAFT_BACKENDS_DIR`, the folder
   `backends` beside the program, `../Resources/backends` of a macOS bundle. When the host has the
   window's platform, the running program itself is used if no dedicated build exists. [GEN]

### Bootstrap

4. Before the backend runs, a minimal bootstrap finds the host's system and processor: `uname -sm`
   through a POSIX shell, else PowerShell (`$env:PROCESSOR_ARCHITECTURE`). No other host tool is
   assumed to detect it. [GEN]
5. On a POSIX host the backend is installed below `~/.cache/idecraft/bin/` under a name that includes
   the version and the build (as SPEC-0317 does); on Windows as
   `%LOCALAPPDATA%\idecraft\bin\idecraft-serve-<version>-<sha12>.exe`, with `<sha12>` the first 12
   hex digits of the SHA-256 of the build. The SHA-256 is computed on the window's side and compared
   with what the host computes before the program is started (on Windows before it is moved into
   place). A mismatch deletes the copy and refuses; an existing installed file is checked the same way
   before each start. A build that cannot run on the host (`--version` fails, for example the glibc
   build on a musl system) is refused and named. [GEN]
5a. A build on the window's machine or a file on the host can be stale although its version number
   matches. The window therefore compares what a backend serves, not what it is called: the answer
   to `session.hello` carries the backend's `minor` (ADR-0026 §4), and a backend whose `minor` is
   lower than the window's, or that names none, is refused while connecting, with its build, both
   revisions and what to do (build the backends again or update IDECraft on the host). The session
   it had opened ends at once, so nothing stays on the host. A newer backend is accepted: it serves
   a superset. This also holds for paired hosts and for a new backend after a reconnection. [GEN]
6. A platform without a build is refused and named with what to do ("no backend build for
   FreeBSD amd64 in …; install `idecraft-serve` there and start it with `--pair`, or build it with
   `scripts/build-backends.sh`"). Nothing is worked around. [GEN, ADR-0025 §8]
7. After the bootstrap no host tool is used for project work: `fs.*`, `doc.*`, search, Git, terminals
   and runs are the backend's (SPEC-0317, SPEC-0318), so the host's shell no longer matters. The
   old single-file path of SPEC-0121 (`Remote::read` and friends) stays POSIX-only and is
   not offered for a host that is not POSIX. [GEN]
8. On a Windows host the reattach socket does not exist (ADR-0025 §5 describes a Unix socket), so a
   broken SSH channel is recovered by starting a new backend and reconciling each document
   (SPEC-0317 behaviour for a lost backend). The window says so in the connection log. [GEN]

### Transports

9. A host is named by one string, which the window, the recent list and MCP tools carry
   unchanged. [GEN]

   | Host string | Transport | Started command |
   |---|---|---|
   | `alias`, `user@host:port` | SSH (ADR-0025 §1a, unchanged) | system `ssh` through `SshCommand` |
   | `docker://<container>` | Docker `exec` | `docker exec -i <container> sh -c <line>` |
   | `podman://<container>` | Podman `exec` | same with `podman` |
   | `k8s://[<context>/]<namespace>/<pod>[/<container>]` | Kubernetes `exec` | `kubectl [--context C] exec -i -n N P [-c X] -- sh -c <line>` |
   | `wsl://<distribution>` | WSL, Windows windows only | `wsl.exe -d <D> --exec sh -c <line>` |
   | `paired://<host>[:<port>]` | paired channel | none (behaviours 13 onward) |

   In `k8s://a/b/c` the first part is a context only when it contains `:` (as cluster ARNs do);
   otherwise it reads as `<namespace>/<pod>/<container>`. A context without `:` needs all four parts.

10. Each part of such a string is checked against a strict character set (letters, digits, `.`, `_`,
    `-`, plus `:` in a paired address) before any command is built, and a part cannot start with
    `-`, so no part is read as an option by the engine program. [GEN]
11. The `docker`, `podman` and `kubectl` channels rely on the access the user already has to that
    engine or cluster; they are local command channels (container runtime socket or the
    cluster's TLS API). No port is opened on either side and no credential passes through
    IDECraft. WSL is a local channel of Windows. On another system a `wsl://` host is refused and
    names the reason. [GEN, ADR-0025]
12. Container and pod names that go away, a missing engine program and refused access are
    named with their cause and what to do, like SSH refusals. [GEN, ADR-0025 §8]

### Paired channel (`idecraft serve --pair`)

13. `idecraft serve --pair [<folder>] [--listen <addr>:<port>] [--allow-network]
    [--new-device] [--grant <rights>] [--key-file]` starts a listener. The default address is
    `127.0.0.1:47100`. A non-loopback address is refused unless `--allow-network` is also given,
    and then the start line says that the listener is reachable from the network. Without `--pair`
    nothing listens. [ADR-0025 §5]
14. The listener accepts only the Noise handshakes of ADR-0025 §1b, `Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s`
    while a pairing window is open and `Noise_XKpsk3_25519_ChaChaPoly_BLAKE2s` from pinned keys, each
    combined with ML-KEM-1024 as §1c says, with prologue `idecraft-session/1;pq=ML-KEM-1024;window>serve`.
    There is no negotiation and no field outside Noise. Message 1 has exactly 1616 bytes in both
    patterns; the responder tries `XKpsk3`, which decrypts only under its static key, and then
    `XXpsk3` only while a pairing window is open. Anything else closes the connection with no byte
    in reply. [ADR-0025 §1b, §1c, SEC-5, SEC-6, SEC-15]
15. Implementation of `ADR-0025` §1b lives in the one crate `crates/pairing` (`idecraft-pairing`),
    holding the Noise wrapper, the hybrid construction, the channel, the key store, the pairing
    flow and the listener. Its dependencies are `snow` 0.10 and `libcrux-ml-kem` pinned exactly,
    RustCrypto `ml-kem` as a dev-dependency only. [ADR-0025 §10]
16. **Channel:** session frames travel as a byte stream in Noise messages with a two-byte big-endian
    length prefix. A type byte inside each message distinguishes data, heartbeat and rekey. The sender
    rekeys its direction every 2^20 messages or 60 minutes, announcing it with a rekey message. A
    heartbeat is sent every 15 s; 45 s without traffic closes the connection. After 24 hours the
    connection is closed and the window reconnects with a fresh handshake and `session.resume`
    (the transparent replacement ADR-0025 §1b asks for). [ADR-0025 §1b, §8]
17. **Pairing:** the listener opens a pairing window (at most 5 minutes, one device) when no device
    is paired yet or when `--new-device` is given, and prints to its console a 10-character
    Crockford Base32 code, the address and its key fingerprint. The window asks the user for address
    and code. Both sides derive the pairing key `HKDF-SHA256(ikm = code, info = "idecraft-pair-v2")`.
    After the handshake each side shows six digits (`HKDF-SHA256(ikm = final handshake hash, info =
    "idecraft-sas-v1" ‖ both fingerprints ‖ device name)`); the user confirms on both sides. Only
    then does each store the other's static key. A rejection, a mismatch, three failed handshakes or
    the end of the 5 minutes closes the window; nothing is stored. The QR code of ADR-0025 §2 belongs
    to M16. [ADR-0025 §2, SEC-13]

    The order after both confirmations (v4): each side sends `ok` (or `no`); the host then asks
    its console user for the rights (behaviour 18), stores the device with them durably (the list
    file synced before it is renamed into place, the folder synced after) and only then sends
    `stored` (or `not-stored` when storing failed). The window pins the host only on `stored`, so
    a connection made right after pairing is accepted without any retry. Without `stored` the
    window pins nothing and says the host did not confirm it (an older host, or one that ended);
    the user pairs again. Heartbeats keep the channel open while the host's user picks the rights.
    Before v4 the host said `ok` before it had stored the device, and an immediate connection
    was refused like a changed key (the mobile core retried for 30 s, SPEC-0326 v1). [GEN]

    **(v5)** Once a handshake with the code has succeeded, the code is used: the pairing window
    closes however the confirmation then ends without storing — a rejection on either side, a
    connection that ends or cannot be written to, a channel that cannot be set up. Before v5 a
    failed write of the host's `ok` only counted as one of the three failures and left the window
    open for the same code. A side whose own `ok` can no longer be sent (the other side declined
    and closed first) still reads the other's answer: a `no` is reported as a rejection (the
    window: declined; the host's console: the window declined), anything else as a connection
    that ended. Found as a test that failed in about two of three runs on Linux (the rejection
    raced the close of the other side). [GEN, SEC-13]
18. **Rights:** a paired device gets only `view` unless the console user adds rights at
    the prompt or with `--grant`; rights are per device and are stored with it. A session on the paired
    channel has its device's rights (`not_granted` otherwise). The device name and fingerprint are the
    audit peer. The session of one device cannot be resumed by another. [ADR-0025 §4, SEC-4]
19. **Revocation:** `idecraft serve --devices` lists the paired devices and `idecraft serve --revoke
    <fingerprint>` revokes one: its key is removed, the fingerprint joins the revocation list and a
    running listener closes its sessions within one second. A revoked key cannot handshake, even
    if an old device list is restored. [ADR-0025 §4, SEC-3]
20. **Keys** (ADR-0025 §3, §3a): in a debug build on macOS or Linux, however it is started, they
    stay in the file store, so development and the test suites (which start real backends and
    windows) never prompt for, read or write the login keychain or the Secret Service of the person
    running them; release builds are unaffected. On Linux this is chosen because a developer's
    desktop has a live Secret Service with their login keyring, and only a build-time rule holds
    for every test, also those that start other programs; the Secret Service code itself is tested
    against a private session bus (AK-22). Otherwise the private key and pinned peers are in the
    system keychain on macOS and Windows. On **Linux with a session bus** (v4) they are in the
    Secret Service of that bus (`oo7` over `zbus`, pure Rust, the release GPUI already uses):
    an encrypted Secret Service session only (no fallback to plain transfer), the `default`
    collection only, nothing is unlocked or created by IDECraft; an unreachable service, a missing
    default collection or a locked one refuses the start with what to do (unlock, log in, or
    `--key-file`), never a silent fallback to a file (ADR-0025 §3a). `--key-file` (the window:
    `IDECRAFT_KEY_FILE=1`) stays the user's explicit choice of the file store on such a machine,
    and a key file that already exists is kept, so a host keeps its identity and paired devices do
    not see a changed key; the start line says which store holds the key and why. Linux without
    a session bus and other Unix systems use the file store of §3a with all the checks there:
    `serve` exits with status 78 before anything listens on any finding and repairs nothing; the
    folder and list beside a Secret Service key are created and checked the same way. Other Unix
    systems with a session bus refuse unless `--key-file` is given (no Secret Service support is
    built there). No key, code or token appears in logs, process arguments or environment. The
    code is printed only to the console. [ADR-0025 §3, §3a, SEC-10, SEC-16]
21. **Limits:** a handshake completes within 10 s; at most 10 handshake attempts per minute per source
    address; after a failure that address waits 1 s, doubling to 5 minutes. A plain JSON, HTTP or SSH
    banner gets no reply. [ADR-0025 §8, SEC-5, SEC-9]
22. A window connects to a paired host only with the key pinned at pairing; another key fails
    the handshake and is named as a changed host key with what to do. [ADR-0025 §1b, SEC-1]
23. A broken paired channel is resumed by a new handshake and `session.resume` on the same backend
    process; when the backend has been restarted the window starts a new session and reconciles
    the documents, as with SSH. [SPEC-0317]

### Window and agents

24. Remote Development accepts every host string of behaviour 9 in the host field. For a host other
    than SSH, which cannot be browsed before it runs a backend, the field takes `<host> <folder>`. The
    window title, connection widget and recent list show the host label. A first pairing is typed as
    `paired://host:port#CODE <folder>`: the code is used for the handshake and never stored or shown
    in a label. The window shows the six digits in a dialog and asks the user to confirm. [GEN]
25. MCP (ADR-0008): `remote_project_open` takes every host string and a Windows path as folder;
    `remote_hosts_paired` lists the paired hosts (name, address, fingerprint, never a key);
    `remote_pair_start` (right `ssh`) runs the pairing up to the digits and opens the confirmation in
    the window; `remote_pair_forget` (right `ssh`) removes a pin; `remote_host_probe` reports
    the platform and the backend that would be used without changing anything. **Confirming the
    digits is a user action only:** no MCP tool can confirm them, and without a window
    `remote_pair_start` is refused and says so. [ADR-0025 §2, §4]

## Security (ADR-0025 §9)

| Id | Covered by |
|---|---|
| SEC-1 | AK-5 |
| SEC-2 | not changed (SSH, SPEC-0317 AK-3) |
| SEC-3 | AK-9 |
| SEC-4 | AK-8 |
| SEC-5 | AK-6 |
| SEC-6 | AK-4 |
| SEC-7 | AK-10 |
| SEC-8 | AK-7 |
| SEC-9 | AK-11 |
| SEC-10 | AK-12 |
| SEC-11 | not changed (reattach socket; the paired channel has none) |
| SEC-12 | not applicable (no relay; M16) |
| SEC-13 | AK-13 |
| SEC-14 | AK-14 |
| SEC-15 | AK-3 |
| SEC-16 | AK-15 |
| SEC-17 | not changed for SSH paths; a Docker, Kubernetes or WSL channel starts no `ssh` (workspace scan stays green) |

## Acceptance criteria

- **AK-1** `idecraft-serve` depends on no windowing toolkit (`cargo tree`), and `--version` starts
  with `idecraft <version>`.
- **AK-2** Given a fake host program for each route, the started command lines are those of
  behaviour 9; a part with a forbidden character or a leading `-` is refused; the bootstrap
  reads `uname` and PowerShell answers correctly and picks the build of behaviour 3; a missing build
  is refused as in behaviour 6.
- **AK-3** (SEC-15) A missing, short, long or invalid encapsulation key, a missing ciphertext, plain
  `XK`, `IK`, `XX` handshakes and a prologue without `pq=ML-KEM-1024` all fail. Two runs with
  fixed Noise and KEM inputs that differ only in the KEM secret give different transport keys and
  different digits. A flipped ciphertext bit fails at message 2 or 3. Cross-check: `libcrux-ml-kem`
  against RustCrypto `ml-kem` both ways; a known-answer check for ML-KEM-1024.
- **AK-4** (SEC-6) Another pattern, cipher suite, prologue version, construction or role fails.
- **AK-5** (SEC-1) An unpaired key is refused by the listener; a window whose pin differs fails
  and names the changed key. No session message is processed.
- **AK-6** (SEC-5) Plain JSON, HTTP and an SSH banner get the connection closed with no byte back.
- **AK-7** (SEC-8) A replayed handshake and replayed, reordered or altered transport messages close
  the connection and apply no operation twice.
- **AK-8** (SEC-4) A device with only `view` gets `not_granted` for `fs.write`, `doc.edit`, `term.*`
  and `run.*`; nothing is done and it is logged.
- **AK-9** (SEC-3) A revoked device cannot handshake; a live session of it ends within 1 s.
- **AK-10** (SEC-7) `serve --pair` binds 127.0.0.1 by default, refuses a non-loopback address without
  `--allow-network`, and `serve --stdio` opens no TCP listener.
- **AK-11** (SEC-9) Over 10 attempts a minute are refused, failures back off, a stalled handshake
  closes after the limit, an expired pairing window refuses, three failed pairings close it.
- **AK-12** (SEC-10) A scan of logs, the store's files, decrypted frames and process arguments
  finds no key, code, pairing key or KEM secret.
- **AK-13** (SEC-13) A rejection or mismatch on either side stores no key on either side.
  **(v5)** Also when the side that declines closes before the other answers: the window reports
  the host's rejection as declined, and the window's rejection closes the host's pairing window so
  that the used code opens nothing (`listener::{ak13_a_host_that_declined_first_is_reported_as_declined,
  ak13_a_window_that_declined_first_closes_the_pairing_window}`, each order forced; after the fix
  45 runs of all listener tests with 1, 16 and the default number of threads, and 60 under the load
  of a parallel release build (load 9–11), without a failure; before it 31 of 45 runs failed).
- **AK-14** (SEC-14) Path escapes and oversized frames over the paired channel are refused or close the
  connection; nothing is written outside the project.
- **AK-15** (SEC-16) Every finding of the SEC-16 table exits with 78 before listening and repairs
  nothing; a new key is 0600 in a 0700 folder under umask 000.
- **AK-16** Open, edit and save a file, resume after a broken connection and reconnect after a backend
  restart over the paired channel, end to end in `cargo test`.
- **AK-17** Over a fake `docker` program that really execs the backend, a project opens and a file
  saves. Against the real engine and cluster (`containers.sh` of the reference environment) the same works
  in a Linux container and a pod.
- **AK-18** `remote_project_open` plans each host kind; `remote_hosts_paired` and
  `remote_pair_forget` work on the store; no MCP tool can confirm the digits.
- **AK-19** In a window: Remote Development opens `docker://` and `paired://` hosts, the pairing
  dialog shows the digits, and revocation is visible. Selftest steps exist; a human or agent run
  in a window is still required.
- **AK-20** A backend without `minor`, or with a lower one, is refused by `ClientSession::open` with
  its build and what to do, and the session is ended; the same or a higher `minor` is accepted. A
  change of the methods of a bound backend fails a test until `MINOR` is raised.
- **AK-21** (v4) A window that connects right after `confirm` returns, with no retry, is accepted
  with the rights the host's user picked, even when picking them took longer than the window's
  confirmation; `confirm` returns only after the host stored the device. A host that ends the
  pairing without `stored` leaves nothing pinned on the window. A real key change is still named
  as a changed key (AK-5).
- **AK-22** (v4) Against a real Secret Service (a `gnome-keyring-daemon` on a private
  `dbus-daemon` the test starts in a temporary folder; never the bus of the person running it;
  skipped where the programs are missing, required in CI): the key is created there once and
  reused, no key is in any file, a device's pin is stored and removed on revocation, a window's
  host pin is stored and forgotten, the folder is 0700. A keychain that cannot be read is an error
  and no new key replaces it; a debug build opens the file store with or without a session bus.

## Out of scope

QR codes, relays and the desktop app accepting paired devices (M16), mobile (M17), native Windows
Docker containers (the route assumes a POSIX shell in the container), a Windows reattach
channel, remote debugging (M6.13).
