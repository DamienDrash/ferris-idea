//! What each MCP tool changes, how it is asked for a dry run, and which rights it checks: one
//! table for two rules (`SPEC-0053` v7 behaviour 11, v8 behaviour 13, ADR-0031).
//!
//! - **The log.** `call_tool` writes one line to the agent log for every call of a tool that
//!   changes something or uses a database connection, from this table, whatever the tool does.
//! - **The rights.** A tool that changes something checks at least one right or database
//!   permission; a tool that uses a database checks the connection's permission.
//!
//! Both rules read [`TOOLS`], so a tool cannot satisfy one and miss the other. A tool missing from
//! the table, a changing tool without a check and a check the handler's code does not make fail
//! the tests (`effect_tests` in `lib.rs`, `tests/it/effects.rs`). A new tool gets its row here in
//! the same change, written from what its code does, not from its description.

use idecraft_core::Grant;
use serde_json::{Map, Value, json};

/// What a tool changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Files of the workspace (or of the user's settings folder for workspace data).
    Files,
    /// The Git repository.
    Repository,
    /// A database or store.
    Database,
    /// The user's or the workspace's settings, rights and grants.
    Settings,
    /// The state of the running window (tabs, tree, UI automation, quitting, closing).
    Ide,
    /// Processes: runs, tests, debug sessions, statements, terminals, containers.
    Processes,
    /// A remote host or remote project.
    Remote,
    /// Network connections: requests, port forwards.
    Network,
}

impl Change {
    /// The word in the log line.
    pub fn name(self) -> &'static str {
        match self {
            Change::Files => "files",
            Change::Repository => "repository",
            Change::Database => "database",
            Change::Settings => "settings",
            Change::Ide => "ide",
            Change::Processes => "processes",
            Change::Remote => "remote",
            Change::Network => "network",
        }
    }
}

/// How a call says whether it only shows what it would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dry {
    /// The tool has no dry run: every call is real.
    None,
    /// `dry_run`, with its default when left out.
    DryRun(bool),
    /// `apply`: a dry run unless `apply` is true.
    Apply,
}

/// What a tool reaches without changing it. Reading through a process or a network connection
/// needs no right (the known limits of `SPEC-0053` v8 behaviour 14); a database always needs the
/// connection's permission (`SPEC-0012` v3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// A database connection or store of the project.
    Database,
    /// A process: a language server, a fixed system tool.
    Process,
    /// A network connection.
    Network,
}

/// A check a tool's code makes before its effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// An agent right of the workspace (`SPEC-0053`).
    Right(Grant),
    /// The database permission of the connection or store (`SPEC-0012` v3).
    Database,
    /// The project is trusted (`SPEC-0278`); never the only check of a change.
    Trusted,
    /// The user is asked in the window for exactly this action.
    AsksUser,
    /// No right on purpose: the emergency stop of the UI driver (`SPEC-0319` behaviour 37,
    /// ADR-0027). Only `ui_driver_stop` may carry it.
    EmergencyStop,
    /// No right on purpose: the tool reads, and what it writes is a record IDECraft keeps by
    /// itself (`SPEC-0053` behaviour 6). Only `local_history` may carry it; its calls are logged.
    OwnRecords,
}

/// One tool: what it changes (empty: it only reads), how a dry run is asked for, what it reaches
/// while reading, and the checks its code makes.
#[derive(Debug, Clone, Copy)]
pub struct ToolEffect {
    /// The tool's name.
    pub tool: &'static str,
    /// What it changes; empty when it only reads.
    pub changes: &'static [Change],
    /// How a call asks for a dry run.
    pub dry: Dry,
    /// What it reaches while reading.
    pub reaches: &'static [Reach],
    /// The checks its code makes.
    pub checks: &'static [Check],
}

impl ToolEffect {
    /// Whether a call of the tool is written to the agent log: it changes something, or it uses
    /// a database connection (`SPEC-0012` v3 behaviour 16).
    pub fn logged(&self) -> bool {
        !self.changes.is_empty() || self.reaches.contains(&Reach::Database)
    }

    /// Whether the tool uses a database connection, changing it or not.
    pub fn uses_database(&self) -> bool {
        self.changes.contains(&Change::Database) || self.reaches.contains(&Reach::Database)
    }
}

const RUN: Check = Check::Right(Grant::Run);
const HTTP: Check = Check::Right(Grant::Http);
const DEBUG: Check = Check::Right(Grant::Debug);
const REMOTE: Check = Check::Right(Grant::Remote);
const SSH: Check = Check::Right(Grant::Ssh);
const TRUST: Check = Check::Right(Grant::Trust);
const WRITE: Check = Check::Right(Grant::Write);
const VCS_WRITE: Check = Check::Right(Grant::VcsWrite);
const QUIT: Check = Check::Right(Grant::Quit);
const TERMINAL_READ: Check = Check::Right(Grant::TerminalRead);
const PORTS: Check = Check::Right(Grant::Ports);
const UI: Check = Check::Right(Grant::Ui);
const SETTINGS: Check = Check::Right(Grant::Settings);
const DATABASE: Check = Check::Database;
const TRUSTED: Check = Check::Trusted;
const ASKS: Check = Check::AsksUser;
const EMERGENCY_STOP: Check = Check::EmergencyStop;
const OWN_RECORDS: Check = Check::OwnRecords;

