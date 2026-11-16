# SPEC-0324: Remote control of the local machine (first slice of M16)

- **Version:** 3 · **Date:** 2026-11-12 (v3: a run's notice exists before the run is reported finished, behaviour 13, found by the server CI; v2: the end of a session is in the audit before the connection closes, behaviour 8, found by the server CI)
- **Sources:** [`REMOTE_AND_MOBILE.md`](../architecture/REMOTE_AND_MOBILE.md) section M16 and the
  Project Owner's order of 2026-11-09 to implement M16 and decide open points (`GEN`),
  [`ADR-0025`](../adr/0025-network-security-for-remote-connections.md) §2, §4, §5, §7,
  [`ADR-0026`](../adr/0026-session-protocol-window-and-serve.md), SPEC-0317, SPEC-0318,
  SPEC-0321, SPEC-0323 (`GEN`). QR symbols: ISO/IEC 18004 (`STD`). Crate for the QR encoder:
  `qrcode` (MIT or Apache-2.0, `OSS`, chosen here as ADR-0025 §10 requires). No observation of the
  reference is needed: remote control of a running IDE from another device is IDECraft's own scope.
- **Feature matrix rows:** M16 (no reference counterpart)
- **Reuses, does not replace:** the pairing, key store, channel and listener of `crates/pairing`
  (SPEC-0321 behaviours 13–23); the rights and audit of `Backend` (SPEC-0317, SPEC-0318).

## Purpose

A project on this machine, served by `idecraft serve --pair <folder>`, can be used from another
paired device without exposing it to the network: the device can read and edit the unsaved
buffers of the session, run, debug, use terminals and tests, use Git, drive agent tools and receive
notifications, within rights set per device, with a log and revocation that act at once.

This slice is headless: the accepting side is `idecraft serve`; the device side is the window
(`paired://`, SPEC-0321) or the command line `idecraft device`. The desktop window accepting
devices for its own local project, its "Pair a device" UI, the relay and mobile clients are not
part of it (see "Out of scope").

## Behaviour

### Listening and pairing

1. Nothing listens without `--pair`. `--pair` binds `127.0.0.1:47100` by default; a non-loopback
   `--listen` address is refused unless `--allow-network` is also given, and the start line says
   the listener is reachable from the network (SPEC-0321 behaviour 13, unchanged). [ADR-0025 §5]
2. **A device is bound to one project.** A session of a paired device needs a backend with a fixed
   folder. `idecraft serve --pair` without `<folder>` is refused before anything listens, and
   `session.hello` from a device on a backend without a fixed folder answers `denied`. A device
   can name no other folder. Serving several projects means several listeners (one port each).
   [ADR-0025 §4; deviation from SPEC-0321 behaviour 13, where the folder was optional, because
   a device would otherwise read any folder of the user.]
3. `--qr` opens the pairing window with a 256-bit secret instead of the short code, and shows a QR
   code plus a link `idecraft-pair://v1/<address>?k=<host key hex>&s=<secret>` that carries the
   address, the host's static public key and the secret. The device derives the pairing key from
   the secret exactly as from a short code (label `idecraft-pair-v2`), and **checks that the
   responder's static key equals `k`** before it shows any digits. `idecraft device pair <link>`
   takes the link. The link is written only to the console of `serve`, like the code. The window
   field and `remote_pair_start` take a short code only; a link there is future window work. [ADR-0025 §2]
4. Digits are confirmed on both sides as in SPEC-0321 behaviour 17. No MCP tool confirms digits,
   widens a right or switches remote access on; these are user actions. The confirmation prompt
   on a command line (`serve --pair` and `idecraft device pair`) reads a terminal only: with
   standard input not a terminal the pairing is declined and says so. [ADR-0025 §2, §4]

### Connection paths

5. **Local network:** `paired://<host>:<port>` against a listener with `--allow-network`, as in
   SPEC-0321. mDNS or any other discovery is not used. [ADR-0025 §5]
6. **SSH tunnel:** the host string `paired://<alias>[:<port>]@ssh` connects through the system
   `ssh` with `ssh -W 127.0.0.1:<port> <alias>`, built by `SshCommand` with all its rules (strict
   host keys, post-quantum key exchange). No local port is opened. The listener on the far side
   stays on its loopback address. The Noise channel runs end to end through the SSH pipe, with the
   same pins, so an SSH server sees only ciphertext. [ADR-0025 §1b, §7]
7. The channel carries over any byte pipe. Pins are stored per path string. Adding a path to a
   host already pinned (`idecraft device add-path` / MCP `remote_pair_add_path`) copies the pin; the
   next handshake proves the key, so no new digits are needed. [GEN]

### Rights, log, revocation

8. Rights per device are the ADR-0025 §4 grants (`view`, `edit`, `run`, `debug`, `vcs-write`, `push`,
   `ports`, `agents`); `view` is always among them. `serve --rights <fingerprint> <list>` replaces
   them at once. **A narrowing ends the device's sessions within a second** (terminals, runs and
   debug sessions end with them; the device reconnects with its new rights), so no open stream keeps a
   right that was taken. A widening applies to the running sessions and they receive
   `device.rights`. [ADR-0025 §4]
   **(v2)** The end and its reason (`rights-reduced`, `revoked`, …) are written to the audit before
   the session's connection is closed, so whoever sees the session end finds the reason recorded;
   before v2 the record followed the close and could be missing for a moment. [ADR-0025 §4, GEN]
