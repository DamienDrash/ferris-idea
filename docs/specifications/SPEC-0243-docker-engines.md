# SPEC-0243: Docker engines: local VMs, contexts and remote hosts over SSH

- **Version:** 3 · **Date:** 2026-11-07 (v1 2026-11-03; v2: post-quantum key exchange required for
  SSH engines, ADR-0025 v3; v3: the one start point of `ssh`, every jump host with the post-quantum
  key exchange, ADR-0025 v5)
- **Sources:** GEN (IDEs connect to local and remote Docker engines and switch between them; F-083),
  STD (`DOCKER_HOST` schemes `unix://`, `tcp://`, `ssh://`; Docker CLI contexts: `currentContext` in
  `~/.docker/config.json`, `~/.docker/contexts/meta/<sha256 of the name>/meta.json` with
  `Endpoints.docker.Host`; `docker system dial-stdio`; OpenSSH `BatchMode`, `StrictHostKeyChecking`,
  `ControlMaster`/`ControlPersist`; Lima's forwarded socket `~/.lima/<instance>/sock/docker.sock`),
  SPEC-0119, SPEC-0121 (SSH conventions), IMP-0044, ADR-0008
- **Feature matrix rows:** F-083

## Purpose
Work with the Docker engine of a local VM (Lima, Colima, …) or of another machine reachable over SSH,
and switch between engines without leaving the IDE.

## Behaviour
1. **Engines:** IDECraft lists engines in this order, each address once: `DOCKER_HOST`; the Docker
   CLI's current context; the local sockets of SPEC-0119 that exist, plus every Lima instance's
   forwarded socket; the other Docker CLI contexts. Each engine has a name (`DOCKER_HOST`, the
   context name, or the tool the socket belongs to) and its address. [STD]
2. **Switching:** the first engine is used; `e` in the Containers window with no container selected
   (on a heading) switches to the next engine and reads again. The heading names the engine and
   its address; the other engines are listed below the heading. [GEN]
3. **SSH engines:** for `ssh://[user@]host[:port]` every request runs the system `ssh` with the
   shared options of SPEC-0121 (batch mode, strict host key checking, hybrid post-quantum key
   exchange only), then the engine's extra arguments, a shared connection (control socket in a
   private folder, kept 60 s) and `docker system dial-stdio` on the host, which reaches the engine
   the host's Docker CLI uses (rootless included). No password is asked or stored; keys and the
   agent are used as on the command line. A host without a post-quantum key exchange is refused
   with SPEC-0121's message, a local `ssh` without one before any connection. Shell, logs, Compose
   (SPEC-0242) and all other actions work the same. **v3:** the command line comes from the start
   point `idecraft_remote::SshCommand`; a `ProxyJump` to the engine's host goes through IDECraft's
   hop stand-in with the same options, a `ProxyCommand` of the user's own is refused unless allowed
   (SPEC-0121 v4 behaviour 1a). [STD, ADR-0025]
4. **Plain TCP:** a `tcp://` engine on another machine than this one is used, but its heading warns
   that the connection is neither encrypted nor authenticated. [GEN]
5. **TLS:** an engine that needs TLS (`DOCKER_TLS_VERIFY`, a context with TLS material) is listed
   with the note that TLS is not supported yet and is not contacted. [GEN]
6. **MCP:** `docker_status` returns the engines with name, address and which one is used, and takes
   an optional `engine` (name or address); `docker_container`, `docker_exec` and `docker_compose`
   take the same `engine`. [ADR-0008]

## Acceptance criteria
- **AK-1** Unit: engine order and de-duplication, context files (current, others, TLS), Lima sockets,
  `ssh://` parsing with user and port, the ssh argument list, TCP warning (`engines::tests`).
- **AK-2** Against the fake Engine API reached through a fake `ssh` that runs a local bridge: version,
  lists and logs over `ssh://` (`crates/containers/tests/containers.rs`).
- **AK-3** Live: the Lima engine through its socket and the same engine over `ssh://` with Lima's ssh
  configuration (`IDECRAFT_TEST_DOCKER`, `IDECRAFT_TEST_DOCKER_SSH`) give the same containers
  (`crates/containers/tests/live.rs`).
- **AK-4** MCP: engines in `docker_status`, an unknown engine refused (`crates/mcp/tests/it/protocol.rs`:
  `container_tools`).
- **AK-5** Window self-test with two fake engines: the heading names the first, `e` switches to the
  second.
- **AK-6** Hand test 173 (a remote host over SSH, a Lima or Colima engine).
- **AK-7** SEC-17 (ADR-0025 v3): the shared options come first in the ssh argument list and carry
  only allowed key exchanges (`docker::tests`); the fake `ssh` receives `KexAlgorithms`; Docker over
  SSH to a test `sshd` limited to classical key exchanges is refused with the post-quantum message
  (`crates/containers/tests/containers.rs`).
- **AK-8** (v3) The engine's line: shared options, then the pinned route, then the engine's
  arguments (`docker::tests`); the fake `ssh` answers `-G` (`fake::ssh_script`).

## Out of scope
TLS client certificates, Podman's own API extensions, creating contexts, SSH password or
keyboard-interactive login.

## Known deviations from the reference
Remote engines go over SSH with the system client and strict host keys instead of an open TCP port;
plain TCP to another machine is marked as unsafe (IMP-0112). Not compared side by side yet.
