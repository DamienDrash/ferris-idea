//! The modal dialogs of the IDE and the MCP tool that makes each of their decisions (`SPEC-0281`).
//!
//! The toolkit has no accessibility tree yet (ADR-0019), so a drawn dialog cannot be answered by
//! a screen reader or an accessibility client. Instead every answer has a tool; this list says
//! which, and the `dialog_decisions` tool hands it to agents. Tests keep it true: every tool and
//! test it names exists, and every `…Dialog` type of the workbench is listed.

use serde::Serialize;

/// How a dialog is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Presentation {
    /// The platform's alert: readable and pressable by accessibility clients.
    NativeAlert,
    /// Drawn by IDECraft in the window: not visible to accessibility clients (ADR-0019).
    Drawn,
}

/// One answer of a dialog and how an agent gives the same answer.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Decision {
    /// What the user decides, in IDECraft's words.
    pub answer: &'static str,
    /// The MCP tool that makes the same decision; `None` when reserved or not offered yet.
    pub tool: Option<&'static str>,
    /// The arguments that matter for this answer.
    pub arguments: &'static str,
    /// Why there is no tool (a security spec reserves it for the user, or it is planned).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

/// One modal dialog.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Dialog {
    /// Stable id, for `dialog_decisions` with `id`.
    pub id: &'static str,
    /// What the dialog asks.
    pub title: &'static str,
    /// The specification that describes the dialog.
    pub spec: &'static str,
    /// Native alert or drawn.
    pub presentation: Presentation,
    /// The workbench type that draws it (empty for a native alert).
    #[serde(skip)]
    pub ui_type: &'static str,
    /// Its answers.
    pub decisions: &'static [Decision],
    /// Tests that make these decisions through the tools (`file::function`).
    pub evidence: &'static [&'static str],
}

const fn tool(answer: &'static str, tool: &'static str, arguments: &'static str) -> Decision {
    Decision {
        answer,
        tool: Some(tool),
        arguments,
        reason: None,
    }
}

const fn reserved(answer: &'static str, reason: &'static str) -> Decision {
    Decision {
        answer,
        tool: None,
        arguments: "",
        reason: Some(reason),
    }
}

/// The command line form of every tool call (`SPEC-0281` behaviour 5).
pub const CLI: &str = "idecraft call <folder> <tool> '<json arguments>'";

