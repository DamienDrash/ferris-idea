//! Context menus (`SPEC-0275` behaviours 1, 2 and 13–16), without toolkit: what a secondary click
//! is, and the items of the menus of the project tree, the editor text and the database tree with
//! their enabled state. Items carry keymap action ids where the action has one, so the menu shows
//! the shortcut of the keymap in use; the workbench runs them.

use idecraft_core::file_ops::NewKind;

pub use crate::blame_model::{ANNOTATE, CLOSE_ANNOTATIONS, COPY_REVISION, SHOW_IN_GIT_LOG};

/// A mouse button as the toolkit reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// Left button, one-finger click.
    Primary,
    /// Right button, two-finger click on a trackpad.
    Secondary,
    /// Middle and other buttons.
    Other,
}

/// Whether a press of `button` with Control held (`control`) opens a context menu. On macOS a
/// Control-click is a secondary click (behaviour 2).
pub fn is_secondary_click(button: Button, control: bool, macos: bool) -> bool {
    match button {
        Button::Secondary => true,
        Button::Primary => control && macos,
        Button::Other => false,
    }
}

/// One entry of a context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    /// A command.
    Item(MenuItem),
    /// A line between groups.
    Separator,
    /// A submenu.
    Sub(String, Vec<MenuEntry>),
}

/// A command of a context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// The label.
    pub label: String,
    /// The action id: a keymap action id where one exists, else an `IDECraft.*` id.
    pub action: String,
    /// Whether it can be chosen now.
    pub enabled: bool,
    /// A switch that is on: shown with a check mark.
    pub checked: bool,
}

fn item(label: impl Into<String>, action: impl Into<String>, enabled: bool) -> MenuEntry {
    MenuEntry::Item(MenuItem {
        label: label.into(),
        action: action.into(),
        enabled,
        checked: false,
    })
}

fn switch(label: &str, action: &str, enabled: bool, on: bool) -> MenuEntry {
    MenuEntry::Item(MenuItem {
        label: label.to_owned(),
        action: action.to_owned(),
        enabled,
        checked: on,
    })
}

fn sub(label: &str, entries: Vec<MenuEntry>) -> MenuEntry {
    MenuEntry::Sub(label.to_owned(), entries)
}

/// Action id of "New › <kind>".
pub fn new_action(kind: NewKind) -> String {
    format!("IDECraft.New.{}", kind.id())
}

/// The kind of a "New › …" action id.
pub fn new_kind(action: &str) -> Option<NewKind> {
    NewKind::from_id(action.strip_prefix("IDECraft.New.")?)
}

/// Copy the absolute path.
pub const COPY_PATH: &str = "CopyPaths";
/// Copy the path from the project folder.
pub const COPY_RELATIVE_PATH: &str = "IDECraft.CopyRelativePath";
/// Copy the file name.
pub const COPY_FILE_NAME: &str = "IDECraft.CopyFileName";
/// Copy the path from the source folder (`SPEC-0332` v2 behaviour 13a).
pub const COPY_SOURCE_ROOT_PATH: &str = crate::copy_path_model::SOURCE_ROOT_PATH;
/// Copy the file's reference name with the caret's line (`SPEC-0332` v2 behaviour 13a).
pub const COPY_PATH_WITH_LINE: &str = crate::copy_path_model::PATH_WITH_LINE;
/// Copy the qualified name of the symbol around the caret (`SPEC-0332` v2 behaviour 13b).
pub const COPY_REFERENCE: &str = crate::copy_path_model::REFERENCE;
/// Show the file in Finder (the file manager elsewhere).
pub const REVEAL: &str = "RevealIn";
/// A terminal in the folder.
pub const OPEN_IN_TERMINAL: &str = "Terminal.OpenInTerminal";
/// Rename the selected file or folder.
pub const RENAME_FILE: &str = "RenameElement";
/// Delete the selected file or folder (asks first).
pub const DELETE_FILE: &str = "$Delete";
/// Local history of the selected file.
pub const SHOW_HISTORY: &str = "LocalHistory.ShowHistory";
/// Cut the selected file or folder (moved on Paste).
pub const CUT_FILE: &str = "$Cut";
/// Copy the selected file or folder (copied on Paste).
pub const COPY_FILE: &str = "$Copy";
/// Paste the cut or copied file or folder into the selection.
pub const PASTE_FILE: &str = "$Paste";
/// Switch column (block) selection on or off.
pub const COLUMN_MODE: &str = "EditorToggleColumnMode";
/// Compare the clipboard with the selection or the file (v6 behaviour 14b).
pub const COMPARE_CLIPBOARD: &str = "CompareClipboardWithSelection";
/// Commit only this file from the Commit tool window (v8 behaviour 14c).
pub const VCS_COMMIT_FILE: &str = "CheckinFiles";
/// Roll this file back, after the Commit tool window's question (v8 behaviour 14c).
pub const VCS_ROLLBACK_FILE: &str = "ChangesView.Revert";
/// The Git log filtered to this file (v8 behaviour 14c).
pub const VCS_FILE_HISTORY: &str = "Vcs.ShowTabbedFileHistory";
/// The diff of this file against HEAD (the keymap's action, v8 behaviour 14c).
pub const VCS_SHOW_DIFF: &str = "Compare.SameVersion";
/// Compare this file with a branch (v8 behaviour 14c).
pub const VCS_COMPARE_BRANCH: &str = "IDECraft.Vcs.CompareWithBranch";
/// The file items of the Git submenu, in order (v8 behaviour 14c).
pub const VCS_FILE_ITEMS: [(&str, &str); 5] = [
    ("Commit File…", VCS_COMMIT_FILE),
    ("Rollback…", VCS_ROLLBACK_FILE),
    ("Show History", VCS_FILE_HISTORY),
    ("Show Diff", VCS_SHOW_DIFF),
    ("Compare with Branch…", VCS_COMPARE_BRANCH),
];

/// Diff rows of Compare with Clipboard: the clipboard text on the left, `text` (the selection or
/// the file) on the right; `None` with an empty clipboard (v6 behaviour 14b).
pub fn clipboard_comparison(
    clipboard: Option<&str>,
    text: &str,
) -> Option<Vec<idecraft_vcs::DiffRow>> {
    let clip = clipboard.filter(|c| !c.is_empty())?;
    Some(idecraft_vcs::side_by_side(clip, text))
}

/// The entries of "New" (behaviour 3): File and Directory, then the typed files.
pub fn new_entries() -> Vec<MenuEntry> {
    let mut out = Vec::new();
    for kind in NewKind::ALL {
        if matches!(kind, NewKind::JavaClass | NewKind::SqlFile) {
            out.push(MenuEntry::Separator);
        }
        out.push(item(kind.label(), new_action(kind), true));
    }
    out
}

/// The Git submenu: what IDECraft carries out for a file today. `annotate` (`Some` for a file)
/// adds Annotate with Git Blame, `Some(true)` while its annotations are shown (`SPEC-0277`
/// behaviour 1); it is on even outside a repository, where it says why it cannot annotate
/// (IMP-0278). A file also gets its commit, rollback, history, diff and branch comparison (v8
/// behaviour 14c).
fn git_entries(repository: bool, annotate: Option<bool>) -> Vec<MenuEntry> {
    let mut out = Vec::new();
    if let Some(annotated) = annotate {
        out.push(annotate_item(annotated));
        out.push(MenuEntry::Separator);
        out.extend(
            VCS_FILE_ITEMS
                .iter()
                .map(|(label, action)| item(*label, *action, repository)),
        );
        out.push(MenuEntry::Separator);
    }
    out.extend([
        item("Commit…", "CheckinProject", repository),
        item(
            "Show Git Log",
            "ActivateVersionControlToolWindow",
            repository,
        ),
        item("Branches…", "Git.Branches", repository),
    ]);
    out
}

/// Label of the Annotate action: Close Annotations while they are shown (`SPEC-0277` behaviour 1).
pub fn annotate_label(annotated: bool) -> &'static str {
    if annotated {
        "Close Annotations"
    } else {
        "Annotate with Git Blame"
    }
}

fn annotate_item(annotated: bool) -> MenuEntry {
    item(annotate_label(annotated), ANNOTATE, true)
}

/// The menu of a secondary click on the line-number gutter (`SPEC-0277` behaviour 1).
pub fn gutter_menu(annotated: bool) -> Vec<MenuEntry> {
    vec![annotate_item(annotated)]
}

/// The menu of a secondary click on the annotation column (`SPEC-0277` behaviour 6); `committed`
/// when the clicked line has a commit.
pub fn annotation_menu(committed: bool) -> Vec<MenuEntry> {
    vec![
        item("Close Annotations", CLOSE_ANNOTATIONS, true),
        MenuEntry::Separator,
        item("Show in Git Log", SHOW_IN_GIT_LOG, committed),
        item("Copy Revision Number", COPY_REVISION, committed),
    ]
}

/// What the project tree's menu needs to know of the clicked row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeTarget {
    /// The row is a folder.
    pub dir: bool,
    /// The row is the project folder.
    pub root: bool,
    /// The project is a Git repository.
    pub repository: bool,
    /// Cut or Copy put a file or folder on the file clipboard (Paste, v2 behaviour 17).
    pub paste: bool,
    /// Several items are selected and the menu acts on all of them (v3 behaviour 25).
    pub several: bool,
}

