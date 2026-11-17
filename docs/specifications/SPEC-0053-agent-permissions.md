# SPEC-0053: Agents running programs and sending requests

- **Version:** 10 · **Date:** 2026-11-16 (v10: no generic tool dispatcher and no agent code in the IDE process, named as deviations, IMP-0447, IMP-0448, OBS-0071; v9: Deny holds the keyboard focus of the question, so
  Space denies as Return does, behaviour 8, SPEC-0281 v3 behaviour 8 — with keyboard navigation
  on, Space pressed "Always for This Workspace", found by the pre-check of hand test 202, rule 12;
  v8: right `settings` for the user's own settings, every tool with a changing effect needs a right and a test holds that for new tools, database permissions move into the grant file — security review 2026-11-14, rule 12, ADR-0031, behaviours 12–14, and the table of behaviour 11 carries the rights, the line of behaviour 11 the database decisions; v7: every call of a tool that changes something leaves a line in the agent log, written at one place for all tools, behaviour 11 — finding of 2026-11-14, rule 12; v6: right `secret-read` for files that commonly hold secrets and
  the private values of the HTTP client — security note from PR 667, rule 12; v5: the logged URL of a request carries no user information and no value of the private environment file, behaviour 5; v2: right `remote` for Git fetch, pull and push, SPEC-0055; v3: right `ssh` for SSH hosts, SPEC-0121; v4: rights `write`, `vcs-write` and `quit` (READINESS A3, IMP-0002), refusals say "not granted", the window asks clients over HTTP, behaviours 6–9)
- **Sources:** GEN (IDE agent integrations let a model start commands and run configurations, usually behind
  a confirmation; F-084), OSS (`intellij-community/plugins/mcp-server`: terminal command and run
  configuration tools with a per-command confirmation — logic template, no code), the project owner's decision on
  PO-STATUS question 3 (2026-10-31), READINESS A3 (decided 2026-11-02: `write` and `vcs-write` in the
  classical phase), OBS-0004 (missing permissions reported as "rejected by user" without asking →
  IMP-0007), SPEC-0046, SPEC-0047, SPEC-0006, SPEC-0221, security review of 2026-11-14 (finding A2:
  `set_shortcut` and `git_branch_favorite` changed files without a right), ADR-0031, SPEC-0012 v3
- **Feature matrix rows:** F-084

## Purpose
Let AI agents build, test, call APIs and change a project once the user has said so, without letting a
project grant that to itself, keep every such action traceable, and ask the user in the window instead of
refusing silently when a client of the running window lacks a right.

## Behaviour
1. Rights: `run` (start programs), `http` (send HTTP requests), `debug` (start debug sessions,
   SPEC-0054), `remote` (fetch, pull and push Git remotes, SPEC-0055), `ssh` (list, read and write files
   on SSH hosts; running commands there also needs `run`, SPEC-0121), `trust` (SPEC-0278), and `write`,
   `vcs-write` and `quit` (behaviour 6), `secret-read` (behaviour 10), `settings` (behaviour 12),
   and `terminal-read`, `ports`, `ui` and `recent-projects` of their own specifications (SPEC-0313,
   SPEC-0318, SPEC-0319, SPEC-0094 v9). Reading tools, dry runs and previews need no right, except for the text of files
   whose names commonly hold secrets (behaviour 10). Database connections have their own four
   permissions per connection (SPEC-0012 v3), kept in the same grant file. [GEN, READINESS A3]
2. Rights are granted per workspace in the user's own settings folder, file `agent-permissions.toml`
   (`$IDECRAFT_CONFIG_DIR`, else `$XDG_CONFIG_HOME/idecraft`, else `~/.config/idecraft`), as
   `[[workspace]]` tables with `path` and `allow`, and `[[connection]]` tables for database
   permissions (SPEC-0012 v3 behaviour 1). Paths are compared after resolving links and `..`. A
   grant file inside the workspace is ignored, a malformed file or an unknown right grants nothing and the
   problem is reported. No MCP tool writes this file; only the user's own answer "Always for This
   Workspace" in the window does (behaviour 8). [improvement IMP-0028]