/// Every modal dialog IDECraft shows (`SPEC-0281` behaviour 2).
pub const DIALOGS: &[Dialog] = &[
    Dialog {
        id: "quit-unsaved",
        title: "Save changes before quitting?",
        spec: "SPEC-0261",
        presentation: Presentation::Drawn,
        ui_type: "DrawnQuestion",
        decisions: &[
            tool(
                "Save All and Quit",
                "quit_ide",
                "pending=save_all (needs the `quit` right; `idecraft quit --save-all`)",
            ),
            tool(
                "Quit Without Saving",
                "quit_ide",
                "pending=discard (`idecraft quit --discard`)",
            ),
            tool(
                "Cancel (see what is pending)",
                "quit_ide",
                "check_only=true",
            ),
        ],
        evidence: &[
            "crates/workbench/src/quit_model.rs::names_the_files_relative_to_their_project",
            "crates/mcp/tests/it/protocol.rs::quit_ide_reports_pending_work_and_needs_the_quit_right",
            "crates/workbench/src/dialogs.rs::the_quit_question_is_drawn_with_its_message_detail_and_buttons",
            "crates/app/src/f084_handtests.rs::hand_test_203_quit_ide_with_an_unsaved_file",
            "crates/app/src/f084_handtests.rs::hand_test_204_idecraft_quit_in_the_terminal",
        ],
    },
    Dialog {
        id: "close-projects-unsaved",
        title: "Save changes before closing <project>?",
        spec: "SPEC-0094",
        presentation: Presentation::Drawn,
        ui_type: "DrawnQuestion",
        decisions: &[
            tool(
                "Save All and Close",
                "close_project",
                "dry_run=false, pending=save_all (needs `quit` and `write`)",
            ),
            tool(
                "Close Without Saving",
                "close_project",
                "dry_run=false, pending=discard (needs `quit`)",
            ),
            tool(
                "Cancel (see what is pending)",
                "close_project",
                "dry_run=true",
            ),
        ],
        evidence: &[
            "crates/workbench/src/close_model.rs::the_question_uses_the_words_of_closing",
            "crates/mcp/tests/it/protocol.rs::close_project_is_a_dry_run_first_and_needs_the_quit_right",
        ],
    },
    Dialog {
        id: "remote-project-close",
        title: "Remote files are unsaved",
        spec: "SPEC-0317",
        presentation: Presentation::Drawn,
        ui_type: "DrawnQuestion",
        decisions: &[
            tool(
                "Save All",
                "remote_project_close",
                "dry_run=false, unsaved=save (needs `write`)",
            ),
            tool(
                "Don't Save",
                "remote_project_close",
                "dry_run=false, unsaved=discard (needs `quit`)",
            ),
            tool(
                "Cancel (see what is unsaved)",
                "remote_project_close",
                "dry_run=true",
            ),
        ],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::remote_projects_of_windows_are_seen_and_acted_on_by_agents",
            "crates/workbench/src/dialogs.rs::the_remote_close_question_names_the_files",
        ],
    },
    Dialog {
        id: "trust-project",
        title: "Trust and open project?",
        spec: "SPEC-0278",
        presentation: Presentation::NativeAlert,
        ui_type: "",
        decisions: &[
            tool(
                "Trust Project",
                "trust_project",
                "dry_run=false (needs the `trust` right)",
            ),
            tool(
                "Trust Projects in the parent folder",
                "trust_project",
                "scope=folder, dry_run=false (trusts the parent folder)",
            ),
            tool(
                "Preview in Safe Mode",
                "project_trust",
                "(nothing to change: untrusted is safe mode)",
            ),
            tool("Stop trusting", "revoke_project_trust", "path"),
        ],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::untrusted_workspace_is_in_safe_mode_and_trust_is_guarded",
            "crates/mcp/tests/it/protocol.rs::granted_trust_is_remembered_logged_and_revocable",
        ],
    },
    Dialog {
        id: "agent-permission",
        title: "Allow an agent to use a right?",
        spec: "SPEC-0053",
        presentation: Presentation::NativeAlert,
        ui_type: "",
        decisions: &[
            tool("See which rights are granted", "agent_permissions", ""),
            reserved(
                "Deny / Allow Once / Always",
                "reserved for the user: an agent cannot grant itself a right (SPEC-0053 v4 behaviour 8)",
            ),
        ],
        evidence: &[
            "crates/mcp/src/window.rs::the_question_names_client_right_action_and_scope",
            "crates/mcp/tests/it/http.rs::the_cli_client_is_named_in_the_question_and_quits",
            "crates/app/src/f084_handtests.rs::hand_test_202_the_window_asks_http_clients_for_a_right",
        ],
    },
    Dialog {
        id: "delete-file",
        title: "Delete file or folder?",
        spec: "SPEC-0275",
        presentation: Presentation::NativeAlert,
        ui_type: "",
        decisions: &[
            tool("Delete", "delete_file", "path, dry_run=false"),
            tool(
                "Cancel (see what would go)",
                "delete_file",
                "path (dry_run defaults to true)",
            ),
        ],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::creates_renames_and_deletes_files",
            "crates/app/src/f084_handtests.rs::hand_test_206_delete_asks_in_the_native_alert",
        ],
    },
    Dialog {
        id: "plugin-run",
        title: "Run a command suggested by a plugin?",
        spec: "SPEC-0276",
        presentation: Presentation::Drawn,
        ui_type: "PluginRunDialog",
        decisions: &[
            tool(
                "Run Once (after Always Allow was chosen by the user)",
                "run_plugin_configuration",
                "plugin, kind, name, dry_run=false",
            ),
            tool(
                "Cancel (read the exact command)",
                "run_plugin_configuration",
                "plugin, kind, name (dry_run defaults to true)",
            ),
            reserved(
                "Always Allow in This Workspace",
                "reserved for the user: an agent cannot confirm a plugin's command in the user's place (SPEC-0276 behaviour 5)",
            ),
            tool(
                "Revoke an allowed command",
                "revoke_plugin_run_permission",
                "plugin, kind, name",
            ),
        ],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::plugin_run_configurations",
            "crates/mcp/tests/it/protocol.rs::plugin_run_configuration_needs_the_run_right",
        ],
    },
    Dialog {
        id: "plugin-network",
        title: "Allow network access for plugin “<name>”?",
        spec: "SPEC-0314",
        presentation: Presentation::Drawn,
        ui_type: "PluginNetworkDialog",
        decisions: &[
            tool(
                "Read the warning (plugin, author, host and port, status, not reviewed)",
                "test_connection",
                "id (answers warning and requires_grant without connecting)",
            ),
            tool(
                "Allow and connect (for agents: the user is asked in the window)",
                "grant_plugin_network",
                "connection",
            ),
            reserved(
                "Allow and connect for the user",
                "reserved for the user: an agent needs a grant of its own, never the user's (ADR-0023 rule 7)",
            ),
            tool(
                "Revoke a grant",
                "revoke_plugin_network",
                "plugin, host, port, holder",
            ),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::plugin_network_warning_and_agent_grant"],
    },
    Dialog {
        id: "rename-database-object",
        title: "Rename table or column",
        spec: "SPEC-0128",
        presentation: Presentation::Drawn,
        ui_type: "DbRenameDialog",
        decisions: &[
            tool(
                "Preview",
                "rename_sql_object",
                "connection, table, column?, new_name",
            ),
            tool(
                "Refactor",
                "rename_sql_object",
                "apply=true, include_other_dialects?, rename_derived?",
            ),
            tool(
                "Tick a file outside the project's own sources",
                "rename_sql_object",
                "apply=true, include_outside=[…]",
            ),
        ],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::rename_sql_object_rewrites_text_and_workspace_files",
            "crates/mcp/tests/it/protocol.rs::rename_sql_object_across_dialects_and_derived_names",
            "crates/mcp/tests/it/protocol.rs::rename_sql_object_leaves_files_outside_the_own_sources_alone",
        ],
    },
    Dialog {
        id: "copy-table",
        title: "Copy table",
        spec: "SPEC-0191",
        presentation: Presentation::Drawn,
        ui_type: "CopyTableDialog",
        decisions: &[
            tool(
                "Preview the statements",
                "copy_table",
                "connection, table, target_schema?, target_table?",
            ),
            tool(
                "Copy",
                "copy_table",
                "apply=true and what to keep (primary_key, rows, …)",
            ),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::copy_table_previews_and_copies"],
    },
    Dialog {
        id: "import-preview",
        title: "Import (exchange file)",
        spec: "SPEC-0303",
        presentation: Presentation::Drawn,
        ui_type: "ImportPreview",
        decisions: &[
            tool("Preview", "import_file", "path"),
            tool("Import", "import_file", "path, dry_run=false"),
            tool("Share…", "export_file", "kind, share=true"),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::import_and_export_files"],
    },
    Dialog {
        id: "breakpoints",
        title: "Breakpoints",
        spec: "SPEC-0127",
        presentation: Presentation::Drawn,
        ui_type: "BreakpointDialog",
        decisions: &[
            tool("List", "saved_breakpoints", ""),
            tool(
                "Condition, pass count, log message, enabled, remove",
                "set_breakpoint",
                "path, line, condition?, hit_condition?, log_message?, enabled?, remove?",
            ),
            tool(
                "Exception and method breakpoints",
                "set_special_breakpoints",
                "caught?, uncaught?, exception_types?, functions?",
            ),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::saved_breakpoint_tools"],
    },
    Dialog {
        id: "run-configurations",
        title: "Run/Debug Configurations",
        spec: "SPEC-0327",
        presentation: Presentation::Drawn,
        ui_type: "RunConfigDialog",
        decisions: &[
            tool("List", "run_configurations", ""),
            tool(
                "Add, change, copy, remove, move, Save Configuration (Apply, OK)",
                "run_configuration_edit",
                "operation, name?, config?, command_line?, new_name?, by?, dry_run=false",
            ),
            tool(
                "Add from a type's template",
                "run_configuration_edit",
                "operation=add, type, config?, dry_run=false",
            ),
            tool(
                "Folder: put a configuration in or take it out, Rename, Remove Folder",
                "run_configuration_edit",
                "operation=set_folder|rename_folder|remove_folder, name?, folder?, new_name?, dry_run=false",
            ),
            tool(
                "Templates: change or Reset a type's template",
                "run_configuration_edit",
                "operation=set_template, type, template?, dry_run=false",
            ),
            tool(
                "Cancel (look at the result first)",
                "run_configuration_edit",
                "operation, … (dry_run defaults to true)",
            ),
            tool("Run", "run_configuration", "name, dry_run=false"),
        ],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::run_configuration_edit_dry_runs_and_needs_the_run_right",
            "crates/mcp/tests/it/protocol.rs::run_configuration_edit_folders_and_templates",
        ],
    },
    Dialog {
        id: "run-already-running",
        title: "The run configuration is already running",
        spec: "SPEC-0047",
        presentation: Presentation::Drawn,
        ui_type: "DrawnQuestion",
        decisions: &[
            tool(
                "Stop and Rerun",
                "run_configuration",
                "name, dry_run=false, if_running=stop_and_rerun",
            ),
            tool(
                "Cancel (see whether it runs)",
                "run_configuration",
                "name (the dry run reports running and second_start)",
            ),
        ],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::run_configuration_instances_and_before_launch_tasks",
        ],
    },
    Dialog {
        id: "attach-to-process",
        title: "Attach to process",
        spec: "SPEC-0147",
        presentation: Presentation::Drawn,
        ui_type: "AttachView",
        decisions: &[
            tool("List processes", "debug_processes", ""),
            tool("Attach", "debug_attach", "pid or host and port"),
            tool("Attach the profiler", "profile_attach", "pid"),
        ],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::attach_lists_processes_and_detaches",
            "crates/mcp/tests/it/protocol.rs::profiles_memory_and_attaches",
        ],
    },
    Dialog {
        id: "database-connection",
        title: "Database connection",
        spec: "SPEC-0108",
        presentation: Presentation::Drawn,
        ui_type: "ConnectionDialog",
        decisions: &[
            tool("Test Connection", "test_connection", "id"),
            tool("Save", "save_connection", "the profile's fields"),
            tool("Remove", "remove_connection", "id"),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::connection_tools"],
    },
    Dialog {
        id: "database-problem-report",
        title: "Report a problem with this database",
        spec: "SPEC-0301",
        presentation: Presentation::Drawn,
        ui_type: "ReportDialog",
        decisions: &[tool(
            "Build the report (copy or open it yourself)",
            "database_problem_report",
            "connection or system, step, error",
        )],
        evidence: &["crates/mcp/tests/it/protocol.rs::database_problem_report_tool"],
    },
    Dialog {
        id: "push",
        title: "Push commits",
        spec: "SPEC-0109",
        presentation: Presentation::Drawn,
        ui_type: "PushView",
        decisions: &[
            tool("See what would be pushed", "git_outgoing", ""),
            tool(
                "Push",
                "git_push",
                "dry_run=false, target?, tags?, force_with_lease? (needs the `remote` right)",
            ),
        ],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::git_outgoing_tool",
            "crates/mcp/tests/it/protocol.rs::git_update_with_rebase_and_push_options",
        ],
    },
    Dialog {
        id: "new-changelist",
        title: "New changelist",
        spec: "SPEC-0335",
        presentation: Presentation::Drawn,
        ui_type: "CommitView",
        decisions: &[
            tool(
                "Create",
                "changelists",
                "action=create, name, set_active?, dry_run=false (needs the `write` right)",
            ),
            tool(
                "Cancel (see what would be created)",
                "changelists",
                "action=create, name (dry_run defaults to true)",
            ),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::changelists_tool"],
    },
    Dialog {
        id: "edit-changelist",
        title: "Edit changelist",
        spec: "SPEC-0335",
        presentation: Presentation::Drawn,
        ui_type: "CommitView",
        decisions: &[
            tool(
                "OK with a new name",
                "changelists",
                "action=rename, name, new_name, dry_run=false",
            ),
            tool(
                "OK with Set active",
                "changelists",
                "action=set_active, name, dry_run=false",
            ),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::changelists_tool"],
    },
    Dialog {
        id: "delete-changelist",
        title: "Delete changelist?",
        spec: "SPEC-0335",
        presentation: Presentation::Drawn,
        ui_type: "CommitView",
        decisions: &[
            tool(
                "Delete",
                "changelists",
                "action=delete, name, new_active? (the list that becomes active), dry_run=false",
            ),
            tool(
                "Cancel (see where the files would go)",
                "changelists",
                "action=delete, name (dry_run defaults to true)",
            ),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::changelists_tool"],
    },
    Dialog {
        id: "move-to-changelist",
        title: "Move to another changelist",
        spec: "SPEC-0335",
        presentation: Presentation::Drawn,
        ui_type: "CommitView",
        decisions: &[
            tool(
                "Move",
                "changelists",
                "action=move, paths, to, create? (a new list), dry_run=false",
            ),
            tool(
                "Cancel (see what would move)",
                "changelists",
                "action=move, paths, to (dry_run defaults to true)",
            ),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::changelists_tool"],
    },
    Dialog {
        id: "rollback-changes",
        title: "Roll back changes",
        spec: "SPEC-0063",
        presentation: Presentation::Drawn,
        ui_type: "CommitView",
        decisions: &[
            tool(
                "Rollback",
                "git_rollback",
                "paths, delete_added?, dry_run=false (needs the `vcs-write` right)",
            ),
            tool("Cancel", "git_rollback", "dry_run=true (changes nothing)"),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::git_amend_and_rollback_tools"],
    },
    Dialog {
        id: "update-project",
        title: "Update project",
        spec: "SPEC-0144",
        presentation: Presentation::Drawn,
        ui_type: "UpdateView",
        decisions: &[
            tool(
                "Merge or Rebase",
                "git_pull",
                "rebase=true|false, dry_run=false",
            ),
            tool(
                "Reset to the upstream",
                "git_pull",
                "reset=true (dry run lists the commits given up), dry_run=false",
            ),
        ],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::git_update_with_rebase_and_push_options",
            "crates/mcp/tests/it/protocol.rs::git_outgoing_tool",
        ],
    },
    Dialog {
        id: "interactive-rebase",
        title: "Interactive rebase",
        spec: "SPEC-0145",
        presentation: Presentation::Drawn,
        ui_type: "RebaseView",
        decisions: &[tool(
            "Start Rebasing with pick, reword, squash, fixup, drop",
            "git_rebase",
            "onto, interactive=true, todo",
        )],
        evidence: &["crates/mcp/tests/it/protocol.rs::git_stash_show_and_interactive_rebase"],
    },
    Dialog {
        id: "rename-preview",
        title: "Rename: occurrences in comments, strings and text",
        spec: "SPEC-0148",
        presentation: Presentation::Drawn,
        ui_type: "RenamePreview",
        decisions: &[
            tool(
                "Preview",
                "rename_symbol",
                "path, line, character, new_name, search_comments_and_strings?, search_text_occurrences?",
            ),
            tool(
                "Do Refactor with exclusions",
                "rename_symbol",
                "exclude=[…], dry_run=false",
            ),
            tool(
                "Tick a file outside the project's own sources",
                "rename_symbol",
                "include_outside=[…], dry_run=false",
            ),
        ],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::code_insight_tools",
            "crates/mcp/tests/it/protocol.rs::rename_leaves_files_outside_the_own_sources_alone",
        ],
    },
    Dialog {
        id: "settings",
        title: "Settings",
        spec: "SPEC-0105",
        presentation: Presentation::Drawn,
        ui_type: "SettingsView",
        decisions: &[
            tool("Read", "ide_settings", ""),
            tool(
                "Change and Apply",
                "set_ide_settings",
                "the settings to change, dry_run=false",
            ),
            tool("Keymap", "set_shortcut", "action, shortcuts or reset"),
            tool("Theme", "themes", "set"),
            tool("Trusted Locations", "revoke_project_trust", "path"),
        ],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::ide_settings_tools",
            "crates/mcp/tests/it/protocol.rs::set_shortcut_tool",
            "crates/mcp/tests/it/protocol.rs::themes_tool",
        ],
    },
    Dialog {
        id: "plugin-install",
        title: "Install plugin and grant its rights",
        spec: "SPEC-0126",
        presentation: Presentation::Drawn,
        ui_type: "SettingsView",
        decisions: &[
            tool(
                "Read what a plugin file contributes and asks for",
                "inspect_plugin_file",
                "path",
            ),
            tool("List installed plugins and granted rights", "plugins", ""),
            reserved(
                "Install, enable, remove, grant",
                "reserved for the user: installing code and granting it rights is the user's decision (SPEC-0126, SPEC-0211)",
            ),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::plugin_extension_points"],
    },
    Dialog {
        id: "unsafe-sql",
        title: "Run a change without WHERE?",
        spec: "SPEC-0192",
        presentation: Presentation::Drawn,
        ui_type: "ChoiceView",
        decisions: &[
            tool("Execute", "run_sql", "connection, sql, confirm_unsafe=true"),
            tool("Cancel", "run_sql", "without confirm_unsafe (nothing runs)"),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::sql_consoles_schemas_and_unsafe_changes"],
    },
    Dialog {
        id: "destructive-schema-changes",
        title: "Include destructive changes?",
        spec: "SPEC-0110",
        presentation: Presentation::Drawn,
        ui_type: "ChoiceView",
        decisions: &[tool(
            "Include",
            "compare_schemas",
            "source, target, include_destructive=true",
        )],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::compare_schemas_orders_views_and_leaves_out_destructive_statements",
        ],
    },
    Dialog {
        id: "new-file",
        title: "New file name",
        spec: "SPEC-0275",
        presentation: Presentation::Drawn,
        ui_type: "NamePrompt",
        decisions: &[tool(
            "Create",
            "create_file",
            "dir, name, kind?, text?, dry_run: false",
        )],
        evidence: &["crates/mcp/tests/it/protocol.rs::creates_renames_and_deletes_files"],
    },
    Dialog {
        id: "rename-file",
        title: "Rename file",
        spec: "SPEC-0275",
        presentation: Presentation::Drawn,
        ui_type: "NamePrompt",
        decisions: &[tool("Rename", "rename_file", "path, new_name")],
        evidence: &["crates/mcp/tests/it/protocol.rs::creates_renames_and_deletes_files"],
    },
    Dialog {
        id: "paste-file",
        title: "Copy or move to folder",
        spec: "SPEC-0275",
        presentation: Presentation::Drawn,
        ui_type: "NamePrompt",
        decisions: &[
            tool(
                "Copy",
                "copy_file",
                "path, target_dir, new_name?, dry_run=false",
            ),
            tool(
                "Move",
                "move_file",
                "path, target_dir, new_name?, dry_run=false",
            ),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::copies_and_moves_files_and_folders"],
    },
    Dialog {
        id: "merge-conflicts",
        title: "Resolve conflicts",
        spec: "SPEC-0101",
        presentation: Presentation::Drawn,
        ui_type: "MergeView",
        decisions: &[
            tool("List conflicts", "git_conflicts", ""),
            tool(
                "Accept ours, theirs or a merged text",
                "git_resolve_conflict",
                "path, take or text",
            ),
            tool(
                "Read the result and the open conflicts",
                "git_resolve_conflict",
                "path",
            ),
            tool(
                "Take or ignore a side of one conflict, or edit it",
                "git_resolve_conflict",
                "path, choices=[{conflict, take or text}]",
            ),
        ],
        evidence: &[
            "crates/mcp/tests/it/protocol.rs::conflict_tools_resolve_a_stopped_merge",
            "crates/mcp/tests/it/protocol.rs::conflict_tool_reads_the_result_and_takes_choices",
        ],
    },
    Dialog {
        id: "stashes",
        title: "Stash, apply, pop or drop",
        spec: "SPEC-0106",
        presentation: Presentation::Drawn,
        ui_type: "StashView",
        decisions: &[tool(
            "Stash / Apply / Pop / Drop",
            "git_stash",
            "action, index?",
        )],
        evidence: &["crates/mcp/tests/it/protocol.rs::git_stash_cherry_pick_and_rebase_tools"],
    },
    Dialog {
        id: "stash-changes",
        title: "Stash changes with a message",
        spec: "SPEC-0106",
        presentation: Presentation::Drawn,
        ui_type: "StashDialog",
        decisions: &[tool(
            "Stash",
            "git_stash",
            "action=push, message?, keep_index?, include_untracked?",
        )],
        evidence: &["crates/mcp/tests/it/protocol.rs::git_branch_popup_tools"],
    },
    Dialog {
        id: "remote-delete",
        title: "Delete on the remote host?",
        spec: "SPEC-0253",
        presentation: Presentation::Drawn,
        ui_type: "RemoteView",
        decisions: &[tool(
            "Delete",
            "remote_delete",
            "host, path, recursive?, dry_run=false",
        )],
        evidence: &["crates/mcp/tests/it/protocol.rs::remote_file_operations"],
    },
    Dialog {
        id: "change-signature",
        title: "Change signature",
        spec: "SPEC-0113",
        presentation: Presentation::Drawn,
        ui_type: "GotoView",
        decisions: &[tool(
            "Refactor",
            "change_signature",
            "path, line, character, parameters, dry_run=false",
        )],
        evidence: &["crates/mcp/tests/it/protocol.rs::code_insight_tools"],
    },
    Dialog {
        id: "bookmark",
        title: "Bookmark with mnemonic",
        spec: "SPEC-0331",
        presentation: Presentation::Drawn,
        ui_type: "BookmarksPopup",
        decisions: &[
            tool(
                "Set with a mnemonic and a description",
                "set_bookmark",
                "path, line, mnemonic?, description?, dry_run=false",
            ),
            tool(
                "Remove",
                "remove_bookmark",
                "path and line, or mnemonic; dry_run=false",
            ),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::bookmark_tools"],
    },
    Dialog {
        id: "save-layout",
        title: "Save Current Layout as New",
        spec: "SPEC-0333",
        presentation: Presentation::Drawn,
        ui_type: "NamePrompt",
        decisions: &[
            tool(
                "Save under the name",
                "window_layouts",
                "action=save, name, shown?, active_window?, dry_run=false",
            ),
            tool(
                "Cancel (see what would be saved)",
                "window_layouts",
                "action=save, name (dry_run defaults to true)",
            ),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::window_layouts_tool"],
    },
    Dialog {
        id: "rename-layout",
        title: "Rename Layout",
        spec: "SPEC-0333",
        presentation: Presentation::Drawn,
        ui_type: "NamePrompt",
        decisions: &[tool(
            "Rename",
            "window_layouts",
            "action=rename, name, new_name, dry_run=false",
        )],
        evidence: &["crates/mcp/tests/it/protocol.rs::window_layouts_tool"],
    },
    Dialog {
        id: "delete-layout",
        title: "Delete layout?",
        spec: "SPEC-0333",
        presentation: Presentation::Drawn,
        ui_type: "DrawnQuestion",
        decisions: &[
            tool(
                "Delete",
                "window_layouts",
                "action=delete, name, dry_run=false",
            ),
            tool(
                "Cancel (see what would go)",
                "window_layouts",
                "action=delete, name (dry_run defaults to true)",
            ),
        ],
        evidence: &["crates/mcp/tests/it/protocol.rs::window_layouts_tool"],
    },
    Dialog {
        id: "file-encoding-change",
        title: "Change the encoding of <file> to <encoding>?",
        spec: "SPEC-0336",
        presentation: Presentation::Drawn,
        ui_type: "DrawnQuestion",
        decisions: &[
            tool(
                "Reload",
                "file_properties",
                "action=reload_encoding, path, encoding, dry_run=false (needs `write`)",
            ),
            tool(
                "Convert",
                "file_properties",
                "action=convert_encoding, path, encoding, dry_run=false (needs `write`)",
            ),
            tool(
                "Cancel (see what each would do)",
                "file_properties",
                "action=reload_encoding or convert_encoding, path, encoding (dry_run defaults to true)",
            ),
        ],
        evidence: &[
            "crates/workbench/src/file_properties_model.rs::the_encoding_question",
            "crates/mcp/tests/it/protocol.rs::file_properties_tool",
        ],
    },
    Dialog {
        id: "file-encoding-reload-unsaved",
        title: "Reload <file> in <encoding> and lose its unsaved changes?",
        spec: "SPEC-0336",
        presentation: Presentation::Drawn,
        ui_type: "DrawnQuestion",
        decisions: &[reserved(
            "Reload and Discard Changes / Cancel",
            "no tool discards an editor's unsaved changes for a reload: `file_properties` refuses a file that is modified in an editor; save it or decide in the window",
        )],
        evidence: &[
            "crates/workbench/src/file_properties_model.rs::the_encoding_question",
            "crates/mcp/tests/it/protocol.rs::file_properties_changes_need_the_write_right_and_stay_in_the_workspace",
        ],
    },
    Dialog {
        id: "file-encoding-lossy",
        title: "Convert <file> to <encoding> and lose characters?",
        spec: "SPEC-0336",
        presentation: Presentation::Drawn,
        ui_type: "DrawnQuestion",
        decisions: &[
            tool(
                "Convert Anyway",
                "file_properties",
                "action=convert_encoding, path, encoding, lossy=true, dry_run=false (needs `write`)",
            ),
            tool(
                "Cancel (see what would be lost)",
                "file_properties",
                "action=convert_encoding, path, encoding (dry_run defaults to true)",
            ),
        ],
        evidence: &[
            "crates/workbench/src/file_properties_model.rs::the_encoding_question",
            "crates/mcp/tests/it/protocol.rs::file_properties_tool",
        ],
    },
];

/// The dialog with `id`.
pub fn find(id: &str) -> Option<&'static Dialog> {
    DIALOGS.iter().find(|d| d.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::Path;

    fn repo() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
    }

    fn rust_sources(dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(std::fs::read_to_string(&path).unwrap());
            }
        }
    }

    #[test]
    fn every_tool_and_test_named_exists() {
        let tools: HashSet<String> = crate::IdeServer::tool_router()
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        let mut ids = HashSet::new();
        for d in DIALOGS {
            assert!(ids.insert(d.id), "{} listed twice", d.id);
            assert!(!d.decisions.is_empty(), "{} has no answers", d.id);
            assert!(!d.evidence.is_empty(), "{} has no test", d.id);
            for decision in d.decisions {
                match (decision.tool, decision.reason) {
                    (Some(t), None) => assert!(tools.contains(t), "{}: no tool {t}", d.id),
                    (None, Some(_)) => {}
                    _ => panic!("{}: an answer needs a tool or a reason", d.id),
                }
            }
            for e in d.evidence {
                let (file, function) = e.split_once("::").expect("file::function");
                let text = std::fs::read_to_string(repo().join(file))
                    .unwrap_or_else(|err| panic!("{}: {file}: {err}", d.id));
                assert!(
                    text.contains(&format!("fn {function}(")),
                    "{}: no test {function} in {file}",
                    d.id
                );
            }
            assert_eq!(
                d.presentation == Presentation::NativeAlert,
                d.ui_type.is_empty(),
                "{}: a drawn dialog names its type, a native alert none",
                d.id
            );
        }
    }

    #[test]
    fn every_dialog_type_of_the_workbench_is_listed() {
        let mut sources = Vec::new();
        rust_sources(&repo().join("crates/workbench/src"), &mut sources);
        let text = sources.join("\n");
        let listed: HashSet<&str> = DIALOGS.iter().map(|d| d.ui_type).collect();
        // Every type named in the list exists in the workbench.
        for t in listed.iter().filter(|t| !t.is_empty()) {
            assert!(
                text.contains(&format!("struct {t} ")) || text.contains(&format!("struct {t}<")),
                "no workbench type {t}"
            );
        }
        // Every `…Dialog` type of the workbench is in the list.
        let mut found = 0;
        for word in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if word.ends_with("Dialog")
                && word.len() > "Dialog".len()
                && word.starts_with(char::is_uppercase)
                && text.contains(&format!("pub struct {word} "))
            {
                found += 1;
                assert!(listed.contains(word), "dialog type {word} is not listed");
            }
        }
        assert!(found >= 5, "only {found} dialog types found");
    }
}
