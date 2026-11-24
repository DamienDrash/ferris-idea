//! Tool groups and what an MCP session is offered (`SPEC-0006` v58 behaviours 17 to 20, ROADMAP
//! M19.1, ADR-0023 rule 3).
//!
//! Every tool belongs to one [`Group`]. A session is offered the [`CORE`] set unless the server or
//! the client asks for more; `find_tools` finds the others and adds them. The offer decides only
//! what `tools/list` shows: it is not a right, and every tool makes its own checks whether it is
//! offered or not (behaviour 20).

use std::collections::BTreeSet;

/// A named group of tools (behaviour 17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    /// Workspace files, local history, bookmarks.
    Files,
    /// Search, symbols, usages, hierarchies, completion, highlighting, language servers.
    Navigation,
    /// Refactorings, code actions, formatting, generation, multi-caret and replace previews.
    Editing,
    /// Diagnostics, inspections, spelling and grammar.
    Inspections,
    /// Build, run configurations, tests, terminals, profiler, environments, dependencies.
    Run,
    /// Debugger sessions and breakpoints.
    Debug,
    /// Git and changelists.
    Vcs,
    /// Connections, SQL, tables, stores.
    Database,
    /// HTTP requests and endpoints.
    Web,
    /// Docker and Kubernetes.
    Containers,
    /// Remote hosts and projects, paired devices, shared rooms.
    Remote,
    /// The window, the UI driver, settings, keymap, themes, plugins, trust, agent rights.
    Ide,
}

impl Group {
    /// Every group, in the order `find_tools` lists them.
    pub const ALL: [Group; 12] = [
        Group::Files,
        Group::Navigation,
        Group::Editing,
        Group::Inspections,
        Group::Run,
        Group::Debug,
        Group::Vcs,
        Group::Database,
        Group::Web,
        Group::Containers,
        Group::Remote,
        Group::Ide,
    ];

    /// The group's name in arguments and answers.
    pub fn name(self) -> &'static str {
        match self {
            Group::Files => "files",
            Group::Navigation => "navigation",
            Group::Editing => "editing",
            Group::Inspections => "inspections",
            Group::Run => "run",
            Group::Debug => "debug",
            Group::Vcs => "vcs",
            Group::Database => "database",
            Group::Web => "web",
            Group::Containers => "containers",
            Group::Remote => "remote",
            Group::Ide => "ide",
        }
    }

    /// One line on what the group holds.
    pub fn description(self) -> &'static str {
        match self {
            Group::Files => {
                "Workspace files: list, read, create, copy, move, delete, compare; local history, bookmarks"
            }
            Group::Navigation => {
                "Code insight: search, symbols, definitions, usages, hierarchies, completion, highlighting"
            }
            Group::Editing => {
                "Changing code: rename, code actions, formatting, refactorings, generation, previews"
            }
            Group::Inspections => {
                "Problems: diagnostics, inspections, quick fixes, spelling and grammar"
            }
            Group::Run => {
                "Build, run and test: run configurations, build tools, terminals, profiler, environments"
            }
            Group::Debug => "Debugger sessions, stepping, variables and breakpoints",
            Group::Vcs => {
                "Git: status, log, diff, blame, branches, commit, merge, rebase, push; changelists"
            }
            Group::Database => {
                "Databases: connections, SQL, tables, schemas, DDL, data export, stores"
            }
            Group::Web => "HTTP requests, web endpoints, Spring beans",
            Group::Containers => "Docker and Kubernetes",
            Group::Remote => {
                "Remote hosts and projects, port forwards, paired devices, shared rooms"
            }
            Group::Ide => {
                "The IDE: window, editor tabs, UI driver, settings, keymap, themes, plugins, trust, agent rights"
            }
        }
    }

    /// The group named `name`.
    pub fn from_name(name: &str) -> Option<Group> {
        Group::ALL.into_iter().find(|g| g.name() == name)
    }
}

/// The key of the `initialize` option in the client's experimental capabilities (behaviour 18).
pub const INITIALIZE_OPTION: &str = "ferris-idea/tools";

/// The tools every session is offered (behaviour 17).
pub const CORE: &[&str] = &[
    "workspace_info",
    "list_files",
    "read_file",
    "search_in_files",
    "find_symbol_locations",
    "file_diagnostics",
    "replace_text_in_file",
    "agent_permissions",
    "find_tools",
];

