# SPEC-0006: MCP server

- **Version:** 56 · **Date:** 2026-11-16 (v56: `replace_text_in_file`, behaviour 8, the `glob` of `list_files`, behaviour 9, `create_file` with text and a dry run, behaviour 10, `editor_selection`, behaviour 11, `build_project`, behaviour 12, AK-7 to AK-11, and two named deviations; tools an agent of the reference's MCP server finds there and missed here, docs/research/observations/PLAN-F084.md); 2026-11-16 (v55: a listing that stops early must say so, named as a deviation, IMP-0444, OBS-0071); 2026-10-31 (v2: binary and server name `idecraft`; v3: `list_connections` from SPEC-0008; v4: `run_sql` from SPEC-0012; v5: `list_tables`, `describe_table` from SPEC-0013; v6: `complete_sql` from SPEC-0014; v7: `inspect_sql` from SPEC-0015; v8: `edit_table_data` from SPEC-0017; v9: `generate_ddl`, `compare_schemas` from SPEC-0018; v10: `rename_sql_object` from SPEC-0019; v11: `explain_sql` from SPEC-0020; v12: `export_data` from SPEC-0021; v13: `er_diagram` from SPEC-0022; v14: `file_diagnostics` from SPEC-0023; v15: `hover_symbol`, `find_symbol_locations`, `complete_code`, `document_symbols` from SPEC-0024; v16: `preview_multi_caret_edit` from SPEC-0025; v17: `file_changes` from SPEC-0026; v18: `highlight_file` from SPEC-0027; v19: `keymap_lookup` from SPEC-0028; v20: `search_in_files`, `replace_in_files_preview` from SPEC-0029; v21: `folding_ranges` from SPEC-0030; v22: `search_everywhere` from SPEC-0031; v23: `git_status`, `git_log`, `git_diff` from SPEC-0032; v24: `git_blame`, `git_branches` from SPEC-0033; v25: `git_commit`, `git_create_branch` from SPEC-0034; v26: `git_switch_branch` from SPEC-0035; v27: `git_merge` from SPEC-0036; v28: `merge_three_way` from SPEC-0037; v29: `terminal_render` from SPEC-0038; v30: `debug_adapters` from SPEC-0039; v31: `rename_symbol` from SPEC-0040; v32: `code_actions`, `apply_code_action` from SPEC-0041; v33: `language_servers` from SPEC-0042; v34: `format_file` from SPEC-0043; v35: `local_history`, `restore_local_revision` from SPEC-0044; v36: `parse_test_report` from SPEC-0045; v37: `http_requests` from SPEC-0046; v38: `run_configurations` from SPEC-0047; v39: `project_model` from SPEC-0048; v40: `idea_project` from SPEC-0049; v41: `python_environments` from SPEC-0050; v42: `node_environment` from SPEC-0051; v43: `java_environment` from SPEC-0052; v44: `run_configuration`, `run_command`, `send_http_request`, `agent_permissions` from SPEC-0053; v45: `debug_start`, `debug_resume`, `debug_evaluate`, `debug_variables`, `debug_stop` from SPEC-0054; v46: `git_remotes`, `git_fetch`, `git_pull`, `git_push` from SPEC-0055; v47: `editor_action` from SPEC-0056; v48: `editor_keystrokes` from SPEC-0057; v49: `signature_help` from SPEC-0080; v50: `find_symbol_locations` kinds `type_definition` and `implementation` from SPEC-0081; v51: `call_hierarchy` from SPEC-0082; v52: `git_diff` with `side_by_side` rows from SPEC-0084; v53: `open_documents` and `read_file` on open buffers from SPEC-0220, Streamable HTTP for the running window from SPEC-0221)
- **Sources:** STD (Model Context Protocol specification; JSON-RPC 2.0), OSS (`intellij-community/plugins/mcp-server`: grouping of IDE capabilities into tools — logic template, no code)
- **Feature matrix rows:** F-084

## Purpose
LLM agents use the IDE through the Model Context Protocol.

## Behaviour
1. `idecraft mcp <directory>` serves that workspace over stdin/stdout (newline-delimited JSON-RPC) until the client disconnects. The running window also serves its projects over Streamable HTTP on loopback (SPEC-0221). [STD]
2. The server identifies as `idecraft` (title `IDECraft`) with its version and announces the tools capability. [STD]
3. Every tool publishes a JSON schema for its arguments. [STD]
4. Tools: `workspace_info`, `list_files` (optional language filter and limit), `read_file` (optional 1-based inclusive line range, max 1 MiB), `detect_language`, `split_sql_statements` (dialect `postgresql` | `mysql` | `sqlite`), `list_connections` (profiles of SPEC-0008 with URL and AI permissions, without secrets), `run_sql` (SPEC-0012), `list_tables` and `describe_table` (SPEC-0013), `complete_sql` (SPEC-0014), `inspect_sql` (SPEC-0015), `edit_table_data` (SPEC-0017), `generate_ddl` and `compare_schemas` (SPEC-0018), `rename_sql_object` (SPEC-0019), `explain_sql` (SPEC-0020), `export_data` (SPEC-0021), `er_diagram` (SPEC-0022), `file_diagnostics` (SPEC-0023), `hover_symbol`, `find_symbol_locations`, `complete_code` and `document_symbols` (SPEC-0024), `signature_help` (SPEC-0080), `call_hierarchy` (SPEC-0082), `preview_multi_caret_edit` (SPEC-0025), `file_changes` (SPEC-0026), `highlight_file` (SPEC-0027), `keymap_lookup` (SPEC-0028), `search_in_files` and `replace_in_files_preview` (SPEC-0029), `folding_ranges` (SPEC-0030), `search_everywhere` (SPEC-0031), `git_status`, `git_log` and `git_diff` (SPEC-0032), `git_blame` and `git_branches` (SPEC-0033), `git_commit` and `git_create_branch` (SPEC-0034), `git_switch_branch` (SPEC-0035), `git_merge` (SPEC-0036), `merge_three_way` (SPEC-0037), `terminal_render` (SPEC-0038), `debug_adapters` (SPEC-0039), `rename_symbol` (SPEC-0040), `code_actions` and `apply_code_action` (SPEC-0041), `language_servers` (SPEC-0042), `format_file` (SPEC-0043), `local_history` and `restore_local_revision` (SPEC-0044), `parse_test_report` (SPEC-0045), `http_requests` (SPEC-0046), `run_configurations` (SPEC-0047), `project_model` (SPEC-0048), `idea_project` (SPEC-0049), `python_environments` (SPEC-0050), `node_environment` (SPEC-0051), `java_environment` (SPEC-0052), `run_configuration`, `run_command`, `send_http_request`, `agent_permissions` (SPEC-0053), `debug_start`, `debug_resume`, `debug_evaluate`, `debug_variables`, `debug_stop` (SPEC-0054), `git_remotes`, `git_fetch`, `git_pull`, `git_push` (SPEC-0055), `editor_action` (SPEC-0056), `editor_keystrokes` (SPEC-0057). [GEN, OSS]
5. Paths are relative to the workspace root. Absolute paths, `..` components and symbolic links leading outside the root are rejected. [GEN]
6. Failures are reported as MCP errors, never as a crash of the server. [STD]
7. No authentication, licence or usage limits (ADR-0007); transport is local only. [GEN]
8. **(v56) Replacing text in a file:** `replace_text_in_file` (`path`, `old_text`, `new_text`,
   `replace_all` default false, `case_sensitive` default true, `dry_run` default true) replaces
   `old_text` in a workspace text file as the window shows it, saved or not (SPEC-0220 behaviour 6).
   Without `replace_all` the text must occur exactly once: none or several occurrences are an
   error that names how many there are and on which 1-based lines, and nothing changes — an agent
   that meant one place never changes another by accident. `old_text` must not be empty. The
   answer names `replacements`, the 1-based `lines` of the replaced places in the old text and
   `applied`; with `dry_run` true nothing is written. `dry_run` false needs the `write` right
   (SPEC-0053) and writes through the open editor as one undo step, or to the disk when the file is
   not open (SPEC-0220 behaviours 7 and 8: *through the buffer*). [GEN; OSS: the reference's MCP
   server offers a tool of this name, see PLAN-F084]
9. **(v56) Files by pattern:** `list_files` also takes `glob`, comma-separated patterns with `*` and
   `?` (`*` also crosses `/`): a pattern with a `/` is matched against the workspace-relative path,
   one without against the file name (`*.rs`, `src/*Test.java`). It combines with `language` and
   `limit`. [GEN]
10. **(v56) Creating a file with text:** `create_file` (SPEC-0275) takes `text`, the file's content in
    place of the kind's template (refused for a directory), and `dry_run`, which defaults to true
    for every call: a dry run checks the name, the folder and that nothing of that name exists, and
    returns the path and the content the file would have, creating nothing; `dry_run` false needs the
    `write` right and creates it as before. Creating never overwrites. A client that relied on the
    old default (create at once) now gets a dry run and passes `dry_run: false`. [GEN]
11. **(v56) The editor's selection:** `editor_selection` returns the selection of the editor that has
    the focus in the running window — the active editor of the focused tab group —: `path`
   (workspace-relative), `start` and `end` as 0-based
    `line` and `character` (Unicode scalar values, as the language-server tools count; the caret
    when nothing is selected), the selected `text` and the number of `carets` (the selection is the
    primary caret's). It is read only and needs no right. Secrets in the text are masked as
    `terminal_read` masks them (`masked` counts them). `selection` is null when no editor of a file
    of this workspace has the focus; a server without a window (stdio) says it has none. [GEN]
12. **(v56) Build Project in one call:** `build_project` runs what the window's Build Project runs
    (SPEC-0100 v3 behaviour 18): the Before Build tasks, the build step of each top module (`cargo
    build`, `mvn test-compile`, `gradle classes testClasses`, `go build ./...`, with the build tools
    window's options) and the After Build tasks, as one chain that stops at the first failure.
    `dry_run` defaults to true and returns the steps; `dry_run` false needs the `run` right and a
    trusted project, as `run_configuration` does, and returns `success`, every step's outcome and
    `problems`: the errors and warnings read from the output (rustc, javac and Maven, kotlinc,
    Go, GCC and Clang, tsc) with the file relative to the workspace where it lies in it, 1-based
    `line` and `column`, `severity` and `message`, at most 200, `more` counting the rest. The call
    is logged in `.idecraft/agent-log.jsonl`. A project without a build IDECraft knows is an
    error. [GEN; OSS: the reference's MCP server offers a build call, PLAN-F084]

## Acceptance criteria
- **AK-1** `tools/list` returns exactly the eleven tools, each with an input schema.
- **AK-2** `workspace_info` reports file and language counts; `list_files` filters by language.
- **AK-3** `read_file` returns a line range; paths outside the root and missing files fail.
- **AK-4** `detect_language` and `split_sql_statements` return the results of SPEC-0004/0005.
- **AK-5** The server handshake works over real stdio (`idecraft mcp`).
- **AK-6** `list_connections` returns an empty list without a profile file and each profile's name, dialect, URL and whether a keychain secret is set.
- **AK-7 (v56)** `replace_text_in_file`: a dry run lists the place and changes nothing; without the
  `write` right nothing is written; an ambiguous and a missing text are errors naming the count
  and lines; `replace_all` replaces every place; case-insensitive matching; an open file with
  unsaved changes is edited through the buffer, not the disk
  (`crates/mcp/tests/it/protocol.rs`: `replace_text_in_file_*`; ratchet
  `open_files::every_file_changing_tool_edits_open_files_through_the_buffer_or_refuses`).
- **AK-8 (v56)** `list_files` with `glob` by name and by path, combined with `language`
  (`protocol.rs`: `list_files_by_glob`).
- **AK-9 (v56)** `create_file`: a dry run returns the path and the text (or the kind's template)
  and creates nothing; without `write` nothing is created; with it the file holds the text; an
  existing name and text for a directory are refused (`protocol.rs`:
  `create_file_with_text_is_a_dry_run_first`; the effects table's default,
  `effects::tests::dry_runs_follow_the_default_of_each_tool`).
- **AK-10 (v56)** `build_project`: the dry run names the steps and runs nothing; a Cargo project
  with an error gives `success` false and the error at `src/main.rs`, line 2, column 13; without
  `run` nothing runs (`protocol.rs`: `build_project_runs_the_build_and_lists_its_errors`, skipped
  without Cargo); the compilers' forms (`build_errors::tests`).
- **AK-11 (v56)** `editor_selection` against a stand-in window: path, range and text with a secret
  masked; a file outside the workspace is not reported; without a window a clear error
  (`protocol.rs`: `editor_selection_reads_the_windows_selection_masked`); the window's own answer
  from a real workbench window of the harness, with the caret alone and with a selection made by
  keys (`crates/workbench/tests/it/editor_selection_mcp.rs`), passed on to the server by
  `crates/app/src/mcp_http.rs`.

## Out of scope
Write tools, diagnostics, run/debug tools, HTTP transport — follow as the IDE grows.

## Known deviations from the reference
- **(v55) A listing that stops early says so (IMP-0444, proposed).** The reference's directory tree
  over MCP shows a folder cut off by its depth limit like an empty one. IDECraft's listings say when
  they stopped early (`search_in_files` with `truncated` and `not_searched`); `list_files`, which
  still stops at `limit` without a word, gets the same once its glob filter (#830) is in
  (OBS-0071 O10).
- **(v56) No "run an action by its id".** The reference's MCP server is expected to offer running
  any IDE action by its id and listing the actions (PLAN-F084). IDECraft does not: an action's
  effect is not always clear from its id, and a generic call would bypass the rights each tool
  checks. An agent reaches an action through its tool (more than 300, each with its declared
  effects and rights, SPEC-0053 v8 behaviour 13) or, with the user's `ui` right and the UI driver
  switched on, through its key with `ui_act` `press`, where each keymap action of the key carries
  its effects and the reserved ones are refused (SPEC-0319 behaviour 35). Security goes first
  (rule 12); a narrower call for actions with declared effects may follow.
- **(v56) No notebooks.** The reference's `runNotebookCell` has no counterpart: IDECraft has no
  notebook support yet (outside F-084).
