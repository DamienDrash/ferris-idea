# SPEC-0156: Plugin API for the editor and the project

- **Version:** 6 · **Date:** 2026-11-15 (v6: AK-13; v5: `http-client.private.env.json` joins the credential files of
  the deny list, which agents now share through SPEC-0053 v6 behaviour 10; v4: plugins are not run by themselves on a file they
  could not read and an action the user picks on one asks first, the same over MCP unless the call
  says it is meant, the folder of a suggested command is not behind a link, a file is read from the
  handle that was checked, behaviours 12 to 15; v2: `list_files` lists only the project's own sources,
  behaviour 6; v3: `read_file` and the `open` request take only a file of the project's own sources,
  files that commonly hold secrets are refused by name, refusals are logged, behaviours 7 to 11)
- **Sources:** STD (POSIX `open` with `O_NOFOLLOW`, `fstat`), GEN (IDE plugins act on the open editor — its file, text, selection and caret — and
  read the project's files, and their actions edit the document as one undoable step; F-080), STD
  (WebAssembly imports and host functions), SPEC-0104, SPEC-0126, ADR-0016 (amendment 2026-11-03),
  ADR-0008
- **Feature matrix rows:** F-080

## Purpose
Let plugins do real editor work — wrap, generate, look things up in the project — without giving
them the machine.

## Behaviour
1. **Edit actions:** an action of kind `edit` gets the editor's context as JSON: `path` (relative to
   the project), `language`, `text`, `selection_start`, `selection_end` (characters), `line`,
   `column`. [GEN]
2. **Answer:** JSON with `edits` (`start`, `end` in characters of the given text, `text`),
   `select` (`[start, end]` afterwards), `message` (status bar) and `open` (`path`, `line`) — all
   optional. Edits outside the text or overlapping are refused. The IDE applies the edits as one
   undo step, then the selection, the message and opens the file (inside the project only). If the
   text changed while the plugin ran, nothing is edited and the status bar says so. [GEN]
3. **Permissions:** a manifest may ask for `project-read`. Settings › Plugins shows "asks to read
   the project (not granted)" or "reads the project (granted)"; Space on the selected plugin grants
   or withdraws it. Grants are kept in `permissions.json` of the user's plugins folder and dropped
   when the plugin is removed. [GEN, ADR-0016]
4. **Host functions** (import module `idecraft`): `list_files() -> i64` (JSON list of the project's
   files as the project tree lists them, at most 50 000) and `read_file(path_ptr, path_len) -> i64`
   (the file's bytes), both returning a packed pointer and length written through the plugin's
   `idecraft_alloc`, or `-1` without the grant, `-2` for a path that is not a project file (absolute,
   `..`, outside after resolving links, missing), `-3` over 4 MiB. Other imports do not link. The
   sandbox limits of SPEC-0104 stay. [STD, ADR-0016]
5. **MCP:** `plugins` lists each plugin's `permissions` and `granted`; `run_plugin_action` runs an
   `edit` action on a workspace file with a selection and returns the answer and the edited text
   without writing it; project access only as the user granted it. [ADR-0008]
6. **`list_files` lists only the project's own sources (v2; decision of the coordinator of 2026-11-13 under the Project Owner's general authority: the rule the Project Owner chose for Rename, Find in Files, Search Everywhere, Inspect Code and Endpoints; security first, CLAUDE.md
   rule 12):** the list holds the files that are the project's own by the rule of SPEC-0148 v4
   behaviour 7 (`OwnSources`), not "the files as the project tree lists them" of behaviour 4: no
   virtual environment, installed dependencies, build output, IDE data, folders the project model
   excludes, nothing behind a symbolic link and nothing an ignore rule of Git matches unless Git
   tracks it. A plugin with `project-read` no longer learns the names of ignored files (local
   configuration, key material kept out of the repository) or of installed packages. There is no
   option. Until v3 `read_file` was unchanged; since v3 behaviour 7 applies. [GEN]