/// The groups of the tools of `lib.rs`, sorted by tool (behaviour 17). A tool module keeps its
/// own `pub(crate) const TOOL_GROUPS: &[(&str, Group)]` next to its router; `build.rs` finds
/// every such const and [`group_of`] joins them (ADR-0035), so a new module edits no shared list.
#[rustfmt::skip]
pub(crate) const TOOL_GROUPS: &[(&str, Group)] = &[
    ("add_idea_data_source", Group::Database),
    ("agent_permissions", Group::Ide),
    ("ai_assistant", Group::Ide),
    ("apply_code_action", Group::Editing),
    ("apply_context_action", Group::Editing),
    ("apply_patch", Group::Files),
    ("bookmarks", Group::Files),
    ("browse_store", Group::Database),
    ("build_command_suggestions", Group::Run),
    ("build_project", Group::Run),
    ("build_task", Group::Run),
    ("build_tool_links", Group::Run),
    ("build_tool_options", Group::Run),
    ("call_hierarchy", Group::Navigation),
    ("cancel_sql", Group::Database),
    ("change_column_type", Group::Database),
    ("change_signature", Group::Editing),
    ("changelists", Group::Vcs),
    ("check_grammar", Group::Inspections),
    ("class_members", Group::Navigation),
    ("clone_repository", Group::Vcs),
    ("close_project", Group::Ide),
    ("code_actions", Group::Editing),
    ("code_authors", Group::Navigation),
    ("compare_files", Group::Files),
    ("compare_schemas", Group::Database),
    ("complete_code", Group::Navigation),
    ("complete_sql", Group::Database),
    ("config_key", Group::Files),
    ("context_actions", Group::Editing),
    ("copy_file", Group::Files),
    ("copy_table", Group::Database),
    ("create_file", Group::Files),
    ("create_test", Group::Editing),
    ("database_problem_report", Group::Database),
    ("debug_adapters", Group::Debug),
    ("debug_attach", Group::Debug),
    ("debug_breakpoints", Group::Debug),
    ("debug_completions", Group::Debug),
    ("debug_disassemble", Group::Debug),
    ("debug_evaluate", Group::Debug),
    ("debug_field_watch", Group::Debug),
    ("debug_hot_swap", Group::Debug),
    ("debug_inspect", Group::Debug),
    ("debug_memory", Group::Debug),
    ("debug_pause", Group::Debug),
    ("debug_processes", Group::Debug),
    ("debug_registers", Group::Debug),
    ("debug_renderers", Group::Debug),
    ("debug_reset_frame", Group::Debug),
    ("debug_resume", Group::Debug),
    ("debug_run_to_line", Group::Debug),
    ("debug_sessions", Group::Debug),
    ("debug_set_variable", Group::Debug),
    ("debug_start", Group::Debug),
    ("debug_step_targets", Group::Debug),
    ("debug_stop", Group::Debug),
    ("debug_threads", Group::Debug),
    ("debug_variables", Group::Debug),
    ("declaration_marks", Group::Navigation),
    ("delegate_methods", Group::Editing),
    ("delete_file", Group::Files),
    ("dependency_analysis", Group::Run),
    ("dependency_tree", Group::Run),
    ("describe_table", Group::Database),
    ("detect_language", Group::Files),
    ("device_list", Group::Remote),
    ("device_log", Group::Remote),
    ("device_restrict", Group::Remote),
    ("device_revoke", Group::Remote),
    ("dialog_decisions", Group::Ide),
    ("docker_compose", Group::Containers),
    ("docker_container", Group::Containers),
    ("docker_exec", Group::Containers),
    ("docker_image", Group::Containers),
    ("docker_status", Group::Containers),
    ("document_colors", Group::Navigation),
    ("document_highlights", Group::Navigation),
    ("document_symbols", Group::Navigation),
    ("edit_table_data", Group::Database),
    ("editor_action", Group::Ide),
    ("editor_keystrokes", Group::Ide),
    ("editor_selection", Group::Ide),
    ("editor_tabs", Group::Ide),
    ("editorconfig", Group::Files),
    ("endpoints", Group::Web),
    ("environment_source", Group::Run),
    ("er_diagram", Group::Database),
    ("eslint_file", Group::Editing),
    ("explain_sql", Group::Database),
    ("export_connection", Group::Database),
    ("export_connector", Group::Database),
    ("export_data", Group::Database),
    ("export_file", Group::Files),
    ("export_store_result", Group::Database),
    ("extract_interface", Group::Editing),
    ("extract_superclass", Group::Editing),
    ("file_changes", Group::Files),
    ("file_diagnostics", Group::Inspections),
    ("file_operation_undo", Group::Files),
    ("file_properties", Group::Files),
    ("file_type", Group::Files),
    ("find_symbol_locations", Group::Navigation),
    ("find_tools", Group::Ide),
    ("find_usages", Group::Navigation),
    ("fix_inspection", Group::Inspections),
    ("folding_ranges", Group::Navigation),
    ("format_file", Group::Editing),
    ("generate_choices", Group::Editing),
    ("generate_code", Group::Editing),
    ("generate_ddl", Group::Database),
    ("git_blame", Group::Vcs),
    ("git_branch_action", Group::Vcs),
    ("git_branch_favorite", Group::Vcs),
    ("git_branches", Group::Vcs),
    ("git_cherry_pick", Group::Vcs),
    ("git_commit", Group::Vcs),
    ("git_commit_action", Group::Vcs),
    ("git_conflicts", Group::Vcs),
    ("git_create_branch", Group::Vcs),
    ("git_diff", Group::Vcs),
    ("git_fetch", Group::Vcs),
    ("git_line_change", Group::Vcs),
    ("git_log", Group::Vcs),
    ("git_merge", Group::Vcs),
    ("git_outgoing", Group::Vcs),
    ("git_pull", Group::Vcs),
    ("git_push", Group::Vcs),
    ("git_rebase", Group::Vcs),
    ("git_remotes", Group::Vcs),
    ("git_repositories", Group::Vcs),
    ("git_resolve_conflict", Group::Vcs),
    ("git_rollback", Group::Vcs),
    ("git_show", Group::Vcs),
    ("git_stash", Group::Vcs),
    ("git_status", Group::Vcs),
    ("git_switch_branch", Group::Vcs),
    ("goto_test", Group::Navigation),
    ("grant_plugin_network", Group::Ide),
    ("header_source", Group::Files),
    ("highlight_file", Group::Navigation),
    ("highlighting_level", Group::Inspections),
    ("hover_symbol", Group::Navigation),
    ("http_requests", Group::Web),
    ("ide_settings", Group::Ide),
    ("idea_project", Group::Run),
    ("import_connector", Group::Database),
    ("import_file", Group::Files),
    ("import_hint", Group::Files),
    ("inlay_hints", Group::Navigation),
    ("inspect_code", Group::Inspections),
    ("inspect_plugin_file", Group::Ide),
    ("inspect_sql", Group::Database),
    ("inspection_profile", Group::Inspections),
    ("java_environment", Group::Run),
    ("keyboard_diagnostics", Group::Ide),
    ("keymap_lookup", Group::Ide),
    ("kubernetes_exec", Group::Containers),
    ("kubernetes_port_forward", Group::Containers),
    ("kubernetes_status", Group::Containers),
    ("kubernetes_workload", Group::Containers),
    ("language_servers", Group::Navigation),
    ("library_sources", Group::Navigation),
    ("list_connections", Group::Database),
    ("list_connectors", Group::Database),
    ("list_files", Group::Files),
    ("list_schemas", Group::Database),
    ("list_stores", Group::Database),
    ("list_tables", Group::Database),
    ("local_history", Group::Files),
    ("merge_three_way", Group::Editing),
    ("method_hierarchy", Group::Navigation),
    ("move_file", Group::Files),
    ("new_project", Group::Run),
    ("node_environment", Group::Run),
    ("notifications", Group::Ide),
    ("object_definition", Group::Database),
    ("object_doc", Group::Database),
    ("open_documents", Group::Files),
    ("optimize_imports", Group::Editing),
    ("override_methods", Group::Editing),
    ("package_scripts", Group::Run),
    ("parse_test_report", Group::Run),
    ("paste_history", Group::Editing),
    ("plugin_file_type", Group::Ide),
    ("plugin_inspections", Group::Ide),
    ("plugin_network", Group::Ide),
    ("plugin_run_configurations", Group::Ide),
    ("plugins", Group::Ide),
    ("prettier_file", Group::Editing),
    ("preview_column_edit", Group::Editing),
    ("preview_multi_caret_edit", Group::Editing),
    ("problem_preview", Group::Inspections),
    ("profile_attach", Group::Run),
    ("profile_last", Group::Run),
    ("profile_processes", Group::Run),
    ("profile_run", Group::Run),
    ("project_dictionary", Group::Inspections),
    ("project_model", Group::Run),
    ("project_tree", Group::Files),
    ("project_trust", Group::Ide),
    ("properties_file", Group::Files),
    ("pull_members_up", Group::Editing),
    ("python_create_venv", Group::Run),
    ("python_environments", Group::Run),
    ("python_install_packages", Group::Run),
    ("python_language_server", Group::Run),
    ("python_package_info", Group::Run),
    ("python_search_packages", Group::Run),
    ("python_select_interpreter", Group::Run),
    ("quick_definition", Group::Navigation),
    ("quit_ide", Group::Ide),
    ("read_file", Group::Files),
    ("read_table_data", Group::Database),
    ("recent_projects", Group::Ide),
    ("redo_refactoring", Group::Editing),
    ("reference_index", Group::Navigation),
    ("related_files", Group::Files),
    ("remote_access_off", Group::Remote),
    ("remote_access_status", Group::Remote),
    ("remote_backend_call", Group::Remote),
    ("remote_backend_start", Group::Remote),
    ("remote_backend_stop", Group::Remote),
    ("remote_create", Group::Remote),
    ("remote_debug_attach", Group::Remote),
    ("remote_debug_start", Group::Remote),
    ("remote_delete", Group::Remote),
    ("remote_documents", Group::Remote),
    ("remote_exec", Group::Remote),
    ("remote_host_add", Group::Remote),
    ("remote_host_probe", Group::Remote),
    ("remote_host_remove", Group::Remote),
    ("remote_hosts", Group::Remote),
    ("remote_hosts_paired", Group::Remote),
    ("remote_list", Group::Remote),
    ("remote_pair_add_path", Group::Remote),
    ("remote_pair_forget", Group::Remote),
    ("remote_pair_start", Group::Remote),
    ("remote_port_close", Group::Remote),
    ("remote_port_forward", Group::Remote),
    ("remote_ports", Group::Remote),
    ("remote_project_close", Group::Remote),
    ("remote_project_open", Group::Remote),
    ("remote_project_reconnect", Group::Remote),
    ("remote_projects", Group::Remote),
    ("remote_read", Group::Remote),
    ("remote_rename", Group::Remote),
    ("remote_write", Group::Remote),
    ("remove_bookmark", Group::Files),
    ("remove_connection", Group::Database),
    ("rename_file", Group::Files),
    ("rename_sql_object", Group::Database),
    ("rename_symbol", Group::Editing),
    ("replace_in_files_preview", Group::Editing),
    ("replace_text_in_file", Group::Files),
    ("resource_bundle", Group::Files),
    ("restore_local_revision", Group::Files),
    ("revoke_plugin_network", Group::Ide),
    ("revoke_plugin_run_permission", Group::Ide),
    ("revoke_project_trust", Group::Ide),
    ("room_apply", Group::Remote),
    ("room_create", Group::Remote),
    ("room_follow", Group::Remote),
    ("room_history", Group::Remote),
    ("room_join", Group::Remote),
    ("room_join_link", Group::Remote),
    ("room_links", Group::Remote),
    ("room_presence", Group::Remote),
    ("room_restore", Group::Remote),
    ("room_revoke_link", Group::Remote),
    ("room_set_role", Group::Remote),
    ("room_share", Group::Remote),
    ("room_status", Group::Remote),
    ("room_undo", Group::Remote),
    ("room_version", Group::Remote),
    ("ruby_environment", Group::Run),
    ("run_command", Group::Run),
    ("run_configuration", Group::Run),
    ("run_configuration_edit", Group::Run),
    ("run_configurations", Group::Run),
    ("run_markers", Group::Run),
    ("run_output", Group::Run),
    ("run_plugin_action", Group::Ide),
    ("run_plugin_configuration", Group::Ide),
    ("run_sql", Group::Database),
    ("run_store_command", Group::Database),
    ("safe_delete", Group::Editing),
    ("save_connection", Group::Database),
    ("save_store", Group::Database),
    ("saved_breakpoints", Group::Debug),
    ("schema_objects", Group::Database),
    ("search_everywhere", Group::Navigation),
    ("search_in_files", Group::Navigation),
    ("semantic_tokens", Group::Navigation),
    ("send_http_request", Group::Web),
    ("session_history_list", Group::Ide),
    ("session_history_read", Group::Ide),
    ("set_bookmark", Group::Files),
    ("set_breakpoint", Group::Debug),
    ("set_ide_settings", Group::Ide),
    ("set_shortcut", Group::Ide),
    ("set_special_breakpoints", Group::Debug),
    ("signature_help", Group::Navigation),
    ("smart_complete_code", Group::Navigation),
    ("spelling_dictionaries", Group::Inspections),
    ("split_sql_statements", Group::Database),
    ("spring_beans", Group::Web),
    ("sql_console", Group::Database),
    ("sql_file_data_source", Group::Database),
    ("sql_history", Group::Database),
    ("sql_refactorings_at", Group::Database),
    ("suggest_names", Group::Editing),
    ("super_method", Group::Navigation),
    ("suppress_inspection", Group::Inspections),
    ("surround_with", Group::Editing),
    ("table_ddl", Group::Database),
    ("terminal_command", Group::Run),
    ("terminal_read", Group::Run),
    ("terminal_render", Group::Run),
    ("terminal_sessions", Group::Run),
    ("test_connection", Group::Database),
    ("themes", Group::Ide),
    ("todo_items", Group::Files),
    ("tool_windows", Group::Ide),
    ("trust_project", Group::Ide),
    ("type_hierarchy", Group::Navigation),
    ("ui_act", Group::Ide),
    ("ui_command", Group::Ide),
    ("ui_driver_status", Group::Ide),
    ("ui_driver_stop", Group::Ide),
    ("ui_expect", Group::Ide),
    ("ui_find", Group::Ide),
    ("ui_screenshot", Group::Ide),
    ("ui_snapshot", Group::Ide),
    ("ui_tooltips", Group::Ide),
    ("ui_trace", Group::Ide),
    ("undo_refactoring", Group::Editing),
    ("update_copyright", Group::Editing),
    ("usage_counts", Group::Navigation),
    ("validate_connector", Group::Database),
    ("web_endpoint_check", Group::Web),
    ("window_layouts", Group::Ide),
    ("workspace_info", Group::Files),
];

