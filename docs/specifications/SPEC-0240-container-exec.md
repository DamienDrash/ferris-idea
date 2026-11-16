# SPEC-0240: Shell and commands inside containers

- **Version:** 2 · **Date:** 2026-11-05 (v2: the tab says how the shell ended)
- **Sources:** GEN (IDEs open a shell inside a running container or pod from their container tool
  window; F-083), STD (Docker Engine API: `POST /containers/{id}/exec`, `POST /exec/{id}/start` with
  the connection upgraded to a raw stream (`Upgrade: tcp`, answer 101), `POST /exec/{id}/resize`,
  `GET /exec/{id}/json`; the multiplexed stream with 8-byte frame headers when no TTY is used;
  `kubectl exec`; POSIX terminal raw mode and `TIOCGWINSZ`), SPEC-0119, SPEC-0038 (terminal
  sessions), SPEC-0053 (agent `run` right and log), ADR-0008
- **Feature matrix rows:** F-083

## Purpose
Open an interactive shell inside a running container or pod in the IDE's terminal, and let agents
run single commands there with a preview first.

## Behaviour
1. **Docker shell:** `e` on a running container in the Containers window opens a new tab
   `Exec: <name>` in the Terminal tool window with an interactive shell inside the container: `bash`
   when the image has it, else `sh`, decided inside the container in one exec. `TERM` is
   `xterm-256color`. A container that is not running is not entered; the status line says to start
   it first. [GEN, STD]
2. **Bridge without the Docker CLI:** the tab runs the IDECraft binary itself as the bridge
   (`idecraft container-exec <engine> <container>`): it creates the exec with a TTY, starts it with
   the connection upgraded to a raw stream, switches its own terminal to raw mode, copies bytes both
   ways, sends the terminal size at start and whenever it changes (checked every 250 ms), and ends
   with the shell's exit code when the shell ends or the container stops. The tab keeps the output
   and shows "Process finished with exit code N" above it (v2, hand test 170). [STD]
3. **Kubernetes shell:** `e` on a pod, deployment, stateful set, daemon set or job opens a tab
   `Exec: <kind> <name>` running `kubectl exec -it <object> --context <c> --namespace <n> -- sh -c
   <bash-or-sh>`; kubectl picks the pod of a workload and the default container. [STD]
4. **Agent commands (Docker):** MCP `docker_exec` runs one command (an argument list, never a host
   shell line) in a running container without a TTY and returns stdout and stderr in order, the exit
   code and whether output was cut (at 1 MiB). Optional `workdir`, `env` (`NAME=value` entries) and
   `user`; ends after `timeout_s` (default 60, at most 600). [STD, ADR-0008]
5. **Agent commands (Kubernetes):** MCP `kubernetes_exec` (`context`, `namespace`, `kind`, `name`,
   `container`, `command`) runs `kubectl exec <object> [-c container] -- <command>` without a TTY with
   the same limits. [STD, ADR-0008]
6. **Preview and rights:** both tools default to `dry_run: true` and only report what would run;
   `dry_run: false` needs the user-granted `run` right and is logged in `.idecraft/agent-log.jsonl`
   with tool, target and command. [SPEC-0053]

## Acceptance criteria
- **AK-1** Unit: exec request bodies (TTY, attach flags, `TERM`, working folder, environment, user),
  the upgrade answer, the shell chooser line, resize paths, kubectl exec argument lists with refused
  names (`docker::tests`, `exec::tests`, `kube::tests`).
- **AK-2** Against the fake Engine API: a command's output and exit code without TTY, an upgraded
  TTY session that echoes input and ends with its exit code, resizes seen, a stopped container's
  refusal (`crates/containers/tests/containers.rs`).
- **AK-3** Live against a real Docker Engine (`IDECRAFT_TEST_DOCKER`, skipped when unset or not
  reachable): `uname -s` prints `Linux`, `sh -c 'exit 3'` gives 3, stderr is kept, an interactive TTY
  session runs `echo` and `exit 4`; against a real cluster (`IDECRAFT_TEST_KUBECONFIG`):
  `kubectl exec` into a pod prints its hostname (`crates/containers/tests/live.rs`).
- **AK-4** MCP: preview without change, refusal without `run`, output and exit code with `run`, log
  entry (`crates/mcp/tests/it/protocol.rs`: `container_exec_tools`).
- **AK-5** Window self-test: `e` on the running fake container opens the exec tab, which shows the
  shell's greeting and echoes typed input; `e` on a stopped container leaves a status message.
- **AK-6** Hand test 170 against the Lima engine and the k3s cluster.

## Out of scope
Attaching to the main process of a container (`docker attach`), copying files into containers,
choosing the container of a multi-container pod in the window (MCP takes `container`).

## Known deviations from the reference
The bridge is IDECraft itself, so no Docker CLI is needed on the machine. One key opens the shell;
the shell is chosen inside the container instead of failing on images without `bash` (IMP-0111).
Not compared side by side yet.
