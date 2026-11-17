# Stability from 1.0

**Status:** proposed · 2026-11-17 · takes effect with the 1.0 release
([CONTENT_STYLE_GUIDE §5](design/CONTENT_STYLE_GUIDE.md#5-project-maturity)). Until then IDECraft is
`0.x`, and nothing on this page is a promise.

This page lists what IDECraft keeps working from 1.0 on: the command line, the MCP tools and their
rights, the HTTP transport, and the files you write or share. It also lists what may change at any
time. It was derived from the code and specifications on `main` at commit `a5398c61`.

## The rules

- **Stable** means: a later 1.x version keeps the name, the arguments and their defaults, the
  meaning, and the shape of the result. A 1.x version may add to a stable interface: a new optional
  argument, a new result field, a new tool, a new subcommand, a new file key. Clients ignore what
  they do not know.
- **Experimental** means: usable, but a minor version may change or remove it. The change is named
  in the [changelog](../CHANGELOG.md).
- **Internal** means: no promise. It may change or disappear in any version without a note.
- **Removing or changing a stable interface incompatibly** needs a new major version. Before that,
  a released minor version marks it deprecated, and it keeps working until the next major version
  and for at least 90 days after that release: the changelog names
  it under *Changed* with the word "Deprecated" and the replacement, and a deprecated MCP tool or
  argument says so in its description.
- **Security fixes may tighten a stable interface** in any version: refuse an input, require a
  right, or narrow a default. The changelog lists such a change under *Security* with what to do.
- Human-readable text is not an interface: messages, hints, refusal wording, `--help`-style usage
  lines and the plain-text output of commands may change. Exit codes and JSON output listed below
  are stable.

## Command line

The `idecraft` binary; the usage line is in `crates/app/src/main.rs`.

### Stable

| Command | What stays | Source |
|---|---|---|
| `idecraft --version` | prints one line whose first two words are `idecraft <version>`; the stage in parentheses follows | `crates/core/src/release.rs` |
| `idecraft ui [<folder> \| <file>] [--trust]` | opens a window on the folder, or on the project of the file; `--trust` trusts that project for this run only | [SPEC-0278](specifications/SPEC-0278-project-trust-and-safe-mode.md) behavior 10, SPEC-0094 |
| `idecraft launch [<folder>]` | hands the folder to a running window, else starts one; also what the app bundle runs | SPEC-0282 |
| `idecraft mcp <folder> [--trust]` | serves that folder over MCP on stdin/stdout | [SPEC-0006](specifications/SPEC-0006-mcp-server.md) |
| `idecraft call <folder> <tool> [<json arguments>]` | one MCP tool call with the rights and safe mode of `idecraft mcp`; result on standard output; exit 0 success, 1 tool refused or failed, 2 wrong arguments or folder | SPEC-0281 behavior 5 |
| `idecraft trust status <folder>`, `add <folder> [--folder]`, `list`, `remove <path>` | the trusted projects and folders in `trusted-projects.toml` | SPEC-0278 |
| `idecraft quit [--save-all \| --discard \| --force] [--pid <pid>]` | quits the running window; exit 0 quit, 1 not quit, 2 pending work and no terminal to ask on | SPEC-0261 v2 behavior 11 |
| `idecraft open <file.idc…>`, `idecraft open --print <file>` | shows an exchange file's import preview; `--print` exits 0 without problems, 1 with problems, 2 when it is not an exchange file | [SPEC-0303](specifications/SPEC-0303-idecraft-file-formats.md) behavior 17 |

Stable environment variables: `IDECRAFT_CONFIG_DIR` (settings folder), `IDECRAFT_LOG_DIR` (log
folder, SPEC-0270), `IDECRAFT_PLUGINS` (plugin folder, SPEC-0104) and `IDECRAFT_TRUST_PROJECTS`
(`all`, or paths separated as in `PATH`, SPEC-0278 behavior 10).

### Experimental

| Command | Why | Source |
|---|---|---|
| `idecraft drive [--project <folder>] [--json] <verb> …` and UI scripts (`kind = "ui-script"`) | the UI driver is Experimental; exit codes 0, 1, 2, 3 as specified | [SPEC-0319](specifications/SPEC-0319-ui-driver.md) behavior 30 |
| `idecraft serve …` (`--stdio`, `--attach`, `--pair`, `--devices`, `--revoke`, `--rights`, `--device-log`, `--access-off`, `--wss…`) | remote projects are Experimental, paired devices In progress | SPEC-0317, SPEC-0321, SPEC-0324, [ADR-0026](adr/0026-session-protocol-window-and-serve.md) |
| `idecraft device pair \| hosts \| forget \| add-path \| call …` | paired devices, In progress | SPEC-0321 |
| `idecraft-serve`, the headless backend binary | started by the window on a host; it is the window's own build | SPEC-0321 behavior 1 |

### Internal

- `idecraft container-exec <engine> <container>`: started by the Containers tool window.
- `idecraft spike-scroll`, `spike-input`, `spike-editor`, `spike-layout`, and `idecraft edit
  <file>`: measurement and prototype windows from before the workbench.
- `idecraft ui --selftest [--list-groups]`: the window self-test
  ([checks and self-test](testing/CHECKS_AND_SELFTEST.md)).
- Launch arguments that macOS adds when it starts the app bundle.

## MCP server

### Stable

- **Server identity:** name `idecraft`, title `IDECraft`, `serverInfo.version` equal to the
  IDECraft version (SPEC-0006 behavior 2).
- **Tools:** every tool that `tools/list` returns and whose description does not say
  "Experimental". Stable are its name, its argument names, types and defaults, and the field names
  and types of its result. The list and each tool's input schema come from `tools/list`; the
  specification of each tool is named in its description.
- **Previews first:** a tool that has `dry_run` keeps `true` as its default, and the database tools
  keep acting only with `apply: true` (SPEC-0006, SPEC-0012).
- **Paths** stay relative to the workspace root; absolute paths, `..` and links leading outside are
  refused (SPEC-0006 behavior 5).
- **Shell access to every tool:** `idecraft call` (above).

### Experimental tools

Tools whose description says "Experimental". At `a5398c61` these are the families `remote_*`,
`device_*` and `room_*`, the UI driver tools `ui_act`, `ui_driver_status`, `ui_driver_stop`,
`ui_expect`, `ui_find`, `ui_screenshot`, `ui_snapshot`, `ui_trace`, and `web_endpoint_check`. A
tool leaves this list when its description drops the word and the changelog says so.

### Rights

[SPEC-0053](specifications/SPEC-0053-agent-permissions.md) and
[ADR-0031](adr/0031-project-files-never-grant-agents.md). Stable:

- The workspace rights by name and meaning: `run`, `http`, `debug`, `remote`, `ssh`, `trust`,
  `write`, `vcs-write`, `quit`, `terminal-read`, `ports`, `ui`, `secret-read`, `settings`,
  `recent-projects` (`crates/core/src/permissions.rs`).
- The database permissions per connection and target: `read-schema`, `read-data`, `modify-data`,
  `modify-schema`; nothing granted by default.
- Grants live only in `agent-permissions.toml` in your settings folder, with `[[workspace]]` and
  `[[connection]]` tables. A project file can narrow a grant, never widen it, and no MCP tool
  writes the grant file.
- A refused call is an MCP error; over HTTP the window asks you first (SPEC-0221 behavior 11).

A new tool may need a right that exists, and a security fix may move an existing tool under a right
(see the rules). A new right is added in a minor version and named in the changelog.

### HTTP transport

[SPEC-0221](specifications/SPEC-0221-mcp-http-transport.md). Stable:

- Off by default; only you switch it on, with the setting `mcp.http`. An agent can switch it off,
  never on.
- Loopback only (`127.0.0.1`), a port the system picks, a new 32-byte bearer token per start,
  `Host` and `Origin` checks.
- Endpoints `http://127.0.0.1:<port>/mcp` and `/mcp/<id>` per project, where `<id>` stays the same
  for a project path across starts.
- Discovery file `mcp-http/<pid>.json` in the settings folder, mode 0600, with the fields `pid`,
  `started`, `port`, `url`, `token`, `header` and `projects` (each with its root and `url`).
- The tools, results and rights are those of the stdio server.

The MCP protocol revisions served today stay served within 1.x; adding a newer revision is a
minor change.

## Session protocol of `idecraft serve`

[ADR-0026](adr/0026-session-protocol-window-and-serve.md) has its own versioning, which stays: the
first message is `session.hello` with protocol `idecraft-session` and the majors each side speaks;
a minor revision adds methods, fields or capabilities, and a window refuses a backend with a lower
minor; removing or changing anything raises the major; a backend serves the current and the
previous major. The methods themselves are **Experimental** while remote projects are. The
protocol is for the window and paired devices, not a second API for agents; agents use MCP.

## Files and formats

[ADR-0021](adr/0021-idecraft-file-formats.md), [SPEC-0303](specifications/SPEC-0303-idecraft-file-formats.md),
[file formats](formats/README.md).

### The rules for files

- A stable file starts with an `[idecraft]` table naming its `kind` and `format` version. A project
  file of an older version may lack it and is read as format 0 of its kind.
- IDECraft reads every older format of a kind and migrates it in memory. The first time it writes
  the new form, it keeps the old file once as `<name>.bak`.
- IDECraft refuses a file of a newer format and says which format it found; it never overwrites one.
- An incompatible change of a kind raises its `format` and adds a migration. A new optional key is
  not a new format.
- The [JSON schemas](formats/README.md#using-a-schema-in-your-editor) are generated from the code; a
  test fails when they differ.

### Stable kinds

| Kind | File | Schema |
|---|---|---|
| `run` | `.idcrun` | [run.schema.json](formats/run.schema.json) |
| `keymap` | `.idckeymap`; your keymap `keymaps/IDECraft.toml` in the settings folder | [keymap.schema.json](formats/keymap.schema.json) |
| `theme` | `.idctheme` | [theme.schema.json](formats/theme.schema.json) |
| `sources` | `.idcsources` | [sources.schema.json](formats/sources.schema.json) |
| `settings` | `.idcsettings` | [settings.schema.json](formats/settings.schema.json) |
| `ide-settings` | `settings.toml` in the settings folder | — |
| `run-configurations` | `.idecraft/run.toml` | — |
| `connections` | `.idecraft/connections.toml` (no passwords; they stay in the OS keychain) | — |
| `sql-files` | `.idecraft/sql-files.toml` | — |
| `python` | `.idecraft/python.toml` | — |
| `breakpoints` | `.idecraft/breakpoints.toml` | [breakpoints.schema.json](formats/breakpoints.schema.json) |
| `bookmarks` | `.idecraft/bookmarks.toml` | [bookmarks.schema.json](formats/bookmarks.schema.json) |

Exchange files keep their extensions, also before a final `.toml` (`my.idckeymap.toml`), their
64 KiB limit, and what an export leaves out: passwords, stored-password references, agent rights,
environment variables named like secrets, paths outside the project, and the AI, MCP and terminal
settings.

Also stable, without an envelope: `agent-permissions.toml` (above) and `trusted-projects.toml`,
both in the settings folder.

### Experimental kinds

- `ui-script`: UI driver scripts, with the UI driver.
- `connector` (`.idcconnector`): the files the [connector guide](connectors/README.md) describes;
  import and export are Planned (SPEC-0302).
- `code-style`, `live-templates`, `plugin`: registered, Planned.

### State files

State files hold what IDECraft remembers for one user: `window-layouts.json` and the other JSON
files with a `version` field. Later versions read them, and a version never overwrites a state file
from a newer one. Their content is not an interface: do not write tools against them.

### Internal files

Everything else IDECraft keeps in `.idecraft/` or the settings folder is internal, among them:
`history/` (the local history: `revisions.jsonl` and `objects/`; use the `local_history` and
`restore_local_revision` tools), `tool-windows.json`, `state.json`, `recent-files`, `consoles/`,
`profiles/`, `terminal-sessions.json`, `debug-renderers.json`, `highlighting.json` and the
generated test reports. Later versions keep reading local history written by earlier ones
([SPEC-0044](specifications/SPEC-0044-local-history.md) v2), but its layout may change.

## Not stable

- **Rust APIs of the crates.** Every crate in `crates/` has `publish = false`; their public items
  serve the workspace, not other programs.
- **Window structure.** Layout, painted text, the order of controls, and the semantic tree's
  element ids, which are stable only for an element's lifetime (SPEC-0319 behavior 4). Automate the
  window through the UI driver's role and name locators, not through positions.
- **Test hooks.** Every `IDECRAFT_TEST_*`, `IDECRAFT_SELFTEST_*` and other `IDECRAFT_*` variable not
  named above, the window self-test, and `IDECRAFT_WRITE_SCHEMAS`.
- **Debug builds.** Behavior that only debug builds have, for example keeping paired-device keys in
  a file store instead of the keychain (SPEC-0321).
- **Benchmarks and budgets** in `crates/bench`: measurement tools, not a contract.

## Open

These are not decided by the sources yet. Each is Experimental until the changelog of a version
decides it.

1. **Schemas for five stable kinds.** `ide-settings`, `run-configurations`, `connections`,
   `sql-files` and `python` have no published schema; their keys are defined only by the code.
2. **Envelope for the grant and trust files.** `agent-permissions.toml` and `trusted-projects.toml`
   carry no `kind` or `format` and refuse unknown keys, so an older version cannot read a file a newer
   one extended.
3. **Workspace settings `.idecraft/settings.toml`.** Read as a layer of the settings
   ([SPEC-0002](specifications/SPEC-0002-layered-settings.md)), not registered as a kind.
4. **Project files outside the registry.** `.idecraft/stores.toml`, `changelists.toml`,
   `branch-favorites.toml`, `build-tools.toml`, `file-types.toml`, `grammar-ignore.toml`,
   `commit-messages.toml` and `entry-points.toml` may be shared through version control but carry no
   envelope. Each needs a kind or an "internal" label.
5. **The agent log.** `.idecraft/agent-log.jsonl` is documented by what a line names (tool, what it
   changes, rights checked, result), but its field names are not specified as a contract.
6. **Test ids of the UI driver.** SPEC-0319 gives each element a kebab-case test id, but no list of
   ids is documented for automation, so none is stable yet.
7. **`idecraft open <folder>`.** The headless workspace summary from before the window; plain text
   with no specification of its own.
8. **`idecraft device`** is handled by the binary but missing from its usage line.
9. **MCP error codes.** The UI driver's codes (SPEC-0319 behavior 40) are specified; the other tools
    report errors as text only.
10. **Built-in theme names** that `appearance.theme` refers to, such as `IDECraft Dark`.
