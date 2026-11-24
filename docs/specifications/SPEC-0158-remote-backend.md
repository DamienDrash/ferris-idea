# SPEC-0158: Ferris IDEA's backend on a remote host

- **Version:** 4 · **Date:** 2026-11-16 (v2 2026-11-03: symbols, problems and run configurations
  through the backend, behaviours 7–9; v3 2026-11-07: post-quantum key exchange required on the SSH
  channel, ADR-0025 v3; v4: behaviour 10, Enter on the listed results; behaviour 4, the header keeps
  its buttons in view; AK-6, hand tests 84 and 184 with real keys)
- **Sources:** GEN (remote development: the IDE's backend runs on the machine that holds the
  code — a server, a VM, a build box — while the window runs locally, so indexing, search,
  inspections and builds happen where the files are; F-083 "Remote"), STD (OpenSSH client as in
  SPEC-0121; MCP over standard input/output: JSON-RPC `initialize`, `tools/list`, `tools/call`),
  SPEC-0121, SPEC-0053, ADR-0008
- **Feature matrix rows:** F-083

## Purpose
Work with a project on another machine through Ferris IDEA's own analysis there, instead of
copying the project or browsing file by file.

## Behaviour
1. **Backend:** the backend is the headless `ferris-idea mcp <folder>` started on the host through
   the system `ssh` with the options of SPEC-0121 (batch mode, strict host keys, no passwords,
   hybrid post-quantum key exchange only; a host without one is refused, ADR-0025 v3).
   The IDE talks to it over the SSH channel; it ends when the IDE leaves the host or closes. [GEN,
   STD]
