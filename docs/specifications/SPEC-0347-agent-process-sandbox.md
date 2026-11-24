# SPEC-0347: Sandbox for agent processes on the desktop

- **Version:** 1 · **Date:** 2026-11-18
- **Sources:** `GEN` (process sandboxes: deny by default, allowed file system roots, network
  egress through a filtering proxy that the sandbox alone may reach, fail closed when the sandbox
  cannot be applied; HTTP `CONNECT` tunnels; addresses that must not be reached through a name:
  loopback, private, link-local, cloud metadata); `STD` (RFC 9110 §9.3.6 `CONNECT`; RFC 1918, RFC
  4193, RFC 6598, RFC 3927 address ranges); `DOC`/`OSS` (Apple's Seatbelt profile language as used by
  `/usr/bin/sandbox-exec -p`; Linux Landlock, kernel documentation and the `landlock` crate
  0.4.7, MIT OR Apache-2.0: access rights per ABI version, `PathBeneath` rules, scopes,
  `restrict_self`; `unshare(2)` with `CLONE_NEWUSER | CLONE_NEWNET`, `user_namespaces(7)`); own
  tests 2026-11-18 on macOS 27.0.1 (`sandbox-exec` with `(deny default)`: `/usr/bin/true` needs to
  read the directory `/`; `(remote tcp "localhost:PORT")` lets through exactly that loopback port);
  ADR-0023 rules 7 (safety rules of MCP tools), 9 and 11 (agent host); ADR-0008 (every feature has
  an MCP tool); CLAUDE.md rule 12 (security first). No observation of the reference: the
  reference has no sandbox for agent processes.
- **Feature matrix rows:** none (ROADMAP M19.9, harness phase after 1.0).

## Purpose
An agent that Ferris IDEA starts (M19.5) runs code it wrote itself or read from untrusted content. It
should be able to work in the project, and nothing else: not read the user's keys, not change files
outside the folders it was given, not send data to hosts nobody allowed. Ferris IDEA starts such a
process inside an operating system sandbox built from a policy — the folders it may read, the
folders it may write, the hosts it may reach — and refuses to start it when the sandbox cannot be
applied. The policy can be shown and checked over MCP before any agent uses it.

## Behaviour

### Policy
1. A policy has three lists: `read` (folders or files the process may read), `write` (folders or
   files it may read and change, including create, delete and rename below them) and `hosts`
   (network destinations it may reach). Everything else is denied. There is no field, setting,
   environment variable or argument that switches the sandbox off or widens it beyond the policy;
   a process that is to run without the sandbox is not started by this module at all. [GEN]
