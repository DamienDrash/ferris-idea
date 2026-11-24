# SPEC-0119: Docker and Kubernetes

- **Version:** 6 · **Date:** 2026-11-23 (v6: text fields take the committed text of the text input, gap D6 of the OBS-0116 window addendum; images pulled from a field with progress, digest and a warning for untagged or `latest` references, removed with Delete twice and refused while used, image details with the containers that use them, MCP `docker_image`; gap D3 of OBS-0116; v5: live updates from the engine's events, followed and searched logs with earlier runs, container details with secret-looking values masked, MCP `docker_container inspect` and logs with `since`/`tail`/a pattern; gaps D1, D2 and D5 of the side-by-side OBS-0116; v4: AK-7, hand tests 46 and 170–173 with real keys against the reference environment's engine and cluster; v3: gaps and deviations from the side-by-side OBS-0029; v2, 2026-11-03: shell inside containers SPEC-0240, ports and port forwarding SPEC-0241, Compose SPEC-0242, engines and remote engines SPEC-0243; live tests against a real engine and cluster)
- **Sources:** GEN (IDEs list the containers and images of a Docker daemon and the workloads of a
  Kubernetes cluster, start, stop and restart them and show their logs; F-083), STD (Docker Engine
  API: `/version`, `/containers/json`, `/images/json`, `/containers/{id}/start|stop|restart|logs`,
  `DELETE /containers/{id}`, the multiplexed log stream with 8-byte frame headers; v5: `/events`
  as a chunked stream of JSON lines and with `until` as history, `/containers/{id}/logs` with
  `follow`, `since`, `tail` and `timestamps`, `/containers/{id}/json`; RFC 3339 times; v6:
  `POST /images/create?fromImage=&tag=` (progress as JSON lines, `errorDetail`),
  `/images/{name}/json`, `DELETE /images/{name}`, image references (repository, tag, digest); HTTP/1.1 with
  chunked bodies; `DOCKER_HOST`; `kubectl config view`, `get … -o json`, `logs`,
  `rollout restart`, `delete pod`; Kubernetes object status fields), SPEC-0053 (agent `run` right
  and log), ADR-0008; OBS-0116 (gaps D1, D2, D3, D5), IMP-0384, IMP-0532
- **Feature matrix rows:** F-083

## Purpose
See and control the containers and workloads a project runs in, and read their output, without
leaving the IDE.

## Behaviour
1. **Docker daemon:** `DOCKER_HOST` (`unix://…` or `tcp://…`) when set, else the first socket
   that exists of Docker Desktop (`~/.docker/run/docker.sock`), Colima, OrbStack, Rancher
   Desktop, Podman machine and `/var/run/docker.sock`. Ferris IDEA speaks the Engine API itself; no
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
   user-granted `run` right and are logged in `.ferris-idea/agent-log.jsonl`. [ADR-0008, SPEC-0053]

9. **Live updates (v5, D1):** while the window is shown it follows the engine's event stream
   (containers, images, networks). A container or image created, started, stopped or removed
   outside Ferris IDEA shows within 1 s without F5: events of one burst lead to one reading of
   Docker (not of Kubernetes) about 0.1 s after the first; exec and health events do not. The
   selection stays on the row of its container (or the old place when the container is gone). When
   the stream ends or the engine cannot be reached, a note under the Docker heading says so with
   the reason and the time to the next try; Ferris IDEA tries again after 0.5 s, doubling the pause
   up to 10 s, waiting without polling. Once connected again the note goes and everything of
   Docker is read anew. Hiding the window ends the stream. [STD]
10. **Logs (v5, D2):** Enter on a container shows its output in the side pane and follows it
    (`follow` with time stamps; up to 20 000 lines kept). Scrolling up (↑, Page Up, Home, the
    wheel or the trackpad) stops following; End jumps to the end and follows again; the status
    line says which. The lines are sorted into runs by the container's start times (the
    engine's event history and inspect); earlier runs are folded under a heading with their
    start and line count, `p` (or a click on a heading) unfolds them. When the followed container
    starts again, a new run begins and following continues after the last line seen (lines up to
    its time stamp are not repeated). Search: `/` or ⌘F, typed text, ⌥C upper/lower case, ⌥W
    whole words, ⌥X regular expression (linear-time engine, size limited; an invalid expression is
    named), ⌥L only matching lines; the status line counts the matches; Enter and ⇧Enter go to the
    next and previous match (and stop following); Escape ends the search. Tab hands the keys to
    the pane and back. Kubernetes and Compose service logs show in the same pane, not followed.
    [STD, GEN]