const fn tool(
    tool: &'static str,
    changes: &'static [Change],
    dry: Dry,
    reaches: &'static [Reach],
    checks: &'static [Check],
) -> ToolEffect {
    ToolEffect {
        tool,
        changes,
        dry,
        reaches,
        checks,
    }
}

/// Every tool of the server, sorted by name: what it changes, its dry run, what it reaches while
/// reading, its checks.
#[rustfmt::skip]
pub const TOOLS: &[ToolEffect] = &[
    tool("agent_permissions", &[], Dry::None, &[], &[]),
    tool("ai_assistant", &[], Dry::None, &[Reach::Process], &[]),
    tool("apply_code_action", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE]),
    tool("bookmarks", &[], Dry::None, &[], &[]),
    tool("browse_store", &[], Dry::None, &[Reach::Database, Reach::Network], &[DATABASE, TRUSTED]),
    tool("build_command_suggestions", &[], Dry::None, &[], &[]),
    tool("build_task", &[Change::Files], Dry::None, &[], &[WRITE]),
    tool("build_tool_links", &[Change::Files], Dry::None, &[], &[WRITE]),
    tool("build_tool_options", &[Change::Files], Dry::None, &[], &[WRITE]),
    tool("call_hierarchy", &[], Dry::None, &[Reach::Process], &[]),
    tool("cancel_sql", &[Change::Processes], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("change_column_type", &[Change::Database], Dry::Apply, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("change_signature", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE]),
    tool("changelists", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("check_grammar", &[], Dry::None, &[], &[]),
    tool("class_members", &[], Dry::None, &[], &[]),
    tool("clone_repository", &[Change::Files, Change::Network], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[REMOTE, WRITE]),
    tool("close_project", &[Change::Files, Change::Ide], Dry::DryRun(true), &[], &[WRITE, QUIT]),
    tool("code_actions", &[], Dry::DryRun(true), &[Reach::Process], &[]),
    tool("code_authors", &[], Dry::None, &[], &[]),
    tool("compare_files", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("compare_schemas", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("complete_code", &[], Dry::None, &[Reach::Process], &[]),
    tool("complete_sql", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("config_key", &[], Dry::None, &[], &[]),
    tool("copy_file", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("copy_table", &[Change::Database], Dry::Apply, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("create_file", &[Change::Files], Dry::None, &[], &[WRITE]),
    tool("database_problem_report", &[], Dry::None, &[], &[]),
    tool("debug_adapters", &[], Dry::None, &[Reach::Process], &[]),
    tool("debug_attach", &[Change::Ide, Change::Processes], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[DEBUG]),
    tool("debug_breakpoints", &[Change::Ide, Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("debug_completions", &[Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("debug_evaluate", &[Change::Ide, Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("debug_field_watch", &[Change::Ide, Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("debug_hot_swap", &[Change::Ide, Change::Processes], Dry::None, &[Reach::Process], &[DEBUG]),
    tool("debug_inspect", &[Change::Ide, Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("debug_pause", &[Change::Ide, Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("debug_processes", &[], Dry::None, &[Reach::Process], &[]),
    tool("debug_renderers", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("debug_reset_frame", &[Change::Ide, Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("debug_resume", &[Change::Ide, Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("debug_run_to_line", &[Change::Ide, Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("debug_sessions", &[], Dry::None, &[], &[]),
    tool("debug_set_variable", &[Change::Ide, Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("debug_start", &[Change::Ide, Change::Processes], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[DEBUG, TRUSTED]),
    tool("debug_step_targets", &[Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("debug_stop", &[Change::Ide, Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("debug_threads", &[Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("debug_variables", &[Change::Processes], Dry::None, &[], &[DEBUG]),
    tool("declaration_marks", &[], Dry::None, &[Reach::Process], &[]),
    tool("delegate_methods", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE]),
    tool("delete_file", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("dependency_analysis", &[Change::Processes], Dry::None, &[Reach::Process, Reach::Network], &[RUN, TRUSTED]),
    tool("dependency_tree", &[Change::Processes], Dry::None, &[Reach::Process, Reach::Network], &[RUN, TRUSTED]),
    tool("describe_table", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("detect_language", &[], Dry::None, &[], &[]),
    tool("device_list", &[Change::Remote], Dry::None, &[], &[SSH]),
    tool("device_log", &[Change::Remote], Dry::None, &[], &[SSH]),
    tool("device_restrict", &[Change::Settings, Change::Remote], Dry::None, &[], &[SSH]),
    tool("device_revoke", &[Change::Settings, Change::Remote], Dry::None, &[], &[SSH]),
    tool("dialog_decisions", &[], Dry::None, &[], &[]),
    tool("docker_compose", &[Change::Processes], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[RUN]),
    tool("docker_container", &[Change::Processes], Dry::DryRun(true), &[Reach::Network], &[RUN]),
    tool("docker_exec", &[Change::Processes], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[RUN]),
    tool("docker_status", &[], Dry::None, &[Reach::Network], &[]),
    tool("document_highlights", &[], Dry::None, &[Reach::Process], &[]),
    tool("document_symbols", &[], Dry::None, &[Reach::Process], &[]),
    tool("edit_table_data", &[Change::Database], Dry::Apply, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("editor_action", &[], Dry::None, &[Reach::Process], &[]),
    tool("editor_keystrokes", &[], Dry::None, &[], &[]),
    tool("editor_tabs", &[Change::Ide], Dry::None, &[], &[UI]),
    tool("editorconfig", &[], Dry::None, &[], &[]),
    tool("endpoints", &[], Dry::None, &[], &[]),
    tool("environment_source", &[], Dry::None, &[], &[]),
    tool("er_diagram", &[Change::Files], Dry::None, &[Reach::Database, Reach::Network], &[SSH, WRITE, DATABASE, TRUSTED]),
    tool("eslint_file", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE, TRUSTED]),
    tool("explain_sql", &[Change::Database], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("export_connection", &[], Dry::None, &[], &[]),
    tool("export_connector", &[], Dry::None, &[], &[]),
    tool("export_data", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("export_file", &[Change::Files], Dry::None, &[], &[WRITE]),
    tool("export_store_result", &[Change::Files], Dry::None, &[Reach::Database, Reach::Network], &[WRITE, DATABASE, TRUSTED]),
    tool("extract_interface", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE]),
    tool("extract_superclass", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("file_changes", &[], Dry::None, &[], &[]),
    tool("file_diagnostics", &[], Dry::None, &[Reach::Process], &[]),
    tool("file_operation_undo", &[Change::Files, Change::Ide], Dry::None, &[], &[WRITE]),
    tool("file_properties", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("file_type", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("find_symbol_locations", &[], Dry::None, &[Reach::Process], &[]),
    tool("find_usages", &[], Dry::None, &[Reach::Process], &[]),
    tool("fix_inspection", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE, TRUSTED]),
    tool("folding_ranges", &[], Dry::None, &[], &[]),
    tool("format_file", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE, TRUSTED]),
    tool("generate_choices", &[], Dry::None, &[], &[]),
    tool("generate_code", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("generate_ddl", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("git_blame", &[], Dry::None, &[], &[]),
    tool("git_branch_action", &[Change::Files, Change::Repository], Dry::DryRun(true), &[Reach::Process], &[VCS_WRITE]),
    tool("git_branch_favorite", &[Change::Files, Change::Settings], Dry::None, &[], &[WRITE]),
    tool("git_branches", &[], Dry::None, &[], &[]),
    tool("git_cherry_pick", &[Change::Files, Change::Repository], Dry::DryRun(true), &[Reach::Process], &[VCS_WRITE]),
    tool("git_commit", &[Change::Repository], Dry::DryRun(true), &[Reach::Process], &[VCS_WRITE]),
    tool("git_commit_action", &[Change::Files, Change::Repository], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[REMOTE, VCS_WRITE]),
    tool("git_conflicts", &[], Dry::None, &[], &[]),
    tool("git_create_branch", &[Change::Repository], Dry::None, &[], &[VCS_WRITE]),
    tool("git_diff", &[], Dry::None, &[], &[]),
    tool("git_fetch", &[Change::Repository, Change::Remote], Dry::None, &[Reach::Process, Reach::Network], &[REMOTE]),
    tool("git_line_change", &[Change::Files, Change::Repository], Dry::DryRun(true), &[Reach::Process], &[VCS_WRITE]),
    tool("git_log", &[], Dry::None, &[Reach::Process], &[]),
    tool("git_merge", &[Change::Files, Change::Repository], Dry::DryRun(true), &[Reach::Process], &[VCS_WRITE]),
    tool("git_outgoing", &[], Dry::None, &[Reach::Process], &[]),
    tool("git_pull", &[Change::Files, Change::Repository, Change::Remote], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[REMOTE, VCS_WRITE]),
    tool("git_push", &[Change::Repository, Change::Remote], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[REMOTE, VCS_WRITE]),
    tool("git_rebase", &[Change::Files, Change::Repository], Dry::DryRun(true), &[Reach::Process], &[VCS_WRITE]),
    tool("git_remotes", &[], Dry::None, &[], &[]),
    tool("git_repositories", &[], Dry::None, &[], &[]),
    tool("git_resolve_conflict", &[Change::Files, Change::Repository], Dry::DryRun(true), &[], &[VCS_WRITE]),
    tool("git_rollback", &[Change::Files, Change::Repository], Dry::DryRun(true), &[], &[VCS_WRITE]),
    tool("git_show", &[], Dry::None, &[], &[]),
    tool("git_stash", &[Change::Files, Change::Repository], Dry::None, &[Reach::Process], &[VCS_WRITE]),
    tool("git_status", &[], Dry::None, &[], &[]),
    tool("git_switch_branch", &[Change::Files, Change::Repository], Dry::DryRun(true), &[], &[VCS_WRITE]),
    tool("goto_test", &[], Dry::None, &[], &[]),
    tool("grant_plugin_network", &[Change::Settings], Dry::None, &[], &[TRUSTED, ASKS]),
    tool("header_source", &[], Dry::None, &[], &[]),
    tool("highlight_file", &[], Dry::None, &[], &[]),
    tool("highlighting_level", &[Change::Files], Dry::None, &[], &[WRITE]),
    tool("hover_symbol", &[], Dry::None, &[Reach::Process], &[]),
    tool("http_requests", &[], Dry::None, &[], &[]),
    tool("ide_settings", &[], Dry::None, &[], &[]),
    tool("idea_project", &[], Dry::None, &[], &[]),
    tool("import_connector", &[Change::Files, Change::Settings], Dry::DryRun(true), &[Reach::Network], &[HTTP, SETTINGS]),
    tool("import_file", &[Change::Files, Change::Settings, Change::Ide], Dry::DryRun(true), &[], &[WRITE, UI, SETTINGS]),
    tool("import_hint", &[], Dry::None, &[Reach::Process], &[]),
    tool("inlay_hints", &[], Dry::None, &[Reach::Process], &[]),
    tool("inspect_code", &[], Dry::None, &[], &[]),
    tool("inspect_plugin_file", &[], Dry::None, &[], &[]),
    tool("inspect_sql", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("inspection_profile", &[Change::Files], Dry::None, &[], &[WRITE]),
    tool("java_environment", &[], Dry::None, &[], &[]),
    tool("keyboard_diagnostics", &[], Dry::None, &[], &[]),
    tool("keymap_lookup", &[], Dry::None, &[], &[]),
    tool("kubernetes_exec", &[Change::Processes], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[RUN]),
    tool("kubernetes_port_forward", &[Change::Ide, Change::Processes], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[RUN]),
    tool("kubernetes_status", &[], Dry::None, &[Reach::Process, Reach::Network], &[]),
    tool("kubernetes_workload", &[Change::Processes], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[RUN]),
    tool("language_servers", &[], Dry::None, &[Reach::Process], &[]),
    tool("library_sources", &[Change::Processes], Dry::DryRun(false), &[Reach::Process, Reach::Network], &[RUN, TRUSTED]),
    tool("list_connections", &[], Dry::None, &[], &[]),
    tool("list_connectors", &[], Dry::None, &[], &[]),
    tool("list_files", &[], Dry::None, &[], &[]),
    tool("list_schemas", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("list_stores", &[], Dry::None, &[], &[]),
    tool("list_tables", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("local_history", &[Change::Files], Dry::None, &[], &[OWN_RECORDS]),
    tool("merge_three_way", &[], Dry::None, &[], &[]),
    tool("method_hierarchy", &[], Dry::None, &[Reach::Process], &[]),
    tool("move_file", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE]),
    tool("new_project", &[Change::Files, Change::Repository], Dry::DryRun(true), &[Reach::Process], &[WRITE]),
    tool("node_environment", &[], Dry::None, &[], &[]),
    tool("notifications", &[], Dry::None, &[], &[]),
    tool("object_definition", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("object_doc", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("open_documents", &[], Dry::None, &[], &[]),
    tool("optimize_imports", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE, TRUSTED]),
    tool("override_methods", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE]),
    tool("package_scripts", &[], Dry::None, &[], &[]),
    tool("parse_test_report", &[], Dry::None, &[], &[]),
    tool("paste_history", &[], Dry::None, &[], &[]),
    tool("plugin_file_type", &[], Dry::None, &[], &[]),
    tool("plugin_inspections", &[], Dry::None, &[], &[]),
    tool("plugin_network", &[], Dry::None, &[], &[]),
    tool("plugin_run_configurations", &[], Dry::None, &[], &[]),
    tool("plugins", &[], Dry::None, &[], &[]),
    tool("prettier_file", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE, TRUSTED]),
    tool("preview_column_edit", &[], Dry::None, &[], &[]),
    tool("preview_multi_caret_edit", &[], Dry::None, &[], &[]),
    tool("problem_preview", &[], Dry::None, &[], &[]),
    tool("profile_attach", &[Change::Processes], Dry::DryRun(true), &[Reach::Process], &[RUN]),
    tool("profile_last", &[], Dry::None, &[], &[]),
    tool("profile_processes", &[], Dry::None, &[Reach::Process], &[]),
    tool("profile_run", &[Change::Processes], Dry::DryRun(true), &[Reach::Process], &[RUN, TRUSTED]),
    tool("project_dictionary", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("project_model", &[], Dry::None, &[], &[]),
    tool("project_tree", &[Change::Settings, Change::Ide], Dry::None, &[], &[UI, SETTINGS]),
    tool("project_trust", &[], Dry::None, &[], &[]),
    tool("properties_file", &[], Dry::None, &[], &[]),
    tool("pull_members_up", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE]),
    tool("python_create_venv", &[Change::Files, Change::Processes], Dry::DryRun(true), &[Reach::Process], &[RUN, WRITE]),
    tool("python_environments", &[], Dry::None, &[], &[]),
    tool("python_install_packages", &[Change::Processes], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[RUN, TRUSTED]),
    tool("python_language_server", &[Change::Ide], Dry::Apply, &[], &[WRITE]),
    tool("python_package_info", &[Change::Processes, Change::Network], Dry::None, &[Reach::Process, Reach::Network], &[RUN, HTTP]),
    tool("python_search_packages", &[Change::Network], Dry::None, &[Reach::Network], &[HTTP]),
    tool("python_select_interpreter", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("quick_definition", &[], Dry::None, &[Reach::Process], &[]),
    tool("quit_ide", &[Change::Files, Change::Ide], Dry::None, &[], &[WRITE, QUIT]),
    tool("read_file", &[], Dry::None, &[], &[]),
    tool("read_table_data", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("recent_projects", &[], Dry::None, &[], &[]),
    tool("reference_index", &[], Dry::None, &[], &[]),
    tool("related_files", &[], Dry::None, &[], &[]),
    tool("remote_access_off", &[Change::Settings, Change::Remote], Dry::None, &[], &[SSH]),
    tool("remote_access_status", &[Change::Remote], Dry::None, &[], &[SSH]),
    tool("remote_backend_call", &[Change::Remote], Dry::None, &[Reach::Network], &[SSH]),
    tool("remote_backend_start", &[Change::Processes, Change::Remote], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[RUN, SSH]),
    tool("remote_backend_stop", &[Change::Ide, Change::Processes], Dry::DryRun(true), &[], &[SSH]),
    tool("remote_create", &[Change::Files, Change::Remote], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[SSH]),
    tool("remote_debug_attach", &[Change::Ide, Change::Processes], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[DEBUG]),
    tool("remote_debug_start", &[Change::Ide, Change::Processes], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[DEBUG]),
    tool("remote_delete", &[Change::Files, Change::Remote], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[SSH]),
    tool("remote_documents", &[], Dry::None, &[], &[]),
    tool("remote_exec", &[Change::Processes, Change::Remote], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[RUN, SSH]),
    tool("remote_host_probe", &[Change::Remote], Dry::None, &[Reach::Process, Reach::Network], &[SSH]),
    tool("remote_hosts", &[], Dry::None, &[Reach::Process], &[]),
    tool("remote_hosts_paired", &[], Dry::None, &[], &[]),
    tool("remote_list", &[Change::Remote], Dry::None, &[Reach::Process, Reach::Network], &[SSH]),
    tool("remote_pair_add_path", &[Change::Settings, Change::Remote], Dry::None, &[], &[SSH]),
    tool("remote_pair_forget", &[Change::Settings, Change::Remote], Dry::None, &[], &[SSH]),
    tool("remote_pair_start", &[Change::Settings, Change::Remote], Dry::None, &[Reach::Network], &[SSH]),
    tool("remote_port_close", &[Change::Ide, Change::Network], Dry::None, &[Reach::Network], &[PORTS]),
    tool("remote_port_forward", &[Change::Ide, Change::Network], Dry::DryRun(true), &[Reach::Network], &[PORTS]),
    tool("remote_ports", &[], Dry::None, &[Reach::Network], &[]),
    tool("remote_project_close", &[Change::Files, Change::Ide], Dry::DryRun(true), &[Reach::Network], &[SSH, WRITE, QUIT]),
    tool("remote_project_open", &[Change::Ide, Change::Processes, Change::Remote], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[RUN, SSH]),
    tool("remote_project_reconnect", &[Change::Ide, Change::Remote], Dry::None, &[Reach::Process, Reach::Network], &[SSH]),
    tool("remote_projects", &[], Dry::None, &[], &[]),
    tool("remote_read", &[Change::Remote], Dry::None, &[Reach::Process, Reach::Network], &[SSH]),
    tool("remote_rename", &[Change::Files, Change::Remote], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[SSH]),
    tool("remote_write", &[Change::Files, Change::Remote], Dry::DryRun(true), &[Reach::Process, Reach::Network], &[SSH]),
    tool("remove_bookmark", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("remove_connection", &[Change::Files, Change::Settings], Dry::None, &[], &[WRITE]),
    tool("rename_file", &[Change::Files], Dry::None, &[], &[WRITE]),
    tool("rename_sql_object", &[Change::Files, Change::Database], Dry::Apply, &[Reach::Database, Reach::Network], &[SSH, WRITE, DATABASE, TRUSTED]),
    tool("rename_symbol", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE]),
    tool("replace_in_files_preview", &[], Dry::None, &[], &[]),
    tool("resource_bundle", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("restore_local_revision", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("revoke_plugin_network", &[Change::Settings], Dry::None, &[], &[SETTINGS]),
    tool("revoke_plugin_run_permission", &[Change::Settings], Dry::None, &[], &[SETTINGS]),
    tool("revoke_project_trust", &[Change::Settings], Dry::None, &[], &[TRUST]),
    tool("room_apply", &[Change::Files], Dry::None, &[Reach::Network], &[WRITE]),
    tool("room_create", &[Change::Remote], Dry::None, &[Reach::Network], &[SSH]),
    tool("room_history", &[], Dry::None, &[Reach::Network], &[]),
    tool("room_join", &[Change::Files], Dry::None, &[Reach::Network], &[WRITE]),
    tool("room_links", &[], Dry::None, &[Reach::Network], &[]),
    tool("room_restore", &[Change::Files], Dry::None, &[Reach::Network], &[WRITE]),
    tool("room_revoke_link", &[Change::Remote], Dry::None, &[Reach::Network], &[SSH]),
    tool("room_set_role", &[Change::Remote], Dry::None, &[Reach::Network], &[SSH]),
    tool("room_share", &[Change::Remote], Dry::None, &[Reach::Network], &[SSH]),
    tool("room_status", &[], Dry::None, &[Reach::Network], &[]),
    tool("room_undo", &[Change::Files], Dry::None, &[Reach::Network], &[WRITE]),
    tool("room_version", &[Change::Files], Dry::None, &[Reach::Network], &[WRITE]),
    tool("ruby_environment", &[], Dry::None, &[], &[]),
    tool("run_command", &[Change::Processes], Dry::DryRun(true), &[Reach::Process], &[RUN, TRUSTED]),
    tool("run_configuration", &[Change::Processes], Dry::DryRun(true), &[Reach::Process], &[RUN, TRUSTED]),
    tool("run_configuration_edit", &[Change::Files, Change::Processes], Dry::DryRun(true), &[Reach::Process], &[RUN, WRITE, TRUSTED]),
    tool("run_configurations", &[], Dry::None, &[], &[]),
    tool("run_markers", &[], Dry::DryRun(true), &[], &[]),
    tool("run_output", &[], Dry::None, &[], &[]),
    tool("run_plugin_action", &[], Dry::None, &[], &[]),
    tool("run_plugin_configuration", &[Change::Processes], Dry::DryRun(true), &[Reach::Process], &[RUN]),
    tool("run_sql", &[Change::Database], Dry::Apply, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("run_store_command", &[Change::Database], Dry::Apply, &[Reach::Database, Reach::Network], &[DATABASE, TRUSTED]),
    tool("safe_delete", &[Change::Files], Dry::DryRun(true), &[Reach::Process], &[WRITE]),
    tool("save_connection", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("save_store", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("saved_breakpoints", &[], Dry::None, &[], &[]),
    tool("schema_objects", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("search_everywhere", &[], Dry::None, &[], &[]),
    tool("search_in_files", &[], Dry::None, &[], &[]),
    tool("semantic_tokens", &[], Dry::None, &[Reach::Process], &[]),
    tool("send_http_request", &[Change::Network], Dry::DryRun(true), &[Reach::Network], &[HTTP]),
    tool("set_bookmark", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("set_breakpoint", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("set_ide_settings", &[Change::Settings], Dry::DryRun(true), &[], &[RUN, SETTINGS]),
    tool("set_shortcut", &[Change::Settings], Dry::None, &[], &[SETTINGS]),
    tool("set_special_breakpoints", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("signature_help", &[], Dry::None, &[Reach::Process], &[]),
    tool("smart_complete_code", &[], Dry::None, &[Reach::Process], &[]),
    tool("spelling_dictionaries", &[], Dry::None, &[], &[]),
    tool("split_sql_statements", &[], Dry::None, &[], &[]),
    tool("spring_beans", &[], Dry::None, &[], &[]),
    tool("sql_console", &[Change::Files], Dry::None, &[Reach::Database, Reach::Network], &[SSH, WRITE, DATABASE, TRUSTED]),
    tool("sql_file_data_source", &[Change::Files], Dry::None, &[], &[WRITE, TRUSTED]),
    tool("sql_history", &[], Dry::None, &[Reach::Database], &[SSH, DATABASE, TRUSTED]),
    tool("sql_refactorings_at", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("suggest_names", &[], Dry::None, &[], &[]),
    tool("super_method", &[], Dry::None, &[Reach::Process], &[]),
    tool("suppress_inspection", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("surround_with", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("table_ddl", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("terminal_command", &[Change::Processes], Dry::DryRun(true), &[Reach::Process], &[RUN]),
    tool("terminal_read", &[], Dry::None, &[], &[TERMINAL_READ]),
    tool("terminal_render", &[], Dry::None, &[], &[]),
    tool("terminal_sessions", &[], Dry::None, &[], &[]),
    tool("test_connection", &[], Dry::None, &[Reach::Database, Reach::Network], &[SSH, DATABASE, TRUSTED]),
    tool("themes", &[Change::Settings], Dry::None, &[], &[SETTINGS]),
    tool("todo_items", &[], Dry::None, &[], &[]),
    tool("tool_windows", &[Change::Files], Dry::None, &[], &[WRITE]),
    tool("trust_project", &[Change::Settings], Dry::DryRun(true), &[], &[TRUST]),
    tool("type_hierarchy", &[], Dry::None, &[Reach::Process], &[]),
    tool("ui_act", &[Change::Ide], Dry::None, &[], &[UI]),
    tool("ui_driver_status", &[], Dry::None, &[], &[]),
    tool("ui_driver_stop", &[Change::Settings, Change::Ide], Dry::None, &[], &[EMERGENCY_STOP]),
    tool("ui_expect", &[], Dry::None, &[], &[UI]),
    tool("ui_find", &[], Dry::None, &[], &[UI]),
    tool("ui_screenshot", &[], Dry::None, &[], &[UI]),
    tool("ui_snapshot", &[], Dry::None, &[], &[UI]),
    tool("ui_tooltips", &[], Dry::None, &[], &[]),
    tool("ui_trace", &[Change::Files, Change::Ide], Dry::None, &[], &[WRITE, UI]),
    tool("undo_refactoring", &[Change::Files], Dry::None, &[], &[WRITE]),
    tool("update_copyright", &[Change::Files], Dry::DryRun(true), &[], &[WRITE]),
    tool("usage_counts", &[], Dry::None, &[Reach::Process], &[]),
    tool("validate_connector", &[], Dry::None, &[], &[]),
    tool("web_endpoint_check", &[Change::Remote], Dry::None, &[Reach::Network], &[SSH]),
    tool("window_layouts", &[Change::Files, Change::Settings], Dry::DryRun(true), &[], &[WRITE, SETTINGS]),
    tool("workspace_info", &[], Dry::None, &[], &[]),
];

/// The entry of `tool`, if it is listed.
pub fn effect(tool: &str) -> Option<&'static ToolEffect> {
    TOOLS
        .binary_search_by(|e| e.tool.cmp(tool))
        .ok()
        .map(|i| &TOOLS[i])
}

/// Whether the call with `args` only shows what it would do.
pub fn is_dry_run(e: &ToolEffect, args: Option<&Map<String, Value>>) -> bool {
    let get = |k: &str| args.and_then(|a| a.get(k)).and_then(Value::as_bool);
    match e.dry {
        Dry::None => false,
        Dry::DryRun(default) => get("dry_run").unwrap_or(default),
        Dry::Apply => !get("apply").unwrap_or(false),
    }
}

/// Arguments whose text names what a call acts on: logged as they are (cut at 200 characters).
/// Every other text is logged by its length only, so no content, statement, command or value
/// reaches the log (`SPEC-0053` v7 behaviour 11).
const NAMED: &[&str] = &[
    "path",
    "paths",
    "files",
    "from",
    "to",
    "left",
    "right",
    "dir",
    "name",
    "new_name",
    "connection",
    "table",
    "schema",
    "store",
    "branch",
    "remote",
    "configuration",
    "config",
    "host",
    "folder",
    "scope",
    "action",
    "refactoring",
    "kind",
    "mode",
    "pending",
    "unsaved",
    "language",
    "plugin",
    "target",
    "group",
    "key",
    "id",
    "session",
    "room",
    "role",
];

/// How a call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The tool answered.
    Ok,
    /// A right was missing or denied: nothing was done.
    Refused,
    /// The tool failed.
    Error,
}

impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Refused => "refused",
            Outcome::Error => "error",
        }
    }
}

/// The log line of a call of `tool` with `args`, the rights checked during it (each with whether
/// it was given), the database permissions decided during it (`database`, one entry per decision:
/// connection, permissions, allowed or why refused, length and kinds of the statements —
/// `SPEC-0012` v3 behaviour 16) and its `outcome`, at `time`; `None` for a tool that only reads
/// and uses no database. A call whose database decisions were all previews is a dry run, whatever
/// its arguments say.
pub fn call_line(
    tool: &str,
    args: Option<&Map<String, Value>>,
    rights: &[(&'static str, bool)],
    database: &[Value],
    outcome: Outcome,
    time: u64,
) -> Option<Value> {
    let e = effect(tool).filter(|e| e.logged())?;
    let dry_run = if database.is_empty() {
        is_dry_run(e, args)
    } else {
        database.iter().all(|d| d["dry_run"] == true)
    };
    let mut targets = Map::new();
    let mut lengths = Map::new();
    for (k, v) in args.into_iter().flatten() {
        let named = NAMED.contains(&k.as_str());
        match v {
            Value::String(s) if named => {
                targets.insert(k.clone(), Value::String(cut(s)));
            }
            Value::String(s) => {
                lengths.insert(k.clone(), json!(s.chars().count()));
            }
            Value::Array(items) if named && items.iter().all(Value::is_string) => {
                let shown: Vec<Value> = items
                    .iter()
                    .take(20)
                    .filter_map(Value::as_str)
                    .map(|s| Value::String(cut(s)))
                    .collect();
                targets.insert(k.clone(), Value::Array(shown));
            }
            Value::Array(items) => {
                lengths.insert(k.clone(), json!(items.len()));
            }
            Value::Bool(_) | Value::Number(_) if named || k == "dry_run" || k == "apply" => {
                targets.insert(k.clone(), v.clone());
            }
            Value::Object(o) => {
                lengths.insert(k.clone(), json!(o.len()));
            }
            _ => {}
        }
    }
    Some(json!({
        "time": time,
        "tool": tool,
        "call": true,
        "changes": e.changes.iter().map(|c| c.name()).collect::<Vec<_>>(),
        "dry_run": dry_run,
        "rights": rights
            .iter()
            .map(|(r, given)| json!({"right": r, "given": given}))
            .collect::<Vec<_>>(),
        "database": database,
        "targets": targets,
        "lengths": lengths,
        "result": outcome.name(),
    }))
}

fn cut(s: &str) -> String {
    if s.chars().count() <= 200 {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(199).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0053 v7 AK-11: the table is sorted (binary search) and names each tool once.
    #[test]
    fn the_table_is_sorted_and_names_each_tool_once() {
        for w in TOOLS.windows(2) {
            assert!(w[0].tool < w[1].tool, "{} before {}", w[0].tool, w[1].tool);
        }
    }

    /// SPEC-0053 v7 AK-11: every changing tool gets a line for a real call, refused or not; a
    /// reading tool none; texts that are not names are logged by their length only.
    #[test]
    fn every_changing_tool_gets_a_line_and_no_content() {
        let args = json!({"path": "src/a.txt", "text": "secret content", "dry_run": false,
                          "apply": true, "sql": "drop table t"});
        let args = args.as_object();
        for e in TOOLS {
            let line = call_line(e.tool, args, &[("write", true)], &[], Outcome::Ok, 7);
            if !e.logged() {
                assert!(line.is_none(), "{} only reads", e.tool);
                continue;
            }
            let line = line.unwrap_or_else(|| panic!("{} has no line", e.tool));
            assert_eq!(line["tool"], e.tool);
            // `apply: true` and `dry_run: false` make every call a real one.
            assert_eq!(line["dry_run"], false, "{}", e.tool);
            assert_eq!(line["targets"]["path"], "src/a.txt");
            assert_eq!(line["lengths"]["text"], 14);
            let text = line.to_string();
            assert!(
                !text.contains("secret content") && !text.contains("drop table"),
                "{text}"
            );
            let refused = call_line(e.tool, args, &[("write", false)], &[], Outcome::Refused, 7);
            assert_eq!(refused.unwrap()["result"], "refused");
        }
    }

    /// SPEC-0053 v7 behaviour 11: dry runs are told apart by `dry_run` and its default, or `apply`.
    #[test]
    fn dry_runs_follow_the_default_of_each_tool() {
        let none = None;
        let copy = effect("copy_file").unwrap();
        assert!(is_dry_run(copy, none));
        assert!(!is_dry_run(copy, json!({"dry_run": false}).as_object()));
        let table = effect("copy_table").unwrap();
        assert!(is_dry_run(table, none));
        assert!(!is_dry_run(table, json!({"apply": true}).as_object()));
        assert!(!is_dry_run(effect("create_file").unwrap(), none));
        assert!(effect("read_file").unwrap().changes.is_empty());
        // A database decision decides: a preview is a dry run, a reading tool on a connection
        // is logged (SPEC-0012 v3 behaviour 16).
        let preview = [json!({"connection": "c", "dry_run": true, "ok": true})];
        let line = call_line(
            "run_sql",
            json!({"apply": true}).as_object(),
            &[],
            &preview,
            Outcome::Ok,
            1,
        )
        .unwrap();
        assert_eq!(line["dry_run"], true);
        assert_eq!(line["database"][0]["connection"], "c");
        let read = [json!({"connection": "c", "dry_run": false, "ok": true})];
        let line = call_line("list_tables", none, &[], &read, Outcome::Ok, 1).unwrap();
        assert_eq!(line["dry_run"], false);
        assert!(line["changes"].as_array().unwrap().is_empty());
        assert!(effect("no_such_tool").is_none());
    }
}