2. Each root must be an absolute path that exists; it is resolved to its real path (symbolic links
   followed) and the rules are written for the real path. A root is refused when it is `/`, the
   user's home folder or a folder that contains the home folder, when its path or its real path has
   a control character, a `"` or a `\`, or when there are more than 32 roots in all. A root given in both lists
   counts as a write root once; a duplicate is kept once. [GEN]
3. A host entry is `name`, `name:port`, `*.domain` or `*.domain:port`, or an IPv4 or bracketed
   IPv6 literal with an optional port; the port defaults to 443 and must be 1–65535. Names are
   1–253 characters of letters, digits, `-` and `.`, compared without case; `*.` matches one or more
   labels in front of the domain (not the domain itself) and needs a domain of at least two labels;
   a bare `*`, a wildcard inside a label, a URL, a path and more than 64 entries are refused. [GEN]
4. Validation answers every problem it finds at once, each with the entry it is about, and gives a
   validated policy only when there is none. Only a validated policy can be launched. [GEN]

### Fixed rights beside the policy
5. Both platforms let the process read a fixed set of system locations that programs need to load
   and run, and nothing of the user's: on macOS `/usr`, `/bin`, `/sbin`, `/System`,
   `/Library/Apple`, `/private/etc`, `/private/var/select`, the directory `/` itself (its entries,
   not below it), and the devices `/dev/null`, `/dev/zero`, `/dev/random`, `/dev/urandom`; on Linux
   `/usr`, `/bin`, `/sbin`, `/lib`, `/lib32`, `/lib64`, `/etc` and the same devices (those that
   exist). `/dev/null` may also be written. File metadata (`stat`) may be read everywhere on macOS,
   since path resolution needs it; contents and directory listings may not. Programs outside these
   locations and the policy's roots cannot be started inside the sandbox. [GEN, own tests]

### macOS: Seatbelt
6. On macOS the process is started as `/usr/bin/sandbox-exec -p <profile> -- <program> <args>`. The
   profile starts with `(deny default)` and allows only: forking; executing and reading below the
   fixed locations and the roots; writing below the write roots and `/dev/null`; `ioctl` on the
   devices; signals within the same sandbox; reading `sysctl` values; looking up exactly the Mach
   services `com.apple.system.opendirectoryd.libinfo` (user and group names),
   `com.apple.system.notification_center` and `com.apple.logd`, and no other (so no name resolution
   through `mDNSResponder` and no other system service); and, when the policy has hosts, outgoing
   TCP to the allowlist proxy's loopback port (behaviour 9) and nowhere else. Unix sockets, incoming
   connections and every other network operation stay denied. Paths are written into the profile as
   quoted literals; the quote and the backslash cannot occur (behaviour 2). [DOC, own tests]
7. Seatbelt applies to the program and everything it starts. When `sandbox-exec` cannot apply the
   profile it does not start the program. [DOC]

### Linux: Landlock and namespaces
8. On Linux the process is started through Ferris IDEA's launcher `ferris-idea-sandbox-launch` (shipped
   beside the Ferris IDEA executable) as `ferris-idea-sandbox-launch <policy as JSON> -- <program> <args>`.
   The launcher, before it executes the program, in this order:
   - refuses a policy with hosts (behaviour 10);
   - moves itself into a new user namespace and a new network namespace (`unshare`), maps its own
     user and group id into the user namespace, and so has no network at all, not even loopback;
   - restricts itself with Landlock: the file system rights of Landlock ABI 3 (execute, read, write,
     create, remove, rename across folders, truncate) for all of the file system, minus the read
     rights below the read roots and fixed locations and all rights below the write roots, plus the
     ABI 6 scopes (no signals to processes outside the sandbox, no abstract Unix sockets outside it)
     and the ABI 9 right to connect to Unix sockets in the file system, which is granted only below
     the write roots. All of these are hard requirements: a kernel that cannot enforce one of them
     (Landlock ABI below 9) makes the launcher refuse. `no_new_privs` is set.
   - executes the program; its children inherit every restriction.
   Every failure in these steps ends the launcher with exit code 125 and one line on standard error
   that starts with `ferris-idea-sandbox: refused:`; the program is never executed after a failed step.
   [DOC (Landlock, `unshare(2)`, `user_namespaces(7)`), OSS (`landlock` crate)]
   Without ABI 9 a process could connect to a socket such as the session bus, a container engine or
   an SSH agent and act outside the sandbox through it; that is why the ABI 9 right is required, not
   best effort. [GEN]

### Network allowlist
9. When the policy has hosts, Ferris IDEA starts an allowlist proxy for that process: an HTTP proxy on
   `127.0.0.1` with a port of its own, which accepts only `CONNECT host:port` (other methods: `405`),
   reads a request head of at most 8 KiB within 10 seconds, and opens the tunnel only when the
   destination matches a host entry (behaviour 3) — otherwise `403`. A destination given by name is
   resolved by the proxy; when any address it resolves to is loopback, unspecified, private (RFC
   1918, RFC 4193), shared (RFC 6598), link-local (RFC 3927, `fe80::/10`, including the metadata
   address `169.254.169.254`), multicast or broadcast, the tunnel is refused, so a name cannot point
   the process at the machine or the local network. An IP literal is reached only when that literal
   is itself an entry. At most 64 tunnels are open at once. The process gets `HTTPS_PROXY`,
   `HTTP_PROXY` and `ALL_PROXY` (and their lower-case forms) naming the proxy; whatever does not use
   them has no network. Each decision (destination, allowed or refused, reason) is kept for the
   process's summary (behaviour 12). The proxy lives as long as the sandboxed process. [GEN, STD]
10. On Linux a policy with hosts is refused in this version: the network namespace leaves the
    process no path to a proxy on the host's loopback. Letting it reach the proxy through a Unix
    socket bound into the namespace is a follow-up (see Out of scope); until then a Linux agent that
    needs the network is not started. [GEN]

### Launch
11. A launch takes a validated policy, the program, its arguments, its working folder and its
    environment. The environment is exactly what the caller gives plus the proxy variables
    (behaviour 9): nothing is inherited from Ferris IDEA. Before the program starts, Ferris IDEA starts
    the same sandbox around a probe (macOS: `/usr/bin/true`; Linux: the launcher's `--probe`, which
    does every step of behaviour 8 except executing a program) and refuses the launch with the
    probe's reason when it fails. On every other operating system, and when the launcher or
    `sandbox-exec` is missing, the launch is refused. A refusal never falls back to starting the
    program without the sandbox. [GEN]
12. A sandboxed process has a summary that the agent host shows per agent (M19.5): the mechanism
    (`macos-seatbelt` or `linux-landlock`), the resolved read roots, write roots and hosts, the proxy
    port when there is one, and the proxy's decisions. [GEN]

### MCP
13. The MCP tool `sandbox_policy` shows and checks a policy without starting anything: arguments
    `read`, `write` and `hosts` (paths relative to the workspace root are taken below it); without
    any argument it shows the default policy of an agent in this workspace: the workspace root as the
    only write root, no read roots, no hosts. It answers `valid`, `problems` (entry and message), the
    resolved `policy`, the `mechanism` of this machine with `available` and `reason` (macOS:
    `sandbox-exec` is present; Linux: the running kernel can enforce the Landlock rights of behaviour
    8 — the user namespace is checked at launch; other systems: not available), the fixed locations
    of behaviour 5, and on macOS the Seatbelt `profile` (with the proxy port left open, since it is
    chosen at launch). On Linux a policy with hosts is answered as not valid (behaviour 10). It
    needs no right and changes nothing. [ADR-0008, ADR-0023 rule 7]

## Acceptance criteria
- **AK-1** Validation refuses a relative root, a missing root, `/`, the home folder and a folder
  above it, a path with `"`, more than 32 roots, a bare `*`, `*.com`, a wildcard inside a label, a
  URL, port 0 and more than 64 hosts, each with its entry; it resolves symbolic links of roots,
  merges a root given twice, and accepts `api.example.com`, `*.example.com:8443`, `10.1.2.3:80` and
  `[2001:db8::1]` (`crates/sandbox/src/policy.rs` tests).