7. **`read_file` reads only the project's own sources (v3; decision of the implementing agent of
   2026-11-13 under the Project Owner's general authority, security first, CLAUDE.md rule 12).**
   Until v2 a plugin with `project-read` could read any regular file below the project folder by
   naming it: a file Git ignores such as `.env`, a private key, IDECraft's own data under
   `.idecraft`, the files inside `.git` (remote URLs, sometimes with a token), an installed package.
   Since v3 the path must pass every check below, in this order; the first that fails decides.
   1. *Form:* UTF-8, at most 4096 bytes, relative, made of plain names only (no `..`, no `.`, no
      empty part), without a control character, a backslash or a colon, and no part ending in a dot
      or a space. The last three close other spellings of one file on Windows (alternate data
      streams, `secret.pem.`). Answer `-2`.
   2. *Data of version control or of an IDE by name:* no part of the path is `.git`, `.hg`, `.svn`,
      `.jj`, `CVS`, `.idecraft` or `.idea`, in any spelling of upper and lower case, as a folder or
      as a file (the `.git` file of a linked working tree names a folder outside the project).
      Answer `-2`.
   3. *Deny list:* behaviour 8. Answer `-5`.
   4. *Own sources:* `OwnSources::readable` accepts the path (below the project folder, no symbolic
      link anywhere on the way, the file itself included) and `OwnSources::not_own` gives no
      reason (SPEC-0148 v4 behaviour 7: no virtual environment, installed dependencies, build
      output, IDE data, folder the project model excludes, nothing an ignore rule of Git matches
      unless Git tracks it). Answer `-2`.
   5. *The file as it is on disk:* a regular file whose resolved path is, name for name, the path
      asked for. `Readme.MD` on a disk that ignores case, and a name in another Unicode
      normalization, are refused: the deny list looked at the name as it was written, so only the
      exact name may reach a file. (`Node_Modules/x.js` is refused by check 4 since SPEC-0148 v5,
      which judges a path by its names on disk.) Answer `-2`.
   6. *Size:* at most 4 MiB, checked while reading. Answer `-3`.

   There is no option and no way for a plugin, a workspace file or an agent to widen this. [GEN]
8. **Deny list (v3).** A file whose name commonly holds secrets is refused even when it is the
   project's own and Git tracks it: a secret committed by mistake, or on purpose in a private
   repository, is still not something a plugin needs for editor work. Names are compared without
   regard to upper and lower case. File patterns apply to the last part of the path, folder names to
   every part. The check looks only at the path the plugin gave, never at the disk, so its answer
   says nothing about whether the file exists.

   | Family | File names | Folder names | Why |
   |---|---|---|---|
   | Environment files | `.env`, `.env.*`, `*.env`, `.envrc` | — | The usual place for API keys, database passwords and tokens of a local or deployed setup. `.env.example` is included: the rule goes by name, and such files are filled in with real values often enough. |
   | Private keys and key stores | `*.pem`, `*.key`, `*.p8`, `*.p12`, `*.pfx`, `*.jks`, `*.keystore`, `*.ppk`, `*.kdbx` | — | Formats made to hold a private key, a certificate with its key, or a password database. A public certificate in a `.pem` is refused with them because the name does not tell them apart. |
   | SSH and GnuPG identities | `id_rsa*`, `id_dsa*`, `id_ecdsa*`, `id_ed25519*` | `.ssh`, `.gnupg` | The default names of SSH private keys and the folders that hold keys and their configuration. The `.pub` half is refused with them; it is no source either. |
   | Credential files of tools | `.netrc`, `_netrc`, `.npmrc`, `.yarnrc`, `.yarnrc.yml`, `.pypirc`, `.git-credentials`, `.pgpass`, `.my.cnf`, `.htpasswd`, `.dockercfg`, `.s3cfg`, `.boto`, `.vault-token`, `auth.json`, `kubeconfig`, `*.kubeconfig`, `http-client.private.env.json` (v5) | `.aws`, `.azure`, `.docker`, `.kube` | Files in which package managers, Git, database clients, web servers, container and cloud tools keep passwords and access tokens in plain text. |
   | Infrastructure state and variables | `*.tfvars`, `*.tfvars.json`, `*.tfstate`, `*.tfstate.*` | — | Terraform variable files carry the secrets passed to a deployment; a state file stores every resource attribute, passwords included, in plain text. |
   | Names that say so | `credentials*`, `.credentials*`, `secrets*`, `.secrets*`, `secret`, `secret.*`, `client_secret*`, `*.secret`, `*.secrets` | `credentials`, `.credentials`, `secrets`, `.secrets` | A file or folder named after what it holds. This also refuses a source file such as `secrets.rs`, which is the price of a rule that needs no look into the file. |

   Deliberately not on the list: configuration files that are sources first and only sometimes
   carry a password (`application.properties`, `settings.xml`, `gradle.properties`,
   `database.yml`, `appsettings.json`, `wp-config.php`). Refusing them would break plugins on
   ordinary projects, and telling a password from a setting needs a look into the content, which
   this rule does not take. The list is fixed. A per-plugin, per-workspace exception in the user's
   settings folder was considered and left out of v3: it needs a place in Settings › Plugins, a
   rule for agents (who must not be able to lift it) and a review step on plugin updates, which is
   more than this change; until then the list holds for every plugin. [GEN]
