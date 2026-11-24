# SPEC-0047: Run configurations

- **Version:** 7 · **Date:** 2026-11-19 (v7: `run_configuration` starts the reference IDE's configurations that
  SPEC-0049 translates, by name as the window does, and they stay read only for agents, behaviour 18, AK-17; gap RC1
  of OBS-0109, L4 of OBS-0085; no open pull request changed this spec when v7 was written); 2026-11-19 (v6: a configuration may send the requests of a `.http` file with
  Ferris IDEA's HTTP client instead of starting a program, behaviour 17, AK-16; gap O8 of F-084 in OBS-0071; no open
  pull request changed this spec when v6 was written); 2026-11-17 (v5: `run_configurations` also lists the reference IDE's
  configurations that SPEC-0049 translates, with their origin, behaviour 16, AK-15; F-004 L4 of OBS-0085);
  2026-11-14 (v4: templates per configuration type, operations on folders, "Open
  browser" may wait for a port; v3: folders, several instances, before-launch tasks "Build", "Run external
  tool" and "Open browser"; v2: starting through MCP with SPEC-0053)
- **Sources:** GEN (IDEs keep named run and test configurations with command, arguments, working
  directory, environment and before-launch tasks, and show output, exit code and test results; F-077),
  STD (process exit codes, environment variables; URL schemes, RFC 3986), SPEC-0045 (test reports), SPEC-0002
  (settings in TOML), OBS-0032 (v3, side by side with the reference: its dialog has a toolbar entry that puts
  configurations into folders, a switch that allows several instances, and before-launch tasks; the list of those
  tasks could not be opened there and is named from general knowledge: build, run another configuration, external
  tool, npm script, open browser; folders were not evaluated), the parity note of F-077 in
  `docs/research/PARITY.md` (worse than the reference: no folders, no several instances, before-launch tasks only
  as configurations), SPEC-0100 (Build Project), SPEC-0278 (trust); v6: SPEC-0046 (HTTP client), SPEC-0102 (environments),
  SPEC-0053 (the `http` right), OBS-0061 (a run of a request in the reference also makes a run configuration of
  it; every response is written as a file into the project)
- **Feature matrix rows:** F-077

## Purpose
Start the programs and tests of a project the same way every time, from the IDE or an agent.

## Behaviour
1. Configurations live in `.ferris-idea/run.toml` as `[[run]]` tables: `name`, `command` (program and
   arguments), optional `cwd` (workspace-relative, default the root), `env` (table), `kind` (`run` or
   `test`), `report` (`libtest`, `tap`, `junit`; for tests), `report_file` (workspace-relative JUnit file
   written by the tool, read after the run), `before` (names of configurations to run first) and
   `timeout` in seconds (default 600). The file is our own format; the reference's run configuration files
   are not read. [GEN, own format]
2. Loading validates the file: names are unique and non-empty, commands non-empty, `before` names exist,
   `before` chains have no cycles, `cwd` and `report_file` stay inside the workspace. Every problem is
   reported with the configuration's name. [GEN, improvement IMP-0023]
3. In `command`, `cwd`, `env` values and `report_file`, `${workspace}` is replaced by the workspace root and
   `${env:NAME}` by the environment variable (empty if unset). [GEN]
4. Running a configuration first runs its `before` configurations in order (each at most once per run); if
   one fails (non-zero exit, timeout or start error) the chain stops and the configuration is not started.
   [GEN]
5. A run captures stdout and stderr separately, the exit code, whether the timeout killed the process, and
   the duration in milliseconds. A program runs in its own process group; a timeout ends the whole group, so
   child processes (e.g. test binaries started by a build tool) cannot keep the run alive. For
   `kind = "test"` the report is read from the output (or from `report_file`) with SPEC-0045 and attached.
   [GEN]
6. MCP tool `run_configurations` listing the configurations and validation problems; MCP tool
   `run_configuration` running one (dry run by default) under the `run` right of SPEC-0053. [ADR-0008]

7. **v3 – Further keys of a `[[run]]` table**, all optional, so a file written before v3 loads unchanged and a
   configuration without them is written without them:
   - `folder` (text): the folder the configuration is listed in (SPEC-0327 v2 behaviour 2). Folders have one level
     and exist only as this key: a folder is there as long as a configuration names it. The name is not empty, has
     no blank at its start or end and no control character.
   - `allow_multiple` (true or false, default false): behaviour 10.
   - `before_launch`: an ordered list of tasks that are not configurations, each an inline table with exactly one
     of `build = "project"`, `tool = [program, arguments…]` (with an optional workspace-relative `cwd`) and
     `browser = "URL"`. `before` keeps naming configurations.
   Loading reports with the configuration's name: a task with none or several of the three keys, a `build` other
   than `"project"`, a `tool` without a program, a `cwd` that leaves the workspace or stands without `tool`, and a
   URL that behaviour 9 refuses. Exchange files (`.idcrun`, SPEC-0303) carry none of the three keys: an export
   leaves them out and says so, and an import refuses a file with `before_launch`, so an import starts no program
   and opens no page that its preview does not name. [OBS-0032 names the three features; the keys and their checks
   are our own design]
8. **v3 – Order of a run:** before a configuration its `before` configurations run (behaviour 4, each with its own
   tasks before it), then its `build` and `tool` tasks in the order written, then the configuration. A failing
   step stops the chain as in behaviour 4.
   - **Build** runs what the project model knows as the project's build: the build steps of Build Project
     (SPEC-0100 behaviour 18: Maven, Gradle, Cargo, Go, with the build tool window's options) and, for every
     package with a script named `build`, that script with the package's package manager. A run builds once, also
     when several of its configurations ask for it. A project without any of these cannot start the
     configuration and says so.
   - **Run external tool** runs the program with its arguments in `cwd` (default the root) as its own step, with
     the variables of behaviour 3, the default timeout and its own lines in the output. No shell is involved.
   Each of these steps is a process started from a project file and is started only in a trusted project
   (SPEC-0278 behaviour 5), like the configuration itself. [GEN; trust: SPEC-0278]
9. **v3 – Open browser:** once the configuration that was started has started its program, each `browser` URL of
   that configuration is handed to the system's opener (`open`, `xdg-open`; the URL as the only argument, no
   shell) and the output says which URL was opened; a URL that is not opened says why and the run goes on. The
   `browser` tasks of configurations run before it are not opened. Because the URL comes from a project file:
   - Only `http://` and `https://` URLs with a host and without a user name, and `file://${workspace}/…` URLs of
     `.html` or `.htm` files inside the workspace, are opened. Every other scheme is refused when loading, in the
     form and again before opening — an application's own scheme, `javascript:`, `data:`, a `file:` URL of an
     application, a script or anything outside the workspace would start a program or leave the project.
   - The scheme is written out; `${workspace}` and `${env:NAME}` are replaced only after the check, and the
     result is checked again (no blank, no control character, still a plain host). In a `file:` URL the
     workspace's path is percent-encoded and no other variable is allowed.
   - Nothing is opened in an untrusted project (safe mode), and nothing on the host of a remote project
     (SPEC-0318), whose output says so instead.
   [GEN for the task; the limits are our own, security first]
10. **v3 – Instances:** a configuration runs once at a time unless `allow_multiple` is true. Starting it while it
    runs asks: **Stop and Rerun** stops the running instance (as Stop, SPEC-0087 behaviour 5) and starts it again;
    **Cancel** leaves it running and starts nothing. With `allow_multiple` a further instance starts beside the
    running ones, in the window in a Run tab of its own titled with the name and a number (`web (2)`); a tab whose
    run ended is used again. Rerun in a Run tab always restarts that tab's own run without asking. OBS-0032 records
    that the reference has the switch, not what a second start does there; this rule is our design and replaces
    the silent stop of SPEC-0087 behaviour 3 for starts other than a tab's Rerun. For agents: `run_configuration`
    refuses to start a configuration that runs and allows one instance, naming the two ways on (`if_running =
    "stop_and_rerun"`, or `allow_multiple`), and never stops a run unasked. [OBS-0032; rule: own design]
11. **v3 – MCP:** `run_configurations` lists `folder`, `allow_multiple` and `before_launch` with the other keys;
    `run_configuration` shows in its dry run the task steps in order, the URLs that would be opened (`opens`) and
    how many instances run (`running`, `second_start`), and reports after a run what "Open browser" did.
    [ADR-0008]

12. **v4 – Templates per type:** `run.toml` may hold one table `[template.<type>]` per configuration type of
    SPEC-0327 behaviour 3 (`command`, `shell_script`, `node_file`, `npm_script`, `python_file`, `cargo`, `maven`,
    `gradle`, `go_test`, `pytest`). It holds the defaults a new configuration of that type starts from, all
    optional: `interpreter` (the program used in place of the type's own, e.g. a certain `node` or the project's
    Python), `options` (arguments placed right after that program, before the type's own), `cwd`, `env`, `before`,
    `before_launch`, `allow_multiple` and `timeout`, each as in a `[[run]]` table.
    - Creating a configuration of a type **copies** the template: the type's command with the interpreter and the
      options put in, and the other keys as they are. The copy is an ordinary configuration; a later change of the
      template does not reach it, and a template never changes a configuration that exists.
    - A template is read only when a configuration is created. It starts nothing by itself, and a problem of a
      template never keeps a configuration from running: template problems are reported apart from the problems
      of behaviour 2.
    - Problems, each naming the template: a type that does not exist, an unknown key, an interpreter that is
      empty or has a control character, an interpreter or options for the type `command` (it has no program),
      a `cwd` that leaves the workspace, an environment name that is none, a timeout of 0, a `before` name that
      is no configuration of the file, and a task that behaviour 7 refuses (a refused URL included). A template
      with problems is not written.
    - A file without `[template.…]` tables reads as before and is written back unchanged; a template that says
      nothing is not written. The writer of SPEC-0327 keeps the comments of a template's table and touches only
      the tables of the types it was asked to change. Exchange files (`.idcrun`) carry no templates.
    - Templates are kept in the project only. [OBS-0032: the reference's dialog has an entry for templates, not
      evaluated there; GEN: IDEs keep a template per configuration type from which new configurations are
      copied; the keys, the checks and the storage are our own design]
13. **v4 – Open browser may wait for a port:** a `browser` task may carry `wait_port` (1–65535). The page is then
    opened once something on this machine accepts connections on that port (loopback addresses only; the wait
    never contacts another host), checked from the moment the configuration's program has started. If the
    program ends first, or 60 s pass, the page is not opened and the output says which of the two; the run goes
    on either way. `wait_port` on a task that is no `browser` task, and a port of 0, are problems of behaviour 7.
    The URL rules of behaviour 9 are unchanged: the wait changes when a page is opened, never which. A dry run
    names the pages that wait and their ports (`opens_after_port`). [GEN: a browser opened before a development
    server listens shows an error page; the limits are our own]
14. **v4 – Operations on folders:** folders stay the `folder` key of the configurations (behaviour 7), so there is
    nothing new in the file. Three operations work on that key for the dialog and for agents: put a configuration
    into a folder or take it out; rename a folder (every configuration in it names the new one; an empty name, a
    name behaviour 7 refuses and the name of another folder are refused); remove a folder (its configurations
    lose the key and are listed on their own, none is removed). [OBS-0032: the reference's dialog has a toolbar
    entry for folders, not evaluated; operations: own design]
15. **v4 – MCP:** `run_configurations` also lists `folders` (in list order), `types` (the configuration types
    with their command), `templates` (per type that has one) and `template_problems`. Changing folders and
    templates: SPEC-0327 v3 behaviour 17. [ADR-0008]
16. **v5 – MCP, the reference's configurations:** `run_configurations` also lists `idea_configs`: the run
    configurations SPEC-0049 reads from the reference IDE's project files (`.idea/runConfigurations/*.xml`,
    the `RunManager` part of `.idea/workspace.xml`, or an `.ipr` and its `.iws`), in its order. Each has
    `origin` `idea`, name, type, source file and folder, and either the translation (kind, command, working
    folder and the *names* of its environment variables) or the reason it was not translated, as
    `idea_project` lists them (SPEC-0049 v3 behaviour 16). `shadowed` is true when a configuration of
    `run.toml` has the same name; the run list then shows only that one (SPEC-0049 behaviour 13). Environment
    values are never returned, since they may hold secrets. `configs` stays `run.toml` alone, since it is what
    the editing tools change; nothing is written. [ADR-0008, CLAUDE.md rule 12]
17. **v6 – HTTP request configurations:** a configuration with the table `http` sends requests of a request file
    (SPEC-0046) with Ferris IDEA's HTTP client instead of starting a program. Keys: `file` (the `.http` or `.rest`
    file, workspace-relative), optional `request` (a request's name, or its number in the file counted from 1;
    without it every request of the file is sent, in file order) and optional `environment` (an environment of
    the environment files of SPEC-0102 found for that file). `command` stays empty (it may be left out), and
    `url` and `kind = "test"` are not allowed beside `http`; a file that leaves the workspace, another file
    type, an empty `request` or `environment`, and each of these combinations is a problem of behaviour 2
    naming the configuration. The rest of the configuration (`before`, `before_launch`, `folder`,
    `allow_multiple`, safe mode, Stop) works as for every configuration. A run:
    - reads the file when it starts, never earlier; a file that is missing, lies outside the workspace or is
      reached through a symbolic link that leads outside, a `request` the file does not have, and an
      `environment` the environment files do not define end the run before anything is sent, with the reason;
    - chooses the environment as SPEC-0102 v5 behaviour 4 does: the named one, else the only one, else none;
      with several environments, none named and a request that needs a variable of one, nothing is sent and
      the output names the environments to choose from;
    - sends each request as the HTTP client does (SPEC-0046: its `# @timeout` and `# @no-redirect` settings,
      body files only from inside the workspace, the redirect rules, response handlers and response targets
      never run or written, each as a note); a request with unresolved names is not sent and the output says
      which names; the next request is still tried;
    - writes to its output, per request, its name (or its number), the request line, the status line with the
      time and the size, `client error` or `server error` for a status of 400 and above, the response headers
      and the body (indented when it is JSON; a body that is no text as its size; at most 1 MiB of body text,
      with a note when cut). Values from the private environment file (SPEC-0102 behaviour 5) and the user
      information of URLs never appear in the output: each private value is written as `{{name}}`, also in
      the body;
    - ends with exit code 0 when every request was sent and answered, whatever the status, and 1 when a
      request was not sent, could not connect or timed out. Stop takes effect before the next request; the
      request being sent ends with its own timeout. No response, cookie or history is written into the
      project (IMP-0506).
    A dry run (`run_configuration`) shows the step with its `http` table and as command the words
    `HTTP request`, file, request and environment. Through MCP, starting a chain with such a step needs the
    `http` right (SPEC-0053 behaviour 1) on top of `run`, both asked before anything starts; the agent log
    keeps the run as for every configuration. A configuration with `http` cannot be debugged.
    `run_configurations` lists the table like every key. [SPEC-0046, SPEC-0102, SPEC-0053; own design]