/// Every tool's group: the `TOOL_GROUPS` of every module, joined once.
static GROUPS: std::sync::LazyLock<std::collections::HashMap<&'static str, Group>> =
    std::sync::LazyLock::new(|| {
        crate::registry::GROUPS
            .iter()
            .flat_map(|rows| rows.iter().copied())
            .collect()
    });

/// The group of `tool`; `None` for a name the server does not have. A new tool gets its row in
/// its module's `TOOL_GROUPS` in the same change (the ratchet in this file's tests fails
/// otherwise).
pub fn group_of(tool: &str) -> Option<Group> {
    GROUPS.get(tool).copied()
}

/// What a session is offered: the core set, every tool of some groups, and single tools enabled
/// by `find_tools` (behaviour 18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    all: bool,
    groups: BTreeSet<Group>,
    tools: BTreeSet<String>,
}

impl Default for Offer {
    /// The core set (behaviour 18).
    fn default() -> Self {
        Offer {
            all: false,
            groups: BTreeSet::new(),
            tools: BTreeSet::new(),
        }
    }
}

impl Offer {
    /// Every tool, as before v58.
    pub fn all() -> Self {
        Offer {
            all: true,
            ..Offer::default()
        }
    }

    /// The offer for these group names: `all` is every tool, `core` the core set (always in);
    /// an unknown name is an error naming the known ones.
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<Self, String> {
        let mut offer = Offer::default();
        for name in names {
            match name.trim() {
                "" | "core" => {}
                "all" => offer.all = true,
                other => match Group::from_name(other) {
                    Some(g) => {
                        offer.groups.insert(g);
                    }
                    None => return Err(unknown_group(other)),
                },
            }
        }
        Ok(offer)
    }