/// The menu of a project tree row (behaviours 1 and 17).
pub fn project_tree_menu(t: TreeTarget) -> Vec<MenuEntry> {
    let reveal = if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else {
        "Open Containing Folder"
    };
    vec![
        sub("New", new_entries()),
        MenuEntry::Separator,
        item("Cut", CUT_FILE, !t.root),
        item("Copy", COPY_FILE, !t.root),
        sub(
            "Copy Path/Reference",
            vec![
                item("Absolute Path", COPY_PATH, true),
                item("Path From Project Root", COPY_RELATIVE_PATH, !t.root),
                item("Path From Source Root", COPY_SOURCE_ROOT_PATH, !t.root),
                item("File Name", COPY_FILE_NAME, true),
            ],
        ),
        item("Paste", PASTE_FILE, t.paste),
        MenuEntry::Separator,
        item("Rename…", RENAME_FILE, !t.root && !t.several),
        item("Delete…", DELETE_FILE, !t.root),
        MenuEntry::Separator,
        item(reveal, REVEAL, true),
        item("Open in Terminal", OPEN_IN_TERMINAL, true),
        MenuEntry::Separator,
        sub(
            "Local History",
            vec![item("Show History", SHOW_HISTORY, !t.dir)],
        ),
        sub("Git", git_entries(t.repository, (!t.dir).then_some(false))),
    ]
}

/// What the editor's menu needs to know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorTarget {
    /// Text is selected (Cut and Copy).
    pub selection: bool,
    /// The file can be run on its own; what "Run '…'" and "Debug '…'" name (behaviour 14a).
    pub runnable: Option<RunNames>,
    /// The project is a Git repository.
    pub repository: bool,
    /// The editor shows annotations (`SPEC-0277`).
    pub annotated: bool,
    /// A debug session is paused (Evaluate Expression, Add to Watches, Run to Cursor; D2).
    pub debugging: bool,
}

/// Pin or unpin the tab (`SPEC-0059` v9 behaviour 20).
pub const PIN_TAB: &str = "PinActiveEditorTab";
/// Open the tab in a second editor window: not yet (v9 behaviour 18, out of scope).
pub const OPEN_TAB_IN_NEW_WINDOW: &str = "IDECraft.OpenTabInNewWindow";
/// Settings › Editor › Editor Tabs (v9 behaviour 17).
pub const CONFIGURE_EDITOR_TABS: &str = "IDECraft.ConfigureEditorTabs";
/// Why Open Tab in New Window is disabled (its tooltip).
pub const NO_NEW_WINDOW: &str = "IDECraft has one editor window per project for now";

/// What the menu of an editor tab needs to know (`SPEC-0059` v9 behaviour 18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabTarget {
    /// The tab is pinned (the item reads Unpin Tab).
    pub pinned: bool,
    /// Some tab of the group is pinned (Close All but Pinned).
    pub any_pinned: bool,
    /// The editor area is split (Move Tab to Opposite Group).
    pub split: bool,
    /// The file can be run on its own: Run and Debug.
    pub runnable: Option<RunNames>,
    /// The tab's file shows annotations.
    pub annotated: bool,
}

/// The menu of an editor tab, carried out on that tab (`SPEC-0059` v9 behaviour 18).
pub fn tab_menu(t: &TabTarget) -> Vec<MenuEntry> {
    let reveal = if cfg!(target_os = "macos") {
        "Finder"
    } else {
        "File Manager"
    };
    let mut out = vec![
        item("Close", "CloseContent", true),
        item("Close Other Tabs", "CloseAllEditorsButActive", true),
        item("Close All Tabs", "CloseAllEditors", true),
        item("Close Tabs to the Left", "CloseAllToTheLeft", true),
        item("Close Tabs to the Right", "CloseAllToTheRight", true),
        item("Close Unmodified Tabs", "CloseAllUnmodifiedEditors", true),
    ];
    if t.any_pinned {
        out.push(item(
            "Close All but Pinned",
            "CloseAllUnpinnedEditors",
            true,
        ));
    }
    out.extend([
        MenuEntry::Separator,
        sub(
            "Copy Path/Reference",
            vec![
                item("Absolute Path", COPY_PATH, true),
                item("Path From Project Root", COPY_RELATIVE_PATH, true),
                item("Path From Source Root", COPY_SOURCE_ROOT_PATH, true),
                item("File Name", COPY_FILE_NAME, true),
                item("Path With Line Number", COPY_PATH_WITH_LINE, true),
                item("Copy Reference", COPY_REFERENCE, true),
            ],
        ),
        MenuEntry::Separator,
        item("Split Right", "SplitVertically", true),
        item("Split and Move Right", "SplitAndMoveRight", true),
        item("Split Down", "SplitHorizontally", true),
        item("Split and Move Down", "SplitAndMoveDown", true),
    ]);
    if t.split {
        out.push(item(
            "Move Tab to Opposite Group",
            "MoveEditorToOppositeTabGroup",
            true,
        ));
    }
    out.extend([
        MenuEntry::Separator,
        item(
            if t.pinned { "Unpin Tab" } else { "Pin Tab" },
            PIN_TAB,
            true,
        ),
        item("Open Tab in New Window", OPEN_TAB_IN_NEW_WINDOW, false),
        item("Configure Editor Tabs…", CONFIGURE_EDITOR_TABS, true),
        MenuEntry::Separator,
    ]);
    if let Some(name) = &t.runnable {
        out.push(item(format!("Run '{}'", name.run), "RunClass", true));
        out.push(item(format!("Debug '{}'", name.debug), "DebugClass", true));
    }
    out.extend([
        sub(
            "Open In",
            vec![
                item(reveal, REVEAL, true),
                item("Terminal", OPEN_IN_TERMINAL, true),
            ],
        ),
        sub(
            "Local History",
            vec![item("Show History", SHOW_HISTORY, true)],
        ),
        sub("Git", vec![annotate_item(t.annotated)]),
        item("Rename File…", RENAME_FILE, true),
    ]);
    out
}

/// Associate the file's extension with a file type (`SPEC-0004` v5 behaviour 13).
pub const ASSOCIATE_FILE_TYPE: &str = "AssociateWithFileType";
/// Override the file's type.
pub const OVERRIDE_FILE_TYPE: &str = "OverrideFileTypeAction";
/// Take the override away (the open platform's id, spelt as it is).
pub const REVERT_FILE_TYPE: &str = "ReverteOverrideFileTypeAction";

/// What the file type items need to know of a file (`SPEC-0004` v5 behaviour 13).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileTypeTarget {
    /// The file's language is known (Associate with File Type is disabled).
    pub known: bool,
    /// The file has an override (Revert File Type Override is enabled).
    pub overridden: bool,
}

/// The file type items: Associate with File Type…, then Override File Type for a file without
/// an override or Revert File Type Override for one with it (`SPEC-0004` v6 behaviour 16).
pub fn file_type_entries(t: FileTypeTarget) -> Vec<MenuEntry> {
    vec![
        item("Associate with File Type…", ASSOCIATE_FILE_TYPE, !t.known),
        if t.overridden {
            item("Revert File Type Override", REVERT_FILE_TYPE, true)
        } else {
            item("Override File Type", OVERRIDE_FILE_TYPE, true)
        },
    ]
}

/// The tab menu with the file type items after Rename File… (`SPEC-0004` v5 behaviour 13).
pub fn with_file_type_entries_in_tab_menu(
    mut menu: Vec<MenuEntry>,
    t: FileTypeTarget,
) -> Vec<MenuEntry> {
    menu.push(MenuEntry::Separator);
    menu.extend(file_type_entries(t));
    menu
}

/// The project tree's menu of a file with the file type items after New (`SPEC-0004` v5
/// behaviour 13); a folder's menu stays as it is.
pub fn with_file_type_entries_in_tree_menu(
    mut menu: Vec<MenuEntry>,
    file: Option<FileTypeTarget>,
) -> Vec<MenuEntry> {
    if let Some(t) = file {
        let at = menu
            .iter()
            .position(|e| matches!(e, MenuEntry::Sub(l, _) if l == "New"))
            .map_or(0, |i| i + 1);
        for (i, e) in file_type_entries(t).into_iter().enumerate() {
            menu.insert(at + i, e);
        }
    }
    menu
}

