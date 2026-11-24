# SPEC-0353: Internal vault for agents, phase 1

- **Version:** 1 · **Date:** 2026-11-18
- **Sources:** `GEN` (secret stores with handles: write-only storage, encryption at rest under a
  key held by the platform's key store, envelope encryption with a data key per entry,
  authenticated file formats with a monotonic counter, hash-chained audit trails, redaction of
  known values and their common encodings in output); `STD` (XChaCha20-Poly1305 as in
  draft-irtf-cfrg-xchacha, HKDF-SHA-256 RFC 5869, HMAC-SHA-256 RFC 2104, base64 and base32 RFC
  4648, percent-encoding RFC 3986); ADR-0033 (the decision this spec implements in part), ADR-0023
  rule 7 (no secrets in context), ADR-0031 (grants are the user's, never the project's), ADR-0008
  (every feature has an MCP tool), rule 12 (security first); SPEC-0008 (secret store of connection
  profiles), SPEC-0053 (agent rights, the grant file in the user's settings folder, the agent
  log), SPEC-0012 v3 (database permissions of agents). No observation of the reference: the
  reference has no vault for agents.
- **Feature matrix rows:** none (ROADMAP M19.15, harness phase after 1.0; owner's order of
  2026-11-17).

## Purpose
An agent that needs a credential to reach a database or to run a program against a service has, so
far, no way to use one without a person typing it or pasting it into the conversation, where it
lands in the model's context and the provider's transcript. An agent that receives a credential has
nowhere safe to put it. The vault stores credentials encrypted, hands agents only **handles**, and
puts a value in place itself where it is used. Agents can store and generate entries but never read
a value back. Each entry carries its own limits (project, injection form, target host); the first
use by an agent asks the person in the window; every use is recorded without the value.

Phase 1 implements ADR-0033 decisions D1 (handles), D2 forms 1 and 4 (database connections and
process environments), D3 (redaction), D4 (store and generate, write-only), D5 (scopes and
first-use approval, with the session as the agent identity), D6 (expiry and audit) and D7 (the key
in the platform's key store, without the Secure Enclave or TPM layer). What is left for later is
listed under "Out of scope".

## Behaviour

### Entries and handles
1. An entry has a **handle** `vlt_` followed by 128 random bits from the operating system's random
   source in lower-case base32 without padding (26 characters), a **name** (1 to 64 characters of
   `A-Z`, `a-z`, `0-9`, `.`, `_`, `-`; unique in the vault), a **kind** (`password`, `token` or
   `secret`), a **scope** (behaviour 7), `created_by` (the agent identity of behaviour 13),
   `created_at`, an optional `expires_at`, `seen_by_agent` and a version. Knowing a handle grants
   nothing. [GEN]
2. A value is 8 to 4 096 bytes of UTF-8 without NUL. Shorter values are refused: they cannot be
   redacted without hiding ordinary text (behaviour 17). [GEN]
3. There is **no tool, argument, setting or build that returns a value** to an agent. A value
   never appears in a tool result, an error, the agent log, the audit log or a question in the
   window. The right `secret-read` (SPEC-0053 behaviour 10) does not reach the vault. [GEN,
   ADR-0033 D1, D9]

### Storage and keys
4. The vault lives in the folder `vault` beside the user's grant file (the settings folder of
   SPEC-0053), never in a project: `vault.json` (the entries), `audit.jsonl` (behaviour 20) and
   `vault.lock`; the folder has mode 0700 and the files 0600 on Unix. `vault.json` is written to a
   temporary file in the same folder and renamed. Every operation holds an exclusive lock on
   `vault.lock`, so two Ferris IDEA processes never interleave a change. Without a known settings
   folder there is no vault. [GEN]