- **AK-2** The Seatbelt profile of a policy starts with `(deny default)`, allows writing only below
  the write roots and `/dev/null`, names exactly the three Mach services, and allows outgoing
  network only as `localhost:<proxy port>` when there are hosts and not at all without
  (`crates/sandbox/src/seatbelt.rs` tests).
- **AK-3** The allowlist proxy tunnels `CONNECT` to an allowed IP literal, answers `403` for a
  destination that is not allowed and for a listed name that resolves to loopback, and `405` for
  `GET`, and records each decision (`crates/sandbox/tests/it/proxy.rs`, `crates/sandbox/src/proxy.rs` tests for the address ranges).
- **AK-4** On macOS (tests compiled only there): inside the sandbox a write below the write root
  succeeds, a write to another folder and a read of a file outside the roots fail, a direct TCP
  connection to a listener on `127.0.0.1` fails, and through the proxy the allowed listener is
  reached and a non-allowed one is refused (`crates/sandbox/tests/it/macos.rs`).
- **AK-5** On Linux (tests compiled only there): with a kernel that has Landlock ABI 9 and user
  namespaces, a write below the write root succeeds, a write elsewhere fails and a TCP connection to
  `127.0.0.1` fails; without them the launch is refused with `ferris-idea-sandbox: refused:` and the
  program does not run; a policy with hosts is refused (`crates/sandbox/tests/it/linux.rs`).
- **AK-6** A launch on an unsupported system, with a missing launcher, or whose probe fails is
  refused and nothing is started (`crates/sandbox/src/lib.rs` tests).
- **AK-7** MCP `sandbox_policy` without arguments shows the workspace root as the write root and no
  hosts; with a bad entry it answers `valid: false` and the problem; on macOS it includes the
  profile (`crates/mcp/tests/it/sandbox.rs`).

## Out of scope
- Starting agents in the sandbox from the agent host and showing the summary in its view (M19.5;
  named follow-up of this spec), and choosing a policy per agent in the window.
- The network allowlist on Linux (behaviour 10): a Unix socket bound into the network namespace
  that leads to the proxy, or an equivalent. A seccomp filter that blocks Unix sockets so that
  kernels before Landlock ABI 9 can be used. Both keep the protection of behaviour 8; neither is a
  weaker option.
- Containers as a third mechanism, Windows, and the web IDE's sandbox (M18.5).
- Limits on CPU, memory and processes; secrets handed into the sandbox (M19.15).
- Plain-HTTP forwarding through the proxy (only `CONNECT`), and name resolution inside the sandbox.

## Known deviations from the reference
None: the reference has no sandbox for agent processes.

## Open questions
- **Q1 (owner):** On Linux, until the follow-ups of Out of scope land, an agent that needs the
  network — and every agent on a kernel before Landlock ABI 9 — is refused. Running such agents
  with a weaker sandbox (for example Landlock without the Unix-socket right, or without the network
  namespace) would need the owner's decision (CLAUDE.md rule 12); this version does not offer it.
