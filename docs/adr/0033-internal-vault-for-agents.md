# ADR-0033: Internal vault for agents — handles instead of values, injection at the point of use

- **Status:** accepted (2026-11-18, by the coordinator under the Project Owner's general authority
  of 2026-11-13; phase 1 implemented by [SPEC-0353](../specifications/SPEC-0353-internal-vault-for-agents.md))
  · **Version:** 1 · **Date:** 2026-11-17 · written on the Project Owner's
  request of 2026-11-17 (ROADMAP M19.15, "interner Tresor für Agenten"): agents should never pass
  credentials around in plain text, should not depend on a person typing them, and should be able
  to store new ones. Rule 12 ("security first") applies throughout; every weaker option is named as
  the Project Owner's decision. Implementation is after 1.0 (M19) and after quality gate QG-1
  (ADR-0015).
- **Related:** [ADR-0007](0007-no-restrictions.md) (no telemetry),
  [ADR-0008](0008-mcp-server.md) (MCP server),
  [ADR-0016](0016-plugins-as-wasm.md) and [ADR-0024](0024-plugin-network-to-profile-host.md)
  (plugins), [ADR-0017](0017-ai-subscriptions-through-provider-clis.md) (provider programs),
  [ADR-0023](0023-ferris-idea-as-agent-harness.md) rule 7 (no secrets in context),
  [ADR-0025](0025-network-security-for-remote-connections.md) sections 3 and 3a (keys in the
  keychain), [ADR-0027](0027-ui-driver-and-semantic-tree.md) (UI driver),
  [ADR-0029](0029-mobile-rust-core-native-ui.md) decision 4 (platform stores on mobile),
  [ADR-0031](0031-project-files-never-grant-agents.md) (grants are the user's),
  [SPEC-0008](../specifications/SPEC-0008-connection-profiles.md) (secret store),
  [SPEC-0053](../specifications/SPEC-0053-agent-permissions.md) (agent rights), ROADMAP M19.9
  (desktop sandbox), M19.10 (session history)

## Context

How Ferris IDEA handles secrets today:

- **Database passwords** live in the operating system's credential store. A connection profile names
  only a secret reference (service and account); the value is read and written only through a
  secret store and shown as `***` in debug output, logs and errors (SPEC-0008 behaviours 3, 11
  and 12). The store is the crate `keyring` 3.6: macOS Keychain, Windows Credential Manager and,
  on Linux, the kernel keyring (`linux-native`). A store that cannot be reached is named as such
  (behaviour 17). Resolving credentials never waits for input; a missing password is reported so a
  non-modal prompt can ask the person (behaviour 13).
- **Test secrets folder:** only in builds with debug assertions, and only while
  `FERRIS_IDEA_TEST_SECRETS_DIR` (or `test_store::use_dir`) names it, a private folder (0700, files
  0600, no symbolic links) replaces the system store, so the functional tests reach
  password-protected databases unattended. Release builds ignore it (SPEC-0008 behaviour 14,
  AK-14).
- **Remote keys** (Noise static keys, paired peers) live in the keychain as well: macOS Keychain,
  Secret Service on Linux through `oo7`, Credential Manager on Windows; a 0600 key file only on
  hosts with no keychain service and in macOS debug builds (ADR-0025 sections 3 and 3a). On mobile
  the platform store holds them (ADR-0029 decision 4). On Linux, SPEC-0008 and ADR-0025 thus use
  two different stores; the kernel keyring does not survive a reboot.
- **Agents never see secrets.** ADR-0023 rule 7, in force now: secrets never enter context or tool
  output, passwords stay in the keychain, `.env`-like values are masked. SPEC-0053 behaviour 10
  hands files that commonly hold secrets to an agent only with the right `secret-read`; the HTTP
  client shows `{{name}}` in place of private values in what it logs (behaviour 5).
- **ADR-0017** mentions a keychain-held API key only as the alternative to the providers' own
  programs; it does not define a store.

What is missing: an agent that needs a credential (to connect to a database, call an API, push to
a Git remote, run a test against a service) has no way to use one without a person typing it or
pasting it into the conversation, where it lands in the model's context, the provider's transcript
and possibly the project. An agent that creates an account or receives an API token has nowhere
safe to put it. The `secret-read` right is the only path today, and it hands the value to the
agent.

## Threat model

Assets: the stored credentials; the vault key; the audit trail; the grants.

| # | Threat | What the attacker has | What they want | Answer in this ADR |
|---|---|---|---|---|
| V1 | **Prompt injection through project content** (file text, issue, web page, database row, another agent's message) | control over text an agent reads | make the agent exfiltrate a credential: print it, write it to a file, send it to a host of theirs, put it in a commit | the agent never holds a value (D1); a use is bound to the entry's hosts and tools (D5), so the agent cannot point a credential at another host; injected values are redacted (D3) |
| V2 | **A malicious or confused MCP client** | a connection to Ferris IDEA's MCP server | read values, use entries it was never granted, widen scopes, swap a value for one it knows | no read tool exists (D1, D9); grants are bound to the client identity Ferris IDEA established, never to a name the client reports (D5); scopes are changed only by the person (D6); replacing a value needs approval (D4) |
| V3 | **A compromised agent process** (the agent program itself, its dependencies, its sandbox escape) | code execution as the agent, its MCP session | as V1 and V2, plus anything in that process's memory and environment | the agent process holds handles only; values reach only the processes Ferris IDEA starts for a use, inside the sandbox (D2, section "Environment variables") |
| V4 | **Malware running as the same OS user** | the user's files, processes, keychain requests | the vault file and its key, memory of Ferris IDEA | the vault file is useless without a key held by the platform's key store, bound to Ferris IDEA's code signature where the platform allows (D7); hardened process (D8). Limit: on a fully compromised account without hardware binding, the vault raises the cost, it does not stop a determined attacker (see Consequences) |
| V5 | **Backups and sync** | a copy of the settings folder (Time Machine, cloud sync, a stolen disk image) | offline decryption; rolling back to restore a revoked credential | the key never leaves the device (this-device-only, not synchronisable); a backup is ciphertext only; a version counter in the key store detects rollback (D7) |
| V6 | **Leakage through logs and transcripts** | access to Ferris IDEA's logs, session history (M19.10), crash reports, the agent provider's transcript | values written there | values never enter tool results; every known value is redacted from output before it is stored or sent (D3); the vault process writes no crash dumps (D8) |
| V7 | **Shoulder-surfing and screen capture in the UI** | a view of the screen, a screenshot, the UI driver | a value shown in the window | values are hidden by default; revealing needs the OS user check and hides again; the semantic tree and the UI driver never carry a value (D10) |
| V8 | **Supply chain** | a malicious or vulnerable dependency, a manipulated importer program | key or values from inside the vault process | the vault process has few, pinned, audited dependencies; importers are the vendors' own programs, called read-only (D11); plugins get injection only (D12) |

Out of scope: an attacker with root or kernel access; a person who willingly reveals a value and
pastes it; the remote service's own handling of the credential.

## Decision

### D1. Handles, not values

An agent works with **handles**: an opaque id (`vlt_` followed by 128 random bits in base32) plus a
human name, a kind (password, token, key pair, user and password), a scope and metadata (created by,
created on, expires, last used). Knowing a handle grants nothing.

There is **no MCP tool, right or setting that returns a value to an agent**, now or later. The
right `secret-read` (SPEC-0053 behaviour 10) keeps its meaning for files in the workspace and does
not reach the vault. A value never appears in a tool result, an error, a log line, the session
history or anything Ferris IDEA hands to a model. This is the rule of ADR-0023 rule 7 made structural:
there is no code path that could break it by mistake.

### D2. Ferris IDEA injects the value at the point of use

A tool that needs a credential takes a handle and Ferris IDEA puts the value in place itself. The forms,
in order of preference, the most contained first:

1. **Inside Ferris IDEA's own code:** a database connection (the profile's secret reference becomes a
   handle; SPEC-0008 behaviour 3 is unchanged in spirit); an HTTP request of Ferris IDEA's HTTP client,
   where a header or field written as `{{vault:<handle>}}` is filled when the request is sent.
2. **Git credential helper:** for a Git command Ferris IDEA starts, it passes
   `-c credential.helper=<Ferris IDEA's helper>` for that invocation only. The helper answers Git's
   credential protocol over a local socket with a one-time token tied to that invocation, and only
   for the protocol and host in the entry's scope. Hooks of the repository get nothing.
3. **Local forwarder** for a sandboxed process: a loopback listener, reachable only from that
   process and authenticated with a per-process token (ADR-0025 section 6), that forwards to the
   entry's host over TLS and adds the header. The process never holds the value.
4. **Environment variable or file descriptor** of a process Ferris IDEA starts in the sandbox (M19.9).
   Only for entries the person marked "may be given to a process", and only when 1–3 cannot work.
   What this still exposes is in the section "Environment variables" below.

Every use is checked against the entry's scope (D5) at the moment of injection, not when the
agent asks.

### D3. Redaction

Ferris IDEA knows every value it injected into a use. Before any output reaches a tool result, a log,
the session history or a model, it is searched for each value in use and its common encodings
(raw, base64 with and without padding, URL-encoded, hex, JSON- and shell-escaped) and the match is
replaced by `«vault:<name>»`. The same applies to files a process with an injected value wrote
inside the workspace, before a reading tool returns them or a change set is shown. Redaction is a
second line of defence: it cannot find a value that a program deliberately transformed. That is why
the forms 1–3 of D2 come first.

### D4. Agents put and generate, write-only

- `vault_generate(name, policy, scope)` creates a password or token **inside the vault** and
  returns its handle. Generation uses the operating system's random source (`getrandom`), uniform
  selection by rejection sampling, default 24 characters from the 94 printable ASCII characters
  (about 157 bits) or the target's stated policy. The agent then uses the handle (for example in a
  sign-up request through D2 form 1) and never sees the value.
