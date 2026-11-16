# SPEC-0119: Docker and Kubernetes

- **Version:** 4 · **Date:** 2026-11-15 (v4: AK-7, hand tests 46 and 170–173 with real keys against the reference environment's engine and cluster; v3: gaps and deviations from the side-by-side OBS-0029; v2, 2026-11-03: shell inside containers SPEC-0240, ports and port forwarding SPEC-0241, Compose SPEC-0242, engines and remote engines SPEC-0243; live tests against a real engine and cluster)
- **Sources:** GEN (IDEs list the containers and images of a Docker daemon and the workloads of a
  Kubernetes cluster, start, stop and restart them and show their logs; F-083), STD (Docker Engine
  API: `/version`, `/containers/json`, `/images/json`, `/containers/{id}/start|stop|restart|logs`,
  `DELETE /containers/{id}`, the multiplexed log stream with 8-byte frame headers; HTTP/1.1 with
  chunked bodies; `DOCKER_HOST`; `kubectl config view`, `get … -o json`, `logs`,
  `rollout restart`, `delete pod`; Kubernetes object status fields), SPEC-0053 (agent `run` right
  and log), ADR-0008
- **Feature matrix rows:** F-083

## Purpose
See and control the containers and workloads a project runs in, and read their output, without
leaving the IDE.

## Behaviour
1. **Docker daemon:** `DOCKER_HOST` (`unix://…` or `tcp://…`) when set, else the first socket
   that exists of Docker Desktop (`~/.docker/run/docker.sock`), Colima, OrbStack, Rancher
   Desktop, Podman machine and `/var/run/docker.sock`. IDECraft speaks the Engine API itself; no
   Docker CLI is needed. The daemon's version and platform are shown with the host. [STD]
2. **Containers and images:** containers with name, image, state, status, published ports and
   compose project, running ones first; images with tags (dangling as `<none>`), short id and
   size. `s` starts, `x` stops (10 s grace), `r` restarts the selected container; Delete pressed
   twice removes it (also when running). Starting a running or stopping a stopped container is
   no error. [STD]
3. **Container logs:** Enter (or `l`, a double click) shows the last 200 lines of the container's
   output, stdout and stderr in order, next to the list. [STD]
4. **Kubernetes:** through `kubectl` on `PATH`, so the user's kubeconfig, credentials and exec
   plugins work unchanged. The current context and its namespace are used first; `c` switches to
   the next context, `n` to the next namespace. Every call names context and namespace
   explicitly, has no input and ends after 20 s; names that would read as options are refused.
   [STD]
5. **Workloads:** deployments, stateful sets, daemon sets, jobs, pods and services of the
   namespace, grouped in that order: status (a pod's waiting reason such as `CrashLoopBackOff`
   before its phase; `Available`/`Progressing` from ready replicas; `Complete`/`Failed` for jobs;
   the type for services), ready count, restarts, age as `kubectl get` prints it, images or ports.
   kubectl's own message is shown when a context cannot be read. Enter shows the logs of a pod or
   workload (all containers). [STD]
6. **Restart:** `r` on a deployment, stateful or daemon set runs `kubectl rollout restart`; on a pod
   it deletes the pod so its controller replaces it. [STD]
7. **Window:** `Containers.ToolWindow` opens "Containers" at the bottom with Docker first and
   Kubernetes below; F5 reads again; Escape and ⇧Esc hide it. [GEN]
8. **MCP:** `docker_status`; `docker_container` (`logs`, or `start`/`stop`/`restart`/`remove`
   with `dry_run` default true); `kubernetes_status` (`context`, `namespace`);
   `kubernetes_workload` (`logs`, or `restart` with `dry_run` default true). Changes need the
   user-granted `run` right and are logged in `.idecraft/agent-log.jsonl`. [ADR-0008, SPEC-0053]

## Acceptance criteria
- **AK-1** Host discovery, HTTP responses plain and chunked, container fields and id checks, log
  frames; ages, pod/workload/service/job status, name checks; window rows and sizes (unit tests
  `docker::tests`, `kube::tests`, `containers_view::tests`).
- **AK-2** Against a fake Engine API on a real Unix socket and a fake `kubectl`: version, lists,
  logs, start/stop/restart/remove with the requests seen, a missing container's 404 message, an
  unreachable socket; contexts, namespaces, resources, an unauthorised context, logs, restarts,
  refused names, a missing kubectl (`crates/containers/tests/containers.rs`).
- **AK-3** MCP: status, logs, previews that change nothing, refusals without the `run` right and
  for unknown actions, changes with the right and their log entries
  (`crates/mcp/tests/it/protocol.rs`: `container_tools`).
- **AK-4** Window self-test against the fakes: lists, `s` starts, Enter shows container and pod
  logs, `r` restarts a deployment, `c` shows the next context's error, Delete twice removes.
- **AK-5** Hand test (round 4): a real Docker (Desktop, Colima, OrbStack or Lima) and a local cluster
  (kind, minikube, k3s or Docker Desktop's), next to the reference.
- **AK-6** Live against a real Docker Engine and a real cluster, skipped when they are absent
  (`IDECRAFT_TEST_DOCKER`, `IDECRAFT_TEST_KUBECONFIG`; set up by `containers.sh` of the reference environment):
  version, lists, logs, start/stop/restart/remove of a container; contexts, namespaces, a
  deployment's status and logs, rollout restart (`crates/containers/tests/live.rs`).
- **AK-7 (v4)** Hand tests 46 and 170–173 in a window of the test harness with real keys against the
  reference environment's Docker Engine and k3s (`crates/workbench/tests/it/f083_handtests.rs`:
  `hand_tests_46_and_170_to_173_containers_with_a_real_engine_and_cluster`; skipped without the
  variables `containers.sh env` prints): Compose up with `u`, `o` on a published port that answers,
  `x`/`s`/Enter on a container, Enter on a service, `r` on a deployment and Enter on its pod, `e`
  on the pod opening a shell through `kubectl` that runs what is typed, `f` forwarding a service
  port that answers, `o`/`x` on the forward, `n`/`c`, `e` on the Docker heading switching to the
  same engine over SSH (SPEC-0243), `e` on a stopped container refused, Delete twice, Compose down
  with `d`. A container's shell runs through the `idecraft` program, which a test binary is not; it
  is tested live in `crates/containers/tests/live.rs`. Not covered: the browser a port opens, the
  look of the rows, the comparison with the reference.

## Out of scope
Building images on their own, editing Kubernetes objects and applying manifests, Helm, events,
metrics, Docker over TLS, Profiler and Remote Development (the other parts of F-083). Shell, ports,
Compose and remote engines: SPEC-0240 … SPEC-0243.

## Known deviations from the reference
Kubernetes goes through `kubectl` instead of a built-in API client, so authentication works exactly
as on the command line. Actions are single keys in one window instead of a tree with context menus.
No standing `kubectl proxy` and no copy of the kubeconfig (IMP-0385, IMP-0386); no Docker program needed
and no raw exception text (IMP-0383). The live path is tested against Docker Engine and k3s in Lima VMs; compared with the
reference through its window and IDECraft's MCP tools in OBS-0029, IDECraft's window not yet.

## Gaps found side by side (OBS-0029, v3; planned, not built)
- **G1** Networks and volumes; pulling an image. **G2** Container details: pause/unpause, inspect as JSON, files, processes,
  attach, environment with values of secret-looking names masked until revealed, never in MCP output or logs (IMP-0384).
- **G3** A tree with context menus next to the single keys. **G4** Compose: scale a service, jump to its source.
- **G5** Kubernetes kinds: cron jobs, replica sets, ingress, config maps, secrets (values masked), storage, CRDs, events.
- **G6** Pod and workload: view YAML, describe, save a log; ephemeral debug containers. Telepresence and Dev Containers
  need their own decision.