9. `agents` lets a device use the host project's MCP tools (`mcp.*`) within the host project's
   agent rights (SPEC-0053, unchanged); it never widens an agent's rights. Because those tools
   can edit, the rights prompt says so. [ADR-0025 §4]
10. **Every request of a device is logged** in `remote/audit.jsonl` (time, `device:<fingerprint>:
    <name>`, method, target, outcome; never content), including refused ones and handlers that
    run later. `session.ping`, `session.ack` and `$/cancel` are not logged, and `doc.edit` is
    logged at most once per 10 s per session and document. `serve --device-log` (all) and `serve --device-log-of <fingerprint>`
    and the MCP tool `device_log` show the newest lines. [ADR-0025 §4, §8]
11. `serve --access-off` closes remote access: a running listener stops accepting, closes every
    session of a device within a second and ends. Starting `--pair` again switches it on. Revocation
    stays as in SPEC-0321 behaviour 19. [ADR-0025 §4, §5]

### What a device can reach

12. Reachable with the rights above, through the session protocol of ADR-0026 and the families of
    SPEC-0318: the project (`fs.*`), buffers including unsaved edits of other sessions on this
    backend (`doc.*`), run and test (`run.*`), debug (`dap.*`), terminals (`term.*`), Git (`git.*`),
    agent tools (`mcp.*`), rooms (`room.*`). [ADR-0026 §6]
13. New methods: `device.whoami` (right `view`) answers the device name, fingerprint and rights;
    `notice.list {since?}` (right `view`) answers the newest notices with increasing `seq`. The
    host keeps a ring of 200. A notice is `{seq, time, kind, title, detail?}`, posted when a run
    ends, when `git.push` ends and when a device is paired or revoked, and it is pushed to every
    session as `notice.posted`. [GEN]
    **(v3)** A run's notice is posted before the run's `finished` event is sent, so a window that
    lists the notices on that event finds it; before v3 the event came first and the list could
    still be empty. [GEN]
14. The session capability list gains `notice` and `device`. Older clients ignore the additions. [ADR-0026 §6]

### Command line and MCP (ADR-0008)

15. `idecraft device pair <address> [--code <code>] | <link>`, `device hosts`, `device forget <address>`,
    `device add-path <address> <new path>` and `device call <address> <folder> <method> [<json>]`.
    `idecraft-serve` has the same commands. `device call` prints the result as JSON and exits non-zero
    with the error. [GEN]
16. MCP tools on the accepting machine, each logged and needing the user-granted `ssh` right:
    `device_list` (never a key), `device_log {fingerprint?, limit?}`, `device_restrict {fingerprint, rights}`
    (only a subset of the current rights, otherwise refused), `device_revoke {fingerprint}`,
    `remote_access_status`, `remote_access_off`; on the device side `remote_pair_add_path {address,
    path}`. There is deliberately no tool that opens the network, confirms digits or widens rights.
    [ADR-0008, ADR-0025 §2, §4]

## Security (ADR-0025 §9)

| Id | Covered by |
|---|---|
| SEC-1, SEC-3–SEC-11, SEC-13–SEC-16 | SPEC-0321, unchanged channel and listener; AK-2 to AK-7 below add device-specific cases |
| SEC-12 | not applicable to this slice (no relay) |

## Acceptance criteria

- **AK-1** Pair, open, edit, save and resume work over the listener bound to a non-loopback
  interface address of this machine, on `--allow-network`; refused without it.
- **AK-2** A device on a backend without a fixed folder gets `denied` at hello; `serve --pair` without
  a folder refuses to start before anything listens.
- **AK-3** A QR link pairs and digits match; a wrong `k` is refused before digits are shown and
  nothing is stored.
- **AK-4** The channel works over a child-process pipe; `ssh -W` is built by `SshCommand` for a
  `@ssh` host (argument test), and a session over a real OpenSSH server (`sshd`) pairs, opens a
  file and is resumed (ignored test, run with a local `sshd`).
- **AK-5** `serve --rights`: a widening applies to a live session without reconnecting and the session is
  told; a narrowing ends the live session within a second and the next session has the new rights.
- **AK-6** Every request of a device is in the log with its device, including a refused one and a
  handler run later; a `doc.edit` burst is coalesced; no file content is in the log.
- **AK-7** `--access-off` closes the sessions within a second, ends the listener and a later connection
  is refused.
- **AK-8** `device.whoami` and `notice.list` answer; a run ending posts a notice and `notice.posted`.
- **AK-9** `device_list`, `device_restrict`, `device_revoke`, `device_log`, `remote_access_status`,
  `remote_access_off` and `remote_pair_add_path` work on the store; `device_restrict` refuses a
  widening; no tool can confirm digits, widen rights or open the network.
- **AK-10** Off-screen window checks are not part of this slice. Window evidence is required before
  M16 can be ticked: the window device side on a real network, the "Pair a device" UI when it exists.

- **AK-11 (v2)** A narrowing ends the session and `rights-reduced` is in the audit as soon as the window
  sees the end (`crates/pairing/tests/it/paths.rs::ak5_widening_applies_live_and_narrowing_ends_the_session`,
  20 runs without a failure).

- **AK-12 (v3)** After `run.event` `finished`, `notice.list` holds the run's notice
  (`crates/host/tests/it/host.rs::runs_stream_their_output_report_tests_stop_and_survive_a_break`, 20 runs).

## Out of scope

The desktop window accepting devices for its local project and its pairing UI (needs a
backend inside the window); the relay (ADR-0025 §7, the option M16 lists last); several projects
behind one listener; dedicated agent task lists (only `mcp.*` exists today); mobile clients (M17).