5. **Keys.** A random 256-bit vault key (VK) and a random 128-bit vault id are made when the first
   entry is stored. Each entry has its own random 256-bit data key, encrypted under a wrapping key
   derived from VK (HKDF-SHA-256, salt the vault id, info `idecraft-vault/v1/wrap`). The value is
   encrypted under the data key with XChaCha20-Poly1305 and a random 192-bit nonce; the associated
   data binds vault id, handle, version and field (`key` or `value`), so neither a data key nor a
   value can be moved to another entry. The whole file is authenticated with HMAC-SHA-256 under a
   key derived from VK (info `idecraft-vault/v1/mac`) and carries a counter raised by every
   change. The file's header names the format version and the algorithms. [STD, ADR-0033 D7]
6. **Key store.** VK, the vault id and the counter are kept as one item of the platform's key
   store: the macOS Keychain; on Linux the Secret Service (freedesktop.org) only — never the kernel
   keyring, which does not survive a restart; the Windows Credential Manager. On any other system,
   or when the key store cannot be reached, the vault refuses with the reason; there is **no key
   file and no plaintext fallback**, in debug builds neither (the test secrets folder of SPEC-0008
   behaviour 14 does not apply). Opening refuses: a vault file without its key; a key whose vault
   id differs from the file's; a file whose authentication fails; a file whose counter is lower
   than the key store's ("older than its key: restored from a backup?"); a missing file while the
   key store's counter says entries were written. A key store entry of the vault is never handed
   out as a database password (behaviour 15). [GEN, ADR-0033 D7]

### Scope and use
7. A **scope** has `projects` (the workspaces the entry may be used in), `uses` (the injection
   forms: `database`, `process-env`; at least one) and `hosts` (`host` or `host:port`, at most 16,
   lower-case names or IPv4 addresses, no wildcards; required when `uses` has `database`). An agent
   creates entries only for its own workspace: `projects` is that workspace's root, resolved, and
   nothing else. [GEN, ADR-0033 D4, D5]
8. A use names the handle, the form and the destination. It is allowed only when the entry exists,
   has not expired, its `projects` holds the workspace, its `uses` holds the form, and — for
   `database` — the destination `host:port` matches one of `hosts` (a `host` entry matches every
   port, `host:port` only that port; names compare without case). The check is made when the value
   is put in place, never earlier. An expired entry is refused with `vault_expired` and its name.
   [GEN, ADR-0033 D5, D6]
9. **First use asks.** The first use of an entry by an agent identity, per form and destination,
   asks the person in the window: the entry's name (never its value), the agent, the MCP tool, the
   destination and the form, and for `process-env` that the value is given to a program started
   from the project and that no sandbox limits it yet. The answers are *Deny*, *Allow Once* and
   *Allow for This Session*; Deny is the default and the focused answer (SPEC-0281 v3). *Allow for
   This Session* holds until the MCP session ends, the entry expires or 30 days pass, whichever is
   first. A denial holds for the session. **Without a window, or without an answer within the
   window's time limit, the use is refused**; nothing is used unattended that a person never
   allowed. [GEN, ADR-0033 D5]
10. **Process environments ask every time.** A `process-env` use is never remembered: *Allow for
    This Session* counts as *Allow Once*, because the desktop sandbox (ROADMAP M19.9) that would
    limit what the program does with the value does not exist yet. [ADR-0033 section
    "Environment variables", rule 12]
11. A grant lives only in the memory of the MCP session; it is never written to the project or to
    the grant file. [ADR-0031]

### Injection forms
12. **Process environment.** `run_command` takes `env_from_vault`, a map from environment variable
    names (`[A-Za-z_][A-Za-z0-9_]*`, at most 16) to handles. The dry run shows the variable names
    and handles. A real run needs the rights it needs today (`run`, a trusted project) and then one
    use per handle (behaviours 8 to 10); Ferris IDEA sets the variables in the started process only.
    A value containing `${` is refused, since the run would expand it. [GEN, ADR-0033 D2 form 4]
13. **Agent identity.** The identity is the MCP session Ferris IDEA serves: a random session id, shown
    with the name the client reported. The reported name alone is never trusted; a new session is a
    new identity and asks again. [ADR-0033 D5]
