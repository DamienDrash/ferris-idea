//! IDE lifecycle, layered configuration and workspace model.
//!
//! Implements `SPEC-0001` (workspace), `SPEC-0002` (layered settings),
//! `SPEC-0026` (file watching), `SPEC-0029` (find and replace) and the
//! matcher of `SPEC-0031` (search everywhere) from `docs/specifications/`.

pub mod advanced_settings;
mod appearance;
pub mod bookmarks;
mod branch_favorites;
pub mod buildtasks;
pub mod buildtools;
pub mod changelists;
pub mod commit_messages;
mod config;
pub mod depanalysis;
mod deptree;
pub mod editorconfig;
pub mod file_ops;
pub mod file_properties;
pub mod file_undo;
pub mod format;
mod fuzzy;
mod history;
mod ide_settings;
mod ideaproject;
mod idearun;
mod java;
pub mod jvmdeps;
pub mod jvmsources;
mod lifecycle;
pub mod local_state;
pub mod logs;
mod node;
pub mod notifications;
mod own_sources;
mod permissions;
mod project;
pub mod project_root;
mod python;
mod python_index;
mod python_manage;
mod recent;
pub mod release;
mod ruby;
mod run;
mod run_context;
mod run_edit;
mod run_tasks;
mod search;
pub mod shell_env;
pub mod source_roots;
pub mod spawn;
mod test_history;
mod test_rerun;
mod testreport;
pub mod trust;
mod watch;
mod workspace;

