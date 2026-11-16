# SPEC-0006: MCP server

- **Version:** 55 · **Date:** 2026-11-16 (v55: a listing that stops early must say so, named as a deviation, IMP-0444, OBS-0071); 2026-10-31 (v2: binary and server name `idecraft`; v3: `list_connections` from SPEC-0008; v4: `run_sql` from SPEC-0012; v5: `list_tables`, `describe_table` from SPEC-0013; v6: `complete_sql` from SPEC-0014; v7: `inspect_sql` from SPEC-0015; v8: `edit_table_data` from SPEC-0017; v9: `generate_ddl`, `compare_schemas` from SPEC-0018; v10: `rename_sql_object` from SPEC-0019; v11: `explain_sql` from SPEC-0020; v12: `export_data` from SPEC-0021; v13: `er_diagram` from SPEC-0022; v14: `file_diagnostics` from SPEC-0023; v15: `hover_symbol`, `find_symbol_locations`, `complete_code`, `document_symbols` from SPEC-0024; v16: `preview_multi_caret_edit` from SPEC-0025; v17: `file_changes` from SPEC-0026; v18: `highlight_file` from SPEC-0027; v19: `keymap_lookup` from SPEC-0028; v20: `search_in_files`, `replace_in_files_preview` from SPEC-0029; v21: `folding_ranges` from SPEC-0030; v22: `search_everywhere` from SPEC-0031; v23: `git_status`, `git_log`, `git_diff` from SPEC-0032; v24: `git_blame`, `git_branches` from SPEC-0033; v25: `git_commit`, `git_create_branch` from SPEC-0034; v26: `git_switch_branch` from SPEC-0035; v27: `git_merge` from SPEC-0036; v28: `merge_three_way` from SPEC-0037; v29: `terminal_render` from SPEC-0038; v30: `debug_adapters` from SPEC-0039; v31: `rename_symbol` from SPEC-0040; v32: `code_actions`, `apply_code_action` from SPEC-0041; v33: `language_servers` from SPEC-0042; v34: `format_file` from SPEC-0043; v35: `local_history`, `restore_local_revision` from SPEC-0044; v36: `parse_test_report` from SPEC-0045; v37: `http_requests` from SPEC-0046; v38: `run_configurations` from SPEC-0047; v39: `project_model` from SPEC-0048; v40: `idea_project` from SPEC-0049; v41: `python_environments` from SPEC-0050; v42: `node_environment` from SPEC-0051; v43: `java_environment` from SPEC-0052; v44: `run_configuration`, `run_command`, `send_http_request`, `agent_permissions` from SPEC-0053; v45: `debug_start`, `debug_resume`, `debug_evaluate`, `debug_variables`, `debug_stop` from SPEC-0054; v46: `git_remotes`, `git_fetch`, `git_pull`, `git_push` from SPEC-0055; v47: `editor_action` from SPEC-0056; v48: `editor_keystrokes` from SPEC-0057; v49: `signature_help` from SPEC-0080; v50: `find_symbol_locations` kinds `type_definition` and `implementation` from SPEC-0081; v51: `call_hierarchy` from SPEC-0082; v52: `git_diff` with `side_by_side` rows from SPEC-0084; v53: `open_documents` and `read_file` on open buffers from SPEC-0220, Streamable HTTP for the running window from SPEC-0221)
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

## Acceptance criteria
- **AK-1** `tools/list` returns exactly the eleven tools, each with an input schema.
- **AK-2** `workspace_info` reports file and language counts; `list_files` filters by language.
- **AK-3** `read_file` returns a line range; paths outside the root and missing files fail.
- **AK-4** `detect_language` and `split_sql_statements` return the results of SPEC-0004/0005.
- **AK-5** The server handshake works over real stdio (`idecraft mcp`).
- **AK-6** `list_connections` returns an empty list without a profile file and each profile's name, dialect, URL and whether a keychain secret is set.

## Out of scope
Write tools, diagnostics, run/debug tools, HTTP transport — follow as the IDE grows.

## Known deviations from the reference
- **(v55) A listing that stops early says so (IMP-0444, proposed).** The reference's directory tree
  over MCP shows a folder cut off by its depth limit like an empty one. IDECraft's listings say when
  they stopped early (`search_in_files` with `truncated` and `not_searched`); `list_files`, which
  still stops at `limit` without a word, gets the same once its glob filter (#830) is in
  (OBS-0071 O10).