18. **v7 – MCP, starting and protecting the reference's configurations:** an agent sees and starts what the window's
    run list holds from the project files (SPEC-0049 v8 behaviour 13), the configurations detected from build files
    aside.
    - `run_configurations` marks each entry of `idea_configs` with `read_only` true; `source` stays the project file
      it was read from (e.g. `.idea/runConfigurations/Main.xml`).
    - `run_configuration` looks a name up in `run.toml` first and then among the translated configurations of
      `idea_configs`, in their order; a name `run.toml` has too starts the configuration of `run.toml` (as in the
      window, behaviour 16). A configuration of the reference starts exactly as one of `run.toml` does: dry run by
      default, the same chain, the `run` right of SPEC-0053 (and the `http` right for an HTTP request
      configuration), only in a trusted project, one instance unless `if_running` says otherwise, logged in the
      agent log, and `failed_only` works on it. The answer adds `origin` `idea` and `source` for it. A
      configuration of the reference that was not translated is refused with the reason `idea_configs` gives and
      its source; nothing is started. Problems of `run.toml` keep every start refused, as before.
    - Its environment values are passed to the program but never returned: where an answer would show the
      configuration itself (the dry run of `failed_only`), `env` holds the variable names only.
    - `run_configuration_edit` changes `run.toml` alone. An operation that names, as `name`, a configuration that
      only the reference's files hold (`update`, `copy`, `remove`, `move`, `set_folder`) is refused before
      anything is read for writing, with a message that names its source and says that Ferris IDEA does not change
      the reference IDE's project files, and that a configuration of the same name added to `run.toml` takes its
      place in the run list; `add` of such a name stays allowed for that reason. Nothing under `.idea` and no
      `.ipr` or `.iws` is ever written. [ADR-0008; read only: SPEC-0049 v8; security first: no values of the
      environment in answers]