pub use appearance::{
    chosen_theme, keymap_file, legacy_keymap_file, load_theme, save_chosen_theme, theme_file,
};
pub use branch_favorites::{
    BRANCH_FAVORITES, BranchFavorites, DEFAULT_FAVORITES, RepoFavorites, is_default_favorite,
    rename_branch_favorite, repo_key, set_branch_favorite,
};
pub use config::{ConfigError, Layer, LayeredConfig};
pub use deptree::{DepNode, MAX_DEPTH, dependency_tree, module_tasks, without_omitted};
pub use fuzzy::{FuzzyMatch, fuzzy_match, hump_match, hump_words_match, rank, words_match};
pub use history::{
    COMPACT_AT, DEFAULT_RETENTION, HISTORY_DIR, HistoryError, LocalHistory, MAX_RECORDED_BYTES,
    MAX_REVISIONS_PER_FILE, Revision, RevisionKind,
};
pub use ide_settings::{
    ActivateOnClose, AiSettings, AppearanceSettings, CODE_STYLE_LANGUAGES, CloseButtonPlace,
    CodeStyleSettings, DEFAULT_FONT, DatabaseSettings, EDITOR_SCHEMES, EditorSettings,
    EditorTabSettings, FONT_SIZES, FoldingArrows, GitSettings, GitSshSettings, IdeSettings,
    IndentStyle, LINE_SPACINGS, McpSettings, NotificationSettings, ProjectViewSettings,
    SshSettings, SystemSettings, TAB_LIMITS, TabLimitRule, TabPlacement, TabRows, TerminalSettings,
    UiDriverSettings, UiSettings, UpdateMethod, VcsSettings, font_size_step, ide_settings_file,
    load_ide_settings, on_save, save_edits, save_ide_settings,
};
pub use ideaproject::{
    ContentRoot, IDEA_DIR, IdeaDependency, IdeaFacet, IdeaFolder, IdeaLibrary, IdeaModule,
    IdeaProject, ModuleSdk, SourceFolder, SourceKind, VcsMapping,
    folder_roles as idea_folder_roles, idea_run_configs, read_idea_project,
};
pub use idearun::IdeaRun;
pub use java::{
    JavaEnvironment, JavaRequirement, JavaSearch, Jdk, find_jdks, java_environment, java_feature,
    java_requirements,
};
pub use lifecycle::{QUIT_DEADLINE, arm_quit_deadline, write_atomic};
pub use node::{
    NodeEnvironment, NodeRuntime, NodeSearch, NodeUnsatisfied, PackageManager, PackageScript,
    find_node_runtimes, node_environment, package_manager_of, package_script_config,
    package_scripts, satisfies_range,
};
pub use own_sources::{
    FolderProblem, LeftOut, Listing, NotOwn, OwnSources, fold_name, folder_below, is_fixed_name,
    open_regular, same_file,
};
pub use permissions::{
    AGENT_LOG, AgentPermissions, CONNECTION_PERMISSIONS, ConnectionGrant, ConnectionGrants,
    GRANT_FILE, Grant, add_connection_grant, add_grant, agent_permissions, connection_grant_entry,
    connection_grants, connection_target, load_agent_permissions, log_agent_action,
    user_config_dir,
};
pub use project::{
    Dependency, Module, ModuleKind, ProjectModel, library_entries, read_project, split_pep508,
    suggest_run_configs,
};
pub use python::{
    EnvironmentTools, Interpreter, InterpreterKind, Package, PythonEnvironments, PythonSearch,
    Unsatisfied, environment_tools, find_interpreters, installed_packages, normalize_name,
    python_environments, satisfies, selected_interpreter,
};
pub use python_index::{
    Fetch, Fetched, InstallPlan, Installer, PYPI, PackageInfo, Release, SEARCH_LIMIT,
    compare_versions, environment_install_plan, find_tool, http_fetch, install_plan, names,
    names_cache_file, names_cache_fresh, on_path, on_path_in, package_info, project_installer,
    requirement, search_names, user_cache_dir,
};
pub use python_manage::{
    ImportPackage, InstallFix, KNOWN_DISTRIBUTIONS, MISSING_IMPORT_RULES, NO_PYTHON,
    PYTHON_SETTINGS, PackageSource, ToolRun, configured_interpreter, import_package, install_fixes,
    interpreter_choices, interpreter_label, is_bare_python, last_line, parse_specs,
    pip_install_command, pip_spec, python_command, run_tool, save_interpreter, show_python,
    venv_base, venv_command, venv_python, workspace_python,
};
pub use recent::{
    RECENT_PROJECTS_LIMIT, load_recent_projects, recent_projects_file, remember_project,
};
pub use ruby::{
    Bundle, RubyEnvironment, RubyRequest, RubyRuntime, RubySearch, find_ruby_runtimes, read_bundle,
    ruby_environment, ruby_version_matches,
};
pub use run::{
    DEFAULT_TIMEOUT_SECS, PlannedRun, RUN_FILE, RunConfig, RunConfigs, RunEvent, RunHandle,
    RunKind, RunOutcome, STOP_GRACE, add_detected, append_run_config, effective_command,
    end_all_runs, expand_vars, launch_command, live_runs, load_run_configs, parse_run_configs,
    run_config, run_one, run_plan, start_host_run, start_run,
};
pub use run_context::{
    context_config, is_js_test_file, js_test_config, js_test_pattern, single_test_config,
    test_method_config,
};
pub use run_edit::{
    MAX_TEMPORARY, RunEditOp, RunEntry, RunField, RunTemplate, RunTemplates, TypeTemplate,
    apply_op, config_problems, entries_of, entries_problems, folder_name_problem, folders_of,
    free_name, join_command_line, load_run_templates, parse_run_templates, push_temporary,
    quote_arg, rewrite_run_file, rewrite_run_toml, save_run_entries, save_run_file,
    split_command_line, start_blocker, start_blocker_trusted, template_problems,
};
pub use run_tasks::{
    BROWSER_PORT_WAIT, BUILD_PROJECT, BeforeTask, BrowserOpener, SecondStart, TaskKind,
    browser_url, browser_url_problem, instance_title, local_port_open, open_browser,
    running_instances, second_start, second_start_question, set_browser_opener, stop_instances,
    wait_for_local_port,
};
pub use search::{
    FileMatches, Finder, MAX_SEARCH_BYTES, Match, Query, Replacement, SearchError, SearchPlace,
    SearchScope, WorkspaceSearch, left_out_line, matches_mask, not_replaceable, not_searched_line,
    search_candidates, search_scope, search_workspace, searchable_text,
};
pub use test_history::{
    HISTORY_LENGTH, HistoryEntry, history_dir, history_file, load_history, record_history,
    tree_only,
};
pub use test_rerun::rerun_failed;
pub use testreport::{
    ReportError, ReportFormat, TestCase, TestReport, TestStatus, TestSuite, Totals, parse_report,
    read_junit_dir,
};
pub use watch::{
    FileChange, FileState, Snapshot, WATCH_INTERVAL, WatchHandle, diff, settle, watch,
};
pub use workspace::{EXCLUDED_DIRS, Workspace, WorkspaceError};
