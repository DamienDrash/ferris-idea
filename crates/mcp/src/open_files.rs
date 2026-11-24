//! How each tool that changes files treats a file open in the window's editor (`SPEC-0220` v4
//! behaviours 7–16, readiness A2): every tool whose row in [`crate::effects::TOOLS`] names
//! [`crate::effects::Change::Files`] is in exactly one of the three lists below. The ratchet
//! `open_files::every_file_changing_tool_edits_open_files_through_the_buffer_or_refuses`
//! (`tests/it/open_files.rs`) fails for a tool in no list or two, for a buffer tool whose code does
//! not read the shown text and write through the buffer, for a refusing tool whose code makes no
//! refusal, and when [`REFUSES_UNSAVED`] grows past [`REFUSES_UNSAVED_MAX`].
//!
//! Where an entry goes (ADR-0035): a tool of `lib.rs` in [`THROUGH_BUFFER_TOOLS`]
//! or [`OWN_FILES_TOOLS`] here; a tool of its own module in that module's `pub(crate) const
//! THROUGH_BUFFER_TOOLS: &[&str]` or `pub(crate) const OWN_FILES_TOOLS: &[(&str, &str)]`.
//! `build.rs` finds every such const, and [`THROUGH_BUFFER`] and [`OWN_FILES`] join them sorted.

use std::sync::LazyLock;

/// The tools of `lib.rs` that edit an open file through its buffer, saved or not: they read the
/// text the editor shows (`read_document`, or the registry) and write with `write_document`, which
/// hands the window one undo step (behaviours 7 and 8), or with `write_documents`, which hands it
/// the change of several files all or nothing (language servers, refactorings and moves with
/// reference updates, behaviours 13–16), or with `write_shown`, which plans one change from the
/// shown texts and saves the editors first where a Git step takes the file from the disk next (v5
/// behaviours 17–19).
pub(crate) const THROUGH_BUFFER_TOOLS: &[&str] = &[
    "apply_code_action",
    "apply_context_action",
    "apply_patch",
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
    "redo_refactoring",
    "rename_sql_object",
    "rename_symbol",
    "replace_text_in_file",
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
/// list never grows, so it stays one list here.
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

/// The tools of `lib.rs` that change only Ferris IDEA's own data or settings files, create what does
/// not exist yet, change remote hosts or rooms, or act through the window's own steps (behaviour
/// 11), with the reason.
pub(crate) const OWN_FILES_TOOLS: &[(&str, &str)] = &[
    ("add_idea_data_source", "the project's connection profiles"),
    ("build_task", "build tool settings in .ferris-idea"),
    ("build_tool_links", "build tool settings in .ferris-idea"),
    ("build_tool_options", "build tool settings in .ferris-idea"),
    ("changelists", ".ferris-idea/changelists.toml"),
    ("clone_repository", "a new folder"),
    ("close_project", "the window's own close step"),
    ("copy_file", "never overwrites: new files only"),
    ("create_file", "never overwrites: new files only"),
    ("create_test", "never overwrites: new files only"),
    ("debug_renderers", ".ferris-idea/debug-renderers.json"),
    ("file_operation_undo", "the window's own Project tree undo"),
    (
        "file_type",
        "the project's file types in .ferris-idea/file-types.toml",
    ),
    ("git_branch_favorite", "a setting"),
    ("highlighting_level", "a setting in .ferris-idea"),
    ("import_connector", "the user's connectors folder"),
    ("import_file", "settings and run configurations"),
    ("inspection_profile", "the .idea inspection profile"),
    ("local_history", "Ferris IDEA's own record"),
    ("new_project", "a new folder"),
    ("project_dictionary", "the .idea project dictionary"),
    ("python_create_venv", "a new environment folder"),
    ("python_select_interpreter", ".ferris-idea/python.toml"),
    ("quit_ide", "the window's own quit step"),
    ("remote_create", "an SSH host"),
    ("remote_delete", "an SSH host"),
    (
        "remote_project_close",
        "the remote backend owns its documents",
    ),
    ("remote_rename", "an SSH host"),
    ("remote_write", "an SSH host"),
    ("remove_bookmark", ".ferris-idea bookmarks"),
    ("remove_connection", "the project's connection profiles"),
    ("room_apply", "a room: the backend owns its documents"),
    ("room_join", "a room: the backend owns its documents"),
    ("room_join_link", "a room: the backend owns its documents"),
    ("room_restore", "a room: the backend owns its documents"),
    ("room_undo", "a room: the backend owns its documents"),
    ("room_version", "a room: the backend owns its documents"),
    ("run_configuration_edit", ".ferris-idea/run.toml"),
    ("save_connection", "the project's connection profiles"),
    ("save_store", "the project's connection profiles"),
    ("set_bookmark", ".ferris-idea bookmarks"),
    ("set_breakpoint", ".ferris-idea breakpoints"),
    ("set_special_breakpoints", ".ferris-idea breakpoints"),
    ("sql_console", "a new console file"),
    ("sql_file_data_source", ".ferris-idea/sql-files.toml"),
    ("tool_windows", "the tool window layout in .ferris-idea"),
    ("ui_trace", "trace files in a new folder"),
    ("window_layouts", "the user's settings folder"),
];

/// Every tool that edits an open file through its buffer (see [`THROUGH_BUFFER_TOOLS`]): the
/// entries of every module, sorted.
pub static THROUGH_BUFFER: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut all: Vec<&'static str> = crate::registry::THROUGH_BUFFER
        .iter()
        .flat_map(|tools| tools.iter().copied())
        .collect();
    all.sort_unstable();
    all
});

/// Every tool that changes only Ferris IDEA's own files and the like (see [`OWN_FILES_TOOLS`]), with
/// the reason: the entries of every module, sorted by tool.
pub static OWN_FILES: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    let mut all: Vec<(&'static str, &'static str)> = crate::registry::OWN_FILES
        .iter()
        .flat_map(|tools| tools.iter().copied())
        .collect();
    all.sort_unstable();
    all
});