14. **Database connection.** A connection profile whose secret reference has the service
    `ferris-idea-vault` names a vault handle as its account. `save_connection` takes `vault` (a handle)
    and writes that reference instead of a keychain entry; the handle must exist, allow `database`,
    and its `hosts` must match the connection's host and port, else nothing is written. When an
    agent's call connects through such a profile (`test_connection` and every tool that opens a
    session for an agent), Ferris IDEA makes a `database` use with the destination `host:port` of the
    profile and hands the value to the driver. File databases take no vault entry. [GEN, ADR-0033
    D2 form 1]
15. A secret reference whose service is the vault's key store item (`ferris-idea-vault-key`) is
    never resolved as a database password, by agents and by the window alike. [GEN, rule 12]

### Agents store and generate
16. `vault_store` stores a value the agent holds (name, value, kind, uses, hosts, optional
    `expires_in_days` 1 to 3 650) and answers the handle and the entry's metadata. The entry is
    marked `seen_by_agent`: its value is already in a model's context. `vault_generate` makes a
    value inside the vault: `length` 16 to 128 (default 24), `charset` `printable` (the 94
    printable ASCII characters, about 157 bits at 24 characters, the default) or `alphanumeric`;
    uniform choice by rejection sampling from the operating system's random source; a value
    containing `${` is drawn again (behaviour 12). Both need the right `vault` (behaviour 22) and
    refuse a name that exists. [GEN, ADR-0033 D4]

### Redaction
17. Every value Ferris IDEA puts in place or stores is registered for redaction for the life of the
    process, with its raw form and the encodings JSON-escaped, percent-encoded, base64 (standard
    with and without padding, URL-safe without padding) and hex (lower and upper case). The
    existing masking path (`mask_secrets`, used for terminal and run output, problems, commands
    and session context) replaces each registered form by `«vault:<name>»` before its own rules.
    Redaction cannot find a value a program transformed on purpose; that is why process
    environments ask every time (behaviour 10). [GEN, ADR-0033 D3]
18. Every tool result and every tool error passes the registered forms before it leaves the MCP
    server. The question in the window and the window's action text never carry `vault_store`'s
    value. The agent log records the value argument by its length only (SPEC-0053 v7 behaviour
    11). [GEN]

### Lifecycle and audit
19. `vault_remove` removes an entry. Without asking only when the same agent identity created it
    and no use is recorded; otherwise the person is asked in the window (no window: refused). The
    data key and the value are gone from the file at once. [GEN, ADR-0033 D4, D6]
20. **Audit.** Every use, refusal, denial, store, generation and removal appends one line to
    `audit.jsonl`: sequence number, time, handle, entry name, agent identity, MCP tool, form,
    destination, result (`used`, `denied`, `refused`, `expired`, `unanswered`, `stored`,
    `generated`, `removed`), the approval it rested on (`window: once`, `session grant until <t>`,
    `own entry`) and `prev`/`mac`: an HMAC-SHA-256 over the previous line's `mac` and this record
    under a key derived from VK (info `idecraft-vault/v1/audit`). Never a value. A changed, removed
    or reordered line breaks the chain, and `vault_audit` says so. The log stays on the machine
    (ADR-0007). [GEN, ADR-0033 D6]

### MCP tools
21. Tools (ADR-0008):
    - `vault_list` — the entries usable in this workspace: handle, name, kind, scope, created by
      and at, expiry and whether it passed, `seen_by_agent`, number of uses and the last use (from
      the audit log); the number of entries outside this workspace; the key store's name and the
      vault folder. Never a value.
    - `vault_store`, `vault_generate` — behaviour 16.
    - `vault_remove` — behaviour 19.
    - `vault_audit` — the newest records (`limit` default 50, at most 500), newest first, and
      whether the chain is intact.
    - `run_command` `env_from_vault` (behaviour 12) and `save_connection` `vault` (behaviour 14).
    No tool reads a value. [ADR-0008]