9. **Answers and what they reveal (v3).** `-2` now stands for "not a file of the project's own
   sources that a plugin may read, or no such file": a path in a bad form, data of version control
   or an IDE, anything `OwnSources` does not count as own, a symbolic link, another spelling, a
   folder, and a file that does not exist all get the same answer, so a plugin cannot find out
   whether a file it may not read exists (a `build` that is ignored only as a folder, a link, an
   entry of `.git`). `-5` is new and means "the name is on the list of files that commonly hold
   secrets"; it depends on the name alone (behaviour 8), so it reveals nothing either, and a plugin
   can tell its user why `.env` was not read. `-1` (no grant), `-3` (over 4 MiB, only for a file
   the plugin may read) and `-4` (the plugin's memory could not be written) are unchanged.
   `idecraft_plugins::file_answer_meaning` gives each code's sentence for hosts and plugin
   authors. [GEN]
10. **Refusals are logged (v3).** Every refusal of behaviours 7 and 11, and every call of
    `list_files` or `read_file` without the grant (reason `reading the project is not granted`,
    the path as asked, nothing looked at on disk), adds a line to
    `file-audit.jsonl` in the user's plugins folder, next to `network-audit.jsonl` of SPEC-0314
    behaviour 9 and with the same rules: `time` (RFC 3339, UTC), `plugin` (file name without
    `.wasm`), `function` (`read_file` or `open`; since v4 also `action`, `run_plugin_action` and
    `plugin_inspections` for a file handed over on purpose, behaviours 12 and 13), `path` as the
    plugin gave it (at most 4096 bytes;
    `(not UTF-8)` otherwise) and `reason` in words. The log names the real reason (`ignored by
    .gitignore`, `behind a symbolic link`, `environment file (deny list)`, …), because the log is
    the user's. Never file contents. A path that passes every rule and simply does not exist is not
    a refusal and is not logged. At most 20 lines per call, then one line saying that further
    refusals of this call are not logged; at 1 MiB the file is moved to `file-audit.jsonl.1`
    (replacing the one before) and started again, so a plugin cannot fill the disk. The network log
    was not reused: its records are connections (holder, target host and port, address, bytes), and
    tools that read it would misread file refusals. Nothing in the log leaves the machine. The MCP
    tool `plugins` returns the last 200 lines as `file_refusals`. [ADR-0007, ADR-0008]
11. **The `open` request follows the same rule (v3).** A file a plugin asks to open (behaviour 2)
    is opened only if `read_file` would read it (checks 1 to 5 of behaviour 7, and behaviour 8);
    otherwise nothing is opened and the refusal is logged with function `open`. Until v2 the window
    compared the joined path with the project folder name by name, so `../../x` passed and a plugin
    could have a file outside the project shown in an editor. [GEN]
12. **Plugins are not run by themselves on a file they could not read (v4; decision of the
    coordinator of 2026-11-13 under the Project Owner's general authority, security first, CLAUDE.md
    rule 12).** An action gets the text of the editor it runs on, and `inspect`, `status` and
    `complete` actions run by themselves on the active file (SPEC-0166, SPEC-0210). Until v3 an
    installed plugin therefore got the text of `.env` as soon as the user opened it, without
    `project-read`. Since v4 the host asks, for the file an editor shows, the question of
    behaviour 7 (checks 1 to 5) and behaviour 8, whether or not the plugin has `project-read`. A
    file that is not on disk yet and passes the checks by its name is not guarded.
    - *Automatic calls:* for a guarded file no `inspect`, `status` or `complete` action is run:
      no findings, no status widget, no completion items from plugins; the language's own
      completion is not affected. A `panel` action and a `run-config` action (SPEC-0276) are not
      about one file and still run, but the active file is not named to them while it is guarded.
      Nothing is asked and nothing is logged: a line per keystroke would bury the log.
    - *An action the user picks* (menu, Search Everywhere, its shortcut) on a guarded file asks
      first: "Give `<file>` to the plugin `<plugin>`?", with the action's title, the reason in the
      words of the log, and the answers "Cancel" and "Run on This File". "Cancel" is the first
      answer and the default; Escape cancels. Only "Run on This File" runs the action, once and for
      this file; the next time it asks again. A confirmed run adds a line to `file-audit.jsonl`
      (behaviour 10) with function `action` and the reason followed by `; handed over all the
      same: confirmed by the user`. Without the question (the action started by other code) it
      does not run and the status bar says why.
    - The plugin's own `read_file` is not widened by any of this: behaviours 7 to 9 hold. [GEN]
13. **MCP (v4).** `run_plugin_action` with a `path` (for an `edit`, `status`, `complete` or `panel`
    action) and `plugin_inspections` refuse a guarded file with a message that names the reason
    and the parameter `allow_secret_file`. With `allow_secret_file: true` the call states that this
    file is meant; it runs, and each plugin that gets the file has a line in `file-audit.jsonl`
    with the tool's name as function and the reason followed by `; handed over all the same: an
    agent asked with allow_secret_file`. The file's text goes to the plugin under the agent's own
    rights (it could read the file itself); the parameter keeps it from happening by accident and
    leaves a trace the user can read (`plugins` returns it under `file_refusals`). [ADR-0008]
14. **A file is read from the handle that was checked (v4).** Between the checks of behaviour 7 and
    the read another program could put a link or another file at the path. Since v4 the checks
    note what was at the path (kind, device and inode). The file is then opened without following
    a symbolic link at its name (`O_NOFOLLOW`; a pipe or device is refused and opening does not
    wait); the handle must be a regular file with the device and inode that were checked, and the
    way to it must still be free of links and resolve to the path asked for. Only then are the
    bytes read, from that handle. Anything else answers `-2` and is logged with the reason
    `changed between the check and the read`. The `open` request (behaviour 11) is accepted only
    if the file could be opened this way. On Windows the file is opened as a link itself
    (`FILE_FLAG_OPEN_REPARSE_POINT`) and a link or junction is refused; see the open points for
    what remains. [STD]
15. **The folder of a suggested command (v4).** SPEC-0276 v3 behaviour 3: the folder of a
    `run-config` suggestion is resolved with the walk of behaviour 7 check 4, and a folder that is
    reached through a symbolic link or lies outside the project is refused, when the suggestion is
    listed and again when it starts.

## Acceptance criteria
- **AK-1** An `edit` action answers with edits, selection and message; reading the project is
  refused without the grant, works with it, `..` is refused; listing; grants kept and withdrawn
  (`crates/plugins/tests/runtime.rs`).
- **AK-2** MCP: permissions and grants listed, the edit action denied and then applied after the
  grant, nothing written, `path` required (`plugin_api_edit_actions_and_granted_project_access`).
- **AK-3** Window self-test: the action says "denied" without the grant; Space on the Plugins page
  grants it; the action then inserts `// ` and selects it.
- **AK-4** Hand test (round 4, no. 82): a plugin built from Rust for `wasm32-unknown-unknown` using
  the editor context and `read_file`.
- **AK-5 (v2)** With the grant, in a project with `.venv/lib/python3.12/site-packages/x.py`,
  `node_modules/y.js`, a file an ignore rule matches and own files: `list_files` returns only the
  own files (`crates/plugins/tests/runtime.rs`:
  `project_access_needs_the_grant_and_stays_in_the_project`).
- **AK-6 (v3)** With the grant, a plugin built from WebAssembly text reads an own file; `.env`, a
  `config/prod.pem`, a file an ignore rule matches, `.git/config`, `.idecraft/state.json`, a file in
  `node_modules`, a path through a symbolic link to a folder outside, a symbolic link to a file,
  `../outside.txt`, an absolute path and (where the disk ignores case) another case of an own
  file's name are refused, the deny list with `-5` and the others with `-2`; a missing own file
  answers `-2` like them; each refusal has its line in `file-audit.jsonl` with plugin, function,
  path and reason and without the file's content, the missing file has none; after 20 refusals of
  one call comes the closing line and nothing more; a call without the grant answers `-1` and is
  logged
  (`crates/plugins/tests/runtime.rs`: `read_file_takes_only_own_sources_and_logs_refusals`).
- **AK-7 (v3)** The deny list by name: the patterns of behaviour 8 in upper and lower case, folder
  names at any depth, and names that must stay readable (`src/main.rs`, `environment.ts`,
  `keys.rs`, `README.md`, `application.properties`); the forms of check 1; the log's rotation at
  1 MiB (`crates/plugins/src/files.rs` unit tests).
  v5: `http-client.private.env.json` is on the list, `http-client.env.json` is not (same unit test).
- **AK-8 (v3)** The `open` request: an own file is accepted, `../outside.txt`, `.env` and a file an
  ignore rule matches are refused and logged with function `open`
  (`crates/plugins/tests/runtime.rs`: `open_requests_follow_the_read_rule`).
- **AK-9 (v4)** The guard: an own file and a file not saved yet are not guarded, relative and
  absolute; `.env`, `deploy/.ENV.production`, `config/prod.pem`, a file an ignore rule matches,
  `.git/config`, `.idecraft/state.json`, a file in `node_modules`, a folder, a file outside the
  project and a symbolic link are, each with its reason; asking logs nothing, handing over on
  purpose logs one line without the file's content
  (`crates/plugins/tests/runtime.rs`: `guarded_files_are_what_read_file_refuses`). The question
  names the file and the plugin, carries the action's title and the reason, and its first answer
  is "Cancel" (`files::tests::the_question_names_the_file_and_the_plugin`).
- **AK-10 (v4)** MCP: `plugin_inspections` refuses `.env` and a file in `node_modules`, naming the
  reason and `allow_secret_file`, and logs nothing; with `allow_secret_file: true` it answers and
  the call is listed under `file_refusals` with the plugin, the tool and the reason
  (`plugin_inspections_tool`). `run_plugin_action` with an `edit` action on `.env` is refused
  without the parameter and with `false`, runs with `true` and is listed
  (`plugin_api_edit_actions_and_granted_project_access`).
- **AK-11 (v4)** Window: with a plugin that has an `inspect`, a `status` and a `complete` action
  installed, opening `.env` shows no finding, no widget and no item of the plugin; picking an
  action of the plugin asks, Return and Escape leave the file alone, "Run on This File" runs it
  (hand test, to be added to the open round).
- **AK-12 (v4)** The handle: a file is opened without following a link, a folder and a missing
  file are refused, and two files are told apart
  (`own_sources::tests::a_file_is_opened_without_following_a_link`); after the checks, a link put
  in the file's place, another regular file moved there and the folder above replaced by a link
  to a folder with a file of the same name are each refused as changed, an unchanged file is
  read, a file that is gone is "not there"
  (`files::tests::a_file_swapped_after_the_check_is_not_read`).

- **AK-13 (v6)** Window test with keys (`crates/workbench/tests/it/f080_window.rs`:
  `hand_test_82_an_edit_action_with_and_without_the_grant`): an `edit` action that reads the project
  answers "denied" and changes nothing without the grant; Space on the Plugins page grants it; then
  its edit is applied and selected, and one ⌘Z undoes it.

## Out of scope
Writing files, network, other extension points (inspections, completion, tool windows, events on
save), a marketplace, signing.

## Known deviations from the reference
The reference's plugins run with full access to the IDE and the machine; IDECraft's see only the
editor context they are given and, if granted, read the project (IMP-0037).
Since v2 `list_files` leaves out what is not the project's own (behaviour 6); since v3 `read_file`
and the `open` request refuse it too, and files that commonly hold secrets by name (behaviours 7 to
11). Since v4 plugins are not run by themselves on such a file (behaviour 12).

## Open points (v4)
The points of v3 about files opened by the user, files agents choose, the folder of a suggestion and
the time between check and read are closed by behaviours 12 to 15. What stays:
- A `run-config` suggestion (SPEC-0276) may carry text the plugin read in its arguments or
  environment. That is at most the project's own, non-secret sources, and the command is shown in
  full before the user confirms it.
- Behaviour 14 on Unix closes a link at the file's own name for good, and a swapped folder above it
  by comparing device and inode and checking the way again. A program that swaps a folder twice
  within that moment, and brings back the very file that was checked, is not excluded; opening
  every folder of the way by handle would close it and needs calls the workspace's ban on unsafe
  code does not allow without a new dependency. On Windows the standard library gives no file
  identity: a link at the name is refused, a regular file put in its place within the moment is
  told apart only by kind, size and times.
- A file a plugin asked to open (behaviour 11) is checked and opened once by the rule, and then
  opened by the editor by its path; what is shown is shown to the user, and the plugin's automatic
  calls on it go through behaviour 12 again.
- Behaviour 12 looks at the file an editor shows. Text the user pastes into another file, and the
  selection handed to a `replace` or `message` action the user picked on an unguarded file, are
  the user's to give.
