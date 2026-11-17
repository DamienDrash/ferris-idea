//! Desktop views of IDECraft on GPUI (ADR-0003, ADR-0009).
//!
//! Implements `SPEC-0058` (editor view) `SPEC-0059` (project tree and tabs), `SPEC-0061` (window chrome) and
//! `SPEC-0062` (terminal tool window) and `SPEC-0063` (Commit tool window) and `SPEC-0076` (context actions) from `docs/specifications/`. The toolkit-free parts live in
//! [`layout`] and are tested on every platform; the views need macOS or Linux.

pub mod appearance;
mod attach_view;
pub mod authors_model;
pub mod blame_model;
pub mod bookmarks_model;
pub mod branches_model;
pub mod breakpoint_dialog;
pub mod breakpoint_popup;
pub mod build_sync_model;
pub mod build_tools_model;
pub mod caret_gesture;
pub mod change_bar_model;
pub mod changelist_model;
pub mod chrome;
pub mod close_model;
pub mod closed_history;
pub mod code_preview;
pub mod commit_model;
pub mod completion_model;
pub mod context_menu;
pub mod copy_path_model;
pub mod db_compare_model;
pub mod db_rename_dialog;
pub mod db_tree_model;
pub mod debug_model;
mod debug_remote;
mod debug_view;
pub mod decl_icon_model;
pub mod decl_marks_model;
pub mod diagnostics_model;
pub mod dialogs;
pub mod diff_actions;
pub mod diff_model;
pub mod doc_model;
pub mod editor_layout;
pub mod er_view_model;
pub mod everywhere_model;
pub mod file_clipboard;
pub mod file_properties_model;
pub mod find_in_files_model;
pub mod find_model;
pub mod find_usages_model;
mod floating;
pub mod fold_model;
pub mod fonts;
pub mod git_branch_tree;
pub mod git_graph;
pub mod git_log_marks;
pub mod git_log_model;
pub mod git_ops_model;
pub mod goto_model;
pub mod grid_model;
pub mod hierarchy_model;
pub mod history_model;
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub mod hit_targets;
pub mod http_model;
pub mod inlay_model;
pub mod intentions_model;
pub mod key_diagnostics;
pub mod keymap_actions;
pub mod keymaps;
pub mod layout;
pub mod list_nav;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod mouse;
pub mod nav_model;
pub mod occurrence_model;
pub mod palette;
pub mod plan_view_model;
pub mod plugin_run_model;
pub mod popup_place;
pub mod problem_marks;
pub mod problems_model;
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub mod project_popup_model;
pub mod project_tree;
pub mod project_tree_style;
pub mod project_windows;
pub mod python_fixes;
pub mod python_widget;
pub mod quit_model;
pub mod result_model;
pub mod run_debug_actions;
pub mod run_editor_model;
pub mod run_list_model;
pub mod run_model;
pub mod semantic_model;
pub mod semantics;
pub mod settings_model;
pub mod speed_search;
pub mod split_model;
pub mod sql_console_model;
pub mod sql_editor_model;
pub mod status_note_model;
pub mod structure_model;
pub mod tabs;
pub mod test_tree_model;
pub mod tips;
pub mod title_bar;
pub mod todo_model;
pub mod toolbar_fit;
pub mod tree_nav;
pub mod trust_model;
pub mod usages_model;
pub mod value_fold_model;
pub mod vcs_ops_model;
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub mod x11_wake;

mod ai_chat_view;
pub mod ai_context_model;
pub mod ai_markdown;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod bookmarks_view;
mod branches_view;
mod build_tools_view;
pub mod bundle_editor;
pub mod choice_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod commit_view;
pub mod committed_text;
mod connection_dialog;
mod connector_import;
mod console_session;
mod containers_view;
mod copy_table_dialog;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod database_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod diff_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod editor_view;
mod endpoints_view;
mod er_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod everywhere_view;
pub mod file_type_icons;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod find_bar;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod find_in_files_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod find_usages_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod git_log_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod goto_view;
mod hierarchy_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod history_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod icon_images;
mod import_preview;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod intentions_view;
pub mod menu_bar;
pub mod merge_session;
mod merge_view;
pub mod metrics;
mod plugin_network_dialog;
mod plugin_run_dialog;
mod profiler_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod projects_view;
mod python_view;
mod rebase_view;
mod remote_view;
mod rename_preview;
mod report_dialog;
mod scroll_bar;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod services_view;
mod settings_view;
mod spring_view;
mod stash_view;
mod tab_list_view;
mod table_session;
mod themes_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod todo_view;
mod vcs_ops_view;

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod run_config_dialog;
mod run_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod structure_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod terminal_view;
pub mod traffic_lights;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod usages_view;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod workbench_view;