22. All vault tools need the new right **`vault`** of the grant file (SPEC-0053), off by default;
    the effects table (SPEC-0053 v8) lists `vault_store`, `vault_generate` and `vault_remove` as
    changing the user's settings and `vault_remove` as asking the user. The right lets an agent
    store, generate, list and remove; a use still needs the person's approval (behaviour 9).
    [SPEC-0053]

## Acceptance criteria
- **AK-1** Given a new vault with an in-memory key store, when an entry is stored and the vault is
  opened again, then the entry's metadata is back, the file holds no value in plain text, a value
  decrypts only for its own entry, and a changed byte, a swapped value, a lower counter, a missing
  key and a different vault id are each refused (behaviours 1, 2, 4–6).
- **AK-2** Given entries with scopes, when uses are checked, then only the workspace, form and
  host:port of the scope pass, expired entries are refused with `vault_expired`, and generated
  values have the asked length and character set and never contain `${` (behaviours 7, 8, 16).
- **AK-3** Given an audit log, when records are appended and one line is changed or removed, then
  verification finds the break; records carry no value (behaviour 20).
- **AK-4** Given a registered value, when text contains it raw or in one of the encodings, then
  `mask_secrets` replaces it by `«vault:<name>»` (behaviour 17).
- **AK-5** Given an MCP client with the right `vault`, when it stores and generates entries, then
  it gets handles and metadata and never a value, `vault_list` shows them, a duplicate name and a
  short value are refused, and without the right every vault tool is refused (behaviours 3, 16,
  21, 22).
- **AK-6** Given an entry for `process-env` and a window, when `run_command` uses it, then the
  window is asked with the entry's name and no value, *Deny* refuses, *Allow* runs with the
  variable set, the program's output that prints the value comes back redacted, the next use asks
  again, and without a window the use is refused; every outcome is in the audit log (behaviours 9,
  10, 12, 17, 18, 20).
- **AK-7** Given an entry for `database` on `127.0.0.1:5432`, when `save_connection` binds it to a
  connection on another port, then nothing is written; bound to a matching connection, the profile
  names the vault reference and no keychain entry is written (behaviour 14).
- **AK-8** Given an entry the session created and never used, when `vault_remove` removes it, then
  nobody is asked; an entry created by another session asks the window and, without one, is
  refused (behaviour 19).

## Out of scope
- **Secure Enclave and TPM.** VK is held by the key store item alone; wrapping it with a
  non-exportable hardware key (ADR-0033 D7) needs a signed release and comes later.
- **A separate vault process** (ADR-0033 D8): the vault runs in the IDE process in this phase; the
  value is in its memory during a use.
- **HTTP client, Git credential helper and local forwarder** (ADR-0033 D2 forms 1–3): next phases.
- **Capture** of credentials from responses (ADR-0033 D4) and replacing a value.
- **Password managers** as a source (ADR-0033 D11), **rotation** and kept older versions (D6),
  **revocation lists** of running processes, **presence on every use** (Touch ID, polkit).
- **The vault view in the window** (D10): revealing, copying, editing scopes, the window's own
  database dialogs using vault references.
- **Persistent agent identities** (registered connection keys): until then a grant lasts one
  session.
- **Unattended process environments**: after the sandbox of M19.9.
- **Hosts without a key store** (ADR-0033 question 4): no vault there.

## Known deviations from the reference
The reference has no vault for agents; nothing is copied. Deviations from common practice, on
purpose:
- **D1** There is no way to read a value back, not even for the agent that stored it (behaviour 3).
- **D2** No key file fallback anywhere, debug builds included (behaviour 6).
- **D3** Process environments are approved on every use until a sandbox exists (behaviour 10).
- **D4** A grant never outlives the MCP session in this phase (behaviours 9, 11, 13).
- **D5** Values shorter than 8 bytes are refused, so redaction never has to hide ordinary words
  (behaviour 2).
