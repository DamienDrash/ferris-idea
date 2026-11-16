//! Functional tests, one module per row of `docs/research/FEATURE_MATRIX.md` (ROADMAP M7.4).
//!
//! Written from the matrix row and its specification's acceptance criteria, not from the
//! implementation: every test drives IDECraft from outside, through the `idecraft` command line and
//! MCP (stdio and the Streamable HTTP transport), on copies of `tests/reference-projects`. Rows
//! that only a window shows name their window self-test steps in the module header
//! (`//! self-test:`); rows without behaviour to test name the reason (`//! not-tested:`).
//! `scripts/functional.sh` runs the suite and writes `docs/research/FUNCTIONAL_COVERAGE.md`.

mod harness;

mod f001_open_folder;
mod f002_settings;
mod f003_project_model;
mod f004_idea_project;
mod f010_text_editing;
mod f011_undo_groups;
mod f012_multi_caret;
mod f013_highlighting;
mod f014_folding;
mod f015_find_replace;
mod f016_search_everywhere;
mod f017_keymap;
mod f018_reference_keymap;
mod f019_local_history;
mod f020_tabs_split;
mod f030_language_detection;
mod f031_completion;
mod f032_diagnostics;
mod f033_quick_fixes;
mod f034_navigation;
mod f035_python;
mod f036_javascript;
mod f037_java_kotlin;
mod f038_other_languages;
mod f039_formatting;
mod f040_frameworks;
mod f050_sql_splitting;
mod f051_connections;
mod f052_introspection;
mod f053_sql_completion;
mod f054_result_grid;
mod f055_data_editing;
mod f056_er_diagram;
mod f057_db_refactoring;
mod f058_schema_compare;
mod f059_query_plan;
mod f060_more_dialects;
mod f070_debugger;
mod f071_conditional_breakpoints;
mod f072_rename;
mod f073_extract_inline;
mod f074_git;
mod f075_merge_conflicts;
mod f076_terminal;
mod f077_run_configurations;
mod f078_test_runner;
mod f079_http_client;
mod f080_plugins;
mod f081_open_platform_plugins;
mod f082_ai_assistant;
mod f083_profiler_containers_remote;
mod f084_mcp_server;
mod f085_themes;
mod f086_icons;
mod f087_window_layout;
mod f088_no_licence;
mod f089_collaboration;