11. **Details (v5, D5):** `i` on a container shows its details in the side pane: name, image,
    state and start, command, environment, ports with the address a browser opens (a click or
    `o` opens it), mounts (kind, source, destination, rw/ro, mode), labels, networks; `j`
    switches to the whole inspect answer as JSON; the search of behaviour 10 works on both.
    Moving the selection to another container shows its details. Values of secret-looking names
    (password, secret, token, API key, private or access key, credential, auth, DSN, …, as whole
    words or name parts), URL passwords (`scheme://user:password@`) and values of secret-looking
    command-line options are replaced by `••••••` before they are shown or returned, also in the
    JSON; there is no way to reveal them in Ferris IDEA and they are never logged (IMP-0384). [STD]
12. **MCP (v5):** `docker_container` `inspect` returns the details of behaviour 11, masked the
    same way; `logs` takes `tail`, `since` (Unix seconds, RFC 3339, or `30s`/`10m`/`2h`/`1d`
    back from now) and `pattern` with `case_sensitive`, `whole_word` and `regex`, and then returns
    only the matching lines and their match count. Both only read and need no right. [ADR-0008]
13. **Images (v6, D3):** image rows show tags, short id, size and age. `p` (on the Images heading
    or an image) opens a field for a full reference; Enter pulls it, Escape cancels. A reference
    without a tag or with `latest` is named as one that can change, before and with the pull;
    references are checked (no spaces, options or query characters) and sent encoded. The pane
    shows the pull's progress (layer states without the moving bars) and ends with the digest and
    the engine's status; a registry's refusal is shown as it is. Delete pressed twice on an image
    removes it (no force); between the presses the header names the image. While a container
    uses the image, nothing is asked of the engine and the message names the containers and their
    state. Enter or `i` on an image shows its details in the pane: id, tags, digests, size,
    platform, creation time and age, and the containers that use it. [STD]
13a. **Typed text (v6, D6):** the window's fields (the log and details search, the pull field) take
    the text the platform's text input commits, as the editor does, not the key behind an event:
    synthetic input (computer use, whose key events carry the key code of `a`), input methods and
    dictation type what they mean, a whole committed word at once. Keys without text (Enter, ⌫,
    Escape, Tab, the arrows) and ⌥ toggles stay keys. [GEN]
14. **MCP `docker_image` (v6, D3):** `pull` (with the warning of behaviour 13 and the digest),
    `remove` (refused while a container uses the image) and `inspect` (the details of behaviour
    13). `pull` and `remove` default to `dry_run: true` and only say what would happen;
    `dry_run: false` needs the user-granted `run` right and is logged in
    `.ferris-idea/agent-log.jsonl`, like `docker_container`. [ADR-0008, SPEC-0053]

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
  (`FERRIS_IDEA_TEST_DOCKER`, `FERRIS_IDEA_TEST_KUBECONFIG`; set up by `containers.sh` of the reference environment):
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
  with `d`. A container's shell runs through the `ferris-idea` program, which a test binary is not; it
  is tested live in `crates/containers/tests/live.rs`. Not covered: the browser a port opens, the
  look of the rows, the comparison with the reference.

- **AK-8 (v5)** Event stream: parsing, which events matter, reconnect pauses (`events::tests`,
  `stream::tests`); against the fake engine on a real socket, a container created and removed
  outside reaches a watcher within 1 s, the socket going away is reported with growing pauses,
  and after it is back the watcher connects again (`crates/containers/tests/containers.rs`:
  `events_reach_the_watcher_and_it_reconnects`).
- **AK-9 (v5)** Logs: frames in pieces, times, runs, search and filter (`logs::tests`); followed
  output, a restart as a new run, `since`/`tail`, stopping a quiet follow
  (`logs_follow_runs_and_queries`); the pane's keys (`containers_pane::tests`).
