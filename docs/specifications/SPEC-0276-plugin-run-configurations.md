# SPEC-0276: Run configurations suggested by plugins

- **Version:** 3 · **Date:** 2026-11-13 (v3: the folder of a suggestion is not reached through a
  symbolic link, behaviour 3, AK-9; v2: Run and Debug in the chooser act on its highlight)
- **Sources:** GEN (IDE plugins commonly add run configuration types for further tools — a task
  runner, a test tool, a deploy script — that fill in program, arguments and folder from the
  project; F-080), STD (POSIX `execve`: a program and an argument list, no shell), the project owner's decision
  on PO-STATUS question 16 (2026-11-04: suggest and confirm), SPEC-0047, SPEC-0053, SPEC-0087,
  SPEC-0104, SPEC-0156, SPEC-0210, SPEC-0211, ADR-0016 (amendment 2026-11-04), ADR-0008
- **Feature matrix rows:** F-080, F-077

## Purpose
Plugins can contribute run configuration types without being able to start a program themselves: a
plugin only suggests a command, IDECraft shows exactly what would run, and only the user's
confirmation starts it.

## Behaviour
1. **Capability:** a plugin that contributes run configuration types asks for the permission
   `suggest-commands` in its manifest. A manifest with an action of kind `run-config` that does not
   ask for it is refused on loading. The install review (SPEC-0211 behaviour 2) lists the
   permission as "suggests commands to run"; the user grants it there before installing (Space or a
   click on the permission ticks it; Install grants what is ticked) or later in Settings › Plugins
   (Space grants or withdraws the selected plugin's permissions). Updating never grants it
   (SPEC-0211 behaviour 4). Withdrawing it, or removing the plugin, also forgets every "always allow"
   of that plugin (behaviour 7) in every workspace. [GEN, IMP-0277]
2. **Host API:** an action of kind `run-config` is one run configuration type; its `title` is the
   type's name. It gets `{project, active}` (the project folder's name and the active file relative
   to it, if any) and answers `{configs: [{name, program, args, cwd, env}]}` (`args` defaults to
   none, `cwd` to the project folder, `env` to none). It runs in a fresh instance with a fifth of an
   action's instruction budget, gets no new host functions, and may read the project only as granted
   for `project-read` (SPEC-0156). IDECraft runs it only for enabled plugins that were granted
   `suggest-commands`; it is not offered as a command. [ADR-0016]
3. **Checks:** the host takes at most 50 suggestions per type and drops each suggestion that breaks
   a rule, naming why: a name of 1 to 80 characters, unique within its type; a program that is not
   empty and not a command interpreter (`sh`, `bash`, `zsh`, `dash`, `ksh`, `fish`, `csh`, `tcsh`,
   `cmd`, `powershell`, `pwsh`, also with `.exe`); at most 256 arguments of at most 8 KiB each; no
   control characters in name, program, arguments, folder or environment; a folder relative to the
   project without `..`; at most 64 environment variables whose names are made of letters, digits
   and `_`, do not start with a digit, and do not start with `DYLD_` or `LD_`. [GEN, IMP-0277]
   **v3 (decision of the coordinator of 2026-11-13 under the Project Owner's general authority,
   security first):** a folder that is relative and without `..` could still lead out of the
   project through a symbolic link. The folder is resolved with the walk that `read_file` uses
   (SPEC-0156 behaviour 7 check 4): a symbolic link anywhere on the way, the folder itself
   included, refuses it, also a link that stays inside the project; so does a folder that resolves
   outside the project or is not a folder. This is checked when the suggestions are listed (the
   suggestion is dropped and named with the reason; a folder that does not exist yet stays listed)
   and again when the command starts, for Run and for Debug: a link put there after the
   confirmation, or after "always allow", starts nothing, and the run reports `the folder … is
   reached through a symbolic link`. A missing folder is reported the same way. Configurations of
   the user's own `run.toml` are not affected. Between this check and the start of the process a
   program could still swap the folder; the command and folder the user confirmed do not change.
4. **Run dialog:** the run configuration chooser (SPEC-0087 behaviour 2) lists the suggestions after
   the configurations of `run.toml` and those detected from build files, under the heading
   "From plugin: <plugin name> › <type>"; each is named `<plugin name>: <name>`, shows its command
   and a plugin tag, and "always allowed" when behaviour 7 applies. Selecting one selects it in the
   run widget like any configuration. [GEN]
5. **Confirmation:** running a suggestion (Run, the run widget, Enter in the chooser) that is not
   always allowed opens a confirmation instead of starting it. The confirmation names the plugin and
   the type and shows the exact program, each argument on its own line as it will be passed, the
   folder as an absolute path, and the added environment variables. It offers Cancel (selected
   first, and Escape), Run Once, and Always Allow in This Workspace; ↑/↓ or Tab move, Enter
   chooses. Debug refuses a suggestion with a message: plugin configurations can only be run. With the
   chooser open, Run and Debug (⌃R, ⌃D) act on its highlighted entry, not on the configuration
   selected before (hand test 194, D53). [GEN]