## Acceptance criteria
- **AK-1** Loading and validation: valid file, duplicate name, empty command, unknown and cyclic `before`,
  paths leaving the workspace; variables replaced.
- **AK-2** Running real processes: output, exit code, working directory, environment; timeout kills a
  sleeping process.
- **AK-3** `before` chain runs in order once each and stops at a failure.
- **AK-4** A test configuration attaches a parsed report (TAP from a real process; JUnit from a written
  report file).
- **AK-5** The MCP tool lists configurations and problems.
- **AK-6** v3 – A file written before v3 loads as before and is written back unchanged; a file with `folder`,
  `allow_multiple` and the three kinds of `before_launch` round-trips through loading and the writer of SPEC-0327;
  every malformed task of behaviour 7 is a problem naming the configuration.
- **AK-7** v3 – `build` and `tool` steps run before the configuration in the order written, after its `before`
  configurations; a failing tool stops the chain; a project without a build cannot start a configuration that asks
  for one; a package's `build` script and Cargo's build are found (real processes for the tool, the plan for the
  build).
- **AK-8** v3 – Safe mode: in an untrusted project no tool or build step starts and no URL is opened.
- **AK-9** v3 – Open browser: after the start exactly the configuration's own URLs reach the opener, with variables
  replaced; every refused form of behaviour 9 (other schemes, `file:` outside the workspace or not a page, user
  name, blank, a variable as scheme or host) is refused when loading and never reaches the opener; a host run opens
  nothing.