- **Capture:** a tool that receives a credential from a service (an API token in an HTTP response, a
  key a command prints) can store a named part of the response into a new entry instead of
  returning it (`capture: { path, into: <name> }`). The rest of the response goes back redacted.
- `vault_put(name, value, scope)` stores a value the agent already holds. Such an entry is marked
  **"seen by an agent"** in its metadata and the audit trail; the window recommends rotating it,
  because the value is already in a model's context and a provider's transcript.
- Agents **create** entries. Replacing the value of an existing entry, deleting one, or changing a
  scope needs the person's approval, except for entries the same agent identity created and nothing
  else has used yet. Agents never widen a scope (D6).
- Putting and generating need a new right `vault` (SPEC-0053), off by default, granted per
  workspace like the others; the entry's scope starts within that workspace.

### D5. Scopes, first-use approval, then unattended within scope

Each entry carries a scope:

- **projects** (workspace ids) where it may be used;
- **tools or injection forms** (database connection X, HTTP client, Git, forwarder, environment);
- **target hosts** (host names and ports, matched exactly, wildcards only on a subdomain level the
  person entered; the database profile's target digest of ADR-0031 for connections).

A use is allowed when the project, the tool and the actual destination all match. The destination
is the one the connection really goes to after DNS and redirects; a redirect to another host drops
the credential.

The **first use** of an entry by an agent identity asks the person in the window: entry name (never
the value), agent, tool, destination, injection form, and whether the form exposes the value to a
process (D2 form 4). Answers: *deny*, *once*, *for this agent within this scope until* a date (the
default is the entry's expiry or 30 days, whichever is earlier). After that, uses within the scope
run unattended. A grant lives in the user's grant file (ADR-0031), never in the project.

The **agent identity** is the one Ferris IDEA established: a hosted agent Ferris IDEA started (ADR-0023
rule 2), or an external MCP client's registered connection key. A name a client reports about itself
is shown, never trusted. A new client is a new identity and asks again.

Entries can be marked **"presence on every use"**: each use then needs the OS user check (Touch ID,
Windows Hello, polkit). For production databases and signing keys this is the proposed default.

### D6. The person owns scopes and lifecycle

- **Expiry:** an entry or a grant past its date is not used; the agent gets the error
  `vault_expired` with the entry name, and the window lists what expired.
- **Rotation:** an entry may carry `rotate_after`; Ferris IDEA reminds. Where Ferris IDEA knows how to
  change the credential at the target (a database role's password through the connection), it
  offers to generate and set a new value in one step. Older versions are kept encrypted for a short
  window (7 days) for rollback, then destroyed.
- **Revocation:** revoking an entry or a grant takes effect at once; the running processes holding
  an injected copy (D2 form 4) are listed and offered to stop. The window states that revoking a
  handle does not undo a leak and recommends rotation at the target.
- **Audit of every use:** an append-only, hash-chained log (SHA-256 over the previous record) in the
  settings folder: time, entry, agent identity, tool, destination, injection form, result, and the
  approval it rested on. Never a value. It stays on the machine (ADR-0007) and is shown in the
  window and through MCP (`vault_audit`, metadata only).

### D7. Encryption at rest, key in the platform's key store

- **Layout:** one vault file per user in the settings folder (the folder of SPEC-0053), never in a
  project; mode 0600 in a 0700 folder, written to a temporary file and renamed. A header carries a
  format version and algorithm ids, so primitives can be replaced later.
- **Keys:** a random 256-bit **vault key** (VK). Each entry has its own random 256-bit data key,
  wrapped by VK; changing VK rewraps the data keys only.
- **Values:** XChaCha20-Poly1305 (RustCrypto `chacha20poly1305`, already in the tree through `snow`)
  with a random 192-bit nonce per encryption; the associated data binds vault id, entry id, version
  and field, so a value cannot be moved to another entry or field. The whole file is authenticated
  with HMAC-SHA-256 under a key derived from VK by HKDF-SHA-256, and carries a monotonic counter.
- **Wrapping VK** with a key that never leaves the device:
  - **macOS:** a P-256 key in the **Secure Enclave**, not exportable, usable only by Ferris IDEA's
    signed binary (access control with `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`). Because
    Secure Enclave keys are elliptic-curve only, which a future quantum computer could break given
    the stored public values, VK is wrapped under `HKDF(ECDH secret ‖ K2)`, where K2 is a random
    256-bit key in a data-protection keychain item, this device only, not synchronisable. Both are
    needed; this follows ADR-0025's post-quantum stance. Macs without a Secure Enclave use K2
    alone.
  - **Linux:** a key sealed by the **TPM 2.0** (`tss-esapi`, Apache-2.0) where present, combined
    with K2 in the **Secret Service** (`oo7`, as ADR-0025 v7). Without a TPM, Secret Service alone.
    The kernel keyring is not used for the vault (it does not survive a reboot).
  - **Windows:** a non-exportable key of the **Microsoft Platform Crypto Provider** (TPM) through
    CNG, combined with K2 under **DPAPI** (user scope). Without a TPM, DPAPI alone.
  - **Mobile** (M17), when the vault comes there: the platform stores of ADR-0029 decision 4.
- **No key store, no vault.** A host with no keychain service (a headless server) gets no vault: a
  key file beside the vault file would protect nothing against V4 and V5. ADR-0025 section 3a
  allowed a key file for remote keys; extending that to the vault is the Project Owner's decision
  (question 4).
- **Rollback:** the counter of the file is also stored in the key store item. A vault file older
  than the stored counter is refused with "the vault file is older than its key; restored from a
  backup?" and not opened unless the person confirms.

### D8. A small process holds the key

The vault runs as its own process (`ferris-idea-vault`), started by Ferris IDEA, speaking a narrow
protocol over a socket only Ferris IDEA's process can reach (peer credentials checked). It holds VK and
decrypts one value at a time, for one use, into the component that injects it. Agent programs,
plugins and the large IDE process never hold VK or entries they do not currently use. The process:

- keeps secrets in memory that is locked against swapping where the OS allows and zeroed after use
  (`zeroize`, `secrecy`);
- disables core dumps and debugger attachment (`RLIMIT_CORE` 0, `PR_SET_DUMPABLE` 0 on Linux, the
  hardened runtime without the `get-task-allow` entitlement on macOS);
- writes no crash report with memory contents;
- has a minimal, pinned dependency set, checked by `cargo deny` and `cargo audit` in CI.

The values a database driver or the HTTP client needs are in the IDE process for the duration of
that use; that is the cost of form 1 and the reason the IDE process is part of what must be trusted.

### D9. No plaintext export into model context, ever

Not through a tool, a setting, a debug build, an environment variable or a "support mode". The test
secrets folder of SPEC-0008 behaviour 14 stays as it is, debug-only, and is not a vault: the vault
has no debug bypass, its tests use an in-memory platform key. A person can see a value in the window
(D10); an export of all values in plain text is not offered. An encrypted export for moving to a new
machine is open (question 3).

### D10. The window

- Values are masked by default. Revealing needs the OS user check, shows one value, and hides it
  again after 20 seconds or when the window loses focus.
- Copying puts the value on the clipboard marked as concealed (on macOS
  `org.nspasteboard.ConcealedType`, so clipboard managers skip it) and clears it after 30 seconds if
  it is still there.
- The semantic tree (ADR-0027) carries a secret field with the role of a password field and without
  its value; the UI driver's screenshots black out revealed values. A screen reader announces that
  the value is hidden and reads it only after the reveal, to the person.

### D11. Import from existing password managers, read-only and optional

Ferris IDEA can use entries of an existing password manager through that manager's **official
interface**: its command line program or documented API (for example the programs of 1Password,
Bitwarden-compatible servers and KeePassXC), never by reading its files or memory. Two modes:

- **Reference:** the vault stores only a pointer; the value is fetched at the point of use. Single
  source, revocation in the manager works, but uses fail while the manager is locked.
- **Copy:** the value is copied into the vault once; afterwards it is independent.

Import never writes to the other manager. Its program runs with a minimal environment, its output is
read by the vault process only and never shown to an agent. Which mode is the default is question 5.

### D12. Plugins

WebAssembly plugins (ADR-0016) get no vault interface. A connector plugin that needs a password gets
it injected into its connection through the host, only for the profile's host and port
(ADR-0024), like form 1.

## Environment variables: what they still expose and how the sandbox limits it

An injected environment variable (D2 form 4) gives the value to code: the program started, and
anything it loads or starts. In a project that code may come from the repository (a test script, a
build step, a package's install hook), so under V1 it may be the attacker's. That code can:

- print the value, plain or transformed (reversed, split, hashed with a known salt), past redaction;
- write it to a file the agent later reads, or into a commit;
- send it over the network;
- pass it to child processes, which inherit the environment;
- be read by other processes of the same user: `/proc/<pid>/environ` on Linux (same user, subject to
  `ptrace` restrictions), the process list with environment on macOS.

The sandbox of M19.9 limits this:

- **Network:** outgoing connections only to the entry's target hosts (plus the forwarder), so the
  value cannot be sent anywhere else. Sending it to an allowed host helps the attacker only if they
  can read what that host receives, which is not the case for the person's own services.
- **Files:** writes only in the worktree and a scratch folder; files written during the run pass
  D3 before a tool returns them, and a change set touching them is flagged.
- **Processes:** the sandboxed process cannot attach to or read other processes; its children stay
  in the same sandbox; the variable is removed from the environment of anything Ferris IDEA starts
  after the run.
- **Lifetime:** the variable exists for that one process tree; on revocation the tree is offered to
  stop (D6).

What remains: a deliberately transforming program can hide the value in output or a file, and send
it to an allowed host. Therefore form 4 is **off per entry by default**; the person enables it per
entry, the approval dialog says that the value is given to project code, and entries with
"presence on every use" never use it unattended.

## Alternatives considered

- **Give agents the value through a `secret-read`-like right.** Rejected: once in context, the value
  is in the provider's transcript and open to every injected instruction; no later rule can take it
  back (V1, V6).
- **Keep using the system keychain directly, one item per credential.** Rejected as the vault: no
  scopes, no audit, no write-only generation, and a keychain item readable by the app is readable by
  any code in the app. It stays the store for the vault's wrapping key.
- **A password derived from a master passphrase the person types (Argon2id).** Rejected as the
  default: it depends on a person typing, which is what the request removes. It could protect an
  encrypted export (question 3).
- **Only an external password manager as the store.** Rejected as the only path: it would make
  agents depend on its unlock state and on its program, and none offers handles with scopes per agent
  and injection; kept as an optional source (D11).
- **AES-256-GCM** instead of XChaCha20-Poly1305. Not chosen: its 96-bit nonce makes random nonces
  risky at volume, and fast constant-time AES needs hardware support; AES-256-GCM-SIV would be the
  choice if a FIPS-style requirement ever came up.
- **Vault inside the IDE process.** Rejected: every bug in the large process (parsers, drivers, UI)
  would be next to the key (D8).
- **A key file next to the vault file on hosts without a key store.** Rejected by default (D7); the
  Project Owner's decision (question 4).

## Consequences

- Agents can connect, call APIs, push and run integration tests with credentials and without a
  person typing them, after one approval per entry and agent.
- New MCP tools: `vault_list` (metadata in the agent's scope), `vault_generate`, `vault_put`,
  `vault_request` (ask the person for a grant), `vault_audit`; existing tools take handles
  (`{{vault:<handle>}}` in HTTP, handles in connection profiles, `env_from_vault` for runs). New right
  `vault`. An implementing spec defines them with the usual rights table (ADR-0031 decision 6).
- SPEC-0008's secret store gets the vault as one more implementation; existing keychain entries keep
  working, and the window offers to move them into the vault. On Linux this also ends the use of the
  kernel keyring for new entries.
- The form 4 path depends on the desktop sandbox (M19.9); until it exists, form 4 is not offered.
- Signing: the Secure Enclave binding needs a signed release; debug builds use K2 in a 0600 file under
  the test rules (as ADR-0025 section 3a for remote keys), never in release.
- Limit stated plainly: malware running as the same user on a machine without a Secure Enclave or TPM
  can wait for the vault to be unlocked and ask it, or read the IDE process during a use. The vault
  narrows that to entries in use and leaves a trail; it does not replace a clean machine.
- Cost: a new process, platform code for three key stores, an import adapter per password manager.

## Questions for the Project Owner (defaults apply until answered)

1. **Grant length after the first approval** — *default:* until the entry's expiry or 30 days,
   whichever is earlier; "once" and "deny" always offered. Longer or unlimited grants are weaker.
2. **"Presence on every use" as the default for which entries** — *default:* production database
   connections and signing keys; everything else unattended within scope.
3. **Moving to a new machine** — *default:* no export; credentials are re-issued or re-imported.
   Alternative: an encrypted export protected by a passphrase (Argon2id, at least 256 MiB, 3
   passes) that the person types once. Weaker than no export.
4. **Hosts without a key store** (headless Linux servers) — *default:* no vault there. Alternative:
   a key file as in ADR-0025 section 3a. Weaker.
5. **Import mode** — *default:* reference (D11), copy as an option per entry. Copy-only would be
   simpler but leaves stale copies after revocation in the manager.
6. **Environment injection (D2 form 4) at all** — *default:* available per entry, off by default,
   only inside the sandbox. Alternative: never offer it. Stricter, but some test suites cannot run.
7. **Should agents rotate on their own** (generate a new value and set it at the target) within
   scope after the first approval — *default:* no, every rotation that changes the target asks.
