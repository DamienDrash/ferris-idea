# SPEC-0241: Published ports and Kubernetes port forwarding

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** GEN (IDEs show the ports a container publishes and forward ports of pods and services
  to the local machine; F-083), STD (Docker Engine API container list `Ports`; `kubectl port-forward`
  with `--address` and its "Forwarding from" line; TCP port numbers, ports below 1024 need rights on
  many systems), SPEC-0119, SPEC-0053, ADR-0008
- **Feature matrix rows:** F-083

## Purpose
Reach what runs in a container or a cluster from the browser or a local tool without typing port
numbers or keeping a terminal open for `kubectl port-forward`.

## Behaviour
1. **Docker published ports:** each container row shows its published ports (SPEC-0119). `o` opens
   the first published TCP port in the default browser as `http://localhost:<port>` (an address of
   all interfaces, `0.0.0.0` or `::`, means localhost; a specific address is used as it is). A
   container without a published TCP port leaves a status message. [STD]
2. **Ports of Kubernetes objects:** pods list their containers' ports, deployments, stateful sets,
   daemon sets and jobs the ports of their pod template, services their service ports. [STD]
3. **Start a forward:** `f` on a pod, deployment, stateful set, daemon set or service with a port
   starts `kubectl port-forward <kind>/<name> <local>:<remote> --address 127.0.0.1 --context <c>
   --namespace <n>` for its first port. The local port is the same number when it is 1024 or more
   and free on 127.0.0.1, else one the system picks. The forward counts as started when kubectl
   prints its forwarding line; when that does not happen within 20 s, or kubectl ends, it is stopped
   and kubectl's message is shown. Forwards listen on 127.0.0.1 only. [STD]
4. **Forwards in the window:** running forwards are listed first in the Kubernetes part as
   `<kind>/<name> 127.0.0.1:<local> → <remote>` with their context and namespace. `o` opens
   `http://127.0.0.1:<local>`, `x` or Delete stops the forward. A forward whose kubectl ended (pod
   gone) shows `ended` with kubectl's last message until it is stopped. [GEN]
5. **Lifetime:** forwards belong to the IDECraft process: the window and agents see the same list;
   all forwards stop when the process ends. [GEN]
6. **MCP:** `kubernetes_port_forward` with `action` `list`, `start` (`context`, `namespace`, `kind`,
   `name`, optional `remote_port`, `local_port`) or `stop` (`id`). `start` defaults to `dry_run: true`
   and returns the command; starting and stopping for real need the `run` right and are logged in
   `.idecraft/agent-log.jsonl`. [ADR-0008, SPEC-0053]

## Acceptance criteria
- **AK-1** Unit: published port parsing and browser addresses, local port choice, the forwarding
  line, kubectl argument lists, ports of pods, workloads and services (`ports::tests`, `kube::tests`).
- **AK-2** Against the fake kubectl: a forward starts, is listed, its process ends on stop; a forward
  whose kubectl fails reports the message (`crates/containers/tests/containers.rs`).
- **AK-3** Live against k3s (`IDECRAFT_TEST_KUBECONFIG`): an nginx pod and service, a forward to the
  service answers HTTP on 127.0.0.1, stopping closes the port; against a real engine
  (`IDECRAFT_TEST_DOCKER`): a container published on 127.0.0.1 answers on the port shown
  (`crates/containers/tests/live.rs`).
- **AK-4** MCP: list, preview, refusal without `run`, start and stop with `run`, log entries
  (`crates/mcp/tests/it/protocol.rs`: `port_forward_tools`).
- **AK-5** Window self-test: `f` on the fake pod lists a forward, `x` stops it; `o` on a container
  without ports leaves a message.
- **AK-6** Hand test 171.

## Out of scope
UDP forwards, forwarding several ports of one object at once in the window (MCP takes
`remote_port`), restarting a forward automatically when its pod is replaced, SSH tunnels to remote
engines' ports.

## Known deviations from the reference
Forwards never listen beyond 127.0.0.1, a busy or privileged local port falls back to a free one
instead of failing, and running forwards are always visible with a stop key (IMP-0110). Not compared
side by side yet.