- **AK-10** v3 – Instances: the rule (start, ask, further instance), tab titles of further instances, the count of
  running instances of this process and stopping them; `run_configuration` refuses a second start, stops and
  reruns on request and starts beside a running one with `allow_multiple`.

- **AK-11** v4 – Folder operations: a configuration is put into a folder and taken out, a folder is renamed and
  removed; an empty or refused name, a folder that is not there and a name that is taken are refused; the file
  keeps its comments and, with every folder removed, is the file it was (unit test).
- **AK-12** v4 – Templates: a new configuration of a type is the type's command with interpreter and options put
  in and a copy of the other keys, and the type's command alone without a template; templates round-trip through
  `run.toml` with comments kept, a file without templates is written back unchanged, a type that is not named
  keeps its table and an empty template removes it; every problem of behaviour 12 names the template, keeps the
  file from being written and leaves the configurations' own problems empty (unit tests).
- **AK-13** v4 – Open browser with `wait_port`: the page reaches the opener only after the port accepts
  connections, and not at all when the program ends first, with the reason in the output; the key round-trips
  through the writer and the task line; the dry run names page and port (real processes and a real listener; the
  opener is a recorder).
- **AK-14** v4 – In a real window (`ferris-idea ui --selftest`, groups `run-config-instances` and
  `run-config-before-launch`): a second start asks, Cancel keeps the running instance, Stop and Rerun replaces
  it; with `allow_multiple` a second tab `name (2)` runs beside the first; an external tool runs before the
  configuration and the page is handed to the opener after its start; a failing tool stops the launch and the
  output names the step.