/// The menu of the editor text (behaviour 14).
pub fn editor_menu(t: &EditorTarget) -> Vec<MenuEntry> {
    let mut out = vec![
        item("Show Context Actions", "ShowIntentionActions", true),
        MenuEntry::Separator,
    ];
    if t.debugging {
        // As in the reference while paused (SPEC-0091 behaviour 6, D2).
        out.extend([
            item("Evaluate Expression…", "EvaluateExpression", true),
            item("Add to Watches", "Debugger.AddToWatch", true),
            item("Run to Cursor", "RunToCursor", true),
            MenuEntry::Separator,
        ]);
    }
    if t.selection {
        out.extend([item("Cut", "$Cut", true), item("Copy", "$Copy", true)]);
    }
    out.extend([
        item("Paste", "$Paste", true),
        item("Column Selection Mode", COLUMN_MODE, true),
        MenuEntry::Separator,
        sub(
            "Go To",
            vec![
                item("Declaration or Usages", "GotoDeclaration", true),
                item("Implementation(s)", "GotoImplementation", true),
                item("Type Declaration", "GotoTypeDeclaration", true),
            ],
        ),
        item("Find Usages", "FindUsages", true),
        sub(
            "Folding",
            vec![
                item("Expand", "ExpandRegion", true),
                item("Collapse", "CollapseRegion", true),
                MenuEntry::Separator,
                // SPEC-0083 v8 behaviour 23.
                item("Expand Recursively", "ExpandRegionRecursively", true),
                item("Collapse Recursively", "CollapseRegionRecursively", true),
                MenuEntry::Separator,
                item("Expand All", "ExpandAllRegions", true),
                item("Collapse All", "CollapseAllRegions", true),
                MenuEntry::Separator,
                item("Expand Doc Comments", "ExpandDocComments", true),
                item("Collapse Doc Comments", "CollapseDocComments", true),
            ],
        ),
        sub(
            "Analyze",
            vec![
                item("Inspect Code…", "InspectCode", true),
                item("Show Problems", "ActivateProblemsViewToolWindow", true),
            ],
        ),
        sub(
            "Refactor",
            vec![
                item("Rename…", "RenameElement", true),
                item("Change Signature…", "ChangeSignature", true),
                MenuEntry::Separator,
                item("Extract Variable…", "IntroduceVariable", true),
                item("Extract Constant…", "IntroduceConstant", true),
                item("Extract Field…", "IntroduceField", true),
                item("Extract Method…", "ExtractMethod", true),
                MenuEntry::Separator,
                item("Extract Interface…", "ExtractInterface", true),
                item("Extract Superclass…", "ExtractSuperclass", true),
                item("Pull Members Up…", "MembersPullUp", true),
                MenuEntry::Separator,
                item("Inline…", "Inline", true),
            ],
        ),
        item("Generate…", "Generate", true),
        item("Reformat Code", "ReformatCode", true),
    ]);
    if let Some(name) = &t.runnable {
        out.push(MenuEntry::Separator);
        out.push(item(format!("Run '{}'", name.run), "RunClass", true));
        out.push(item(format!("Debug '{}'", name.debug), "DebugClass", true));
        out.push(sub(
            "More Run/Debug",
            vec![item("Run Configurations…", "ChooseRunConfiguration", true)],
        ));
    }
    let reveal = if cfg!(target_os = "macos") {
        "Finder"
    } else {
        "File Manager"
    };
    out.push(MenuEntry::Separator);
    out.push(sub(
        "Open In",
        vec![
            item(reveal, REVEAL, true),
            item("Terminal", OPEN_IN_TERMINAL, true),
        ],
    ));
    out.push(sub(
        "Local History",
        vec![item("Show History", SHOW_HISTORY, true)],
    ));
    out.push(item("Compare with Clipboard", COMPARE_CLIPBOARD, true));
    out.push(sub("Git", git_entries(t.repository, Some(t.annotated))));
    out
}

/// What the editor's Run and Debug items name (behaviour 14a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunNames {
    /// Name in "Run '…'".
    pub run: String,
    /// Name in "Debug '…'".
    pub debug: String,
}

impl RunNames {
    /// The same name for both items.
    pub fn both(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            run: name.clone(),
            debug: name,
        }
    }
}

/// Names of the Run and Debug items for the file `file_name` with text `text` and the caret on
/// 0-based `caret_line` (behaviour 14a): `Class.main()` for a Java main class, the class for a
/// test class, `method()` for Run inside a test method, else the file name.
pub fn run_names(file_name: &str, text: &str, caret_line: Option<usize>) -> RunNames {
    use idecraft_languages::jvm::{self, MarkerKind};
    let Some(lang) = jvm::Jvm::of(file_name) else {
        return RunNames::both(file_name);
    };
    let markers = jvm::run_markers(lang, text);
    let class = markers
        .iter()
        .find(|m| m.kind == MarkerKind::Main && !m.class.is_empty())
        .map(|m| format!("{}.main()", m.class))
        .or_else(|| {
            markers
                .iter()
                .find(|m| m.kind == MarkerKind::TestClass)
                .map(|m| m.class.clone())
        });
    let Some(class) = class else {
        return RunNames::both(file_name);
    };
    let method = caret_line
        .and_then(|line| jvm::test_method_at(lang, text, line))
        .and_then(|m| m.method)
        .map(|m| format!("{m}()"));
    RunNames {
        run: method.unwrap_or_else(|| class.clone()),
        debug: class,
    }
}

/// What is selected in the database tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbTarget {
    /// Nothing (empty tree, or a message row).
    Nothing,
    /// A connection.
    Connection,
    /// A schema or a folder of objects.
    Folder,
    /// A table or view.
    Table,
    /// A column.
    Column,
    /// An index, sequence or named foreign key (`SPEC-0164` v3 behaviour 10).
    Object,
    /// A routine, trigger or type: its definition can be shown (`SPEC-0089` v13 behaviour 25).
    Definition,
}

/// Open the connection dialog for a new data source.
pub const DB_NEW_SOURCE: &str = "IDECraft.Database.NewDataSource";
/// Open a query console on the selected data source.
pub const DB_CONSOLE: &str = "Console.Open";
/// Jump to the query console of the selected data source.
pub const DB_JUMP_CONSOLE: &str = "IDECraft.Database.JumpToConsole";
/// Read the schema again.
pub const DB_REFRESH: &str = "Refresh";
/// Properties of the connection, or the data of a table (F4).
pub const DB_PROPERTIES: &str = "EditSource";
/// Change the selected column's type (`SPEC-0164` v3 behaviour 10).
pub const DB_CHANGE_TYPE: &str = "ChangeSignature";
/// Remove the connection (asks first).
pub const DB_DELETE: &str = "$Delete";
/// Copy the name.
pub const DB_COPY: &str = "$Copy";
/// Rename a table or column.
pub const DB_RENAME: &str = "RenameElement";
/// ER diagram of the selection.
pub const DB_DIAGRAM: &str = "Sql.ShowErDiagram";
/// The selected table's or view's definition as SQL in a read-only tab (`SPEC-0089` v5
/// behaviour 11).
pub const DB_DDL: &str = "IDECraft.Database.ShowDdl";
/// Compare the selected row's connection with another one (`SPEC-0110` v5 behaviour 9).
pub const DB_COMPARE: &str = "IDECraft.Database.CompareSchemas";
/// Compare the selected table or view with one other object (`SPEC-0110` v12 behaviour 18).
pub const DB_COMPARE_OBJECTS: &str = "IDECraft.Database.CompareObjects";
/// The plan of the statement at the caret (`SPEC-0110` behaviour 1).
pub const SQL_EXPLAIN: &str = "Sql.ExplainPlan";
/// The measured plan of the statement at the caret.
pub const SQL_EXPLAIN_ANALYZE: &str = "Sql.ExplainAnalyze";

/// The editor's menu of a `.sql` file gets Explain Plan and Explain Analyze after Find Usages
/// (`SPEC-0110` v5 behaviour 9); other files keep their menu.
pub fn with_sql_entries(mut menu: Vec<MenuEntry>, sql: bool) -> Vec<MenuEntry> {
    if !sql {
        return menu;
    }
    let at = menu
        .iter()
        .position(|e| matches!(e, MenuEntry::Item(i) if i.action == "FindUsages"))
        .map_or(menu.len(), |i| i + 1);
    menu.splice(
        at..at,
        [
            item("Explain Plan", SQL_EXPLAIN, true),
            item("Explain Analyze", SQL_EXPLAIN_ANALYZE, true),
        ],
    );
    menu
}

/// Send every request of the active request file (`SPEC-0096` v5 behaviour 8).
pub const HTTP_RUN_ALL: &str = "IDECraft.HttpRunAllRequests";

/// The editor's menu of a `.http` or `.rest` file gets Run All Requests in File after Reformat
/// Code, behind a separator (`SPEC-0096` v6 behaviour 8); other files keep their menu.
pub fn with_http_entries(mut menu: Vec<MenuEntry>, request_file: bool) -> Vec<MenuEntry> {
    if !request_file {
        return menu;
    }
    let at = menu
        .iter()
        .position(|e| matches!(e, MenuEntry::Item(i) if i.action == "ReformatCode"))
        .map_or(menu.len(), |i| i + 1);
    menu.splice(
        at..at,
        [
            MenuEntry::Separator,
            item("Run All Requests in File", HTTP_RUN_ALL, true),
        ],
    );
    menu
}

