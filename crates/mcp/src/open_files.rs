//! How each tool that changes files treats a file open in the window's editor (`SPEC-0220` v4
//! behaviours 7–16, readiness A2): every tool whose row in [`crate::effects::TOOLS`] names
//! [`crate::effects::Change::Files`] is in exactly one of the three lists below. The ratchet
//! `open_files::every_file_changing_tool_edits_open_files_through_the_buffer_or_refuses`
//! (`tests/it/open_files.rs`) fails for a tool in no list or two, for a buffer tool whose code does
//! not read the shown text and write through the buffer, for a refusing tool whose code makes no
//! refusal, and when [`REFUSES_UNSAVED`] grows past [`REFUSES_UNSAVED_MAX`].

/// Tools that edit an open file through its buffer, saved or not: they read the text the editor
/// shows (`read_document`, or the registry) and write with `write_document`, which hands the window
/// one undo step (behaviours 7 and 8), or with `write_documents`, which hands it the change of
/// several files all or nothing (language servers, refactorings and moves with reference updates,
/// behaviours 13–16), or with `write_shown`, which plans one change from the shown texts and saves
/// the editors first where a Git step takes the file from the disk next (v5 behaviours 17–19).
pub const THROUGH_BUFFER: &[&str] = &[
    "apply_code_action",
    "change_signature",
    "compare_files",
    "delegate_methods",
    "eslint_file",
    "extract_interface",
    "extract_superclass",
    "file_properties",
    "fix_inspection",
    "format_file",
    "generate_code",
    "git_line_change",
    "git_resolve_conflict",
    "git_rollback",
    "move_file",
    "optimize_imports",
    "override_methods",
    "prettier_file",
    "pull_members_up",
    "rename_sql_object",
    "rename_symbol",
    "resource_bundle",
    "restore_local_revision",
    "safe_delete",
    "suppress_inspection",
    "surround_with",
    "undo_refactoring",
    "update_copyright",
];

/// Tools whose change is computed from the disk: they refuse when a file they change is open with
/// unsaved changes (behaviour 9). A tool leaves this list when it moves to [`THROUGH_BUFFER`]; the
/// list never grows.
pub const REFUSES_UNSAVED: &[&str] = &[
    // Deleting and renaming a file: the editor would lose its file or not follow the new name
    // (SPEC-0220 v5 behaviour 22).
    "delete_file",
    "rename_file",
    // Git operations of the working tree as a whole.
    "git_branch_action",
    "git_cherry_pick",
    "git_commit_action",
    "git_merge",
    "git_pull",
    "git_rebase",
    "git_stash",
    "git_switch_branch",
    // Exports into the workspace.
    "er_diagram",
    "export_file",
    "export_store_result",
];

/// The length [`REFUSES_UNSAVED`] may have; lower it when a tool moves to [`THROUGH_BUFFER`].
pub const REFUSES_UNSAVED_MAX: usize = 13;

/// Tools that change only IDECraft's own data or settings files, create what does not exist yet,
/// change remote hosts or rooms, or act through the window's own steps (behaviour 11), with the
/// reason.
pub const OWN_FILES: &[(&str, &str)] = &[
    ("build_task", "build tool settings in .idecraft"),
    ("build_tool_links", "build tool settings in .idecraft"),
    ("build_tool_options", "build tool settings in .idecraft"),
    ("changelists", ".idecraft/changelists.toml"),
    ("clone_repository", "a new folder"),
    ("close_project", "the window's own close step"),
    ("copy_file", "never overwrites: new files only"),
    ("create_file", "never overwrites: new files only"),
    ("debug_renderers", ".idecraft/debug-renderers.json"),
    ("file_operation_undo", "the window's own Project tree undo"),
    (
        "file_type",
        "the project's file types in .idecraft/file-types.toml",
    ),
    ("git_branch_favorite", "a setting"),
    ("highlighting_level", "a setting in .idecraft"),
    ("import_connector", "the user's connectors folder"),
    ("import_file", "settings and run configurations"),
    ("inspection_profile", "the .idea inspection profile"),
    ("local_history", "IDECraft's own record"),
    ("new_project", "a new folder"),
    ("project_dictionary", "the .idea project dictionary"),
    ("python_create_venv", "a new environment folder"),
    ("python_select_interpreter", ".idecraft/python.toml"),
    ("quit_ide", "the window's own quit step"),
    ("remote_create", "an SSH host"),
    ("remote_delete", "an SSH host"),
    (
        "remote_project_close",
        "the remote backend owns its documents",
    ),
    ("remote_rename", "an SSH host"),
    ("remote_write", "an SSH host"),
    ("remove_bookmark", ".idecraft bookmarks"),
    ("remove_connection", "the project's connection profiles"),
    ("room_apply", "a room: the backend owns its documents"),
    ("room_join", "a room: the backend owns its documents"),
    ("room_restore", "a room: the backend owns its documents"),
    ("room_undo", "a room: the backend owns its documents"),
    ("room_version", "a room: the backend owns its documents"),
    ("run_configuration_edit", ".idecraft/run.toml"),
    ("save_connection", "the project's connection profiles"),
    ("save_store", "the project's connection profiles"),
    ("set_bookmark", ".idecraft bookmarks"),
    ("set_breakpoint", ".idecraft breakpoints"),
    ("set_special_breakpoints", ".idecraft breakpoints"),
    ("sql_console", "a new console file"),
    ("sql_file_data_source", ".idecraft/sql-files.toml"),
    ("tool_windows", "the tool window layout in .idecraft"),
    ("ui_trace", "trace files in a new folder"),
    ("window_layouts", "the user's settings folder"),
];