- **AK-15** v5 – MCP (`crates/mcp/tests/it/protocol.rs`: `run_configurations_lists_the_idea_ones`): with
  `.idea/runConfigurations`, a personal configuration in `workspace.xml` and a `run.toml`, `configs` holds
  `run.toml` only; `idea_configs` holds the shared and the personal configuration with origin `idea`, source,
  command and the environment variable's name, an untranslated one with its reason, and `shadowed` for the
  one `run.toml` names too; the environment value appears nowhere in the answer.
- **AK-16** v6 – HTTP request configurations (`crates/core/tests/it/run_http.rs`, a server on 127.0.0.1): a
  configuration with `http` loads and every malformed form of behaviour 17 is a problem naming it; a run sends
  one request by name, by number and all requests of the file, with the named environment, and its output holds
  request line, status, headers and the indented body with the private value written as `{{name}}`; with two
  environments and none named nothing is sent and the output names both; an unresolved name, a missing request,
  an unknown environment and a file reached through a link to outside are refused; a status of 404 ends with exit
  code 0, a refused connection with 1; Stop before the start sends nothing; the streamed run reports start and
  output lines; the dry run shows the `http` table. MCP (`crates/mcp/tests/it/protocol.rs`:
  `an_http_run_configuration_needs_the_http_right`): `run_configuration` refuses without the `http` right and
  sends with it.