/// The menu of the database tree (behaviour 15).
pub fn database_menu(t: DbTarget) -> Vec<MenuEntry> {
    let any = t != DbTarget::Nothing;
    vec![
        sub(
            "New",
            vec![
                item("Data Source…", DB_NEW_SOURCE, true),
                item("Query Console", DB_CONSOLE, any),
            ],
        ),
        item("Jump to Query Console…", DB_JUMP_CONSOLE, any),
        MenuEntry::Separator,
        item("Refresh", DB_REFRESH, any),
        item(
            if t == DbTarget::Table {
                "Edit Data"
            } else {
                "Properties"
            },
            DB_PROPERTIES,
            matches!(t, DbTarget::Connection | DbTarget::Table),
        ),
        item(
            "Show DDL",
            DB_DDL,
            matches!(t, DbTarget::Table | DbTarget::Definition),
        ),
        MenuEntry::Separator,
        item("Copy", DB_COPY, any),
        item(
            "Rename…",
            DB_RENAME,
            matches!(t, DbTarget::Table | DbTarget::Column | DbTarget::Object),
        ),
        item("Change Column Type…", DB_CHANGE_TYPE, t == DbTarget::Column),
        item("Delete…", DB_DELETE, t == DbTarget::Connection),
        MenuEntry::Separator,
        item("Compare Schemas…", DB_COMPARE, any),
        item("Compare With…", DB_COMPARE_OBJECTS, t == DbTarget::Table),
        item(
            "Diagrams › Show Diagram",
            DB_DIAGRAM,
            matches!(t, DbTarget::Connection | DbTarget::Folder | DbTarget::Table),
        ),
    ]
}

/// Explain the terminal's selection, else its last command, with AI (`SPEC-0313` behaviour 4).
pub const AI_EXPLAIN_TERMINAL: &str = "IDECraft.AiExplainTerminal";
/// Attach the terminal's selection to the chat (`SPEC-0313` behaviour 2).
pub const AI_ATTACH_SELECTION: &str = "IDECraft.AiAttachTerminalSelection";
/// Attach the terminal's last command with output and exit code.
pub const AI_ATTACH_COMMAND: &str = "IDECraft.AiAttachTerminalCommand";
/// Attach the terminal's last lines.
pub const AI_ATTACH_LINES: &str = "IDECraft.AiAttachTerminalLines";
/// Share the terminal tab with the chat, or stop (`SPEC-0313` behaviour 5).
pub const AI_SHARE_TERMINAL: &str = "IDECraft.AiShareTerminal";
/// Explain the Run console's output with AI.
pub const AI_EXPLAIN_RUN: &str = "IDECraft.AiExplainRun";
/// Attach the Run console's output to the chat (`SPEC-0313` behaviour 3).
pub const AI_ATTACH_RUN: &str = "IDECraft.AiAttachRun";
/// Explain the selected failed test with AI.
pub const AI_EXPLAIN_TEST: &str = "IDECraft.AiExplainTest";
/// Attach the selected failed test to the chat.
pub const AI_ATTACH_TEST: &str = "IDECraft.AiAttachTest";
/// Attach the problems of the Problems tool window to the chat.
pub const AI_ATTACH_PROBLEMS: &str = "IDECraft.AiAttachProblems";
/// Attach the working tree's diff to the chat.
pub const AI_ATTACH_DIFF: &str = "IDECraft.AiAttachGitDiff";

/// What the terminal's context menu acts on (`SPEC-0313` behaviours 2, 4, 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalTarget {
    /// Text is selected.
    pub selection: bool,
    /// The shell marks prompts and a command ran.
    pub command: bool,
    /// The tab is shared with the chat.
    pub shared: bool,
    /// The AI Assistant is on.
    pub ai: bool,
    /// Lines "Last n Lines" takes.
    pub lines: usize,
}

/// The menu of a terminal tab: Explain with AI, Attach to Chat, sharing (`SPEC-0313`). Without
/// the assistant the entries are disabled and nothing is read (behaviour 10).
pub fn terminal_menu(t: TerminalTarget) -> Vec<MenuEntry> {
    let lines = format!("Last {} Lines", t.lines);
    let mut attach = vec![item("Selection", AI_ATTACH_SELECTION, t.ai && t.selection)];
    if t.command {
        attach.push(item("Last Command", AI_ATTACH_COMMAND, t.ai));
    }
    attach.push(item(lines, AI_ATTACH_LINES, t.ai));
    vec![
        item("Explain with AI", AI_EXPLAIN_TERMINAL, t.ai),
        sub("Attach to Chat", attach),
        MenuEntry::Separator,
        item(
            if t.shared {
                "Stop Sharing with Chat"
            } else {
                "Share Tab with This Chat"
            },
            AI_SHARE_TERMINAL,
            t.ai || t.shared,
        ),
    ]
}

/// What the Run tool window's context menu acts on (`SPEC-0313` behaviours 3, 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunTarget {
    /// The selected tab has output.
    pub output: bool,
    /// A failed test is selected.
    pub failed_test: bool,
    /// The AI Assistant is on.
    pub ai: bool,
}

/// The menu of the Run console and test tree.
pub fn run_menu(t: RunTarget) -> Vec<MenuEntry> {
    let mut out = vec![
        item("Explain with AI", AI_EXPLAIN_RUN, t.ai && t.output),
        item("Attach Output to Chat", AI_ATTACH_RUN, t.ai && t.output),
    ];
    if t.failed_test {
        out.push(MenuEntry::Separator);
        out.push(item("Explain Failure with AI", AI_EXPLAIN_TEST, t.ai));
        out.push(item("Attach Failed Test to Chat", AI_ATTACH_TEST, t.ai));
    }
    out
}

/// The menu of a task in the build tools window (`SPEC-0100` v3 behaviour 10): run or debug it,
/// keep it as a run configuration of the project, run it before or after Build Project or before
/// every run (`triggers`: which are on, in that order), give it a shortcut, copy its command line.
pub fn build_task_menu(task: bool, triggers: [bool; 5]) -> Vec<MenuEntry> {
    use crate::workbench_view as b;
    vec![
        item("Run", crate::workbench_view::BUILD_RUN_TASK, task),
        item("Debug", b::BUILD_DEBUG_TASK, task),
        item(
            "Create Run Configuration",
            crate::workbench_view::BUILD_SAVE_TASK,
            task,
        ),
        MenuEntry::Separator,
        // v4: also before and after a sync of the build files.
        switch(
            "Execute Before Sync",
            b::BUILD_BEFORE_SYNC_TASK,
            task,
            triggers[0],
        ),
        switch(
            "Execute After Sync",
            b::BUILD_AFTER_SYNC_TASK,
            task,
            triggers[1],
        ),
        switch(
            "Execute Before Build",
            b::BUILD_BEFORE_BUILD_TASK,
            task,
            triggers[2],
        ),
        switch(
            "Execute After Build",
            b::BUILD_AFTER_BUILD_TASK,
            task,
            triggers[3],
        ),
        switch(
            "Execute Before Run",
            b::BUILD_BEFORE_RUN_TASK,
            task,
            triggers[4],
        ),
        MenuEntry::Separator,
        item("Assign Shortcut…", b::BUILD_SHORTCUT_TASK, task),
        item(
            "Copy Command Line",
            crate::workbench_view::BUILD_COPY_TASK,
            task,
        ),
    ]
}

/// Copy the value of the selected row of the Variables list (`SPEC-0091` v11 behaviour 23).
pub const DEBUG_COPY_VALUE: &str = "IDECraft.Debug.CopyValue";
/// Copy the name of the selected row.
pub const DEBUG_COPY_NAME: &str = "IDECraft.Debug.CopyName";
/// Put the selected row's expression into the expression field.
pub const DEBUG_EVALUATE_ROW: &str = "IDECraft.Debug.EvaluateRow";
/// Add the selected row's expression to the watches.
pub const DEBUG_WATCH_ROW: &str = "IDECraft.Debug.WatchRow";
/// Take the label off the selected row's object (v11 behaviour 24).
pub const DEBUG_UNMARK: &str = "IDECraft.Debug.Unmark";
/// Copy the call stack (frame menu).
pub const DEBUG_COPY_STACK: &str = "IDECraft.Debug.CopyStack";
/// Prefix of the Smart Step Into items: the adapter's target id follows (v11 behaviour 18).
pub const DEBUG_STEP_TARGET: &str = "IDECraft.Debug.StepTarget.";
/// Open an Inspect tab on the selected row (`SPEC-0091` v12 behaviour 30).
pub const DEBUG_INSPECT: &str = "IDECraft.Debug.Inspect";
/// Open a tab with the objects referring to the selected row's value (v12 behaviour 31).
pub const DEBUG_REFERRING: &str = "IDECraft.Debug.Referring";
/// Prefix of the "View as" items: the view's id follows (v12 behaviour 32).
pub const DEBUG_VIEW: &str = "IDECraft.Debug.View.";
/// Give the selected row's type a renderer (v12 behaviour 33).
pub const DEBUG_RENDERER: &str = "IDECraft.Debug.Renderer";
/// Mute or unmute the renderers.
pub const DEBUG_MUTE_RENDERERS: &str = "IDECraft.Debug.MuteRenderers";
/// Prefix of the filter switches: `statics`, `synthetic` or `nulls` follows (v12 behaviour 34).
pub const DEBUG_FILTER: &str = "IDECraft.Debug.Filter.";
/// Add a field watchpoint on the selected field (`SPEC-0146` v3).
pub const DEBUG_FIELD_WATCH: &str = "IDECraft.Debug.FieldWatch";
/// Resume every thread (v12 behaviour 27).
pub const DEBUG_RESUME_ALL: &str = "IDECraft.Debug.ResumeAll";
/// Pause every thread (v12 behaviour 27).
pub const DEBUG_PAUSE_ALL: &str = "IDECraft.Debug.PauseAll";

