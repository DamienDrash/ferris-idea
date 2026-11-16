# ADR-0016: Plugins as sandboxed WebAssembly modules

- **Status:** accepted · 2026-11-01
- **Context:** The feature matrix plans an own plugin API on WebAssembly (F-080) with no compatibility
  to the reference platform's JVM plugins. Plugins must not weaken the workspace protections of
  ADR-0007/SPEC-0053: a cloned project must not run code by bringing plugins, and a plugin must not
  read files, open connections or hang the IDE.
- **Decision:** New crate `idecraft-plugins` on `wasmi` (MIT/Apache-2.0, a pure-Rust interpreter;
  `wat` for tests). A plugin is one `.wasm` module in the user's IDECraft settings folder
  (`plugins/`, or `IDECRAFT_PLUGINS`); workspaces cannot install plugins. The module imports nothing
  — there are no host functions, so it has no files, network, clock or randomness — and exports
  `memory`, `idecraft_alloc`, `idecraft_manifest` (JSON: name, version, description, actions) and
  `idecraft_run` (action id and input text in, output text out). Every call runs in a fresh instance
  with an instruction budget (fuel) and a memory limit. First capability: actions that replace the
  selection or show a message, offered in Search Everywhere and to agents (`plugins`,
  `run_plugin_action`).
- **Consequences:** Plugins are pure functions over text; anything that needs more (files of the
  project, editor state, inspections, UI) needs explicit, reviewed host functions in a later version
  of this ABI, each with the same sandbox rules. An interpreter is slower than a JIT; the fuel budget
  keeps that bounded. Plugins of the reference platform keep needing a port (F-081, ADR-0006).
- **Amendment 2026-11-03 (SPEC-0156):** the ABI gains two reviewed host functions of module
  `idecraft`, `list_files` and `read_file`, which answer only for the open project's files (relative
  paths, no `..`, links resolved inside the project, 4 MiB at most) and only when the manifest asks
  for `project-read` and the user granted it in Settings › Plugins (kept in `plugins/permissions.json`,
  withdrawn on removal). Plugins never write files; `edit` actions return edits that the IDE applies
  to the open editor as one undo step. Any other import still fails to link.
- **Amendment 2026-11-03 (SPEC-0166):** a third action kind, `inspect`, takes the file
  (`{path, language, text}`) and answers findings with optional quick fixes (character offsets,
  checked by the host). It runs by itself after typing pauses, with a fifth of the instruction
  budget, and gets no new host functions; project access stays as granted for `project-read`.
- **Amendment 2026-11-03 (SPEC-0210, SPEC-0211):** further extension points without new host
  functions: action kinds `complete` (completion items) and `status` (status bar text) with a fifth of
  the budget, `panel` (rows of the Plugins tool window), and declarative `file_types` in the manifest
  (highlighting and line comments done by IDECraft, no plugin code runs). The sandbox model is
  unchanged. Run configuration types stay out: starting programs would need a new, reviewed
  capability and is an open decision. Installing and updating show a review of contributions and
  permissions first; an update never grants a permission the old version did not ask for.
- **Amendment 2026-11-04 (SPEC-0276, PO-STATUS question 16):** run configuration types come in through
  a reviewed capability "suggest commands", decided by the product owner on 2026-11-04. A plugin
  that asks for the permission `suggest-commands` (granted by the user at install or later in
  Settings › Plugins) may declare `run-config` actions that answer suggested commands as data —
  program, argument list, folder relative to the project, optional environment and a name. The
  plugin itself still starts nothing and gets no new host function. IDECraft checks each suggestion
  (no command interpreters, no `..`, no loader variables, no control characters, limits), shows the
  exact program, arguments and folder in the Run dialog and starts it only after the user confirms,
  as an argument list without a shell and without variable expansion. "Always allow in this
  workspace" is kept outside the workspace in the user's settings folder, like the agent rights of
  SPEC-0053, holds only for the identical command and can be revoked; agents can list, dry-run and
  revoke, and run only what the user has allowed, with the `run` right.
- **Amendment 2026-11-05 (ADR-0024, SPEC-0314, PO-STATUS question 25):** a narrow network capability for
  connector plugins, decided by the project owner on 2026-11-05 with a warning for the user and for agents. A
  plugin with the permission `profile-network` gets host functions `net_connect`, `net_send`,
  `net_recv` and `net_close`. They reach only the host and port of the connection profile the user
  picked, and only after a grant per plugin and host (holder user or agent, stored in the settings
  folder, revocable). Every connection is logged locally. All other network access stays impossible.
- **Amendment 2026-11-13 (SPEC-0156 v3, security first):** what `project-read` reaches is narrowed to
  the threat it was meant for. A granted plugin is still untrusted code; the grant lets it help with
  the sources, not collect what lies beside them. `read_file` and the `open` request take only a file
  of the project's own sources (SPEC-0148 behaviour 7: nothing Git ignores, no data of version
  control or of an IDE, no installed packages or build output, nothing behind a symbolic link or
  outside the project), and files whose names commonly hold secrets (environment files, keys and key
  stores, credential files of tools) are refused even when Git tracks them. The answer does not
  tell a plugin whether a file it may not read exists. Every refusal is logged locally in
  `plugins/file-audit.jsonl` without file contents. There is no option; an exception per plugin and
  workspace would be a decision of the Project Owner.
- **Amendment 2026-11-13 (SPEC-0156 v4, SPEC-0148 v5, SPEC-0276 v3; decided by the coordinator
  under the Project Owner's general authority, security first):** the rule of the amendment above
  also governs what the host hands to a plugin unasked. A plugin's `inspect`, `status` and
  `complete` actions are not run by themselves on a file the plugin could not read with
  `project-read` (a name that commonly holds secrets, or not one of the project's own sources), and
  such a file is not named to `panel` and `run-config` actions; an action the user picks on such a
  file asks first, naming the file and the plugin, with "Cancel" as the default. Agents get the
  same refusal from `run_plugin_action` and `plugin_inspections` unless the call states the intent
  with `allow_secret_file`; a confirmed or stated hand-over is logged in `file-audit.jsonl`. The
  own-sources rule judges a path by the names it has on disk and matches its fixed folder names in
  any case, so that another spelling on a file system that ignores case reaches nothing the exact
  spelling would not. A file is read from a handle opened without following a link and verified to
  be the file that was checked. The folder of a suggested command is refused when a symbolic link
  leads to it, when it is listed and when it starts. None of this has an option.