- **AK-17** v7 – Starting the reference's configurations: in `crates/core/tests/it/ideaproject.rs`
  (`startable_configurations_add_the_idea_ones`, `idea_origin_and_read_only_refusal`) the startable list is
  `run.toml` followed by the translated configurations not shadowed, an untranslated one is found with its reason
  and source, and the refusal names the source; MCP (`crates/mcp/tests/it/protocol.rs`:
  `run_configuration_starts_an_idea_one`) with a temporary project holding `.idea/runConfigurations/Main.xml`
  (Application, plain `.idea` module, an environment variable): `run_configurations` lists it with `source` and
  `read_only`; the dry run of `run_configuration` names it with `origin` and `source` and no environment value;
  without the `run` right it is refused; with the right it compiles and runs the class and its output holds the
  program's line; `run_configuration_edit` refuses `remove` and `update` of it with the source in the message
  and leaves the files as they were.

## Out of scope
Running inside the terminal view (SPEC-0038 offers the PTY), debug launches (SPEC-0039), compound
configurations running in parallel, importing the reference's run
configurations into `run.toml`. v6: the HTTP client's response view, cookies, response handlers and
saved responses for runs of HTTP request configurations (the output is the run's result); choosing the
environment when the run starts. v7: starting the configurations detected from build files through MCP
`run_configuration` (the window lists them; agents read them from `idea_project` and the project model) and
changing or importing a reference configuration through `run_configuration_edit`. v4: templates as a user default outside the project (the layered settings of SPEC-0002 hold
single values by key, not tables with lists; and commands that do not come from the project file would need their
own rule for when they are shown — a later step with the Project Owner's decision); a template for temporary
configurations made by Run Current File and for detected configurations; re-applying a changed template to
existing configurations; waiting for a port on another host or for an HTTP answer. Live output in the window: SPEC-0087. v3: nested folders; a before-launch task per build module or
build tool (the task builds the project); waiting for a server to answer before the browser opens; opening the
browser of a remote project's run on the user's machine; the instance rule for runs on a remote project's host and
for debug sessions.

## Known deviations from the reference
Own TOML format instead of the reference's XML. Compared side by side in OBS-0032; the configuration dialog is
SPEC-0327, which writes this file.
- v3: "Open browser" opens only web pages and pages of the project (behaviour 9); the reference's task takes any
  URL. A project file must not be able to start a program through a URL scheme.
- v3: before-launch tasks other than configurations are a second list (`before_launch`) that runs after the
  `before` configurations, so files written before v3 keep their meaning; the reference orders both kinds in one
  list.
- v3: what a second start of a running configuration does is our design (behaviour 10); the reference's behaviour
  is not recorded in OBS-0032.
- v4: templates are part of the project file, readable and changeable by people and agents like the
  configurations; one template form serves every type (the single form of SPEC-0327). The reference's templates
  were not evaluated in OBS-0032, so nothing here is taken from them.
- v4: "Open browser" waits for a local port only; it does not fetch the page to see whether the server answers.
- v6: a run of an HTTP request configuration writes nothing into the project: no response files and no history
  (IMP-0506; OBS-0061: the reference saves every response as a file in the project). Private environment values
  are masked in the run's output. Its result is the output of the run, not a separate tree of requests.