3. Tools with side effects default to `dry_run = true` and then show exactly what would run or be sent.
   Without the right a real run is refused with a message that starts with `not granted:`, names the
   right, the workspace, the grant file and the exact entry to add, and says that nobody was asked. It
   never says the user rejected or denied anything when the user was not asked. Nothing runs and nothing
   is logged. [GEN, IMP-0028, IMP-0007]
4. Tools: `run_configuration` (a configuration of SPEC-0047 with its before-launch chain; the dry run
   lists the chain with variables replaced), `run_command` (program and arguments without a shell,
   working directory inside the workspace, timeout default 120 s, at most 600 s, process group ended on
   timeout), `send_http_request` (a request of a `.http` file by name or number; unresolved names are never
   sent), `agent_permissions` (the granted rights, the file consulted and any problem). Output texts are
   capped at 256 KiB each and marked when cut. [GEN]
5. Every real run or request appends one JSON line to `.idecraft/agent-log.jsonl` in the workspace: time,
   tool, command or configuration chain with exit codes and timeouts, or method, URL, status and error —
   never request headers or bodies. The logged URL and error text of a request carry no user
   information (`user:password@` is left out) and show `{{name}}` in place of every value that comes
   from the private environment file (SPEC-0102 behaviour 5), because the log is a file in the
   project. If the log cannot be written, the tool reports it. [improvement IMP-0028]
6. Rights for changing things (READINESS A3). Each right is checked once, in the step that really
   changes something, for every tool that exposes the capability (IMP-0007):
   - `write`: creating, changing, moving or deleting files of the workspace, including IDECraft's project
     settings under `.idecraft/` and `.idea/`: `create_file`, `rename_file`, `delete_file`, `copy_file`,
     `move_file`, `format_file`, `restore_local_revision`, `apply_code_action`, `undo_refactoring`,
     `rename_symbol`, `change_signature`, `optimize_imports`, `generate_code`, `fix_inspection`,
     `update_copyright`, `resource_bundle`, `project_dictionary` (add), `eslint_file` and `prettier_file`
     (writing), `python_select_interpreter`, `rename_sql_object` (file edits), `er_diagram` (a written
     image), `tool_windows` (move, stripes), `set_breakpoint`, `set_special_breakpoints`,
     `save_connection`, `remove_connection`, `sql_console`, `sql_file_data_source` (setting a binding).
     Not covered: files IDECraft keeps by itself while working (agent log, local history, undo groups)
     and the user's settings outside the workspace.
   - `vcs-write`: operations that change the repository or the working tree through version control:
     `git_commit`, `git_create_branch`, `git_switch_branch`, `git_merge`, `git_stash` (push, apply, pop,
     drop), `git_cherry_pick`, `git_rebase`, `git_resolve_conflict`, `git_pull` and `git_push` (both also
     `remote`). A reset tool, when one is added, needs it too. Fetching needs only `remote`.
   - `quit`: quitting the window through MCP while files are unsaved or programs run (SPEC-0261 v2);
     saving the files on the way also needs `write`.
   [GEN, READINESS A3, IMP-0002]
7. Over the stdio transport (`idecraft mcp`, no window) a missing right is refused as in behaviour 3.
   Over the window's HTTP transport (SPEC-0221) the window asks the user instead (behaviour 8); the
   refusal of behaviour 3 is returned only when no window can ask or nobody answers within 2 minutes,
   and then it says so. [GEN, IMP-0007]