/// What the Debug window's menus depend on (`SPEC-0091` v11 behaviours 19, 21, 23, 24; v12).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DebugTarget {
    /// The program is paused.
    pub paused: bool,
    /// The selected row of the Variables list is an object with an id.
    pub object: bool,
    /// That object is marked.
    pub marked: bool,
    /// The program runs (v12 behaviour 27).
    pub running: bool,
    /// Changed classes wait to be reloaded (v12 behaviour 37).
    pub code_changed: bool,
    /// The selected row shows an integer (v12 behaviour 32).
    pub number: bool,
    /// The selected row is a field of an object (`SPEC-0146` v3).
    pub field: bool,
    /// A row is selected.
    pub row: bool,
    /// The selected row's view.
    pub view: idecraft_debugger::ViewAs,
    /// The session's language: evaluated views exist for Java and Python.
    pub lang: Option<idecraft_debugger::Lang>,
    /// The filters of the Variables list (v12 behaviour 34).
    pub filters: idecraft_debugger::extras::VarFilters,
    /// Whether the renderers are muted (v12 behaviour 33).
    pub renderers_muted: bool,
}

/// The toolbar's ⋮ menu (v11 behaviour 21): the further stepping actions, as in the reference's
/// overflow menu, with the keymap's shortcuts.
pub fn debug_more_menu(t: DebugTarget) -> Vec<MenuEntry> {
    let p = t.paused;
    vec![
        item("Force Step Over", "ForceStepOver", p),
        item("Force Step Into", "ForceStepInto", p),
        item("Smart Step Into", "SmartStepInto", p),
        MenuEntry::Separator,
        item("Run to Cursor", "RunToCursor", p),
        item("Force Run to Cursor", "ForceRunToCursor", p),
        MenuEntry::Separator,
        item("Show Execution Point", "ShowExecutionPoint", p),
        item("Evaluate Expression…", "EvaluateExpression", p),
        item("Reset Frame", "Debugger.PopFrame", p),
        // SPEC-0091 v12 behaviours 25–29, 37.
        MenuEntry::Separator,
        item("Step Out of Code Block", "StepOutOfBlock", p),
        item("Resume All Threads", DEBUG_RESUME_ALL, p),
        item("Pause All Threads", DEBUG_PAUSE_ALL, p || t.running),
        item("Get Thread Dump", "DumpThreads", p),
        MenuEntry::Separator,
        item("Force Return", "Debugger.ForceEarlyReturn", p),
        item("Throw Exception", "Debugger.ThrowException", p),
        item("Reload Changed Classes", "Hotswap", t.code_changed),
    ]
}

/// The menu of a frame of the call stack (v11 behaviour 19).
pub fn debug_frame_menu(t: DebugTarget) -> Vec<MenuEntry> {
    vec![
        item("Reset Frame", "Debugger.PopFrame", t.paused),
        MenuEntry::Separator,
        item("Copy Stack", DEBUG_COPY_STACK, t.paused),
    ]
}

/// The menu of a row of the Variables list (v11 behaviours 23 and 24).
pub fn debug_variable_menu(t: DebugTarget) -> Vec<MenuEntry> {
    let p = t.paused;
    let mut out = vec![
        item("Set Value…", "XDebugger.SetValue", p),
        item("Copy Value", DEBUG_COPY_VALUE, p),
        item("Copy Name", DEBUG_COPY_NAME, p),
        MenuEntry::Separator,
        item("Evaluate Expression…", DEBUG_EVALUATE_ROW, p),
        item("Add to Watches", DEBUG_WATCH_ROW, p),
        MenuEntry::Separator,
    ];
    if t.marked {
        out.push(item("Unmark Object", DEBUG_UNMARK, p));
    } else {
        out.push(item("Mark Object…", "Debugger.MarkObject", p && t.object));
    }
    // SPEC-0091 v12 behaviours 30–34, SPEC-0146 v3.
    let evaluated = p && t.row && t.lang.is_some_and(|l| l != idecraft_debugger::Lang::Other);
    out.push(item("Inspect…", DEBUG_INSPECT, p && t.row));
    out.push(item(
        "Show Referring Objects…",
        DEBUG_REFERRING,
        p && t.object,
    ));
    out.push(MenuEntry::Separator);
    // Only the views that fit the value, as in the reference (v13): numeric ones for a number,
    // the evaluated ones for an object.
    let views = idecraft_debugger::ViewAs::ALL
        .into_iter()
        .filter(|v| {
            *v == idecraft_debugger::ViewAs::Auto || if v.numeric() { t.number } else { !t.number }
        })
        .map(|v| {
            let on = if v.numeric() {
                p && t.number
            } else if v == idecraft_debugger::ViewAs::Auto {
                p && t.row
            } else {
                evaluated
            };
            switch(
                v.label(),
                &format!("{DEBUG_VIEW}{}", v.id()),
                on,
                t.view == v,
            )
        })
        .collect();
    out.push(sub("View as", views));
    out.push(item("Renderer for This Type…", DEBUG_RENDERER, evaluated));
    out.push(switch(
        "Mute Renderers",
        DEBUG_MUTE_RENDERERS,
        true,
        t.renderers_muted,
    ));
    out.push(sub(
        "Filter",
        vec![
            switch(
                "Show Static Fields",
                &format!("{DEBUG_FILTER}statics"),
                true,
                t.filters.statics,
            ),
            switch(
                "Show Synthetic Fields",
                &format!("{DEBUG_FILTER}synthetic"),
                true,
                t.filters.synthetic,
            ),
            switch(
                "Show Null Values",
                &format!("{DEBUG_FILTER}nulls"),
                true,
                t.filters.nulls,
            ),
        ],
    ));
    out.push(MenuEntry::Separator);
    out.push(item(
        "Add Field Watchpoint",
        DEBUG_FIELD_WATCH,
        p && t.field,
    ));
    out
}

/// The calls Smart Step Into offers, one item each (v11 behaviour 18).
pub fn step_target_menu(targets: &[(i64, String)]) -> Vec<MenuEntry> {
    targets
        .iter()
        .map(|(id, label)| item(label.clone(), format!("{DEBUG_STEP_TARGET}{id}"), true))
        .collect()
}

/// The shortcut text a menu shows beside `action`: its first shortcut in `keymap`, the keymap in
/// use, written for macOS when `macos` (`SPEC-0275` v5 behaviour 14a); empty when it has none.
pub fn shortcut_label(action: &str, keymap: &idecraft_ui::keymap::Keymap, macos: bool) -> String {
    keymap.shortcut_label(action, macos)
}

/// Every command of `entries`, submenus included, in order.
pub fn flatten(entries: &[MenuEntry]) -> Vec<&MenuItem> {
    let mut out = Vec::new();
    for e in entries {
        match e {
            MenuEntry::Item(i) => out.push(i),
            MenuEntry::Sub(_, sub) => out.extend(flatten(sub)),
            MenuEntry::Separator => {}
        }
    }
    out
}

