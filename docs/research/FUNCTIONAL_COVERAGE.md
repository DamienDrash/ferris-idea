# Funktionstests je Matrixzeile

**Stand:** 2026-11-04 · Commit `8c8bf1b` · erzeugt von `scripts/functional.sh` · Grundlage: [Feature-Matrix](FEATURE_MATRIX.md), Tests: `tests/functional/`

Je Zeile der Feature-Matrix ein Testmodul, geschrieben aus Zeile und Akzeptanzkriterien der Spezifikation,
nicht aus der Implementierung. Die Tests bedienen Ferris IDEA nur von außen: Befehlszeile `ferris-idea`, MCP über
stdio und über den HTTP-Transport des Fensters, auf Kopien von `tests/reference-projects` und gegen die
Referenzdatenbanken. Verhalten, das nur ein Fenster zeigt, prüft der Fenster-Selbsttest
(`ferris-idea ui --selftest`, `scripts/ci-window.sh`); die Spalte nennt seine Schritte.

**Ergebnis:** 56 Zeilen, 55 mit Tests, 1 mit benanntem Grund ·
135 Tests: 135 bestanden, 0 übersprungen (Werkzeug oder Datenbank fehlt), 0 fehlgeschlagen.

| ID | Funktion | Tests | Ergebnis | Fenster-Selbsttest |
|---|---|---|---|---|
| F-001 | Ordner als Projekt öffnen | `f001_open_folder`: `folder_lists_sorted_relative_files_without_build_and_vcs_folders`, `cli_opens_a_folder_and_refuses_a_file_or_missing_path` | 2/2 passed | — |
| F-002 | Einstellungen IDE/Projekt | `f002_settings`: `project_settings_beat_the_defaults`, `invalid_project_settings_are_an_error_naming_the_problem`, `global_settings_live_in_the_users_folder_and_are_changed_there` | 3/3 passed | — |
| F-003 | Projektmodell aus Build-Dateien (Maven, Gradle, npm, pip) | `f003_project_model`: `maven_project_gives_module_and_dependencies`, `npm_and_python_projects_are_read_and_a_broken_file_is_a_problem` | 2/2 passed | — |
| F-004 | Projektdateien der Referenz-IDE lesen | `f004_idea_project`: `no_idea_folder_gives_null`, `idea_folder_gives_modules_and_sdk_without_opening_private_files` | 2/2 passed | — |
| F-010 | Texteditor-Grundfunktionen | `f010_text_editing`: `typing_and_backspace_count_characters_not_bytes`, `line_commands_duplicate_delete_and_move`, `replacing_a_selection_by_typing` | 3/3 passed | — |
| F-011 | Undo/Redo mit Gruppen | `f011_undo_groups`: `typed_text_and_its_undo_and_redo`, `a_rename_over_several_files_is_undone_in_one_step` | 2/2 passed | — |
| F-012 | Mehrfach-Cursor, Spaltenauswahl | `f012_multi_caret`: `all_occurrences_get_a_caret_and_the_file_stays_unchanged`, `typing_at_three_carets_and_one_undo`, `column_selection_replaces_the_same_columns_on_every_line` | 3/3 passed | — |
| F-013 | Syntaxhervorhebung | `f013_highlighting`: `rust_keywords_functions_strings_and_comments_at_character_offsets`, `every_reference_language_highlights_and_lines_can_be_limited`, `a_file_without_highlighting_is_refused_with_a_reason` | 3/3 passed | — |
| F-014 | Code-Folding | `f014_folding`: `python_class_and_function_bodies_fold`, `rust_blocks_comments_and_imports_fold_but_one_line_blocks_do_not` | 2/2 passed | — |
| F-015 | Suchen/Ersetzen, Regex, in Dateien | `f015_find_replace`: `find_in_files_with_case_whole_words_and_mask`, `regex_replace_preview_with_groups_writes_nothing`, `an_invalid_regex_is_an_error` | 3/3 passed | — |
| F-016 | „Überall suchen“ / Dateien, Klassen, Symbole, Aktionen per Name | `f016_search_everywhere`: `files_symbols_and_actions_by_name`, `camel_humps_find_a_class` | 2/2 passed | — |
| F-017 | Tastenkürzel-Schema | `f017_keymap`: `a_user_shortcut_replaces_the_default_and_can_be_reset`, `an_unknown_keymap_is_refused` | 2/2 passed | — |
| F-018 | Kompatible Tastenkürzel-Belegung der Referenz | `f018_reference_keymap`: `default_and_macos_bindings_of_well_known_actions`, `a_keystroke_names_its_actions` | 2/2 passed | — |
| F-019 | Lokale Historie | `f019_local_history`: `changes_are_recorded_and_a_restore_can_be_undone`, `history_works_without_git` | 2/2 passed | — |
| F-020 | Editor-Tabs, geteilte Ansichten | `f020_tabs_split`: `without_a_window_no_document_is_open` | 1/1 passed | opening tree and tab steps, split_steps, split_move_steps |
| F-030 | Sprache einer Datei erkennen | `f030_language_detection`: `extensions_and_special_names_give_lsp_ids`, `files_are_counted_and_filtered_by_language` | 2/2 passed | — |
| F-031 | Codevervollständigung | `f031_completion`: `members_of_a_python_class_are_proposed`, `parameter_info_of_a_call`, `paths_outside_the_workspace_are_refused` | 3/3 passed | — |
| F-032 | Diagnosen / Inspektionen | `f032_diagnostics`: `a_type_error_is_reported_with_its_range`, `own_inspections_find_unused_imports_and_locals` | 2/2 passed | — |
| F-033 | Quick-Fixes | `f033_quick_fixes`: `a_missing_semicolon_is_fixed_by_the_preferred_action`, `an_own_inspection_fix_removes_an_unused_import` | 2/2 passed | — |
| F-034 | Gehe zu Definition / Verwendungen | `f034_navigation`: `definition_references_hover_symbols_and_callers` | 1/1 passed | — |
| F-035 | Python-Unterstützung | `f035_python`: `interpreters_requirements_and_test_configuration_are_read_statically`, `choosing_an_interpreter_is_saved_in_the_project` | 2/2 passed | — |
| F-036 | JavaScript/TypeScript, Frameworks | `f036_javascript`: `node_runtime_package_manager_and_missing_packages`, `package_scripts_get_run_configurations`, `typescript_errors_come_from_the_language_server` | 3/3 passed | — |
| F-037 | Java/Kotlin | `f037_java_kotlin`: `jdks_and_the_projects_java_version_are_read_statically`, `main_classes_get_run_markers`, `getters_are_generated_and_imports_optimised`, `kotlin_has_main_markers` | 4/4 passed | — |
| F-040 | Spring, Jakarta EE, Micronaut, Quarkus | `f040_frameworks`: `spring_beans_and_injection_points`, `rest_endpoints_are_listed_with_a_request` | 2/2 passed | — |
| F-038 | PHP, Go, Ruby, C/C++, HTML, CSS, Rust | `f038_other_languages`: `every_language_names_its_server_or_why_none_is_usable`, `c_diagnostics_from_clangd`, `php_symbols_and_ruby_environment` | 3/3 passed | — |
| F-039 | Code-Formatierung | `f039_formatting`: `preview_then_apply_a_format`, `a_server_without_formatting_says_so` | 2/2 passed | — |
| F-050 | SQL-Skript in Anweisungen zerlegen | `f050_sql_splitting`: `literals_comments_and_dialect_rules_do_not_split`, `ranges_index_into_the_script`, `the_whole_reference_script_runs_statement_by_statement` | 3/3 passed | — |
| F-051 | Verbindungen verwalten | `f051_connections`: `a_profile_is_saved_without_password_and_listed_with_its_url`, `invalid_profiles_are_refused_naming_the_field`, `testing_a_sqlite_connection_and_removing_it`, `testing_the_reference_postgresql`, `a_read_only_connection_refuses_changes` | 5/5 passed | — |
| F-052 | Schema-Introspektion | `f052_introspection`: `sqlite_reference_schema`, `postgresql_reference_schema`, `mysql_reference_schema`, `without_read_schema_both_tools_are_refused` | 4/4 passed | — |
| F-053 | SQL-Editor mit Schema-Vervollständigung | `f053_sql_completion`: `tables_after_from_and_columns_of_aliases`, `completion_against_postgresql`, `sql_inspection_finds_unknown_names_and_unsafe_deletes` | 3/3 passed | — |
| F-054 | Ergebnisraster | `f054_result_grid`: `sort_filter_and_page_a_result`, `export_keeps_null_and_empty_string_apart` | 2/2 passed | — |
| F-055 | Daten bearbeiten | `f055_data_editing`: `preview_apply_and_conflict_on_a_second_apply`, `applying_needs_modify_data_and_keyless_tables_are_refused`, `postgresql_preview_uses_is_not_distinct_from` | 3/3 passed | — |
| F-056 | ER-Diagramme | `f056_er_diagram`: `reference_schema_as_mermaid_dot_and_selection`, `png_is_written_only_inside_the_workspace` | 2/2 passed | — |
| F-057 | Datenbank-Refactorings | `f057_db_refactoring`: `renaming_a_table_rewrites_sql_files_and_returns_the_ddl`, `ambiguous_columns_are_reported_and_unknown_objects_refused`, `applying_on_sqlite_keeps_the_data` | 3/3 passed | — |
| F-058 | Schema-Vergleich, DDL-Erzeugung | `f058_schema_compare`: `differences_script_and_round_trip_on_sqlite`, `create_script_rebuilds_the_schema`, `different_dialects_are_refused` | 3/3 passed | — |
| F-059 | Abfrageplan anzeigen | `f059_query_plan`: `sqlite_plan_tree_with_a_full_scan_hint`, `postgresql_measured_plan_rolls_back_changes`, `mysql_plan_is_read` | 3/3 passed | — |
| F-060 | Weitere Dialekte (Oracle, SQL Server, …) | `f060_more_dialects`: `oracle_and_sql_server_profiles`, `script_separators_of_sql_server_and_oracle`, `mariadb_reference_schema` | 3/3 passed | — |
| F-070 | Debugger: Haltepunkte, Schritte, Variablen | `f070_debugger`: `adapters_are_listed_with_their_availability`, `starting_needs_the_debug_right`, `stop_at_a_breakpoint_read_locals_step_and_run_to_the_end` | 3/3 passed | — |
| F-071 | Bedingte Haltepunkte, Ausdrücke auswerten | `f071_conditional_breakpoints`: `a_condition_stops_once_and_an_expression_is_evaluated`, `saved_breakpoints_keep_condition_and_pass_count` | 2/2 passed | — |
| F-072 | Umbenennen | `f072_rename`: `preview_then_apply_across_files`, `comments_and_strings_on_request` | 2/2 passed | — |
| F-073 | Methode/Variable extrahieren, inline | `f073_extract_inline`: `extract_an_expression_into_a_variable` | 1/1 passed | — |
| F-074 | Git: Status, Diff, Commit, Branches, Log, Blame | `f074_git`: `status_diff_commit_log_and_blame`, `branches_switch_and_merge`, `network_operations_need_the_remote_right` | 3/3 passed | — |
| F-075 | Merge-Konflikte lösen (Drei-Wege) | `f075_merge_conflicts`: `three_way_merge_of_texts`, `a_conflicted_merge_is_resolved_in_the_repository` | 2/2 passed | — |
| F-076 | Integriertes Terminal | `f076_terminal`: `a_command_line_runs_in_the_project_folder_as_the_terminal_shows_it`, `without_the_run_right_only_a_preview`, `escape_sequences_render_like_xterm` | 3/3 passed | terminal_session_steps (tabs, split, restore in a window) |
| F-077 | Run-Konfigurationen | `f077_run_configurations`: `configurations_are_listed_and_previewed_without_running`, `with_the_run_right_the_chain_runs_in_order`, `invalid_files_are_problems` | 3/3 passed | — |
| F-078 | Test-Runner mit Ergebnisbaum | `f078_test_runner`: `junit_tap_and_libtest_reports_become_trees`, `a_test_configuration_attaches_its_report` | 2/2 passed | — |
| F-079 | HTTP-Client (Anfragedateien) | `f079_http_client`: `reference_request_file_is_parsed`, `unresolved_variables_are_never_sent_and_sending_needs_the_http_right`, `a_post_reaches_a_local_server_and_the_answer_comes_back` | 3/3 passed | — |
| F-080 | Plugin-System | `f080_plugins`: `installed_plugins_are_listed_and_their_actions_run_in_the_sandbox`, `a_workspace_plugin_file_is_inspected_but_not_installed` | 2/2 passed | — |
| F-081 | Funktionen offener Plattform-Plugins übernehmen | `f081_open_platform_plugins`: `properties_files_and_resource_bundles`, `project_dictionary_lives_where_the_reference_keeps_it` | 2/2 passed | — |
| F-082 | KI-Assistenz | `f082_ai_assistant`: `off_and_without_provider_by_default_and_no_key_in_the_settings` | 1/1 passed | ai_steps (settings page, Explain Code against a fake endpoint) |
| F-083 | Profiler, Docker, Kubernetes, Remote-Entwicklung | `f083_profiler_containers_remote`: `profiler_lists_own_processes_and_never_the_reference`, `docker_compose_services_of_the_project`, `ssh_hosts_need_the_ssh_right` | 3/3 passed | profiler_steps, containers_steps, remote_steps |
| F-084 | MCP-Server für LLM-Agenten | `f084_mcp_server`: `stdio_and_http_offer_the_same_tools_and_answers`, `paths_outside_the_workspace_are_rejected`, `rights_come_only_from_the_users_settings_folder`, `the_cli_prints_its_version_and_usage` | 4/4 passed | — |
| F-085 | Themes Darcula, Light, New UI, Islands | `f085_themes`: `bundled_themes_are_listed_chosen_and_unknown_names_refused` | 1/1 passed | theme_steps (popup ⌃`, switch to Light and back in a window) |
| F-086 | Icon-Satz New UI | `f086_icons`: — | not tested: icons are pixels in a window; there is no tool or command that shows them. The bundled icons and their dark/light variants are checked by unit tests (SPEC-0060 AK-7) and the window self-test paints them (status_bar_steps, tool_window_steps). | — |
| F-087 | Fensteraufbau (Werkzeugleisten, Tool-Fenster, Statusleiste) und Metriken | `f087_window_layout`: `tool_windows_have_sides_and_can_be_moved`, `the_chosen_theme_has_the_metrics_of_trees_tabs_and_menus` | 2/2 passed | tool_window_steps, status_bar_steps, compact_mode_steps |
| F-088 | Keine Testphase, keine Sperren, keine Telemetrie | `f088_no_licence`: `no_licence_or_trial_tool_and_every_tool_works_without_an_account`, `opening_a_project_writes_nothing_outside_it_and_contacts_no_server` | 2/2 passed | — |