- **AK-10 (v5)** Details and masking (`secrets::tests`, `details::tests`,
  `container_details_are_masked`, `containers_pane::tests`).
- **AK-11 (v5)** MCP `inspect` masked, logs with `tail`, `since` and a pattern, an invalid
  expression refused (`crates/mcp/tests/it/protocol.rs`: `container_tools`).
- **AK-12 (v5)** Window test with real keys against the fake engine
  (`crates/workbench/tests/it/containers_live_f083.rs`): a container created, stopped and removed
  outside shows without F5 while the selection stays; the engine going away shows the note and
  coming back clears it; Enter follows a new line, ↑ pauses, End follows, a restart folds the
  earlier run, `/` with ⌥C/⌥L counts and filters; `i` shows the details with the password
  masked, `j` the JSON, `o` opens the port.

- **AK-13 (v6)** Images: references, warnings, progress lines, users (`images::tests`); against the
  fake engine a pull with progress, digest and the `latest` warning, a registry's refusal, details
  with the containers that use the image, removing refused while used without asking the engine,
  removing an unused image (`crates/containers/tests/containers.rs`: `images_pull_details_and_remove`).
- **AK-14 (v6)** MCP `docker_image`: previews with the warning and the users, refusal without
  `run`, an invalid reference and an unknown action refused, inspect, pull and remove with `run`,
  removing a used image refused, the log entry (`crates/mcp/tests/it/protocol.rs`:
  `docker_image_tool`).
- **AK-15 (v6)** Window test with real keys against the fake engine
  (`crates/workbench/tests/it/containers_live_f083.rs`: `images_and_compose_groups_with_real_keys`):
  `p` opens the field, Escape cancels, Enter pulls with progress and digest; Enter on an image shows
  its details with the containers using it; Delete twice on a used image is refused with their
  names, on an unused one removes it. D6: the pull field and the log search take synthetic key
  events (key `a`, real text) and a word committed at once as the text they mean
  (`synthetic_text`, `commit_text`).

## Out of scope
Building images on their own, editing Kubernetes objects and applying manifests, Helm, events,
metrics, Docker over TLS, Profiler and Remote Development (the other parts of F-083). Shell, ports,
Compose and remote engines: SPEC-0240 … SPEC-0243.

## Known deviations from the reference
v5: secret-looking values are masked in the details and the JSON and cannot be revealed (the
reference shows them, OBS-0116 S2, IMP-0384); events from an SSH engine (SPEC-0243) end only with
the next event after the window is hidden, as `ssh` gives no read timeout.

Kubernetes goes through `kubectl` instead of a built-in API client, so authentication works exactly
as on the command line. Actions are single keys in one window instead of a tree with context menus.
No standing `kubectl proxy` and no copy of the kubeconfig (IMP-0385, IMP-0386); no Docker program needed
and no raw exception text (IMP-0383). The live path is tested against Docker Engine and k3s in Lima VMs; compared with the
reference through its window and Ferris IDEA's MCP tools in OBS-0029, Ferris IDEA's window not yet.

## Gaps found side by side (OBS-0029, v3; planned, not built)
- **G1** Networks and volumes; pulling an image. **G2** Container details: pause/unpause, files, processes, attach
  (details with environment, inspect JSON and masking: v5 behaviour 11).
- **G3** A tree with context menus next to the single keys. **G4** Compose: scale a service, jump to its source.
- **G5** Kubernetes kinds: cron jobs, replica sets, ingress, config maps, secrets (values masked), storage, CRDs, events.
- **G6** Pod and workload: view YAML, describe, save a log; ephemeral debug containers. Telepresence and Dev Containers
  need their own decision.

## Gaps found side by side (OBS-0116, v5)
D1 (live updates), D2 (logs) and D5 (details) are behaviours 9–12, D3 (images) behaviours 13–14
(v6), D4 (Compose grouping) SPEC-0242 v4 behaviours 7–8. Removing an image names it between the two
presses and says who uses it instead of the reference's delayed question without a name (IMP-0532).