6. **Starting:** a confirmed suggestion starts like a configuration of SPEC-0047 behaviour 5 with
   its output in the Run tool window: the program with the arguments as a list, without a shell and
   without replacing `${…}` variables (SPEC-0047 behaviour 3 does not apply), in its folder, with
   IDECraft's environment plus the suggested variables, in its own process group, with the default
   timeout. [STD, GEN]
7. **Always allow:** kept in the user's own settings folder (as the agent rights of SPEC-0053
   behaviour 2), file `plugin-run-permissions.toml`, as `[[workspace]]` tables with `path` and
   `[[workspace.allow]]` entries holding plugin file, type, name, program, arguments, folder and
   environment. Paths are compared after resolving links and `..`; a file inside the workspace is
   ignored; a malformed file allows nothing. An entry allows exactly the stored command: when the
   plugin later suggests anything else under the same name, the confirmation is shown again.
   [IMP-0028, IMP-0277]
8. **Revoking:** in the chooser, Delete or Backspace on an always-allowed suggestion (or a click on
   its "always allowed" tag) forgets it for this workspace; behaviour 1 forgets all of a plugin's
   entries. [GEN]
9. **MCP:** `plugin_run_configurations` lists each plugin's run configuration types, whether
   `suggest-commands` is granted, and — when granted — the checked suggestions with exact program,
   arguments, folder, environment and whether they are always allowed in this workspace, plus the
   problems of behaviour 3. `run_plugin_configuration` (`plugin`, `type`, `name`, `dry_run` default
   true, `timeout`) returns the exact command in a dry run; a real run needs the grant of behaviour
   1, the agent right `run` (SPEC-0053), and the user's "always allow" for exactly this command in
   this workspace — an agent cannot confirm in the user's place, so without it the run is refused
   with a message saying how the user allows it. A real run is logged in `.idecraft/agent-log.jsonl`
   and answers like `run_command`. `revoke_plugin_run_permission` (`plugin`, optional `type` and
   `name`) forgets matching entries of this workspace. No tool grants or allows anything.
   [ADR-0008, SPEC-0053]

## Acceptance criteria
- **AK-1** Runtime with the sample plugin `tests/plugins/runconfig.wat` (WebAssembly text and built
  module): manifest and permission, suggestions checked (interpreter, `..`, absolute folder,
  `DYLD_`/`LD_` and malformed variable names, control characters, duplicate names, limits), a
  manifest with a `run-config` action but no `suggest-commands` refused, grants dropped on update
  and on removal (`crates/plugins/tests/samples.rs`: `run_config_suggestions`,
  `run_config_needs_the_permission`).
- **AK-2** Always allow: stored per workspace, only the identical command allowed, another
  workspace not allowed, a file inside the workspace and a malformed file allow nothing, revoking
  by name and by plugin (`crates/plugins/src/run_configs.rs` unit tests).
- **AK-3** Starting: the argument list runs as given, `${workspace}` is not replaced, the folder and
  the environment apply (`crates/core/tests/it/run.rs`: `verbatim_configuration_is_not_expanded`).
- **AK-4** Run dialog model: chooser entries and the confirmation's lines, choices and keys
  (`crates/workbench/src/plugin_run_model.rs` unit tests).
- **AK-5** MCP: listing with and without grant, dry run, refusals without grant, without `run` and
  without "always allow", a real run with all three and its log line, revoking
  (`crates/mcp/tests/it/protocol.rs`: `plugin_run_configurations`).
- **AK-9 (v3)** The folder: `.` and a folder of the project are taken; `..`, an absolute path, a
  missing folder and a file are refused; a link to a folder outside, a folder below such a link
  and a link to a folder inside are refused as reached through a symbolic link
  (`own_sources::tests::a_folder_for_a_command_is_inside_and_behind_no_link`). A suggestion whose
  folder is such a link is dropped from the list and named, and listed again once the folder is a
  real one (`crates/plugins/tests/samples.rs`: `run_config_suggestions`). A command of a plugin
  does not start in such a folder or in a missing one and says why, starts in a real folder of the
  project, and a configuration of `run.toml` may still name a link
  (`crates/core/tests/it/run.rs`: `verbatim_configuration_does_not_start_through_a_link`).
- **AK-6** Hand test (round 5, no. 194): install the sample plugin, grant it in the review, run a
  suggestion once, allow it always, revoke it.

## Out of scope
Debugging plugin configurations, before-launch chains for them, plugins starting or stopping
programs themselves, plugin settings pages, writing suggestions into `run.toml`.

## Known deviations from the reference
The reference's plugins add run configuration types with JVM code that runs inside the IDE and can
start any program, through a shell if it likes, without the user seeing the command line first.
IDECraft's plugins can only suggest a command; IDECraft checks it, shows the exact argument list and
starts it only after the user's confirmation, and "always allow" holds only for the identical
command (IMP-0277).