8. The question is a native alert of the project's window (else the window in front), one question at a
   time. It names the client (the `clientInfo` name and version the client sent at `initialize`), the
   right and what it allows, the action (the tool and its arguments, cut to 300 characters) and the
   scope (the workspace path). Answers: "Deny" (the default, Return), "Allow Once" (this call only) and
   "Always for This Workspace" (adds a `[[workspace]]` entry with that right for the workspace to the
   user's grant file, and allows it for the rest of the session). **v9:** Deny holds the keyboard
   focus when the question appears — the question names Deny as its safe answer, the one that
   grants nothing (SPEC-0281 v3 behaviour 8); Deny is also its default — so with keyboard
   navigation on Space denies as Return does; Allow Once and Always
   need the mouse, VoiceOver or Tab first, and a stray key never grants a right.
   [GEN, IMP-0007, IMP-0005, rule 12]
9. "Deny" refuses the call with a message starting with `denied:` that says the user denied it in the
   window; that MCP session is not asked again for that right and gets the same refusal. A new session
   asks again. [GEN]
10. **v6 – Secrets:** a file whose name commonly holds secrets — the deny list of SPEC-0156 v5
    behaviour 8 (`.env`, `.env.*`, keys and key stores, SSH identities, credential files of tools
    such as `.npmrc`, `credentials.json`, `http-client.private.env.json`, Terraform state), judged on
    the path relative to the workspace root — is handed to a tool only with the right `secret-read`
    ("read files that commonly hold secrets (.env, keys, credentials) and the private values of the
    HTTP client"). The check is made once for every tool call, before the tool runs, on its
    arguments `path`, `paths`, `left`, `right`, `from`, `to` and `files`, so it holds for every tool
    that returns a file's text — `read_file`, `local_history` and `restore_local_revision`,
    `git_show`, `git_diff`, `compare_files`, previews and dry runs of edits, fixes and formatting,
    `remote_read` on SSH hosts by the same names — and for copying, moving or renaming such a file
    under another name. Only `git_commit`, `git_rollback`, `delete_file` and the path filter of
    `git_log`, which return no text and make no copy, are not checked. Without the right the call is
    refused before anything happens, with a message starting with `withheld:` that names the file,
    its family and how to grant the right (behaviours 3, 7 to 9: the window asks a client over HTTP).
    Results over many files leave such files out and list them in `withheld` with a note:
    `search_in_files`, `replace_in_files_preview`, the text lines of `search_everywhere`, and
    `git_conflicts` gives their path and state but no texts. The private values of the HTTP client
    are masked (SPEC-0102 v3 behaviour 5). Every refusal, leaving out and masking is a line in the
    agent log with the tool, the paths or names and the right — never a value. The window's own
    editor and the user are not affected. [GEN, rule 12, IMP-0007]
11. **v7 – Every change is logged:** every call of a tool that changes something — workspace files,
    the repository, a database or store, settings, rights and grants, the window's state, processes,
    a remote host, network connections — appends one JSON line to the agent log, refused calls and
    dry runs included. The line is written at one place for every tool, when the call ends, from a
    table that names for each tool what it changes and how it is asked for a dry run (`dry_run` with
    its default, `apply`, or none); a tool that only reads leaves no such line. The line has `time`,
    `tool`, `call: true`, `changes`, `dry_run`, `rights` (each right checked during the call with
    whether it was given), `targets` (the arguments that name what is acted on — paths, names of
    connections, tables, branches, hosts, the scope or action — cut at 200 characters), `lengths`
    (every other text argument by its length in characters, lists and objects by their number of
    items) and `result` (`ok`, `refused` when a right was missing or denied, `error`). It never
    holds file contents, statements, commands, messages or values. The lines a tool writes itself
    (behaviours 5 and 10) stay. If the line cannot be written, a call that succeeded reports it.
    The table lists every tool of the server; a tool missing from it fails the tests, so a new tool
    cannot slip past the log. [rule 12, IMP-0028]
    **v8:** the same table carries the rights each tool checks (behaviour 13), so the log and the
    rights are decided by one row per tool. A tool that uses a database connection or store is
    logged also when it only reads (SPEC-0012 v3 behaviour 16); its line has `database`: one entry
    per decision on the connection's permissions with `connection`, `permissions`, `dry_run`,
    `ok`, for a refusal `refused` (the reason in one word) and for statements `chars` and `kinds`.
    There is one line per call: the decisions are part of it, not lines of their own. When
    `database` has entries, the line's `dry_run` is true only if all of them were previews
    (a reading `run_sql` runs without `apply`), and `result` is `refused` also for a call refused
    in safe mode or for its keychain entry. [rule 12, ADR-0031]

12. **v8 – The user's own settings.** The right `settings` ("change your own IDECraft settings,
    which hold for every project: IDE settings, keymap, theme, plugin grants") is needed by every
    tool that changes a file of the user's settings folder or a choice kept there:
    `set_shortcut`, `themes` with `set`, `set_ide_settings` (the real change; the dry run is free),
    `project_tree` with `compact`, `import_file` when the imported file changes the IDE settings,
    the keymap or the theme (besides `write` for what it changes in the project),
    `import_connector` (the real import: the definition is kept in the settings folder for every
    project), `window_layouts` for `save`, `rename` and `delete` (the named layouts are the user's
    own; `apply` and `restore` write the project's layout and need `write`, and `settings` too
    when they change which layout is chosen), `revoke_plugin_network` and
    `revoke_plugin_run_permission`. Like every right it is granted per
    workspace: agents working in that workspace may then change these settings, and the settings
    hold everywhere. A setting that names a program IDECraft starts later needs the right of that
    effect as well: `set_ide_settings` with `shell` also needs `run`. Settings an agent can never
    change stay refused (`ui.driver.external`, switching the SSH exceptions on). No right fitted
    before v8: the user's settings were out of scope, and the tools wrote them unasked. [rule 12,
    ADR-0031]
13. **v8 – Every changing effect has a right.** Each tool is classified by what its code does:
    reads only; writes project files; changes the repository; changes the user's settings or the
    state of the IDE (windows, tabs, sessions); starts a process; opens a network connection;
    uses a database connection. The classification is the table `TOOLS` in
    `crates/mcp/src/effects.rs` — the table of behaviour 11, one row per tool: what it changes,
    how a dry run is asked for, what it reaches while only reading (a database, a process, the
    network) and the checks its code makes. Where the two classifications differed, a tool
    changes what either of them said it changes. A tool with a changing effect — project files, repository, settings or IDE state,
    a database — checks at least one right or database permission on every such path; a test
    fails for a tool without a row, for a row of a tool that no longer exists, for a changing
    effect without a right, and for a right named in the row that the tool's code does not check.
    Changes of v8 (before → now):
    - `set_shortcut`, `themes` (set), `set_ide_settings`, `project_tree` (compact),
      `revoke_plugin_network`, `revoke_plugin_run_permission`: none → `settings`; `import_file`:
      `write` → `write`, and `settings` for the user's files; with `show` → `ui`;
    - `git_branch_favorite`: none → `write` (it writes `.idecraft/branch-favorites.toml`);
      `python_language_server` with `apply`: none → `write`; `python_create_venv` and
      `run_configuration_edit`: `run` → `run` and `write` (they write `.idecraft/python.toml`,
      `.venv` and `.idecraft/run.toml`); `ui_trace` writing into a workspace folder: `ui` → `ui`
      and `write`; `er_diagram` as PNG: `write` is checked before the target folder is created;
      `export_file` with `path`: the path lies inside the workspace, as for every file an agent
      writes;
    - `editor_tabs` and `project_tree`, every action but `list`: none → `ui`;
    - `quit_ide`: quitting needs `quit` also when nothing is unsaved or running;
      `remote_project_close` without unsaved changes: none → `ssh` for a project this MCP server
      opened, `quit` for a project of a window; `debug_stop`: none → `debug`;
      `remote_backend_stop`: none → `ssh`; `remote_port_close`: none → `ports`;
    - `revoke_project_trust`: none → `trust`;
    - `git_commit_action` with `push-up-to`: `vcs-write` → `vcs-write` and `remote`;
      `import_connector`: `write` → `settings`, and with `url` the download needs `http`, also
      in a dry run; `window_layouts`: `write` → `settings` for the user's named layouts, `write`
      for the project's layout; `remove_connection` removes only IDECraft's own keychain entry
      (SPEC-0012 v3 behaviour 20);
    - `test_connection`, `sql_history`, `cancel_sql`, the store tools and every database tool:
      SPEC-0012 v3.
    Where two readings were possible the stricter was taken: tools that only take something away
    (revoking a grant or trust, stopping, closing) need a right too, because they change the
    user's state. The one exception is `ui_driver_stop`, the emergency stop of the UI driver
    (SPEC-0319 behaviour 37, ADR-0027): it stays free for every client, because a stop that has to
    be granted first is a weaker safety control; the table marks it as that exception. The
    second marked exception is `local_history`: it reads, and what it writes is a record
    IDECraft keeps by itself (behaviour 6); its calls are logged. The test accepts no other. [rule 12, ADR-0031]
14. **v8 – Known limits of the classification.** Tools that read but start a fixed program or
    connect to a service of the user's are classified and need no right: language servers
    (governed by safe mode, SPEC-0278), `ps` (`debug_processes`, `profile_processes`), `ssh -G`
    (`remote_hosts`), `rustup which` (`language_servers`), the sign-in state of the assistant's
    programs (`ai_assistant`), reading the Docker engine and the Kubernetes cluster
    (`docker_status`, `kubernetes_status`, logs and listings of the container tools), and calls
    over an open remote session that only read (`room_status`, `room_links`, `room_history`,
    `remote_ports`). `format_file`, `prettier_file`, `eslint_file` and `optimize_imports` start the
    project's own formatter or linter with trust alone (SPEC-0278), also in a dry run; commit hooks
    run under `vcs-write`. These are recorded in the table and are open for a later decision.
    [GEN]

## Acceptance criteria
- **AK-1** Grant file: per-workspace rights, path matching through `..`, another workspace's grant not
  applied, malformed file and unknown right grant nothing, refusal text names right, file and entry.
- **AK-2** A grant file inside the workspace is ignored; log lines are appended.
- **AK-3** Through MCP without rights: dry run shows the chain without running; real runs of
  `run_configuration` and `run_command` are refused, leave no files and no log.
- **AK-4** With `run`: a chain runs in order with exit code, stdout and stderr; a command runs in a
  sub-folder; a timeout ends a sleeping process; a working directory outside is refused; `http` stays
  refused; three log lines.
- **AK-5** With `http`: unresolved names refused, dry run resolves, a POST reaches a local server and the
  response comes back; the log has method and status but no headers, and neither the user information
  of the URL nor a private environment value.
- **AK-6** Without `write`, the write tools refuse their real write with `not granted:` and change
  nothing while their dry run still works; with `write` the write happens. Without `vcs-write`,
  `git_commit`, `git_create_branch` and `git_stash` push refuse and HEAD, branches and the working tree
  stay; with it they work. `vcs-write`, `write` and `quit` round-trip through the grant file.
- **AK-7** With a window that asks: "Allow Once" runs the call and the next call asks again; "Always for
  This Workspace" runs it, writes the grant entry and is not asked again; "Deny" refuses with `denied:`
  and the session is not asked again. The question names client, right, action and scope. Without a
  window the refusal says `not granted:` and contains neither "rejected" nor "denied".
- **AK-8** Hand test: an MCP client over HTTP calls `create_file` without `write`; the window asks with
  the client's name; each answer. v9: with keyboard navigation on, Space denies and no grant is
  written (SPEC-0281 v3 AK-7, AK-8).
- **AK-9 (v6)** Without `secret-read` over MCP: `read_file` of `.env` and of
  `http-client.private.env.json`, `compare_files` with `.env` on one side, `problem_preview` of
  `.env` and `copy_file` of `.env` to another name are refused with `withheld:` and the right named,
  the value in no answer and no copy made; `search_in_files` gives the match of `notes.txt` and lists
  `.env` in `withheld`, a search for the value finds nothing; the agent log names the refused path,
  the withheld path and the masked name, never the value. With the right `read_file` gives the text
  and the search both matches (`protocol.rs`:
  `secret_files_and_private_http_values_need_the_secret_read_right`).
- **AK-10 (v7)** Over MCP a real `copy_file` with `write` leaves a line with `tool`, `call`, the paths
  in `targets`, `dry_run: false`, the right `write` given and `result: ok`; without `write` the
  refused call leaves a line with the right not given and `result: refused`; a dry run leaves a line
  with `dry_run: true`; `create_file` logs its text by length only; `read_file` leaves no line
  (`protocol.rs`: `every_changing_call_leaves_one_line_in_the_agent_log`).
- **AK-11 (v7)** Ratchet: every tool the server lists has an entry in the effects table and every entry
  names a served tool (`every_tool_is_classified_in_the_effects_table`); for every changing tool the
  line is built for a real and a refused call, with no content (`effects::tests`).

- **AK-12 (v8)** Without `settings`: `set_shortcut`, `themes` with `set` and `set_ide_settings`
  with `dry_run: false` are refused with `not granted:` naming `settings`, and neither the keymap
  file nor the settings file is created or changed; the dry run of `set_ide_settings` and reading
  the themes still work. With `settings` each writes. `set_ide_settings` with `shell` and without
  `run` is refused naming `run`. Without `write`, `git_branch_favorite` is refused and
  `.idecraft/branch-favorites.toml` is not written; with it the mark is kept (`protocol.rs`:
  `settings_and_favorites_need_a_right`).
- **AK-13 (v8)** The ratchet: every tool of `tools/list` has exactly one row in `TOOLS`,
  every row names a tool that exists, every row with a changing effect names a right or database
  permission (`ui_driver_stop` and `local_history` are the two marked exceptions), every tool that uses a
  database checks the connection's permission, and the code of the tool's handler (with the helpers it calls) checks each right the
  row names (`effects.rs`: `every_tool_with_a_changing_effect_checks_a_right`).
- **AK-14 (v8)** `settings` round-trips through the grant file; a `[[connection]]` entry is read
  for its workspace only, an unknown permission in one makes the file grant nothing
  (`crates/core/tests/it/permissions.rs`).

## Out of scope
Per-command allow lists, a right per setting (v8: one right `settings` covers the user's settings), sandboxing of started programs, streaming output while a program runs, asking stdio clients
(they have no window).
Secrets (v6): telling a secret from a setting by the content of a file that is not on the deny list
(`application.properties`, see SPEC-0156); what the window shows of an open secret file through the UI
driver (right `ui`, SPEC-0319) and what programs started with `run` print; values a server sends back
in a response to a request the agent sent with `http`.

## Known deviations from the reference
The reference asks per command in a dialog (or skips asking in a "brave" mode); IDECraft uses standing
per-workspace rights held outside the project plus a log of every action (IMP-0028), and keeps the text of files that commonly hold secrets and the HTTP client's private
values from agents unless the user grants `secret-read` (v6), and asks in the
window only for a right that is missing, naming client, right, action and scope. A client that was not
asked is told the right is not granted, never that the user rejected it (IMP-0007).

v10 (OBS-0071): the reference's MCP server has a tool that runs any of its other tools from a command
string, so a client that allows or denies tools by name is bypassed; IDECraft has no such dispatcher:
every capability is its own tool with its row in the effects table of behaviour 13 and the rights it
checks (IMP-0447). By its description, the reference also compiles and runs a script an agent
supplies inside its own process (custom inspections); IDECraft runs no agent code inside the IDE —
what an agent wants run is a separate process through `run_command`, `terminal_command` or a run
configuration, under the `run` right, in a trusted project, with a dry run first (IMP-0448).
