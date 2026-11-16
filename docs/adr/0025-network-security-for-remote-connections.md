# ADR-0025: Network security for remote connections

- **Status:** accepted · **Version:** 7 · **Date:** 2026-11-10 (v1–v6 on 2026-11-07) · makes the
  Project Owner's rule "remote work is always secured on the network side" (PO-STATUS 2026-11-07,
  memory note, [plan](../architecture/REMOTE_AND_MOBILE.md) section "Network security") testable;
  the procedures below were chosen by the architect and are binding for M6.12–M6.14, M16, M17 and
  every later remote feature. v2 applies the Project Owner's answers to questions 1 and 3 (PO-STATUS
  question 28, 2026-11-07: key file on hosts without a keychain; post-quantum protection in the
  first version) and his rule "security first"; v3 applies his answer to question 4 (PO-STATUS
  question 29, 2026-11-07: every SSH path requires a post-quantum key exchange, section 1a); v4
  applies his answer to PO-STATUS question 30 (2026-11-07: "Ja, Ausnahme" — Git over SSH to Azure
  DevOps, as narrow as possible, section 1a); v5 applies the rule "security first" to the jump-host
  residual risk — every hop of a `ProxyJump` gets the same post-quantum options, a `ProxyCommand` of
  the user's own is refused unless a setting allows it — and makes one start point of `ssh` binding
  (section 1a); v6 adds the device grant `ui` for the UI driver (ADR-0027); v7 applies the Project
  Owner's decision of 2026-11-09 to use the Secret Service on Linux (sections 3, 3a, 10) (see
  "Revision")
- **Related:** [ADR-0026](0026-session-protocol-window-and-serve.md) (session protocol),
  [ADR-0012](0012-headless-core-capability-api.md) (headless core), SPEC-0053 (agent rights),
  SPEC-0121 (SSH host), SPEC-0158 (backend on a host), SPEC-0221 (MCP over HTTP),
  [SPEC-0317](../specifications/SPEC-0317-remote-project-session-and-window.md) (first spec under
  this ADR)

## Context

The plan for remote work opens network paths that IDECraft never had: a window that talks to its
backend on another machine (M6.12), debug adapters and forwarded ports on that machine (M6.13), a
backend started by hand on hosts without SSH (M6.14), a desktop app that paired phones and other PCs
control (M16), and mobile apps (M17). Today IDECraft listens only on `127.0.0.1` with a per-start
token (SPEC-0221) and reaches hosts only through the system `ssh` in batch mode with strict host
keys (SPEC-0121). The eight rules of the plan say what must hold; this ADR says how, so that every
remote spec can be checked against one list.

## Threat model

Assets: the user's source code and unsaved edits, the right to run programs and debuggers on the
host (which is code execution), credentials for Git, databases and AI providers, and the device keys
of this ADR.

| Id | Attacker | Can | Must not be able to |
|---|---|---|---|
| T1 | **On the network** (same Wi-Fi, ISP, a hostile DNS server, a hotel captive portal) | read, drop, delay, replay, inject and redirect packets; spoof names | read or change session content, impersonate either end, downgrade the protocol, pair a device, reach any unauthenticated port |
| T2 | **On the host or on the window's machine as another user** | list processes with their arguments, connect to loopback ports, open world-readable files and sockets, race on `/tmp` | connect to the backend, read tokens or keys, attach to the user's debug ports through IDECraft, hijack a session that waits for reconnection |
| T3 | **A compromised relay** (M16, M17) | see addresses, timing and sizes; drop, delay, reorder and replay frames | read or change content, impersonate an endpoint, keep a session alive after revocation |
| T4 | **A stolen or lost device** (phone, second PC) | use whatever is unlocked on it | keep access after the user revokes it; exceed the rights granted to that device; read keys from files |
| T5 | **A compromised host or backend** | anything the user can do on that host (it holds the project) | escalate to the window's machine: write local files outside the remote project's cache, open local listeners, run local programs, read local keychain entries |
| T6 | **A web page in the user's browser** | send HTTP requests to loopback and LAN addresses, DNS rebinding | talk to any remote listener (they do not speak HTTP) or to SPEC-0221 without its token |
| T7 | **Record now, decrypt later** (an attacker of T1 or T3 who stores today's traffic and later has a quantum computer that breaks X25519) | compute every Diffie-Hellman result of a recorded handshake | read recorded session content or derive its transport keys (section 1c) |

Out of scope: an attacker who controls the user's own account or operating system on either end,
hardware side channels, and an *active* attacker who breaks X25519 in real time during a handshake
(authentication stays classical; see section 1c, "Residual risks").

## Decision

### 1. Two kinds of channel, both encrypted and mutually authenticated

**(a) SSH channel** — for every connection to a host that has an SSH server (M6.12, M6.13, M6.14
SSH transport). IDECraft keeps using the **system OpenSSH client** exactly as SPEC-0121 does:
`BatchMode=yes`, `StrictHostKeyChecking=yes`, the user's own keys, agent, `~/.ssh/config` and
`known_hosts`; no password or passphrase ever passes through IDECraft; an unknown or changed host key
is refused with what to do and is never accepted from inside the IDE. The backend runs as the user
behind the SSH channel and speaks the session protocol over the channel's standard input and output
(ADR-0026). There is no second encryption layer inside SSH: SSH already gives confidentiality,
integrity and both-way authentication (host key, user key).

**Post-quantum key exchange required (v3, PO-STATUS question 29: "Ja, erzwingen"):** against T7,
every SSH path of IDECraft — remote host commands and files, the SSH terminal, the backend and
`idecraft serve` (SPEC-0158, SPEC-0317), Docker over SSH and the Compose CLI (SPEC-0243,
SPEC-0242), database SSH tunnels, later debug and port tunnels (M6.13), and Git over SSH
(SPEC-0055) — passes

`-o KexAlgorithms=mlkem768x25519-sha256,sntrup761x25519-sha512,sntrup761x25519-sha512@openssh.com`

to the system `ssh`, in that order of preference, and no classical method. The rules:

- **One start point (v5; v3: one option function).** Every `ssh` of the product comes from
  `idecraft_remote::SshCommand` (`crates/remote/src/launch.rs`): `SshCommand::new(purpose)` (remote
  host, `serve`, terminal, tunnel, Docker, `ssh -G`) sets the program, checks the client, puts the
  shared options first (batch mode, strict host keys, 10 s connect timeout, the key exchange), then
  the route (jump hosts, below), then the caller's own options; `build` returns the command line,
  which only that file turns into a process. Git gets it through `GIT_SSH_COMMAND`
  (`SshCommand::git_ssh_command`), programs that start `ssh` from `PATH` themselves (the Compose
  CLI) through a private stand-in folder (`SshCommand::stand_in_dir`, 0700). The shared options
  themselves are private to the crate, so no other crate can build its own `ssh` line from them
  (the static lock; Clippy's `disallowed-methods` cannot tell an `ssh` from any other
  `Command::new`, so it is not used). The options always come before configuration and extra
  arguments, and OpenSSH keeps the first value it obtains, so neither `~/.ssh/config`, `-o` extras,
  nor a user's `GIT_SSH_COMMAND`, `GIT_SSH` or `core.sshCommand` (replaced for Git) can bring back a
  classical exchange. The stand-ins of Git and the Compose CLI and every hop also refuse shared
  connections (`ControlMaster=no`, `ControlPath=none`), so a master a user opened by hand is never
  reused. A test scans the workspace: a process started with an argument that mentions `ssh` —
  directly, through a `let` binding, a parameter of a function about `ssh` or a field documented as
  an `ssh` program — an `ssh` command line built by hand, a `GIT_SSH_COMMAND` not from the start
  point, or shell text that runs `ssh` is allowed only in the start point and the stand-in writers
  of `crates/remote`; the scanner has its own test with such variants.
- **Local client checked first.** On first use IDECraft asks the local client (`ssh -Q kex`, once
  per process) and passes only the allowed methods it knows (an OpenSSH 9.0–9.8 client does not know
  the ML-KEM name and would reject the whole option otherwise). A client that knows none of them is
  refused before any connection: "the local ssh client offers no post-quantum key exchange; update
  OpenSSH (OpenSSH 9.0 or later; ML-KEM from 9.9). IDECraft does not fall back to a classical key
  exchange (ADR-0025)". There is no silent fallback.
- **Host without one refused.** `ssh`'s "no matching key exchange method found" becomes its own
  error (`RemoteError::NoPqKex`): "<host>: the host offers no post-quantum key exchange (OpenSSH 9.0
  or later needed there, ML-KEM from 9.9). IDECraft connects only with post-quantum protection
  (ADR-0025); nothing was sent" — in the window, in MCP errors (Git: `ssh_refusal`) and in logs; the
  message names only the host, never keys, the host's offer or other details of `ssh`'s output.