pub use ai_chat_view::{AiChatEvent, AiChatView};
pub use attach_view::{AttachEvent, AttachRow, AttachView, attach_rows};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use bookmarks_view::{BookmarkEvent, BookmarksPopup, BookmarksToolView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use branches_view::{BranchesEvent, BranchesView};
pub use build_tools_view::{BuildToolsEvent, BuildToolsView};
pub use choice_view::{ChoiceEvent, ChoiceView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use commit_view::{CommitEvent, CommitView};
pub use connection_dialog::{
    CONNECTION_RESULT_HEIGHT, ConnectionDialog, ConnectionDialogEvent, Dialect, DialogPage,
};
pub use connector_import::{ConnectorImport, ConnectorImportEvent};
pub use containers_view::{ContainerTarget, ContainersEvent, ContainersView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use database_view::{DatabaseEvent, DatabaseView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use debug_view::{DebugMenu, DebugState, DebugView, DebugViewEvent};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use diff_view::{DiffEvent, DiffView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use editor_view::{Decorations, EditorEvent, EditorView};
pub use endpoints_view::{EndpointsEvent, EndpointsView, GENERATED_REQUESTS};
pub use er_view::{ErEvent, ErExport, ErView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use everywhere_view::{EverywhereEvent, EverywhereView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use find_in_files_view::{FindInFilesEvent, FindInFilesView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use find_usages_view::{FindState, FindUsagesEvent, FindUsagesView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use git_log_view::{BranchAction, GitLogEvent, GitLogView, MenuPage};
pub use goto_view::{GotoEvent, GotoView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use hierarchy_view::{HierarchyEvent, HierarchyMode, HierarchyView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use history_view::{HistoryEvent, HistoryView};
/// The database crate, for the window self-test of the connector catalogue (`SPEC-0302`).
pub use idecraft_database as database;
/// Isolation levels of data editor transactions (`SPEC-0190` v3 behaviour 13).
pub use idecraft_database::Isolation;
/// How a result is shown: table, tree or text (`SPEC-0099` v3 behaviour 10).
pub use idecraft_database::grid::ResultView;
pub use idecraft_plugins as plugins;
pub use idecraft_terminal::{
    LinkTarget as TerminalLinkTarget, MouseButton as TerminalMouseButton,
    MouseMode as TerminalMouseMode,
};
/// Tool window layout (SPEC-0167), shared with the MCP server.
pub use idecraft_ui::tool_windows;
pub use import_preview::{ImportPreview, ImportPreviewEvent};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use intentions_view::{IntentionsEvent, IntentionsView};
pub use merge_view::{MergeAsk, MergeEvent, MergeView};
pub use plugin_network_dialog::{PluginNetworkDialog, PluginNetworkDialogEvent};
pub use plugin_run_dialog::{PluginRunDialog, PluginRunDialogEvent};
pub use profiler_view::{ProfilerEvent, ProfilerTab, ProfilerView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use projects_view::{ProjectsEvent, ProjectsView};
pub use python_view::{FAILURE_LINES, InstallFailure, PythonEvent, PythonView};
pub use rebase_view::{RebaseEvent, RebaseView};
pub use remote_view::{Pending as RemotePending, RemoteEvent, RemoteView};
pub use rename_preview::{RenamePreview, RenamePreviewEvent};
pub use report_dialog::{ReportDialog, ReportDialogEvent};
pub use run_config_dialog::{RunConfigDialog, RunConfigDialogEvent};
pub use run_view::{RunChooser, RunChooserEvent, RunView, RunViewEvent};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use services_view::{
    ConsoleRun, ResultSet, SchemaDiffTab, ServicesEvent, ServicesView, ToolbarGroup,
};
pub use settings_view::{SettingsEvent, SettingsView};
pub use spring_view::{BeanFilter, SpringEvent, SpringView};
pub use stash_view::{StashDialog, StashDialogEvent, StashEvent, StashView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use structure_view::{StructureEvent, StructureFilter, StructureView};
pub use tab_list_view::{TabListEvent, TabListRow, TabListView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use terminal_view::{TerminalView, set_local_shell};
pub use themes_view::{ThemesEvent, ThemesView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use todo_view::{TodoEvent, TodoToolView};
pub use usages_view::{UsagesEvent, UsagesView};
pub use vcs_ops_view::{PushEvent, PushView, UpdateEvent, UpdateView, VcsOpsEvent, VcsOpsView};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use workbench_view::{
    EditorSelectionState, HiddenBottom, MenuPlace, PluginPanel, RemoteStateDir, WorkbenchView,
    open_live_window, queue_opened, take_opened,
};