/// The command labelled `label` (labels of submenus joined with ` › `), if any.
pub fn find<'a>(entries: &'a [MenuEntry], path: &str) -> Option<&'a MenuItem> {
    let (head, rest) = match path.split_once(" › ") {
        Some((h, r)) => (h, Some(r)),
        None => (path, None),
    };
    entries.iter().find_map(|e| match (e, rest) {
        (MenuEntry::Item(i), None) if i.label == head => Some(i),
        (MenuEntry::Sub(l, sub), Some(rest)) if l == head => find(sub, rest),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Debug window's menus (`SPEC-0091` v11 behaviours 18, 19, 21, 23, 24).
    #[test]
    fn debug_menus() {
        let paused = DebugTarget {
            paused: true,
            object: true,
            marked: false,
            row: true,
            ..DebugTarget::default()
        };
        let more = debug_more_menu(paused);
        let labels: Vec<&str> = flatten(&more).iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Force Step Over",
                "Force Step Into",
                "Smart Step Into",
                "Run to Cursor",
                "Force Run to Cursor",
                "Show Execution Point",
                "Evaluate Expression…",
                "Reset Frame",
                "Step Out of Code Block",
                "Resume All Threads",
                "Pause All Threads",
                "Get Thread Dump",
                "Force Return",
                "Throw Exception",
                "Reload Changed Classes"
            ]
        );
        assert!(
            flatten(&debug_more_menu(DebugTarget::default()))
                .iter()
                .all(|i| !i.enabled)
        );
        // SPEC-0091 v12: Pause All while running, Reload once classes changed.
        let running = debug_more_menu(DebugTarget {
            running: true,
            code_changed: true,
            ..DebugTarget::default()
        });
        assert!(find(&running, "Pause All Threads").unwrap().enabled);
        assert!(find(&running, "Reload Changed Classes").unwrap().enabled);
        assert!(!find(&running, "Step Out of Code Block").unwrap().enabled);
        // The variables' further items: views by the value, renderers, filters, watchpoints.
        let number = debug_variable_menu(DebugTarget {
            row: true,
            number: true,
            field: true,
            view: idecraft_debugger::ViewAs::Hex,
            lang: Some(idecraft_debugger::Lang::Java),
            ..paused
        });
        let hex = flatten(&number)
            .into_iter()
            .find(|i| i.label == "Hex")
            .unwrap();
        assert!(hex.enabled && hex.checked);
        assert_eq!(hex.action, "IDECraft.Debug.View.hex");
        assert!(find(&number, "Add Field Watchpoint").unwrap().enabled);
        assert!(find(&number, "Renderer for This Type…").unwrap().enabled);
        let native = debug_variable_menu(DebugTarget {
            row: true,
            lang: Some(idecraft_debugger::Lang::Other),
            ..paused
        });
        let text = flatten(&native)
            .into_iter()
            .find(|i| i.label == "String (toString)")
            .unwrap();
        assert!(!text.enabled, "no evaluated views without Java or Python");
        let statics = flatten(&native)
            .into_iter()
            .find(|i| i.label == "Show Static Fields")
            .unwrap();
        assert_eq!(statics.action, "IDECraft.Debug.Filter.statics");
        let vars = debug_variable_menu(paused);
        assert!(find(&vars, "Mark Object…").is_some_and(|i| i.enabled));
        let marked = debug_variable_menu(DebugTarget {
            marked: true,
            ..paused
        });
        assert_eq!(find(&marked, "Unmark Object").unwrap().action, DEBUG_UNMARK);
        let primitive = debug_variable_menu(DebugTarget {
            object: false,
            ..paused
        });
        assert!(!find(&primitive, "Mark Object…").unwrap().enabled);
        assert_eq!(
            find(&debug_frame_menu(paused), "Reset Frame")
                .unwrap()
                .action,
            "Debugger.PopFrame"
        );
        let steps = step_target_menu(&[(7, "twice(int)".into()), (8, "square(int)".into())]);
        assert_eq!(
            find(&steps, "square(int)").unwrap().action,
            "IDECraft.Debug.StepTarget.8"
        );
    }

    /// SPEC-0313: the terminal's and the Run console's entries, disabled without the assistant.
    #[test]
    fn terminal_and_run_menus() {
        let on = TerminalTarget {
            selection: false,
            command: true,
            shared: false,
            ai: true,
            lines: 200,
        };
        let labels = |e: &[MenuEntry]| {
            flatten(e)
                .into_iter()
                .map(|i| (i.label.clone(), i.enabled))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(&terminal_menu(on)),
            [
                ("Explain with AI".to_owned(), true),
                ("Selection".to_owned(), false),
                ("Last Command".to_owned(), true),
                ("Last 200 Lines".to_owned(), true),
                ("Share Tab with This Chat".to_owned(), true),
            ]
        );
        let no_marks = terminal_menu(TerminalTarget {
            command: false,
            shared: true,
            ..on
        });
        assert!(find(&no_marks, "Attach to Chat › Last Command").is_none());
        assert!(find(&no_marks, "Stop Sharing with Chat").is_some_and(|i| i.enabled));
        let off = terminal_menu(TerminalTarget { ai: false, ..on });
        assert!(flatten(&off).iter().all(|i| !i.enabled));
        let run = run_menu(RunTarget {
            output: true,
            failed_test: true,
            ai: true,
        });
        assert_eq!(flatten(&run).len(), 4);
        assert!(find(&run, "Explain Failure with AI").is_some());
        let plain = run_menu(RunTarget {
            output: false,
            failed_test: false,
            ai: true,
        });
        assert!(flatten(&plain).iter().all(|i| !i.enabled));
    }

    #[test]
    fn control_click_is_secondary_on_macos_only() {
        assert!(is_secondary_click(Button::Secondary, false, true));
        assert!(is_secondary_click(Button::Secondary, false, false));
        assert!(is_secondary_click(Button::Primary, true, true));
        assert!(!is_secondary_click(Button::Primary, true, false));
        assert!(!is_secondary_click(Button::Primary, false, true));
        assert!(!is_secondary_click(Button::Other, true, true));
    }

    #[test]
    fn project_tree_menu_creates_files_of_every_kind() {
        let file = project_tree_menu(TreeTarget {
            dir: false,
            root: false,
            repository: true,
            paste: false,
            several: false,
        });
        for label in [
            "New › File",
            "New › Directory",
            "New › SQL File",
            "New › HTTP Request",
            "New › Java Class",
            "New › Kotlin File",
            "New › Python File",
            "New › TypeScript File",
            "New › JavaScript File",
        ] {
            let i = find(&file, label).unwrap_or_else(|| panic!("{label}"));
            assert!(i.enabled);
            assert!(new_kind(&i.action).is_some(), "{label}");
        }
        assert_eq!(
            new_kind(&find(&file, "New › SQL File").unwrap().action),
            Some(NewKind::SqlFile)
        );
        for label in [
            "Rename…",
            "Delete…",
            "Copy Path/Reference › Absolute Path",
            "Open in Terminal",
            "Local History › Show History",
            "Git › Commit…",
        ] {
            assert!(find(&file, label).is_some_and(|i| i.enabled), "{label}");
        }
        assert_eq!(find(&file, "Rename…").unwrap().action, "RenameElement");
        assert_eq!(find(&file, "Delete…").unwrap().action, "$Delete");
    }

    #[test]
    fn project_tree_menu_states_follow_the_row() {
        let root = project_tree_menu(TreeTarget {
            dir: true,
            root: true,
            repository: false,
            paste: false,
            several: false,
        });
        assert!(!find(&root, "Rename…").unwrap().enabled, "root");
        assert!(!find(&root, "Delete…").unwrap().enabled, "root");
        assert!(!find(&root, "Local History › Show History").unwrap().enabled);
        assert!(
            !find(&root, "Git › Commit…").unwrap().enabled,
            "no repository"
        );
        assert!(find(&root, "New › File").unwrap().enabled);
    }

    #[test]
    fn rename_is_for_one_item() {
        let one = TreeTarget {
            dir: false,
            root: false,
            repository: false,
            paste: true,
            several: false,
        };
        assert!(find(&project_tree_menu(one), "Rename…").unwrap().enabled);
        let several = project_tree_menu(TreeTarget {
            several: true,
            ..one
        });
        assert!(
            !find(&several, "Rename…").unwrap().enabled,
            "SPEC-0275 v3 AK-14"
        );
        for label in [
            "Cut",
            "Copy",
            "Paste",
            "Delete…",
            "Copy Path/Reference › Absolute Path",
        ] {
            assert!(find(&several, label).unwrap().enabled, "{label}");
        }
    }

    /// SPEC-0332 v2 AK-13: the copy entries of the tree's and of a tab's menu.
    #[test]
    fn copy_entries_of_tree_and_tab() {
        let copy = |m: &[MenuEntry]| -> Vec<(String, String, bool)> {
            m.iter()
                .find_map(|e| match e {
                    MenuEntry::Sub(label, sub) if label == "Copy Path/Reference" => Some(
                        flatten(sub)
                            .iter()
                            .map(|i| (i.label.clone(), i.action.clone(), i.enabled))
                            .collect(),
                    ),
                    _ => None,
                })
                .expect("the copy entries")
        };
        let row = |label: &str, action: &str, enabled: bool| {
            (label.to_owned(), action.to_owned(), enabled)
        };
        let target = TreeTarget {
            dir: false,
            root: false,
            repository: false,
            paste: false,
            several: true,
        };
        assert_eq!(
            copy(&project_tree_menu(target)),
            [
                row("Absolute Path", "CopyPaths", true),
                row("Path From Project Root", "IDECraft.CopyRelativePath", true),
                row("Path From Source Root", "CopySourceRootPath", true),
                row("File Name", "IDECraft.CopyFileName", true),
            ]
        );
        // The project folder is below no folder of the project.
        let project = copy(&project_tree_menu(TreeTarget {
            dir: true,
            root: true,
            several: false,
            ..target
        }));
        assert!(!project[1].2);
        assert_eq!(
            project[2],
            row("Path From Source Root", "CopySourceRootPath", false)
        );
        let tab = copy(&tab_menu(&TabTarget {
            split: false,
            pinned: false,
            any_pinned: false,
            runnable: None,
            annotated: false,
        }));
        assert_eq!(
            tab,
            [
                row("Absolute Path", "CopyPaths", true),
                row("Path From Project Root", "IDECraft.CopyRelativePath", true),
                row("Path From Source Root", "CopySourceRootPath", true),
                row("File Name", "IDECraft.CopyFileName", true),
                row("Path With Line Number", "CopyPathWithLineNumber", true),
                row("Copy Reference", "CopyReference", true),
            ]
        );
    }

    #[test]
    fn project_tree_menu_has_cut_copy_paste() {
        let target = TreeTarget {
            dir: true,
            root: false,
            repository: false,
            paste: false,
            several: false,
        };
        let m = project_tree_menu(target);
        for (label, action) in [("Cut", "$Cut"), ("Copy", "$Copy"), ("Paste", "$Paste")] {
            assert_eq!(find(&m, label).map(|i| i.action.as_str()), Some(action));
        }
        assert!(find(&m, "Cut").unwrap().enabled);
        assert!(find(&m, "Copy").unwrap().enabled);
        assert!(!find(&m, "Paste").unwrap().enabled, "nothing to paste");
        assert!(
            find(&m, "Copy Path/Reference › Absolute Path").is_some_and(|i| i.enabled),
            "Copy Path/Reference stays"
        );
        let labels: Vec<&str> = flatten(&m).iter().map(|i| i.label.as_str()).collect();
        let at = |l: &str| labels.iter().position(|x| *x == l).unwrap();
        assert!(at("Cut") < at("Copy") && at("Copy") < at("Paste"));
        let with_clip = project_tree_menu(TreeTarget {
            paste: true,
            ..target
        });
        assert!(find(&with_clip, "Paste").unwrap().enabled);
        let root = project_tree_menu(TreeTarget {
            root: true,
            paste: true,
            ..target
        });
        assert!(!find(&root, "Cut").unwrap().enabled, "root");
        assert!(!find(&root, "Copy").unwrap().enabled, "root");
        assert!(find(&root, "Paste").unwrap().enabled, "paste into the root");
    }

    #[test]
    fn editor_menu_has_clipboard_navigation_refactorings_and_run() {
        let plain = EditorTarget {
            selection: false,
            runnable: None,
            repository: true,
            annotated: false,
            debugging: false,
        };
        let m = editor_menu(&plain);
        assert!(find(&m, "Cut").is_none() && find(&m, "Copy").is_none());
        assert!(find(&m, "Paste").unwrap().enabled);
        for (label, action) in [
            ("Show Context Actions", "ShowIntentionActions"),
            ("Go To › Declaration or Usages", "GotoDeclaration"),
            ("Go To › Implementation(s)", "GotoImplementation"),
            ("Find Usages", "FindUsages"),
            ("Refactor › Rename…", "RenameElement"),
            ("Refactor › Extract Variable…", "IntroduceVariable"),
            ("Refactor › Extract Method…", "ExtractMethod"),
            ("Refactor › Extract Constant…", "IntroduceConstant"),
            ("Refactor › Inline…", "Inline"),
            ("Refactor › Change Signature…", "ChangeSignature"),
            ("Reformat Code", "ReformatCode"),
            ("Local History › Show History", "LocalHistory.ShowHistory"),
        ] {
            assert_eq!(find(&m, label).map(|i| i.action.as_str()), Some(action));
        }
        assert!(flatten(&m).iter().all(|i| i.action != "RunClass"));
        assert!(find(&m, "Add to Watches").is_none());
        // D2: while paused the menu offers Evaluate Expression, Add to Watches, Run to Cursor.
        let paused = editor_menu(&EditorTarget {
            debugging: true,
            ..plain.clone()
        });
        for (label, action) in [
            ("Evaluate Expression…", "EvaluateExpression"),
            ("Add to Watches", "Debugger.AddToWatch"),
            ("Run to Cursor", "RunToCursor"),
        ] {
            assert_eq!(
                find(&paused, label).map(|i| i.action.as_str()),
                Some(action)
            );
        }
        let runnable = editor_menu(&EditorTarget {
            selection: true,
            runnable: Some(RunNames::both("main.py")),
            repository: false,
            annotated: false,
            debugging: false,
        });
        assert!(find(&runnable, "Cut").unwrap().enabled);
        assert_eq!(find(&runnable, "Run 'main.py'").unwrap().action, "RunClass");
        assert_eq!(
            find(&runnable, "Debug 'main.py'").unwrap().action,
            "DebugClass"
        );
    }

    // SPEC-0275 v6 AK-21.
    #[test]
    fn editor_menu_without_selection_hides_cut_and_copy() {
        let target = EditorTarget {
            selection: false,
            runnable: Some(RunNames::both("main.py")),
            repository: true,
            annotated: false,
            debugging: false,
        };
        let plain = editor_menu(&target);
        assert!(find(&plain, "Cut").is_none() && find(&plain, "Copy").is_none());
        let selected = editor_menu(&EditorTarget {
            selection: true,
            ..target.clone()
        });
        assert_eq!(find(&selected, "Cut").unwrap().action, "$Cut");
        assert_eq!(find(&selected, "Copy").unwrap().action, "$Copy");
        let reveal = if cfg!(target_os = "macos") {
            "Open In › Finder"
        } else {
            "Open In › File Manager"
        };
        for (label, action) in [
            ("Column Selection Mode", "EditorToggleColumnMode"),
            ("Folding › Expand", "ExpandRegion"),
            ("Folding › Collapse", "CollapseRegion"),
            ("Folding › Expand All", "ExpandAllRegions"),
            ("Folding › Collapse All", "CollapseAllRegions"),
            ("Analyze › Inspect Code…", "InspectCode"),
            ("Analyze › Show Problems", "ActivateProblemsViewToolWindow"),
            ("Generate…", "Generate"),
            (
                "More Run/Debug › Run Configurations…",
                "ChooseRunConfiguration",
            ),
            (reveal, REVEAL),
            ("Open In › Terminal", OPEN_IN_TERMINAL),
            ("Compare with Clipboard", "CompareClipboardWithSelection"),
        ] {
            assert_eq!(
                find(&plain, label).map(|i| (i.action.as_str(), i.enabled)),
                Some((action, true)),
                "{label}"
            );
        }
        // Without a runnable file there is no run submenu.
        let text = editor_menu(&EditorTarget {
            runnable: None,
            ..target
        });
        assert!(find(&text, "More Run/Debug › Run Configurations…").is_none());
    }

    // SPEC-0275 v6 AK-21.
    #[test]
    fn clipboard_comparison() {
        assert!(super::clipboard_comparison(None, "a\n").is_none());
        assert!(super::clipboard_comparison(Some(""), "a\n").is_none());
        let rows = super::clipboard_comparison(Some("a\nb\n"), "a\nc\n").unwrap();
        assert_eq!(rows, idecraft_vcs::side_by_side("a\nb\n", "a\nc\n"));
        assert!(rows.len() >= 2);
    }

    // SPEC-0275 v5 AK-20.
    #[test]
    fn run_items_name_class_and_method() {
        let main = "package lab;\n\npublic class Inventory {\n    public static void main(String[] args) {\n    }\n}\n";
        assert_eq!(
            run_names("Inventory.java", main, Some(3)),
            RunNames::both("Inventory.main()")
        );
        let test = "import org.junit.jupiter.api.Test;\n\nclass CalcTest {\n    @Test\n    void adds() {\n        int a = 1;\n    }\n}\n";
        assert_eq!(
            run_names("CalcTest.java", test, Some(2)),
            RunNames::both("CalcTest")
        );
        assert_eq!(
            run_names("CalcTest.java", test, Some(5)),
            RunNames {
                run: "adds()".into(),
                debug: "CalcTest".into()
            }
        );
        assert_eq!(
            run_names("main.kt", "fun main() {\n}\n", Some(0)),
            RunNames::both("main.kt")
        );
        assert_eq!(
            run_names("main.py", "print(1)\n", None),
            RunNames::both("main.py")
        );
        let menu = editor_menu(&EditorTarget {
            selection: false,
            runnable: Some(run_names("Inventory.java", main, None)),
            repository: false,
            annotated: false,
            debugging: false,
        });
        assert!(find(&menu, "Run 'Inventory.main()'").is_some());
        assert!(find(&menu, "Debug 'Inventory.main()'").is_some());
    }

    // SPEC-0275 v5 AK-20, SPEC-0028 v3.
    #[test]
    fn shortcut_labels_come_from_the_keymap() {
        let mac = idecraft_ui::keymap::Keymap::bundled_macos();
        assert_eq!(shortcut_label("RunClass", &mac, true), "⌃⇧R");
        assert_eq!(shortcut_label("DebugClass", &mac, true), "⌃⇧D");
        let default = idecraft_ui::keymap::Keymap::bundled_default();
        assert_eq!(
            shortcut_label("RunClass", &default, false),
            "Ctrl+Shift+F10"
        );
        assert_eq!(shortcut_label("IDECraft.Nothing", &mac, true), "");
    }

    #[test]
    fn database_menu_states_follow_the_node() {
        let none = database_menu(DbTarget::Nothing);
        assert!(find(&none, "New › Data Source…").unwrap().enabled);
        assert!(!find(&none, "New › Query Console").unwrap().enabled);
        assert!(!find(&none, "Refresh").unwrap().enabled);
        let conn = database_menu(DbTarget::Connection);
        for label in [
            "New › Query Console",
            "Jump to Query Console…",
            "Refresh",
            "Properties",
            "Copy",
            "Delete…",
        ] {
            assert!(find(&conn, label).is_some_and(|i| i.enabled), "{label}");
        }
        assert!(!find(&conn, "Rename…").unwrap().enabled);
        let table = database_menu(DbTarget::Table);
        assert!(find(&table, "Edit Data").unwrap().enabled);
        // SPEC-0089 v5 AK-10: Show DDL on tables and views only.
        assert!(find(&table, "Show DDL").unwrap().enabled);
        assert!(!find(&conn, "Show DDL").unwrap().enabled);
        assert!(
            !find(&database_menu(DbTarget::Column), "Show DDL")
                .unwrap()
                .enabled
        );
        assert!(find(&table, "Rename…").unwrap().enabled);
        assert!(!find(&table, "Delete…").unwrap().enabled);
        // SPEC-0164 v3 AK-7: Rename on index, sequence and key rows; Change Column Type on columns.
        let object = database_menu(DbTarget::Object);
        assert!(find(&object, "Rename…").unwrap().enabled);
        assert!(!find(&object, "Change Column Type…").unwrap().enabled);
        let column = database_menu(DbTarget::Column);
        assert!(find(&column, "Change Column Type…").unwrap().enabled);
        assert!(!find(&table, "Change Column Type…").unwrap().enabled);
    }

    /// SPEC-0110/AK-9 (v5).
    #[test]
    fn database_entry_points_for_plan_and_comparison() {
        let base = || {
            editor_menu(&EditorTarget {
                selection: false,
                runnable: None,
                repository: false,
                annotated: false,
                debugging: false,
            })
        };
        let sql = with_sql_entries(base(), true);
        for label in ["Explain Plan", "Explain Analyze"] {
            assert!(find(&sql, label).is_some_and(|i| i.enabled), "{label}");
        }
        let plain = with_sql_entries(base(), false);
        assert!(find(&plain, "Explain Plan").is_none());
        assert_eq!(plain.len() + 2, sql.len());
        for t in [
            DbTarget::Connection,
            DbTarget::Folder,
            DbTarget::Table,
            DbTarget::Column,
        ] {
            let menu = database_menu(t);
            assert!(
                find(&menu, "Compare Schemas…").is_some_and(|i| i.enabled),
                "{t:?}"
            );
        }
        assert!(
            !find(&database_menu(DbTarget::Nothing), "Compare Schemas…")
                .unwrap()
                .enabled
        );
    }

    /// SPEC-0096 v6 AK-10: the editor's menu of a request file has Run All Requests in File after
    /// Reformat Code; other files' menus do not.
    #[test]
    fn request_files_have_run_all_in_the_editor_menu() {
        let base = || {
            editor_menu(&EditorTarget {
                selection: false,
                runnable: None,
                repository: false,
                annotated: false,
                debugging: false,
            })
        };
        let http = with_http_entries(base(), true);
        let run_all = find(&http, "Run All Requests in File").expect("the entry");
        assert!(run_all.enabled);
        assert_eq!(run_all.action, HTTP_RUN_ALL);
        let at = |menu: &[MenuEntry], action: &str| {
            menu.iter()
                .position(|e| matches!(e, MenuEntry::Item(i) if i.action == action))
                .unwrap()
        };
        let reformat = at(&http, "ReformatCode");
        assert!(matches!(http[reformat + 1], MenuEntry::Separator));
        assert_eq!(at(&http, HTTP_RUN_ALL), reformat + 2);
        let plain = with_http_entries(base(), false);
        assert!(find(&plain, "Run All Requests in File").is_none());
        assert_eq!(plain, base());
    }

    // SPEC-0275 v8 AK-23: the Git submenu of a file lists its file items in order.
    #[test]
    fn git_submenu_has_the_file_items() {
        let target = |repository| EditorTarget {
            selection: false,
            runnable: None,
            repository,
            annotated: false,
            debugging: false,
        };
        let sub = |menu: &[MenuEntry]| -> Vec<(String, String, bool)> {
            menu.iter()
                .find_map(|e| match e {
                    MenuEntry::Sub(l, items) if l == "Git" => Some(items.clone()),
                    _ => None,
                })
                .unwrap()
                .iter()
                .filter_map(|e| match e {
                    MenuEntry::Item(i) => Some((i.label.clone(), i.action.clone(), i.enabled)),
                    _ => None,
                })
                .collect()
        };
        let items = sub(&editor_menu(&target(true)));
        let labels: Vec<&str> = items.iter().map(|i| i.0.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Annotate with Git Blame",
                "Commit File…",
                "Rollback…",
                "Show History",
                "Show Diff",
                "Compare with Branch…",
                "Commit…",
                "Show Git Log",
                "Branches…"
            ]
        );
        assert_eq!(items[4].1, "Compare.SameVersion");
        assert_eq!(items[3].1, VCS_FILE_HISTORY);
        assert!(items.iter().all(|i| i.2));
        // Outside a repository the file items are greyed, Annotate stays on (IMP-0278).
        let outside = sub(&editor_menu(&target(false)));
        assert!(outside[0].2);
        assert!(outside[1..].iter().all(|i| !i.2));
        // A file of the tree has them too, a folder not.
        let tree = |dir| {
            sub(&project_tree_menu(TreeTarget {
                dir,
                root: false,
                repository: true,
                paste: false,
                several: false,
            }))
        };
        assert!(tree(false).iter().any(|i| i.1 == VCS_COMMIT_FILE));
        assert!(!tree(true).iter().any(|i| i.1 == VCS_COMMIT_FILE));
    }

    // SPEC-0277 AK-3: Annotate in the editor, gutter, project tree and annotation column menus.
    #[test]
    fn annotate_is_offered_where_a_file_is_and_closes_while_shown() {
        let target = |annotated, repository| EditorTarget {
            selection: false,
            runnable: None,
            repository,
            annotated,
            debugging: false,
        };
        let m = editor_menu(&target(false, true));
        let i = find(&m, "Git › Annotate with Git Blame").unwrap();
        assert_eq!((i.action.as_str(), i.enabled), ("Annotate", true));
        let shown = editor_menu(&target(true, true));
        assert!(find(&shown, "Git › Annotate with Git Blame").is_none());
        assert_eq!(
            find(&shown, "Git › Close Annotations").unwrap().action,
            ANNOTATE
        );
        // Outside a repository it stays on and says why it cannot annotate (IMP-0278).
        let outside = editor_menu(&target(false, false));
        assert!(
            find(&outside, "Git › Annotate with Git Blame")
                .unwrap()
                .enabled
        );
        assert!(!find(&outside, "Git › Commit…").unwrap().enabled);

        let gutter = gutter_menu(false);
        assert_eq!(
            find(&gutter, "Annotate with Git Blame").unwrap().action,
            ANNOTATE
        );
        assert!(find(&gutter_menu(true), "Close Annotations").is_some());

        let file = project_tree_menu(TreeTarget {
            dir: false,
            root: false,
            repository: true,
            paste: false,
            several: false,
        });
        assert_eq!(
            find(&file, "Git › Annotate with Git Blame").unwrap().action,
            ANNOTATE
        );
        let dir = project_tree_menu(TreeTarget {
            dir: true,
            root: false,
            repository: true,
            paste: false,
            several: false,
        });
        assert!(flatten(&dir).iter().all(|i| i.action != ANNOTATE));

        let column = annotation_menu(true);
        for (label, action) in [
            ("Close Annotations", CLOSE_ANNOTATIONS),
            ("Show in Git Log", SHOW_IN_GIT_LOG),
            ("Copy Revision Number", COPY_REVISION),
        ] {
            let i = find(&column, label).unwrap();
            assert_eq!((i.action.as_str(), i.enabled), (action, true), "{label}");
        }
        let uncommitted = annotation_menu(false);
        assert!(find(&uncommitted, "Close Annotations").unwrap().enabled);
        assert!(!find(&uncommitted, "Show in Git Log").unwrap().enabled);
        assert!(!find(&uncommitted, "Copy Revision Number").unwrap().enabled);
    }

    /// SPEC-0004 AK-8 (v5), AK-10 (v6): the tab menu ends with the file type items, the tree's
    /// menu of a file has them after New, a folder's has none; Associate only for an unknown
    /// type, Override without an override and Revert with one, never both.
    #[test]
    fn file_type_items_in_the_tab_and_tree_menus() {
        let tab = with_file_type_entries_in_tab_menu(
            tab_menu(&TabTarget {
                pinned: false,
                any_pinned: false,
                split: false,
                runnable: None,
                annotated: false,
            }),
            FileTypeTarget {
                known: true,
                overridden: false,
            },
        );
        let n = tab.len();
        let labels: Vec<_> = tab[n - 3..]
            .iter()
            .map(|e| match e {
                MenuEntry::Item(i) => (i.label.as_str(), i.action.as_str(), i.enabled),
                MenuEntry::Separator => ("-", "-", true),
                MenuEntry::Sub(l, _) => (l.as_str(), "", true),
            })
            .collect();
        assert_eq!(
            labels,
            [
                ("-", "-", true),
                ("Associate with File Type…", ASSOCIATE_FILE_TYPE, false),
                ("Override File Type", OVERRIDE_FILE_TYPE, true),
            ]
        );
        let target = TreeTarget {
            dir: false,
            root: false,
            repository: false,
            paste: false,
            several: false,
        };
        let file = with_file_type_entries_in_tree_menu(
            project_tree_menu(target),
            Some(FileTypeTarget {
                known: false,
                overridden: true,
            }),
        );
        assert!(matches!(&file[0], MenuEntry::Sub(l, _) if l == "New"));
        assert_eq!(
            file[1],
            item("Associate with File Type…", ASSOCIATE_FILE_TYPE, true)
        );
        assert_eq!(
            file[2],
            item("Revert File Type Override", REVERT_FILE_TYPE, true)
        );
        assert!(
            flatten(&file)
                .iter()
                .all(|i| i.action != OVERRIDE_FILE_TYPE),
            "an overridden file is not offered Override"
        );
        let dir = with_file_type_entries_in_tree_menu(
            project_tree_menu(TreeTarget {
                dir: true,
                ..target
            }),
            None,
        );
        assert!(flatten(&dir).iter().all(|i| i.action != OVERRIDE_FILE_TYPE));
    }
}