    /// The offer of `--tools <groups>`: comma-separated group names.
    pub fn parse(list: &str) -> Result<Self, String> {
        Offer::from_names(list.split(','))
    }

    /// Whether `tool` is offered.
    pub fn offers(&self, tool: &str) -> bool {
        self.all
            || CORE.contains(&tool)
            || self.tools.contains(tool)
            || group_of(tool).is_some_and(|g| self.groups.contains(&g))
    }

    /// Whether every tool is offered.
    pub fn is_all(&self) -> bool {
        self.all
    }

    /// Offers what `other` offers too.
    pub fn widen(&mut self, other: &Offer) {
        self.all |= other.all;
        self.groups.extend(other.groups.iter().copied());
        self.tools.extend(other.tools.iter().cloned());
    }

    /// Offers `tool` too; whether it was not offered before.
    pub fn enable(&mut self, tool: &str) -> bool {
        if self.offers(tool) {
            return false;
        }
        self.tools.insert(tool.to_owned());
        true
    }
}

/// The error for a group name the server does not know.
pub fn unknown_group(name: &str) -> String {
    let known: Vec<&str> = Group::ALL.iter().map(|g| g.name()).collect();
    format!(
        "unknown tool group {name:?}; known: all, core, {}",
        known.join(", ")
    )
}

