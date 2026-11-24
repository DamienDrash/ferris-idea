# SPEC-0242: Docker Compose

- **Version:** 4 · **Date:** 2026-11-23 (v4: each line of the CLI's output once, gap D7 of the OBS-0116 window addendum; containers nested under their service with the project network, Compose projects of other folders grouped by their labels; gap D4 of OBS-0116; v1 2026-11-03; v2: the CLI's own `ssh` gets the
  post-quantum key exchange, ADR-0025 v3; v3: the stand-in holds every jump host to it, ADR-0025 v5)
- **Sources:** GEN (IDEs detect a project's Compose file, show its services with their containers
  and run up, down and logs per service; F-083), STD (Compose specification: default file names
  `compose.yaml`, `compose.yml`, `docker-compose.yaml`, `docker-compose.yml` and their `.override`
  files, top-level `name` and `services`, project name rules (lowercase letters, digits, `_` and `-`,
  starting with a letter or digit), container labels `com.docker.compose.project` and
  `com.docker.compose.service`, v4 also `com.docker.compose.project.working_dir`, the project network
  `<project>_default`; the Compose CLI `up --detach`, `down`, `rm --stop --force`,
  `--project-directory`, `-f`, `-p`; YAML block mappings), SPEC-0119, SPEC-0053, ADR-0008
- **Feature matrix rows:** F-083

## Purpose
Run a project's Compose services from the IDE and see which of them run, next to the containers.

## Behaviour
1. **Detection:** the project folder's first file of `compose.yaml`, `compose.yml`,
   `docker-compose.yaml`, `docker-compose.yml` is the Compose file, together with its override file
   (`compose.override.yaml` … in the same order) when present. [STD]
2. **Project and services:** the project name is the file's top-level `name`, else the folder name
   made valid (lowercase, other characters dropped). Services are the keys of the top-level
   `services` mapping in file order, read by Ferris IDEA itself (block style; a file it cannot read
   shows why). [STD]
3. **In the window:** a `Compose: <project> (<file>)` group in the Docker part lists each service
   with the state of its containers (by the two Compose labels): `running`, `n/m running`, `exited`
   or `not created`. [GEN]
4. **Actions:** on the project row `u` runs `up --detach` for all services and `d` runs `down`
   (containers and networks removed, volumes and images kept); on a service row `u` runs
   `up --detach <service>` and `d` runs `rm --stop --force <service>`. Enter on a service shows the
   last 200 lines of its containers' output from the Engine API, each line prefixed with the
   container name when there are several. The Docker list is read again afterwards. [STD]
5. **Compose CLI:** up and down use `docker compose` when the Docker CLI has the Compose plugin,
   else the standalone `docker-compose`, with `--project-directory <project> -f <files> -p <name>` and
   `DOCKER_HOST` set to the engine shown; they run in the background (at most 10 minutes) and their
   last output line goes to the status line. Without either CLI the window says which to install;
   listing services and logs work without it. For an `ssh://` engine the CLI starts `ssh` from
   `PATH` itself; Ferris IDEA puts a private folder (0700) first on its `PATH` whose `ssh` runs the
   system `ssh` with the shared options of SPEC-0121 before the CLI's own arguments, so Compose too
   connects only with a post-quantum key exchange (ADR-0025 v3). **v3:** the stand-in resolves the
   host with `ssh -G`, routes a `ProxyJump` through Ferris IDEA's hop stand-in (every hop post-quantum),
   refuses a `ProxyCommand` of the user's own unless allowed (SPEC-0121 v4 behaviour 1a) and never
   reuses a shared connection the user opened by hand. [STD, ADR-0025]
6. **MCP:** `docker_compose` with `action` `services` (files, project, services with states),
   `logs` (`service`, `tail`), `up` or `down` (optional `service`). `up`/`down` default to
   `dry_run: true` and return the command; for real they need the `run` right and are logged in
   `.ferris-idea/agent-log.jsonl`. [ADR-0008, SPEC-0053]