- **Jump hosts (v5).** OpenSSH opens the hops of a `ProxyJump` with its own `ssh` and without
  IDECraft's options. IDECraft therefore never lets it: before a connection the start point asks
  `ssh -G` (with the target's port, login and extra arguments) for the resolved configuration.
  - A `ProxyJump` (from `~/.ssh/config` or `-J`, which is taken out of the command line) becomes
    IDECraft's own `ProxyCommand`: a private hop stand-in (0700, its name a hash of its text) that
    runs the same `ssh` with the shared options and `-W` to the next host, the hops before it in
    turn, and for the first hop resolves that host's own configuration again — so a hop's own
    `ProxyJump` gets the same treatment. Multi-step jumps (`a,b,c`), `ssh://` and
    `user@host:port` entries and `-F` are kept; at most 8 hops (a loop in the configuration ends
    there). Each hop's `ssh` must negotiate a post-quantum method, or the connection ends before
    anything reaches the target: "<host>: the jump host <hop> offers no post-quantum key exchange …;
    nothing was sent to <host>" (`RemoteError::HopNoPqKex`).
  - Without a jump the direct route is pinned with `ProxyCommand=none`, so nothing in the
    configuration can add a hop afterwards.
  - A `ProxyCommand` of the user's own (target or hop) runs a program IDECraft cannot hold to the
    rule. It is **refused by default** ("<host>: the SSH configuration runs its own ProxyCommand …;
    nothing was started. Use ProxyJump instead …", `RemoteError::ProxyCommandRefused`). The setting
    `ssh.allowOwnProxyCommand` (Settings → Version Control, default **off**, shown with a warning;
    MCP can only switch it off) allows it, like the Azure DevOps exception: each such connection
    writes one line to `ssh-proxy-command.log` in the IDECraft log folder (time and host, never the
    command).
  - Git and the Compose CLI resolve the same way at run time: their stand-ins call `ssh -G` with
    the arguments they were given and use the same shell functions as the hop stand-in.
- **Why sntrup761 is allowed next to ML-KEM:** both are *hybrid* (X25519 plus a post-quantum KEM;
  the session key needs both secrets), so either protects against T7 as long as one of its two
  halves holds. `sntrup761x25519-sha512(@openssh.com)` is OpenSSH's default since 9.0 (2022) and the
  only post-quantum method of 9.0–9.8, which many long-term-support systems still ship (Debian 12:
  9.2, Ubuntu 24.04: 9.6; the method exists since 8.5 and is preferred from 9.0); ML-KEM arrived in 9.9 and became the default in 10.0.
  Allowing only ML-KEM would refuse these hosts while adding no protection against the recorded-
  traffic attacker that a hybrid sntrup761 does not already give. Streamlined NTRU Prime is not
  standardised by NIST (it was a round-3 alternate), which is why ML-KEM comes first and is chosen
  whenever both sides have it. The stricter variant is prepared: `IDECRAFT_SSH_KEX=mlkem` limits the
  set to `mlkem768x25519-sha256` (the variable can only narrow the set; any other value keeps the
  default); making it the default needs only that switch, once OpenSSH 9.9+ is common.
- **Git over SSH** is included: on 2026-11-07 `github.com`, `gitlab.com`, `bitbucket.org` and
  `codeberg.org` negotiated `mlkem768x25519-sha256` (checked without any key or login: the
  connection ended at host key verification). `ssh.dev.azure.com` (Azure DevOps) offers only
  classical Diffie-Hellman; HTTPS remotes are unaffected. Git to Azure DevOps is the one exception
  below (PO-STATUS question 30).
- **The one exception: Git over SSH to Azure DevOps (v4, PO-STATUS question 30: "Ja, Ausnahme").**
  *Why:* Azure DevOps is a common Git host whose SSH server (`SSHBlackbox.10`) offers no
  post-quantum method; without the exception its repositories work only over HTTPS. The Project
  Owner accepts the recorded-traffic risk (T7) for this one case; everything else in this section
  stays. *Limits*, each enforced in code (`crates/remote/src/pq_git.rs`):
  - **Git only.** Only Git's `GIT_SSH_COMMAND` can carry it (`idecraft_remote::git_ssh_command`).
    Remote hosts, remote projects and `serve`, the SSH terminal, Docker and Compose over SSH and
    tunnels keep `ssh_options`, which never contains a classical method, also to an Azure host name.
  - **Azure DevOps only, by the resolved host name.** Git runs a private stand-in script (0700, in
    IDECraft's own SSH folder, its name a hash of its text). It asks `ssh -G` with Git's own
    arguments for the `hostname` that `ssh` will really connect to (after `~/.ssh/config` aliases
    and `Hostname`) and compares it exactly — no pattern, no case folding, no trailing dot — with
    `ssh.dev.azure.com` and `vs-ssh.visualstudio.com`. An alias whose `Hostname` is Azure gets the
    exception; a host argument that reads like Azure but resolves elsewhere does not. Every other
    host gets the strict options from the same script.
  - **Strongest classical methods only, post-quantum first.** On 2026-11-07 both hosts offered
    `diffie-hellman-group14-sha256`, `diffie-hellman-group16-sha512`,
    `diffie-hellman-group-exchange-sha256` and `diffie-hellman-group18-sha512` (no `curve25519`,
    nothing post-quantum; read from the key exchange offer without a key or login). Allowed are only
    `diffie-hellman-group18-sha512` and `diffie-hellman-group16-sha512` (8192- and 4096-bit groups,
    RFC 3526/8268), behind the post-quantum list: `KexAlgorithms=mlkem768x25519-sha256,
    sntrup761x25519-sha512,sntrup761x25519-sha512@openssh.com,diffie-hellman-group18-sha512,
    diffie-hellman-group16-sha512`. Never SHA-1, `group1`, `group14` (2048 bit) or group exchange
    (the server picks the size). Because the post-quantum methods come first, a post-quantum one is
    chosen as soon as Azure offers it. A local client without a post-quantum method is refused for
    Azure too. Host keys stay strict (`StrictHostKeyChecking=yes`, `known_hosts`, never accepted
    automatically); batch mode and options-first apply as everywhere.
  - **Visible and logged.** For an Azure connection the stand-in reads the negotiated method from
    `ssh`'s debug log (written to a private temporary file, removed at once; `ssh`'s own messages are
    passed on) and reports it. Git operations that ran without a post-quantum exchange carry `pq:
    false` and `pq_notice` in their result (MCP `git_fetch`, `git_pull`, `git_push`), the window's
    status line shows the warning once per session, and one line goes to `git-ssh-kex.log` in the
    IDECraft log folder (time, Git subcommand, host, method; no URL, reference or credential). The
    notice recommends HTTPS with a token, or SSH once Azure DevOps offers a post-quantum exchange.
  - **Switchable, strictest wins.** The setting `git.ssh.allowClassicKexForAzureDevOps` (in
    `settings.toml`, Settings → Version Control; default **on** by the Project Owner's decision)
    turns it off; `IDECRAFT_SSH_KEX=mlkem` also turns it off. MCP can only switch it off.
  - **Re-check.** An ignored test (`azure_devops_still_lacks_post_quantum` in
    `crates/remote/tests/pq_kex.rs`), run by hand and in the nightly job, fails as soon as Azure
    DevOps offers a post-quantum exchange (then the exception is removed in a new revision) or stops
    offering both allowed classical ones.
- **Residual risks:** (0) Git to Azure DevOps over SSH is open to T7 while the exception is on
  (above). (1) Jump hosts: closed in v5 (above) — every `ProxyJump` hop gets the shared options.
  What remains is a `ProxyCommand` of the user's own while `ssh.allowOwnProxyCommand` is on: IDECraft
  cannot see which key exchange that program's hops use (off by default, warning, log line); and a
  configuration changed in the 30 s for which a resolved route is reused applies only after that.
  (2) IDECraft's shared connections (`ControlMaster`, own folder 0700) are only ever opened with
  these options; masters a user opened by hand are not reused because IDECraft uses its own
  `ControlPath`, and Git, the Compose CLI and every hop use none (v5). (3) Host authentication stays classical (host keys), as for paired
  channels (section 1c).

**(b) Paired channel** — for connections without SSH: a hand-started `idecraft serve --pair` on a
host without SSH (M6.14), a paired device controlling the desktop app (M16), mobile apps (M17), and
any path through a relay. It uses the **Noise Protocol Framework** (revision 34) through the crate
`snow` (Apache-2.0 OR MIT; pure-Rust primitives: `curve25519-dalek` BSD-3-Clause, `chacha20poly1305`,
`blake2`, `sha2` MIT OR Apache-2.0 — all on the `deny.toml` allow list) with exactly two patterns
and one cipher suite, **always combined with an ML-KEM-1024 exchange** (section 1c):

| Use | Pattern | Full protocol name | Hybrid part |
|---|---|---|---|
| First pairing | `XXpsk3` | `Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s` | ML-KEM-1024 secret and pairing secret in `psk3` |
| Every later connection | `XKpsk3` | `Noise_XKpsk3_25519_ChaChaPoly_BLAKE2s` | ML-KEM-1024 secret in `psk3` |

There is **no negotiation**: a peer that offers another pattern, suite, version or no
post-quantum part fails the handshake. The Noise prologue is the fixed string
`idecraft-session/<major>;pq=ML-KEM-1024;<initiator role>><responder role>` (roles `window`,
`serve`, `device`), so a version, construction or role mismatch fails cryptographically, not by a
field one side could ignore. Each side has one long-term static X25519 key per installation; there
is no long-term ML-KEM key (the KEM keys are ephemeral, section 1c).

`XKpsk3` replaces v1's `IK` for later connections: the initiator knows the responder's pinned
static key in both, but only a three-message pattern lets the responder's ML-KEM ciphertext arrive
before the secret is mixed in (section 1c), and `XK` sends the initiator's static key only in the
third message, so the first message names no device and carries no command (section 8). The cost
is half a round trip more when a connection is set up; established sessions are unaffected.

**Why Noise and not TLS 1.3 with pinned keys (`rustls`):**

| Criterion | Noise (`snow`) | TLS 1.3 (`rustls` 0.23 + `ring`, already in the tree) |
|---|---|---|
| Identity | raw static public keys, exactly what pairing exchanges | X.509 certificates; self-signed certificates must be generated (`rcgen`) and their keys pinned |
| Pinning | built in: XK (like IK) checks the responder's static key; the responder checks the initiator's key against the paired list | needs custom server and client certificate verifiers through rustls' `danger` API — the very place a mistake disables authentication |
| Mutual authentication | native in both patterns | client certificates; more parsing (ASN.1/X.509) on both sides |
| Configuration surface | one fixed name; nothing to turn off | versions, suites, resumption tickets and 0-RTT must each be pinned or disabled |
| Pairing with a short code | `psk3` mixes the code into the handshake; the handshake hash gives a short authentication string | needs an extra PAKE or a separate confirmation step outside TLS |
| Relays | end-to-end over any byte pipe | also end-to-end, but SNI and certificate traffic patterns add metadata |
| Maturity | used by WireGuard, Lightning and libp2p; `snow` is smaller and less audited than rustls | rustls is audited and widely deployed |
| Mobile (M17) | the same Rust code compiles for iOS and Android | same |

The deciding points are that pinning and mutual authentication are the *default* behaviour in
Noise and an *override* in TLS, that there is nothing to negotiate, and that pairing needs a
handshake-bound authentication string, which Noise gives directly. `snow`'s smaller audit record is
handled by keeping it on the current release, running `cargo deny check advisories` in CI (already
part of the checks), wrapping it behind one small module with its own tests (section 9) and naming
its version in every release's security review (rule 8). TLS stays where a standard requires it
(HTTPS to Git hosts, database drivers, provider APIs); it is not used for IDECraft's own remote
channels.

**Rekeying and lifetime:** the transport rekeys (Noise `rekey`) every 2^20 messages or 60 minutes,
whichever comes first; after 24 hours a fresh handshake (with fresh X25519 and ML-KEM ephemeral
keys) replaces the session transparently. Noise messages are at most 65 535 bytes; the paired channel carries the session protocol's frames
(ADR-0026) as a byte stream in Noise messages with a two-byte length prefix.

### 1c. Hybrid post-quantum key exchange (from the first version)

Every paired handshake is **hybrid**: its transport keys depend on the X25519 results of Noise
*and* on an ML-KEM-1024 shared secret (FIPS 203). An attacker must break both to read a session; a
recorded session stays confidential if X25519 falls to a quantum computer later (T7), and equally
if ML-KEM should turn out weaker than believed.

**Construction** (identical for both patterns; Noise's own pre-shared-key slot carries the KEM
secret, the extension point that Noise defines for secrets from outside the handshake and that
WireGuard with Rosenpass uses the same way):

1. **Message 1 (initiator → responder):** the initiator generates a fresh ML-KEM-1024 key pair from
   the operating system's random source and sends the 1568-byte encapsulation key as the *whole*
   payload of message 1 (in `XKpsk3` encrypted under `es`; in `XXpsk3` under a key derived only from
   the public ephemeral key, because a psk pattern's `e` token mixes a key, so anyone can read it
   — an encapsulation key is public anyway; in both cases it is mixed into the handshake hash, and
   message 1 has the same length in both patterns; corrected 2026-11-09 by SPEC-0321).
2. **Message 2 (responder → initiator):** the responder checks the encapsulation key as FIPS 203
   requires (modulus check), encapsulates to it and sends the 1568-byte ciphertext as the whole
   payload of message 2 (encrypted under `ee` and `es`, mixed into the handshake hash). A
   missing, short, long or invalid encapsulation key, or a message-2 payload of another length,
   is a handshake failure.
3. **Before message 3:** both sides now hold the KEM shared secret `ss` (the initiator by
   decapsulation; an altered ciphertext gives a different `ss` through ML-KEM's implicit
   rejection, so the handshake fails at message 3, never silently). Each side computes

   `psk3 = HKDF-SHA256(salt = h2, ikm = ss ‖ pair, info = "idecraft-pq-v1" ‖ pattern name ‖ prologue)`

   where `h2` is the Noise handshake hash after message 2 (it covers the prologue, both ephemeral
   X25519 keys, the encapsulation key and the ciphertext), `pair` is the 32-byte pairing key of
   section 2 for `XXpsk3` and is absent for `XKpsk3`, and sets it with `snow`'s `set_psk(3, …)`.
4. **Message 3 (initiator → responder):** Noise's `psk` token runs `MixKeyAndHash(psk3)`, so the
   chaining key, every later handshake key, the final handshake hash (and with it the SAS of
   section 2) and both transport keys depend on `ss`. The payload of message 3 (protocol version,
   device name) is the first one encrypted under hybrid keys.
5. The ML-KEM decapsulation key, `ss`, `psk3` and the pairing key are held in zeroising buffers
   and dropped right after message 3; nothing of the KEM is stored.

**Transcript binding:** the KEM values are bound twice: as handshake payloads in Noise's hash
(any change makes the next AEAD check fail), and through `h2` as the HKDF salt of `psk3`. The info
string names the construction, the pattern and the prologue, so a secret computed for one use
cannot be replayed into the other.

**No downgrade:** there is no code path that builds a handshake without the KEM part: the
responder knows only the two hybrid names, `snow` refuses to process the `psk` token when no key
was set, and a peer that leaves the KEM out (another payload length, a plain `Noise_XK_…` or
`Noise_IK_…` handshake, a prologue without `pq=ML-KEM-1024`) fails the handshake (SEC-6, SEC-15).
Every major of the session protocol keeps the hybrid; a later major may only *change* the KEM
(for example to the HFS extension once `snow` ships ML-KEM there), never drop it — removing the
post-quantum part needs a new decision of the Project Owner.

**Why ML-KEM-1024 and not ML-KEM-768:** both are standardised in FIPS 203; ML-KEM-768 is what TLS
(`X25519MLKEM768`) and OpenSSH (`mlkem768x25519-sha256`) deploy. ML-KEM-1024 is the highest
parameter set (NIST category 5; the one CNSA 2.0 requires), and the costs are negligible here: about
1.5 KiB more per direction once per handshake and well under a millisecond of computation. The rule
"security first" (AGENTS.md) takes the stronger option when the cost does not matter.

**Crate:** `libcrux-ml-kem` (Cryspen; Apache-2.0; feature `mlkem1024`), pinned to an exact version
(`=0.0.x`, the crate is pre-release) and wrapped in the one Noise module of section 9 together with
`snow`. Its portable and AVX2 code is formally verified with hax and F* (field arithmetic, NTT,
serialisation and the generic algorithm: panic freedom, correctness, secret-independent timing),
and its C extraction is the ML-KEM inside OpenSSH's `mlkem768x25519-sha256`. Its own dependencies
(`libcrux-sha3`, `libcrux-intrinsics`, `libcrux-platform`, `libcrux-secrets`, `libcrux-traits`,
`core-models`, `hax-lib`: Apache-2.0; `num-bigint`, `num-traits`, `libc`: MIT OR Apache-2.0) are on
the allow list, so `deny.toml` stays unchanged. `libcrux-sha3` had three advisories in 2026
(incremental SHAKE output, a panic in AVX2 SHAKE-256; all fixed from 0.0.10); the pin requires a
fixed version and `cargo deny check advisories` watches it. RustCrypto's `ml-kem` (Apache-2.0 OR
MIT, pure Rust, not independently audited) is a **dev-dependency only**: the tests encapsulate with
one implementation and decapsulate with the other (SEC-16).

**Residual risks:** (1) Authentication stays classical: the static keys are X25519, so an attacker
who breaks X25519 *during* a handshake could impersonate a peer (out of scope above; pairing over
the QR code also binds the 256-bit QR secret into `psk3`, which a quantum attacker cannot compute).
A post-quantum authentication (ML-DSA or a static ML-KEM key per device) needs its own amendment
when such attackers become realistic. (2) The initiator's static public key in message 3 and the
responder's in message 2 of `XXpsk3` are encrypted under classical keys only; a T7 attacker could
learn later *which* paired device connected (metadata, like the addresses), not what it did.
(3) `libcrux-ml-kem` is formally verified but pre-release and not audited as a Rust crate; the
cross-check against a second implementation, the NIST test vectors and the release review (rule
8) cover that.

### 2. Pairing (short code or QR code, then a short authentication string)

Pairing is always started on the machine that will accept connections (the desktop app for M16,
`idecraft serve --pair` for M6.14) and confirmed on both sides:

1. The user switches on "Pair a device". The accepting side opens a **pairing window of at most
   5 minutes** for **one** device and shows:
   - a **short code** of 10 characters from Crockford's Base32 (50 bits, single use), and
   - a **QR code** with the accepting side's address, its static public key and a 256-bit random
     pairing secret.
2. The new device either scans the QR code or the user types the code (plus the address, or the
   relay's rendezvous name). Both sides derive the 32-byte **pairing key** with HKDF-SHA256 from
   the QR secret or from the short code (label `idecraft-pair-v2`; v1's `idecraft-pair-v1` used it
   directly as the `psk`); it enters `psk3` together with the ML-KEM secret (section 1c). With the
   QR code the device also checks that the responder's static key equals the one in the QR code.
3. After the handshake both sides show a **six-digit short authentication string** derived from the
   final Noise handshake hash (HKDF-SHA256, label `idecraft-sas-v1`; the hash already depends on
   the ML-KEM secret and the pairing key through `psk3`) and the other side's name and key
   fingerprint. The user confirms on **both** devices that the digits match. Only then does each side
   store the other's static public key. A man in the middle would produce different digits on the
   two screens.
4. A mismatch, a rejection on either side, three failed handshakes or the end of the 5 minutes
   closes the pairing window; nothing is stored. Only one pairing runs at a time.
5. The accepting side then asks for the device's **rights** (section 4); default is "view" only.

The short code alone could be attacked offline by an active attacker who intercepts one handshake;
50 bits make that impractical within the 5-minute window, and the SAS comparison catches it in any
case. The code is never the only factor.

### 3. Keys live in the keychain

- Each installation's **static private key** and every paired peer's public key and metadata are
  stored in the operating system's keychain through the crate `keyring` (MIT OR Apache-2.0, already
  used for database passwords): macOS Keychain, Secret Service on Linux, Credential Manager on
  Windows, Keystore/Keychain on mobile (M17).
- The **device list** (name, key fingerprint, rights, paired on, last seen) is kept in
  `remote/devices.toml` in the user's settings folder (the folder of SPEC-0053), never in a project.
  It holds no key material; the fingerprint links it to the keychain entry.
- Resume tokens (ADR-0026) live only in process memory on both ends.
- No key, token or short code is written to project files, logs, the session protocol's payloads,
  crash reports, command-line arguments or environment variables (process lists are visible to
  other users, T2).
- SSH keys stay in the user's SSH agent and files, managed by OpenSSH; IDECraft never reads them.
- Hosts without a keychain: section 3a.

### 3a. Key file on hosts without a keychain (decided 2026-11-07, PO-STATUS question 28 a)

Debug builds on macOS also use this file store (decided by the project owner, 2026-11-09): each rebuild has a new ad-hoc signature, so the keychain would prompt every time. Release builds always use the keychain. Debug builds on Linux do the same (v7), so no test reaches the Secret Service of the person running it.

On Linux the keychain is the Secret Service on the session bus (v7, decided by the project owner on 2026-11-09): an encrypted Secret Service session only, its `default` collection, nothing unlocked or created by IDECraft; locked or unreachable refuses the start. `--key-file` stays the explicit choice of this file store on a machine with a session bus, and an existing key file is kept.

A headless Linux or other Unix host has no keychain service (no Secret Service on a session bus).
There, and only there, `idecraft serve --pair` keeps its key **in a file, like an OpenSSH host
key**:

- **Where:** the static X25519 private key in `$XDG_CONFIG_HOME/idecraft/remote/serve-key`
  (default `~/.config/idecraft/remote/serve-key`), the paired peers' public keys and metadata in
  `paired-keys.toml` beside it (not secret, but whoever can change that file could add a device).
  Both use the file header of ADR-0021 (`kind = "remote-serve-key"` and `"remote-paired-keys"`).
  No ML-KEM key is stored (section 1c: the KEM keys are ephemeral).
- **When:** only when no keychain service exists. A keychain that exists but is locked or refuses
  access is *not* a reason to use a file: `serve` stops and says how to unlock it (no silent
  fallback, section 8). An existing key file is used as long as it exists, so the host keeps its
  identity. macOS and Windows always have a keychain; the file store is not offered there.
- **Mode and owner:** the folder `remote/` is 0700, both files 0600 (0400 is accepted), all three
  owned by the user `serve` runs as. The parent folders up to the home folder must not be writable
  by group or others (as OpenSSH's `StrictModes`).
- **Creating:** the folder is created with mode 0700; a new key is written to a temporary file
  opened with `O_CREAT | O_EXCL | O_NOFOLLOW` and mode 0600, synced and renamed into place
  (SPEC-0262), so it is never readable by others, not even for a moment; the umask does not matter.
- **Check at every start, before anything listens:** the folder and each file are opened without
  following links (`O_NOFOLLOW`, `O_DIRECTORY` for the folder) and checked with `fstat` on the open
  descriptor (no gap between check and use): a regular file (or folder), owner equal to the
  effective user id, `mode & 0o077 == 0`; the key file has the expected length and header. Any
  failure refuses the start.
- **Refusal:** `serve` exits with status 78 (`EX_CONFIG`), opens no listener and writes one line to
  standard error and the connection log that names the path, the finding and what to do, never any
  key material, for example:
  - `idecraft serve: refusing to start: the key file ~/.config/idecraft/remote/serve-key can be read
    by group or others (mode 0644). If someone else may have read it, delete it and pair your
    devices again; otherwise run: chmod 600 ~/.config/idecraft/remote/serve-key`
  - `… is owned by uid 1002 (backup), not by you (uid 1000). Check where it came from; delete it to
    create a new key (paired devices must pair again).`
  - `… is a symbolic link (or not a regular file); refusing to follow it.`
  - `… the folder ~/.config/idecraft/remote can be read or written by group or others (mode 0755);
    run: chmod 700 ~/.config/idecraft/remote`
  `serve` never repairs modes by itself: a key that was readable may already be copied, so the
  user decides.
- **Shown to the user:** the start line and the pairing confirmation say "key stored in file
  `<path>` (no keychain on this host)" together with the key fingerprint, so the paired device's
  user sees where the host keeps its identity.
- **Tests:** SEC-16; SEC-10 checks the modes and that the file holds nothing but the key.

### 4. Rights per device and per remote project, deny by default

- A paired device gets separate grants: `view` (read projects, files, buffers, problems, test
  results), `edit` (change buffers and save), `run` (run configurations, terminals), `debug`,
  `vcs-write` (commit, branch, merge), `push` (the `remote` right of SPEC-0053), `ports` (port
  forwarding), `agents` (start and steer agent tasks) and `ui` (operate the window through the UI
  driver, [ADR-0027](0027-ui-driver-and-semantic-tree.md); effects of an action also need their own
  grant). Only `view` is granted at pairing unless the
  user ticks more. Missing rights are refused with `not_granted` (ADR-0026 error codes), named like
  SPEC-0053 behaviour 3.
- An SSH session is the user themself: it has the rights the user's account has on the host. Agents
  acting on a remote project keep the host project's agent rights (SPEC-0053, SPEC-0158 behaviour 3);
  a device's `agents` grant never widens an agent's own rights.
- **Every remote action is logged** by the side that performs it, in `remote/audit.jsonl` in that
  side's user state folder: time, peer (device fingerprint and name, or `ssh:<user>@<window-host>`),
  method, target path or id, outcome. Never content, never secrets. The log stays on the machine
  (ADR-0007).
- **Revocation is immediate:** revoking a device deletes its key from the keychain, adds its
  fingerprint to a revocation list in `remote/devices.toml`, and closes its live sessions within one
  second. A revoked key cannot handshake again even if an old copy of the device list is restored.
- Switching remote access off closes every paired listener and session.

### 5. Nothing listens openly

- `idecraft serve --stdio` (the SSH backend) opens **no TCP listener**. For reattachment after a
  network break it may open one Unix domain socket in a directory of mode 0700 owned by the user
  (`$XDG_RUNTIME_DIR/idecraft/` or `~/.cache/idecraft/run/`); it checks the peer's user id
  (`getpeereid`/`SO_PEERCRED`) and refuses any other user before reading a byte, and it still
  requires the resume token (ADR-0026).
- A paired listener (`idecraft serve --pair`, M16 remote access) listens on a network interface
  **only while the user has switched it on**, accepts nothing but Noise handshakes from pinned keys
  (or the one pairing handshake while the window is open), and closes when switched off. It never
  speaks HTTP, so browsers cannot talk to it (T6).
- The window's own MCP over HTTP stays loopback-only with its token (SPEC-0221); it is not a remote
  path.
- Pairing discovery (for example mDNS) is not used; addresses are typed, scanned or come from the
  relay. A later discovery feature needs its own amendment.

### 6. Unauthenticated protocols stay local; forwarded ports bind to loopback

- Debug ports (JDWP, debugpy, lldb-server, gdbserver, Node inspector), language servers and database
  ports on a host are reached **only** through the session channel: the backend connects to them on
  the host's loopback and relays the bytes over the encrypted session (ADR-0026 `port.*` and
  `dap.*` streams). Debug adapters run on the host (M6.13), so the window needs no local port for
  them at all.
- A **port forward** the user asks for (for example a dev server for the local browser) binds on the
  window's machine to `127.0.0.1` (and `[::1]`) only. Binding to another interface is not offered.
  The host side connects to `127.0.0.1` on the host by default; another target host or address
  needs the user's explicit entry and is shown in the forwards list.
- Any local user can connect to a loopback port (T2). Therefore a forward of a port that a debug
  adapter or debuggee on the host opened (JDWP, debugpy, lldb-server, gdbserver, Node inspector —
  these allow code execution) is refused, with a pointer to the debugger, which uses the session
  instead. A forward of a database port is offered only after a warning that other local users
  can reach it, confirmed per forward by the user (agents cannot confirm it), with a pointer to
  the database tools, which use the session.
- The backend never asks the window to open a local listener on its own; forwards start only from a
  user action or an agent with the `ports` right (T5).

### 7. Relays see nothing

- A relay (M16, optional, run by the user) forwards opaque frames between two endpoints that meet
  under a rendezvous id: 128 random bits, derived per pairing and rotated with each new handshake.
- The relay holds **no session keys**; the Noise channel runs end to end through it. A relay may
  require its own access token against abuse; that token is not a session credential and never
  appears inside the session.
- What a relay can still learn (addresses, timing, sizes) is stated in the M16 spec; padding is not
  planned.

### 8. Replay protection, time and rate limits

- **Replay:** every session has fresh ephemeral keys; transport messages carry Noise's implicit
  nonce counter and a replayed, reordered or altered message fails decryption and **closes the
  connection**. Handshake payloads carry no commands: messages 1 and 2 carry only the ML-KEM
  encapsulation key and ciphertext (section 1c), message 3 only the protocol version and the device
  name; commands start after the handshake. A replayed message 1 makes the responder answer with a
  fresh ephemeral key and ciphertext that the replaying attacker cannot use, and is counted by the
  rate limit.
  Session-level requests also carry increasing sequence numbers (ADR-0026), and the backend applies a
  document operation at most once per `(client, sequence)`.
- **Time limits:** a handshake completes within 10 s; an idle connection sends a heartbeat every
  15 s and is closed after 45 s without traffic; a pairing window lasts at most 5 minutes; a
  disconnected backend waits at most 10 minutes (setting, 1–60 minutes) for reattachment, then ends
  its session (ADR-0026 says what happens to unsaved text); SSH keeps SPEC-0121's 10 s connect time.
- **Rate limits:** per source address at most 10 handshake attempts per minute on a paired listener,
  with exponential back-off after failures (1 s doubling to 5 minutes); three failed pairing
  handshakes close the pairing window; reattachment attempts with a wrong resume token: three, then
  the waiting session is ended.
- **Clear refusals:** every refusal names the cause and what to do (unknown host key, device not
  paired, device revoked, right not granted, version not supported, pairing expired). There are no
  silent fallbacks: no plaintext mode, no "accept any key" switch, no fallback from Noise to anything
  else, no automatic retry with weaker settings.

### 9. Mandatory security tests

Every remote spec lists, for each test below, the acceptance criterion that covers it, or why it
does not apply (for example SEC-12 for a spec without relay). The tests run in `cargo test` against
real endpoints where possible (a test `sshd` as in SPEC-0121 AK-2, in-process Noise peers, a test
relay).

| Id | Test | Expected |
|---|---|---|
| SEC-1 | **Wrong key:** an unpaired static key connects to a paired listener; a window connects to a responder whose static key differs from the pinned one | handshake fails; no session message is processed; refusal logged |
| SEC-2 | **Changed or unknown host key** (SSH) | connection refused without prompt; message says how to check the key in a terminal |
| SEC-3 | **Revoked device:** reconnect after revocation; revocation during a live session | handshake refused; live session closed within 1 s |
| SEC-4 | **Missing right:** each method behind a right called without it | `not_granted` naming the right; nothing done; logged |
| SEC-5 | **Plaintext attempt:** plain JSON, HTTP and an SSH banner sent to a paired listener and to the reattach socket | connection closed without any content in reply; no parser beyond the handshake reached |
| SEC-6 | **Downgrade:** another Noise pattern (also v1's `IK`), suite, prologue version, construction (`pq=` missing or different) or role | handshake fails |
| SEC-7 | **Exposed port:** list the listening sockets of every IDECraft process in each mode (stdio backend, window with forwards, paired listener on and off); ask to forward a debug port | only loopback sockets and the switched-on paired listener; none after switching off; forwards bound to loopback; the debug-port forward refused |
| SEC-8 | **Replay and tampering:** replay a recorded handshake and recorded transport messages; flip one bit in a message; reorder two messages | refused; connection closed; no operation applied twice |
| SEC-9 | **Rate and time limits:** many handshakes from one address; a stalled handshake; an expired pairing window; wrong resume tokens | back-off and refusals as in section 8; stalled handshake closed after 10 s |
| SEC-10 | **Secrets stay secret:** scan logs, `remote/*.toml`, project files, protocol frames (decrypted in the test) and the arguments and environment of every spawned process | no key, token, short code, pairing key, ML-KEM secret or password appears; no key files except the serve key file of section 3a (0600, nothing but the key); folders 0700, files 0600 |
| SEC-11 | **Other local user:** connect to the reattach socket with another user id (unit test of the peer check plus mode checks of the directory) | refused before reading |
| SEC-12 | **Relay sees only ciphertext:** a test relay records all bytes of a session that sends a known marker | the marker never appears; a relay that alters a frame breaks the session |
| SEC-13 | **SAS mismatch or rejection** on either side | pairing aborted; no key stored on either side |
| SEC-14 | **Host cannot escalate (T5):** the backend sends paths outside the project (`..`, absolute, links), asks for a local listener, or sends oversized frames | refused or connection closed; nothing written outside the remote cache |
| SEC-15 | **Post-quantum part is mandatory and effective** (section 1c). (a) *Refused without it:* a peer sends message 1 without an encapsulation key, with one byte too few or too many, with an invalid key (coefficient ≥ q), or message 2 without a ciphertext; a peer runs plain `Noise_XK_…`, `Noise_IK_…` or `Noise_XX_…` without `psk3`; a prologue without `pq=ML-KEM-1024` or with another KEM. (b) *It enters the session key:* with fixed X25519 keys and fixed KEM randomness (test-only builder), two runs that differ only in the ML-KEM shared secret give different transport keys, different first ciphertexts and different SAS digits; flipping one bit of the ciphertext or swapping the encapsulation key makes the handshake fail at message 2 or 3. (c) *Correct ML-KEM:* NIST's FIPS 203 test vectors for ML-KEM-1024 pass; encapsulating with `libcrux-ml-kem` and decapsulating with RustCrypto `ml-kem` (and the reverse) gives the same secret | (a) handshake fails before any session message, refusal logged, no `psk3` ever set to a default or zero value; (b) all differences hold, tampering fails; (c) all equal |
| SEC-16 | **Key file with too open rights refuses the start** (section 3a; hosts without a keychain): the key file or `paired-keys.toml` with mode 0640, 0604, 0644; owned by another user; a symbolic link to a 0600 file; not a regular file; the folder 0750 or 0755; a parent folder writable by others; a key file of wrong length; a locked keychain | `serve --pair` exits with status 78 before opening any listener; the message names the path, the finding and what to do and contains no key material; nothing is repaired; a new key is created 0600 in a 0700 folder regardless of the umask (umask 000 in the test) |
| SEC-17 | **SSH channel without post-quantum key exchange** (section 1a): a test `sshd` limited to classical key exchanges (`KexAlgorithms curve25519-sha256,…`); a current test `sshd`; Docker over SSH to the classical one; every product file that starts `ssh` (workspace scan) | the classical host is refused with `NoPqKex` and nothing runs there, although OpenSSH's defaults would reach it; the current one is accepted and `ssh -v` with the shared options reports a method of the allowed list; Docker names the refusal; every caller uses the shared options. v4: the Git stand-in allows the classical methods only for a resolved Azure DevOps host name (alias to Azure yes, Azure-looking argument resolving elsewhere no), non-Git paths to an Azure host name stay post-quantum only, the classical list has no SHA-1, `group1`, `group14` or group exchange, the setting off or `IDECRAFT_SSH_KEX=mlkem` restores the refusal. v5: test `sshd`s as jump hosts (`crates/remote/tests/jump.rs`) — a classical hop is refused alone, first or second in a chain, through a hop's own `ProxyJump` and as `-J`, by the start point and by the Git and Compose stand-ins, while plain OpenSSH reaches the target through it; post-quantum hops (one or two) are accepted; a `ProxyCommand` of the user's own, for the target or a hop, is refused unless `ssh.allowOwnProxyCommand` is on (then logged without the command); the workspace scan finds `ssh` started through variables and allows only `SshCommand` |

Release review (rule 8): before each release that contains a remote feature, the network paths,
the `snow` and `libcrux-ml-kem` versions and these tests' results are recorded in the release
notes' security section.

**Which specs carry SEC-15 and SEC-16:** every spec with a paired channel — the spec of M6.14's
`idecraft serve --pair`, the spec of M16's remote access (paired devices, relay) and the M17 mobile
specs — lists both; the one Noise module of section 1b (it wraps `snow` and
`libcrux-ml-kem`, in `crates/session` or a crate of its own, chosen by the M6.14 spec) holds the
unit tests of SEC-15. SEC-16 applies to every spec in which `serve --pair` can run on a host
without a keychain (M6.14); M16's desktop app always has a keychain and states SEC-16 as not
applicable. Specs with the SSH channel only (SPEC-0317) state both as not applicable. SEC-17 applies to every
spec with an SSH path (SPEC-0121, SPEC-0158, SPEC-0317, SPEC-0243, SPEC-0055 and the specs of
database tunnels and M6.13); its tests live in `crates/remote/tests/pq_kex.rs` and
`crates/containers/tests/containers.rs`.

### 10. Dependencies

Planned crates, all on the existing allow list of `deny.toml` (no new licence exception; checked
2026-11-07 down to the transitive dependencies):

| Crate | Purpose | Licence | Note |
|---|---|---|---|
| `snow` 0.10 | Noise `XXpsk3`, `XKpsk3` | Apache-2.0 OR MIT | default resolver (pure Rust: `curve25519-dalek` BSD-3-Clause, `chacha20poly1305`, `blake2`, `sha2` MIT OR Apache-2.0); features `hfs` and `pqclean_kyber1024` stay off |
| `libcrux-ml-kem` | ML-KEM-1024 (FIPS 203) | Apache-2.0 | exact pin `=0.0.x`; with `libcrux-sha3` ≥ 0.0.10 (advisories RUSTSEC-2026-0074, -0207, -0208 fixed); dependencies Apache-2.0 or MIT OR Apache-2.0 |
| `ml-kem` (RustCrypto) | second implementation for the cross-check of SEC-15 | Apache-2.0 OR MIT | dev-dependency only |
| `hkdf`, `sha2` | `psk3`, pairing key, SAS | MIT OR Apache-2.0 | already in the tree |
| `zeroize` | KEM secrets, `psk3`, pairing key | Apache-2.0 OR MIT | already in the tree through the RustCrypto crates |
| `keyring` | keychain on macOS and Windows | MIT OR Apache-2.0 | already used |
| `oo7` 0.5 | Secret Service on Linux (v7) | MIT | pure Rust over `zbus` 5, the release GPUI already uses on Linux, so no new crate; features `async-std` (as GPUI; `oo7` refuses two runtimes) and `native_crypto`; `zbus` 5 carries the informational advisories RUSTSEC-2026-0370/0371/0372 (unmaintained, merged into `zbus` 6), listed in `deny.toml`; `oo7` 0.7 on `zbus` 6 needs Rust 1.95 (workspace 1.89) |
| QR encoder | M16 | MIT or Apache-2.0 | chosen in the M16 spec |

No OpenSSL, no SSH library: SSH stays the system client. Adding them is part of the implementing
spec, with `cargo deny check` green; a crate whose licence is not on the list needs the Project
Owner's approval first.

## Consequences

- M6.12 needs only the SSH channel, which exists; Noise, pairing and the device list arrive with
  M6.14's `--pair` and M16. The session protocol (ADR-0026) is the same on both channels.
- Every remote spec carries a "Security" section that maps SEC-1–SEC-17 to its acceptance
  criteria; a spec without it is incomplete.
- SSH hosts, local clients and Git servers without a hybrid post-quantum key exchange (OpenSSH
  before 9.0) are refused with a message that says what to update (section 1a). The one exception,
  Git over SSH to Azure DevOps, runs with its strongest classical methods and a visible warning
  until Azure offers a post-quantum exchange (v4).
- No convenience switches: users on networks that block the paths get a clear refusal, not a weaker
  connection.
- Jump hosts must offer a post-quantum key exchange too; a `ProxyCommand` of the user's own works
  only after the user allows it (v5). Each connection asks `ssh -G` once (reused for 30 s).
- Two crypto dependencies (`snow`, `libcrux-ml-kem`) to watch in advisories; the Noise module
  wraps both so that a later change of the KEM (for example to `snow`'s HFS once it offers
  ML-KEM) touches one module and raises the protocol major.
- Handshakes of paired channels grow by about 3.2 KiB and half a round trip (`XKpsk3` instead of
  `IK`); sessions themselves are unchanged.
- Paired channels are protected against "record now, decrypt later" from their first version;
  authentication stays classical until a later amendment (section 1c, residual risks).

## Alternatives considered

- **TLS 1.3 with pinned self-signed certificates** for paired channels — rejected for the reasons in
  section 1 (pinning through an override API, certificate machinery, extra PAKE for pairing). Kept
  for standard protocols that require it.
- **A built-in SSH client or SSH server** (for paired channels or for hosts without OpenSSH) —
  rejected: duplicates OpenSSH, its config and agent support, and adds a large attack surface;
  Windows ships OpenSSH.
- **WireGuard/VPN as a prerequisite** — rejected: requires system privileges and setup outside
  IDECraft; users may still use one underneath.
- **Short code as the only pairing factor** — rejected: an active attacker could guess offline; the
  SAS comparison is required.
- **HTTP/WebSocket listener with tokens** (extending SPEC-0221 to the network) — rejected: a
  bearer token is not mutual authentication, and HTTP invites browser attacks (T6).
- **Post-quantum protection only in a later protocol version** (v1's default for question 3) —
  rejected by the Project Owner (PO-STATUS question 28 b): recorded traffic cannot be protected
  afterwards.

Hybrid constructions compared for section 1c (state of 2026-11-07):

| Construction | KEM | Standing | Rust crate and review | Fits Noise, psk and SAS | Verdict |
|---|---|---|---|---|---|
| **(i) Noise HFS extension in `snow`** (`Noise_XXpsk3hfs_25519+…`, tokens `e1`/`ekem1`) | released `snow` 0.10.0 (2025-07-19, newest release): only `Kyber1024`, the pre-standard round-3 Kyber, not FIPS 203; ML-KEM-768 exists only as the open pull request mcginty/snow#210 (since 2026-06-14, not merged, not released) | HFS is an unratified Noise extension draft; `Kyber1024` ≠ ML-KEM | Kyber through `pqcrypto-kyber` (C code from PQClean; RUSTSEC-2024-0381: unmaintained, replaced by `pqcrypto-mlkem`) | yes, in the handshake itself | rejected: no FIPS 203 KEM in a release; a git or forked `snow` is not allowed (`deny.toml`: unknown git sources denied) and would leave the reviewed release line. Re-examined when a `snow` release offers ML-KEM in HFS; switching is a new major |
| **(ii) Noise as in v1 plus an ML-KEM exchange whose secret goes into `psk3`** (chosen) | ML-KEM-1024, FIPS 203 | Noise's psk slot is the specified way to add an outside secret (WireGuard + Rosenpass do the same) | `libcrux-ml-kem` (formally verified, used through its C extraction in OpenSSH), RustCrypto `ml-kem` as cross-check | yes: one HKDF step, transcript-bound through `h2`; `psk3` also carries the pairing key; SAS from the final hash | **chosen**: standard KEM, released and verified crates, own code limited to one KDF call and two fixed payload layouts |
| (ii') same, but `IKpsk2` (keep two messages) | ML-KEM | — | — | the ciphertext would have to travel outside Noise before message 2 (own framing) or the KEM key would have to be a long-term static key (no post-quantum forward secrecy) | rejected: more own construction or weaker |
| **(iii) X-Wing** (X25519 + ML-KEM-768 as one KEM, `draft-connolly-cfrg-xwing-kem`) | ML-KEM-768 only | IETF CFRG draft (crate implements draft 06), not an RFC | RustCrypto `x-wing` 0.1.1, not independently audited | as a KEM it would still need the psk slot; its X25519 half duplicates Noise's Diffie-Hellman | rejected: draft status, only 768, no gain inside Noise |
| HPKE (RFC 9180) with a hybrid KEM | ML-KEM or X-Wing | RFC for HPKE, PQ KEM identifiers still drafts | `hpke` | one-way encryption, not a mutually authenticated channel | rejected: wrong tool |
| TLS 1.3 with `X25519MLKEM768` (`rustls`) | ML-KEM-768 | IETF draft, widely deployed | `rustls` + `aws-lc-rs` (licences include OpenSSL-style terms not on the allow list) or the pure-Rust provider | brings back X.509 pinning and the extra PAKE of section 1 | rejected for the reasons of section 1 |

## Questions for the Project Owner (defaults apply until answered)

1. **Hosts without a keychain** — *answered 2026-11-07 (PO-STATUS question 28 a): as proposed*;
   rule in section 3a.
2. **Relay** (already open in the plan): *default:* direct and SSH paths first, the relay as an
   option the user runs on their own server.
3. **Post-quantum protection** — *answered 2026-11-07 (PO-STATUS question 28 b): in the first
   version*; construction in section 1c.
4. **Require a post-quantum key exchange on SSH channels?** — *answered 2026-11-07 (PO-STATUS
   question 29): "Ja, erzwingen"*; rule in section 1a, applied to every SSH path including Git.
5. **Exception for Git over SSH to Azure DevOps?** — *answered 2026-11-07 (PO-STATUS question 30):
   "Ja, Ausnahme"*; limits in section 1a.

## Revision

- **v7 · 2026-11-10** — section 3a: the Secret Service on Linux with a session bus through `oo7`
  (Project Owner, 2026-11-09 evening: „Alle Fragen wie von dir empfohlen“), encrypted session
  only, no unlocking, refusal instead of fallback; debug builds on Linux keep keys in files;
  section 10: `oo7` added.
- **v6 · 2026-11-07** — section 4 gets the device grant `ui` for the UI driver of ADR-0027
  (deny by default like every grant; an action's effects need their own grants as well).
- **v5 · 2026-11-07** — rule "security first" (AGENTS.md rule 12) applied to residual risk (1):
  section 1a gets the jump-host rules (every `ProxyJump` hop through IDECraft's hop stand-in with
  the shared options, `ProxyCommand=none` without a jump, a `ProxyCommand` of the user's own refused
  unless `ssh.allowOwnProxyCommand`, default off, warning, log line) and one start point of `ssh`
  (`SshCommand`) instead of one option function, with a stricter workspace scan and crate-private
  options; Git and the Compose CLI stand-ins and every hop use no shared connection; residual risks
  (1) and (2) updated; SEC-17 extended; consequences.
- **v4 · 2026-11-07** — after the Project Owner's answer to PO-STATUS question 30 ("Ja,
  Ausnahme"): section 1a gets the one exception, Git over SSH to Azure DevOps — Git only, exact
  resolved host name, `diffie-hellman-group18-sha512` and `-group16-sha512` behind the post-quantum
  list, strict host keys, `pq: false` with a notice and a log line, setting
  `git.ssh.allowClassicKexForAzureDevOps` (default on) and `IDECRAFT_SSH_KEX=mlkem` to switch it
  off, nightly re-check; residual risk (0); SEC-17 extended; question 5 answered; consequences.
- **v3 · 2026-11-07** — after the Project Owner's answer to PO-STATUS question 29 ("Ja,
  erzwingen"): section 1a requires a hybrid post-quantum key exchange on every SSH path (one shared
  option function, local client checked with `ssh -Q kex`, own refusal for hosts without one, Git
  included after checking GitHub, GitLab, Bitbucket and Codeberg, stricter ML-KEM-only variant
  prepared, residual risks); question 4 answered; new SEC-17; consequences.
- **v2 · 2026-11-07** — after the Project Owner's answer to PO-STATUS question 28 ("1.": (a) as
  proposed, (b) post-quantum protection in the first version) and his rule "Ziel ist immer eine
  hohe Sicherheit" (AGENTS.md rule "security first"):
  - new threat T7 ("record now, decrypt later"); the active quantum attacker is named as out of
    scope instead of every quantum attacker;
  - section 1b: later connections use `XKpsk3` instead of `IK`; prologue with `pq=ML-KEM-1024`;
  - new section 1c: hybrid handshake X25519 + ML-KEM-1024, secret in `psk3` through HKDF with the
    handshake hash after message 2 as salt, no downgrade, crate `libcrux-ml-kem`, residual risks;
  - section 2: the pairing key (`idecraft-pair-v2`) enters `psk3` with the KEM secret; the SAS
    comes from the final hash;
  - new section 3a: key file on hosts without a keychain (0600 in 0700, owner check with `fstat`
    without following links, refusal with status 78, no automatic repair);
  - section 8: what the handshake payloads carry; section 9: SEC-6 and SEC-10 extended, new
    SEC-15 and SEC-16 and the specs that carry them; section 10: crates and licences;
  - alternatives: the compared hybrid constructions; new question 4 (post-quantum key exchange
    required on SSH channels).
- **v1 · 2026-11-07** — first version.