/// The first sentence of a tool's description on one line, at most 160 characters.
pub fn summary(description: &str) -> String {
    let flat = description.split_whitespace().collect::<Vec<_>>().join(" ");
    let first = match flat.find(". ") {
        Some(i) => &flat[..=i],
        None => flat.as_str(),
    };
    if first.chars().count() <= 160 {
        first.to_owned()
    } else {
        let cut: String = first.chars().take(159).collect();
        format!("{}…", cut.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0006 v58 AK-18: `--tools` and the `initialize` option read group names.
    #[test]
    fn offers_are_read_from_group_names() {
        let core = Offer::parse("core").unwrap();
        assert_eq!(core, Offer::default());
        assert!(core.offers("read_file") && core.offers("find_tools"));
        assert!(!core.offers("git_commit"));
        let all = Offer::parse("all").unwrap();
        assert!(all.is_all() && all.offers("git_commit"));
        let vcs = Offer::parse("vcs, database").unwrap();
        assert!(vcs.offers("git_commit") && vcs.offers("run_sql") && vcs.offers("read_file"));
        assert!(!vcs.offers("debug_start"));
        let e = Offer::parse("core,gits").unwrap_err();
        assert!(e.contains("\"gits\"") && e.contains("vcs"), "{e}");
        let mut offer = Offer::default();
        assert!(offer.enable("git_log"));
        assert!(!offer.enable("git_log") && !offer.enable("read_file"));
        assert!(offer.offers("git_log") && !offer.offers("git_diff"));
    }

    /// SPEC-0006 v58 AK-17 (ratchet): every tool the server serves has a group, every name with a
    /// group is served, and the core set exists.
    #[test]
    fn every_tool_has_one_group_and_the_core_set_exists() {
        // ADR-0035: a tool is named in one module's TOOL_GROUPS only, and only tools that exist.
        let mut seen = BTreeSet::new();
        for rows in crate::registry::GROUPS {
            for (tool, _) in *rows {
                assert!(seen.insert(*tool), "{tool} has a group in two modules");
            }
        }
        let served: BTreeSet<String> = crate::IdeServer::all_tools()
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        let without: Vec<&String> = served.iter().filter(|t| group_of(t).is_none()).collect();
        assert!(
            without.is_empty(),
            "tools without a group (their module's TOOL_GROUPS): {without:?}"
        );
        for tool in CORE {
            assert!(served.contains(*tool), "core tool {tool} is not served");
        }
        for g in Group::ALL {
            assert!(
                served.iter().any(|t| group_of(t) == Some(g)),
                "group {} has no tool",
                g.name()
            );
        }
    }

    #[test]
    fn a_summary_is_one_short_line() {
        assert_eq!(
            summary("Reads a file.\nMore text. And more."),
            "Reads a file."
        );
        assert_eq!(summary("No full stop"), "No full stop");
        let long = "word ".repeat(60);
        let s = summary(&long);
        assert!(s.chars().count() <= 160 && s.ends_with('…'), "{s}");
    }

    #[test]
    fn every_group_has_a_name_that_reads_back() {
        for g in Group::ALL {
            assert_eq!(Group::from_name(g.name()), Some(g));
            assert!(!g.description().is_empty());
        }
        for tool in CORE {
            assert!(group_of(tool).is_some(), "{tool}");
        }
    }
}