2. **Finding or bringing Ferris IDEA:** a Ferris IDEA of the same version uploaded earlier by this build
   (`~/.cache/ferris-idea/bin/ferris-idea-<version>-<build>`) or an `ferris-idea` on the host's `PATH` that
   reports the same version is used. Otherwise, when the host has the same system and processor
   (`uname -sm`), this build is uploaded there through a temporary file, made executable and renamed
   into place. A host with another platform is refused with what to install ("the host runs Linux
   x86_64, this Ferris IDEA is built for Darwin arm64; install Ferris IDEA … there"). [GEN]
3. **The backend's rights** on the host are those of the host's project (its own agent permissions,
   SPEC-0053); it reads and runs nothing more than the local backend would. [SPEC-0053]
4. **Window:** in the Remote Host window (SPEC-0121) "Start Backend" starts it for the folder shown;
   the header then shows "Backend: ferris-idea <version> in <folder>", its help line names `?text`,
   `@symbol`, `!` and `>`, and the status bar says how it was installed ("uploaded" or "found on
   the host") and how many tools it has. Leaving the host stops it. **v4:** when the header is too
   narrow, the host's folder, the backend line and the help line are shortened, never the
   buttons: they stay in view. Before, a long folder pushed Start Backend and Delete out of the
   window (found by the re-check of hand test 184 on 2026-11-16). [GEN]
5. **Search on the host:** `?text` and Enter in the field searches the host's project through the
   backend (`search_in_files` there); the hits are listed as `file:line  text`; Enter opens the
   hit's file (SPEC-0121 behaviour 4) on its line; Escape returns to the folder. [GEN]
6. **MCP:** `remote_backend_start` (`dry_run` default true: the platforms and the plan;
   dry_run=false needs the user-granted `ssh` and `run` rights and is logged),
   `remote_backend_call` (any tool of the backend, needs `ssh`, logged without arguments) and
   `remote_backend_stop`. Symbols (`search_everywhere`), problems (`file_diagnostics`) and run
   configurations (`run_configurations`, `run_configuration`) of the host's project reach agents
   through `remote_backend_call`; running one there needs the host project's `run` right.
   [ADR-0008, SPEC-0053]
7. **Symbols on the host (indexing):** `@name` and Enter in the field finds symbols and files of
   the host's project by name through the backend's index (`search_everywhere` there, camel humps
   work); hits read `name  kind  file:line`; Enter opens the file at the symbol's line. [GEN]
8. **Problems on the host (language servers):** when a host file inside the backend's folder is
   opened or uploaded, the backend's language server checks it on the host (`file_diagnostics`
   with the path relative to the folder); the status bar says "N problems on the host (! lists
   them)" or why no server could check it; `!` and Enter lists them as `file:line  severity
   message`, Enter opens the line. [GEN]
9. **Run configurations on the host:** `>` and Enter lists the run configurations of the host's
   project (`run_configurations` there) with their commands; Enter on one asks the backend for its
   chain (`run_configuration` dry run: before-launch configurations, expanded commands, folders)
   and runs that chain on the host in a new SSH terminal tab (SPEC-0121 behaviour 6): each step
   `cd`s into its folder, sets its environment and runs; a failing step stops the chain. The tab
   is named after the configuration. Running there is the user's own action, as with a local
   run. [GEN]
10. **v4 – Enter on the listed results:** while the field still holds the query that listed the
   rows shown (`?text`, `@name`, `!`, `>`), or is empty, Enter acts on the selected row: it opens a
   hit or a problem at its line, or runs a configuration (behaviour 9). ↑ and ↓ choose the row from
   the field, and a double click on a row does the same as Enter. Any other text in the field is a
   new query, a path or a host. Before, Enter with the query still in the field asked the backend
   again and selected the first row, so no hit could be opened and no configuration started with
   the keyboard (found by the re-check of hand tests 84 and 184 on 2026-11-16). [GEN]

## Acceptance criteria
- **AK-1** Platform names (`arm64` = `aarch64`), paths, build fingerprint (`backend::tests`).
- **AK-2** Against a real `sshd` started as the user: this binary is uploaded, found again the
  second time, started for a folder; its server name and tools are read, `search_in_files` finds a
  file there, an unknown tool is an error (`crates/app/tests/it/remote_backend.rs`).
- **AK-3** MCP: the plan without rights, refusal without `ssh`/`run`, a call without a backend
  (`remote_backend_tools_plan_and_refuse`).
- **AK-2b** Against the same `sshd`: the backend finds a symbol with `search_everywhere`, lists and
  plans a run configuration, and answers `file_diagnostics` for a file (problems or the missing
  server) (`crates/app/tests/it/remote_backend.rs`).
- **AK-4** Window self-test: Start Backend on the test host, `?remote` lists `notes.txt:1`, Enter
  opens it; `@` finds a function of a host file and Enter opens its line; `>` lists the host's run
  configuration and Enter opens an SSH terminal tab that runs it; the status bar reports the
  host-side check of an opened file.
- **AK-5** Hand test (round 4, no. 84): a Linux VM or server with Ferris IDEA installed, and a Mac
  over SSH with upload; search and agent calls next to the reference's remote development.
- **AK-6 (v4)** Hand tests 84 and 184 in a window of the test harness with real keys and mouse
  clicks, against the system `sshd` started as the current user with a home folder of the test's
  own, and this build's `ferris-idea` as the backend (`crates/app/tests/it/f083_handtests.rs`;
  skipped without `sshd`). `hand_test_84_backend_on_a_host_with_keys`: a folder typed into the
  field with Enter, Start Backend clicked uploads this build ("uploaded" in the status bar);
  `?text` and Enter list the hits as `file:line  text`, ↓ and Enter open the second hit at its
  line, a double click the first; Hosts ends the backend on the host; an `ferris-idea` on the host's
  `PATH` is used ("found on the host") and nothing is uploaded; on a Linux host without Ferris IDEA
  (the Docker VM of the reference environment, when `FERRIS_IDEA_TEST_DOCKER_SSH` is set) the error
  names both platforms and what to install. `hand_test_84_an_agent_inspects_the_host_project`: `ferris-idea mcp` plans
  with `remote_backend_start`, starts it with the `ssh` and `run` rights, `remote_backend_call`
  runs `inspect_code` there, `remote_backend_stop` ends it; start and call are logged, the call
  without its arguments. `hand_test_184_symbols_problems_and_runs_on_the_host_with_keys`: `@name`
  and Enter list the function with kind and line, Enter opens it there; the status bar reports
  the host's `clangd` problems in the file (or why no server ran); `!` and Enter list them, Enter
  opens one at its line; `>` and Enter list the configurations, ↓ and Enter run one with its
  before-launch step in an SSH terminal tab named after it, ending with "Process finished with
  exit code 0".

## Out of scope
Completion, navigation and refactoring in the local editor through the backend (the editor works
on the local copy; symbols, problems and runs come from the host), debugging on the host, port
forwarding, a thin client, installing for another
platform, Windows hosts.

## Known deviations from the reference
The window stays a full local IDE that edits copies of the host's files (SPEC-0121); the backend
adds Ferris IDEA's tools on the host instead of moving the whole IDE there.