7. **Grouping (v4, D4):** a container of the folder's Compose project shows under the row of its
   service, indented, and not again in the flat container list (which keeps only containers outside
   any Compose project); containers of a service the file no longer names show under the project
   after its services. The project's networks (from its containers, without `bridge`, `host`,
   `none`) follow as `network <name>` rows. All container keys (`s`, `x`, `r`, Enter, `e`, `o`,
   `i`, Delete twice) work on nested rows as on flat ones; the selection keeps its container when it
   moves between flat and nested. [GEN, STD]
8. **Projects of other folders (v4, D4):** containers whose project label names another project are
   grouped the same way under `Compose: <project> (<working_dir>, read-only)`, services sorted, with
   a state per service from its containers. `s`/`x`/`r` and the other container keys work on their
   containers; `u`/`d` on such a group or service only say that up and down are for the project
   folder's Compose project. [GEN]

9. **Output (v4, D7):** the CLI's stdout and stderr are read line by line in the order they
   arrive; a line equal to the line before it is dropped (Compose repeats progress lines such as
   `Network x Creating` without a terminal), so `up`/`down` over MCP and in the status line show each
   line once. [STD]

## Acceptance criteria
- **AK-1** Unit: file detection order with overrides, project names, service keys from block YAML
  (comments, quoting, nested keys, flow style refused), states from labels, command lines
  (`compose::tests`).
- **AK-2** Against the fake Engine API and a fake Compose CLI: services with states, logs of a
  service, up/down command lines with `DOCKER_HOST` (`crates/containers/tests/containers.rs`).
- **AK-3** Live against a real engine with `docker-compose` (`FERRIS_IDEA_TEST_DOCKER`): `up` of a
  two-service project creates running containers with the labels, logs of one service, `rm` of one
  service, `down` removes the rest (`crates/containers/tests/live.rs`).
- **AK-4** MCP: services, previews, refusal without `run`, up and down with `run`, log entries
  (`crates/mcp/tests/it/protocol.rs`: `compose_tools`).
- **AK-5** Window self-test: the self-test project's `compose.yaml` shows its services; `u` on the
  project runs the fake CLI; Enter on a service shows its logs.
- **AK-6** Hand test 172.
- **AK-9 (v4)** Window test with real keys against the fake engine
  (`crates/workbench/tests/it/containers_live_f083.rs`: `images_and_compose_groups_with_real_keys`):
  the folder's project nests its container under its service with its network and leaves only the
  other container flat; a project created outside from another folder is grouped by its labels,
  `u` on it is refused with the reason, `x` stops its container and the service state follows;
  labels and network of such containers against the fake engine
  (`crates/containers/tests/containers.rs`: `images_pull_details_and_remove`); the window self-test
  shows the nested row (`containers_steps`).
- **AK-10 (v4)** D7: `compose::tests::each_line_once`; the fake Compose CLI repeats a progress line
  and `run` returns it once (`compose_services_states_and_commands`), as does MCP `docker_compose up`
  (`compose_tools`).
- **AK-7** SEC-17 (ADR-0025 v3): for an `ssh://` engine the fake Compose CLI finds Ferris IDEA's `ssh`
  stand-in first on `PATH`, and it carries `KexAlgorithms` before `"$@"` (`crates/containers/tests/
  containers.rs`, `pq::tests`).
- **AK-8** (v3) The stand-in refuses a classical jump host and a `ProxyCommand` of the user's own and
  reaches the target through two post-quantum hops (`crates/remote/tests/jump.rs`).

## Out of scope
Profiles, several projects per folder, `extends` and `include`, environment interpolation in the
project name, building images on their own, scaling, Compose files outside the project folder.

## Known deviations from the reference
`down` never removes volumes, so database data in named volumes survives (IMP-0113); services are
listed without any CLI. Not compared side by side yet.
