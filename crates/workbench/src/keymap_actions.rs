//! The decisions behind the actions that exist under their keymap id because the bundled keymaps
//! bind them (`docs/research/KEYMAP_COVERAGE.md`): which part of the window an action belongs to,
//! the order of the actions of one chord, and the small rules of Compare Two Files, Save As and
//! Jump to Last Tool Window. No window is needed here (`SPEC-0084` v4, `SPEC-0063` v9, `SPEC-0068`
//! v9, `SPEC-0086` v11, `SPEC-0167` v5, `SPEC-0059` v9.3), and of the navigation actions: the Find
//! tool window's grouping toggles (`SPEC-0293` v6), the popup of Quick Definition (`SPEC-0081` v4),
//! the titles and messages of Go to Test and Go to Related (`SPEC-0071` v10), the key of "Show
//! edited only" in Recent Files (`SPEC-0069` v3), and of the fourth package: Expand All and Collapse
//! All of the focused tree, File Path and New in the same place (`SPEC-0275` v13).

use crate::changelist_model::ListOp;
use std::path::{Path, PathBuf};

use ferris_idea_languages::quick_definition::{MAX_TARGETS, Source};
use ferris_idea_ui::tool_windows::ToolWindow;
use ferris_idea_vcs::CommitAction;

/// The part of the window whose focus an action's key needs. The keys of these actions belong to
/// other actions everywhere else (F2 is Next Error in the editor, ⌘D duplicates a line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The diff viewer (`SPEC-0084` v4 behaviour 14).
    Diff,
    /// The Commit tool window (`SPEC-0063` v9 behaviour 21).
    Commit,
    /// The message field of the Commit tool window (`SPEC-0063` v10 behaviour 22). It lies in
    /// the window, so an action of the field goes before one of the window on the same key.
    CommitMessage,
    /// The branch popup (`SPEC-0068` v9 behaviour 33).
    Branches,
    /// The commit list of the Git log (`SPEC-0086` v11 behaviour 20).
    Log,
    /// The Project tree (`SPEC-0084` v4 behaviour 15).
    Tree,
    /// The Find tool window of Find Usages (`SPEC-0293` v6 behaviour 18).
    Find,
    /// The TODO tool window (`SPEC-0334` behaviour 25).
    Todo,
    /// A tree with Expand All or Collapse All: Project, Commit, Find, the test tree, Build Tools,
    /// Database (`SPEC-0275` v13 behaviour 33).
    Trees,
    /// The Run tool window (`SPEC-0087` v10 behaviour 18).
    Run,
}

/// The keymap action of Expand All in the focused tree (`SPEC-0275` v13 behaviour 33).
pub const EXPAND_ALL: &str = "ExpandAll";
/// The keymap action of Collapse All in the focused tree.
pub const COLLAPSE_ALL: &str = "CollapseAll";
/// The keymap action that opens the selected file in the right split (`SPEC-0275` v13 behaviour
/// 36, `SPEC-0067` v8 behaviour 19).
pub const OPEN_IN_RIGHT_SPLIT: &str = "OpenInRightSplit";

/// The action a tree with the focus gets when the menu bar took the key for one of its folding
/// items: ⌘+ and ⌘− are Expand All and Collapse All there (`SPEC-0275` v13 behaviour 33).
pub fn for_focused_tree(id: &str) -> Option<&'static str> {
    match id {
        "ExpandRegion" => Some(EXPAND_ALL),
        "CollapseRegion" => Some(COLLAPSE_ALL),
        _ => None,
    }
}

/// What File Path shows (`SPEC-0275` v13 behaviour 34): the Project tree's selected item while
/// the tree has the focus, else the file of the active editor.
pub fn file_path_target(
    tree_focused: bool,
    active: Option<&Path>,
    selected: Option<&Path>,
) -> Option<PathBuf> {
    let target = if tree_focused {
        selected.or(active)
    } else {
        active
    };
    target.map(Path::to_path_buf)
}

/// What File Path says without a file.
pub const NO_FILE_TO_SHOW: &str = "No file is open to show";

/// The folder New in the same place creates in (`SPEC-0275` v13 behaviour 35): the folder of the
/// active editor's file; `None` without one, where it is New of the tree's selection.
pub fn same_place_folder(active: Option<&Path>) -> Option<PathBuf> {
    active.and_then(Path::parent).map(Path::to_path_buf)
}

/// What Open in Right Split says when the Project tree's selection is no file.
pub const SELECT_A_FILE_TO_SPLIT: &str =
    "Select a file in the Project tree to open it in the right split";

/// The part an action's key is limited to, if any.
pub fn scope_of(id: &str) -> Option<Scope> {
    Some(match id {
        // `SPEC-0084` v5 behaviours 18–22.
        "Diff.ApplyLeftSide"
        | "Diff.ApplyRightSide"
        | "Diff.FocusOppositePane"
        | "Diff.NextChange"
        | "Diff.PrevChange"
        | "Diff.ShowSettingsPopup" => Scope::Diff,
        "Diff.ShowDiff"
        | "Vcs.ToggleAmendCommitMode"
        | "ChangesView.GroupBy.Directory"
        | "ChangesView.GroupBy.Module"
        | "ChangesView.ShowCommitOptions"
        | NEW_CHANGELIST
        | EDIT_CHANGELIST
        | SET_ACTIVE_CHANGELIST
        | DELETE_CHANGELIST
        | MOVE_TO_CHANGELIST => Scope::Commit,
        MESSAGE_HISTORY => Scope::CommitMessage,
        "Git.Rename.Local.Branch" => Scope::Branches,
        "Git.Reword.Commit" | "Git.New.Branch.In.Log" => Scope::Log,
        // `SPEC-0086` v12 behaviour 21.
        SHOW_ALL_AFFECTED => Scope::Log,
        "CompareTwoFiles" | OPEN_IN_RIGHT_SPLIT => Scope::Tree,
        EXPAND_ALL | COLLAPSE_ALL => Scope::Trees,
        // With their keys the tree node actions act only in a tree (`SPEC-0275` v13 behaviour
        // 32): elsewhere `+`, `-` and `*` are typed text, in the terminal as in a field.
        crate::tree_nav::EXPAND_NODE
        | crate::tree_nav::COLLAPSE_NODE
        | crate::tree_nav::EXPAND_FULLY => Scope::Trees,
        _ if usage_grouping(id).is_some() => Scope::Find,
        // `SPEC-0293` v9 behaviours 22–26.
        _ if usage_view_change(id).is_some() => Scope::Find,
        EXPORT_TO_TEXT_FILE => Scope::Find,
        _ if todo_grouping(id).is_some() => Scope::Todo,
        // `SPEC-0087` v10 behaviour 18: ⌘D and Ctrl+D duplicate a line everywhere else.
        SEND_EOF => Scope::Run,
        _ => return None,
    })
}

/// The keymap action that sends the end of input to the program of the Run tool window
/// (`SPEC-0087` v10 behaviour 18): ⌘D in the bundled macOS keymap, Ctrl+D in the default one.
pub const SEND_EOF: &str = "SendEOF";

/// What the status bar says after Send EOF: `sent` is what the Run tool window answered —
/// `None` without a running program in the selected tab, else whether an open input was closed.
pub fn eof_message(sent: Option<bool>) -> &'static str {
    match sent {
        Some(true) => "End of input sent to the program",
        Some(false) => "The program's input is already closed",
        None => "No program is running in the selected Run tab",
    }
}

/// The keymap action that opens the chooser of the file's custom regions (`SPEC-0083` v9
/// behaviour 24).
pub const GOTO_CUSTOM_REGION: &str = "GotoCustomRegion";
/// The title of that chooser.
pub const CUSTOM_REGION_TITLE: &str = "Custom Folding";
/// What the status bar says in a file without custom regions.
pub const NO_CUSTOM_REGIONS: &str = "No custom folding regions in this file";

/// The keymap action that shows the type of the expression at the caret (`SPEC-0077` v7
/// behaviour 6).
pub const EXPRESSION_TYPE_INFO: &str = "ExpressionTypeInfo";

/// The keymap action that lists the background tasks that can be stopped (`SPEC-0061` v16
/// behaviour 62).
pub const STOP_BACKGROUND_PROCESSES: &str = "StopBackgroundProcesses";
/// The title of that list.
pub const STOP_BACKGROUND_TITLE: &str = "Stop Background Processes";
/// What the status bar says when nothing runs that can be stopped.
pub const NO_BACKGROUND_TASKS: &str = "No background task is running that can be stopped";

/// A background task of a window that has a way to be stopped (`SPEC-0061` v16 behaviour 62),
/// in the order the list shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundTask {
    /// A repository is fetched from the project popup (`SPEC-0094` v5).
    CloneRepository,
    /// A remote project opens (`SPEC-0318`).
    RemoteOpen,
    /// Find Usages searches (`SPEC-0293`).
    FindUsages,
    /// The TODO tool window scans the project (`SPEC-0334`).
    TodoScan,
    /// A statement of the SQL console runs (`SPEC-0093` v3 behaviour 12).
    ConsoleStatement,
    /// The AI Assistant answers (`SPEC-0157` behaviour 2).
    AssistantAnswer,
}

impl BackgroundTask {
    /// Every task, in the order of the list.
    pub const ALL: [Self; 6] = [
        Self::CloneRepository,
        Self::RemoteOpen,
        Self::FindUsages,
        Self::TodoScan,
        Self::ConsoleStatement,
        Self::AssistantAnswer,
    ];

    /// What the task does, as its row says it.
    pub fn title(self) -> &'static str {
        match self {
            Self::CloneRepository => "Cloning a repository",
            Self::RemoteOpen => "Opening a remote project",
            Self::FindUsages => "Finding usages",
            Self::TodoScan => "Scanning the project for TODO items",
            Self::ConsoleStatement => "Running a console statement",
            Self::AssistantAnswer => "AI Assistant is answering",
        }
    }

    /// The row of the list: the title, with `detail` (the progress line of a clone) behind it.
    pub fn label(self, detail: Option<&str>) -> String {
        match detail.map(str::trim).filter(|d| !d.is_empty()) {
            Some(detail) => format!("{}: {detail}", self.title()),
            None => self.title().to_owned(),
        }
    }

    /// What the status bar says after the task was told to stop.
    pub fn stopped(self) -> String {
        format!("Stopped: {}", self.title())
    }
}

/// The actions of one chord in the order they are tried: the actions of the focused part first,
/// the actions of a part without the focus not at all, the others as they were.
pub fn order_for_focus(actions: Vec<String>, focused: impl Fn(Scope) -> bool) -> Vec<String> {
    let (mut scoped, rest): (Vec<String>, Vec<String>) = actions
        .into_iter()
        .filter(|a| scope_of(a).is_none_or(&focused))
        .partition(|a| scope_of(a).is_some());
    // The narrower part first: the message field before the window around it.
    scoped.sort_by_key(|a| scope_of(a) != Some(Scope::CommitMessage));
    scoped.extend(rest);
    scoped
}

impl Scope {
    /// Whether the part is a text input itself, not a part that holds one (`SPEC-0028` v6
    /// behaviour 11b). The actions of such a part go before the input's editing actions on the
    /// same chord.
    pub fn is_text_input(self) -> bool {
        matches!(self, Scope::CommitMessage)
    }
}

/// Of the actions of one chord, those of the focused text input itself, in the order they are
/// tried before the input's editing actions (`SPEC-0028` v6 behaviour 11b): ⌃M in the commit
/// message field is Commit Message History before Move Caret to Matching Brace. Empty where the
/// chord has no such action; the input and the part around it then get the key as before.
pub fn input_actions(actions: Vec<String>, focused: impl Fn(Scope) -> bool) -> Vec<String> {
    actions
        .into_iter()
        .filter(|a| scope_of(a).is_some_and(|s| s.is_text_input() && focused(s)))
        .collect()
}

/// The keymap action that lists the commit messages used before (`SPEC-0063` v10 behaviour 22).
pub const MESSAGE_HISTORY: &str = "Vcs.ShowMessageHistory";

/// New Changelist… (`SPEC-0335` behaviour 8); the keymaps bind no key to it.
pub const NEW_CHANGELIST: &str = "ChangesView.NewChangeList";

/// Edit Changelist… (`SPEC-0335` behaviour 9): F2 and Shift+F6 in the Commit tool window.
pub const EDIT_CHANGELIST: &str = "ChangesView.Rename";

/// Set Active Changelist (`SPEC-0335` behaviour 10): Ctrl+Space in the Commit tool window.
pub const SET_ACTIVE_CHANGELIST: &str = "ChangesView.SetDefault";

/// Delete Changelist (`SPEC-0335` behaviour 11); the keymaps bind no key to it.
pub const DELETE_CHANGELIST: &str = "ChangesView.RemoveChangeList";

/// Move to Another Changelist… (`SPEC-0335` behaviour 12): ⇧⌘M on macOS, Alt+Shift+M elsewhere,
/// in the Commit tool window.
pub const MOVE_TO_CHANGELIST: &str = "ChangesView.Move";

/// An operation of the Commit tool window an action stands for (`SPEC-0063` v9 behaviour 21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitOp {
    /// Show Diff of the selected row.
    ShowDiff,
    /// Flip the Amend switch.
    ToggleAmend,
    /// Flip Group By › Directory.
    GroupByDirectory,
    /// Flip Group By › Module.
    GroupByModule,
    /// Open or close the options panel.
    Options,
    /// Open or close the list of the commit messages used before.
    MessageHistory,
    /// A changelist action (`SPEC-0335` behaviours 8–12).
    Changelist(ListOp),
}

/// The Commit tool window's operation of an action id.
pub fn commit_op(id: &str) -> Option<CommitOp> {
    Some(match id {
        "Diff.ShowDiff" => CommitOp::ShowDiff,
        "Vcs.ToggleAmendCommitMode" => CommitOp::ToggleAmend,
        "ChangesView.GroupBy.Directory" => CommitOp::GroupByDirectory,
        "ChangesView.GroupBy.Module" => CommitOp::GroupByModule,
        "ChangesView.ShowCommitOptions" => CommitOp::Options,
        MESSAGE_HISTORY => CommitOp::MessageHistory,
        NEW_CHANGELIST => CommitOp::Changelist(ListOp::New),
        EDIT_CHANGELIST => CommitOp::Changelist(ListOp::Edit),
        SET_ACTIVE_CHANGELIST => CommitOp::Changelist(ListOp::SetActive),
        DELETE_CHANGELIST => CommitOp::Changelist(ListOp::Delete),
        MOVE_TO_CHANGELIST => CommitOp::Changelist(ListOp::Move),
        _ => return None,
    })
}

/// What a grouping action says when the Find tool window has no result.
pub const NO_USAGES_TO_GROUP: &str = "No Find Usages result to group";

/// The toolbar toggle of the Find tool window a grouping action flips, by the id
/// `FindUsagesView::press` takes (`SPEC-0293` v6 behaviour 18).
pub fn usage_grouping(id: &str) -> Option<&'static str> {
    Some(match id {
        "UsageGrouping.UsageType" => "group-kind",
        "UsageGrouping.Module" => "group-module",
        "UsageGrouping.Directory" => "group-package",
        "UsageGrouping.FileStructure" => "group-structure",
        _ => return None,
    })
}

/// Exclude of the Find tool window (`SPEC-0293` v9 behaviour 25); the keymap files bind no key
/// to it, Ferris IDEA gives it Delete and Backspace in the window.
pub const USAGE_EXCLUDE: &str = "UsageView.Exclude";
/// Include of the Find tool window: ⇧⌫ on macOS, Shift+Delete elsewhere.
pub const USAGE_INCLUDE: &str = "UsageView.Include";
/// Export to Text File (`SPEC-0293` v9 behaviour 26).
pub const EXPORT_TO_TEXT_FILE: &str = "ExportToTextFile";

/// What a filter, grouping or exclusion action of the Find tool window changes, by the id
/// `FindUsagesView::change` takes (`SPEC-0293` v9 behaviours 22–25).
pub fn usage_view_change(id: &str) -> Option<&'static str> {
    Some(match id {
        "UsageFiltering.ReadAccess" => "show-read",
        "UsageFiltering.WriteAccess" => "show-write",
        "UsageFiltering.Imports" => "show-imports",
        "UsageGrouping.DirectoryStructure" => "group-dirs",
        "UsageGrouping.FlattenModules" => "flatten-modules",
        USAGE_EXCLUDE => "exclude",
        USAGE_INCLUDE => "include",
        _ => return None,
    })
}

/// What Exclude or Include says when the selected row gives it nothing to do.
pub fn usage_view_unchanged(id: &str) -> &'static str {
    match id {
        USAGE_INCLUDE => "Nothing excluded in the selected row",
        _ => "Nothing to exclude in the selected row",
    }
}

/// Whether an action of the Find tool window that changed nothing leaves its key to the next
/// action on it: Flatten Modules without Group by Module, whose chord is also Export to Text
/// File's in the macOS keymap (`SPEC-0293` v9 behaviours 24 and 26).
pub fn usage_view_passes_on(id: &str) -> bool {
    id == "UsageGrouping.FlattenModules"
}

/// What Export to Text File says without a result in the Find tool window.
pub const NO_USAGES_TO_EXPORT: &str = "No Find Usages result to export";

/// The file name Export to Text File offers: the symbol with every character a file name
/// should not hold replaced, `usages.txt` without one.
pub fn export_file_name(symbol: &str) -> String {
    let name: String = symbol
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let name = name.trim_matches(['_', '.']);
    if name.is_empty() {
        "usages.txt".into()
    } else {
        format!("usages-of-{name}.txt")
    }
}

/// What the status bar says after an export.
pub fn exported(count: usize, to: &Path) -> String {
    format!(
        "Exported {} to {}",
        crate::find_usages_model::count_label(count, false),
        to.display()
    )
}

/// The keymap action that moves the caret to the next argument of the call parameter info is
/// shown for (`SPEC-0080` v3 behaviour 7): Tab in the bundled keymaps.
pub const NEXT_PARAMETER: &str = "NextParameter";
/// The keymap action that moves the caret to the argument before: ⇧Tab.
pub const PREV_PARAMETER: &str = "PrevParameter";

/// Whether the actions bound to a keystroke hold Next (`Some(true)`) or Prev Parameter
/// (`Some(false)`).
pub fn parameter_action(bound: &[&str]) -> Option<bool> {
    if bound.contains(&NEXT_PARAMETER) {
        Some(true)
    } else if bound.contains(&PREV_PARAMETER) {
        Some(false)
    } else {
        None
    }
}

/// A string literal that starts at `i` and ends on the same line: the index after it.
fn literal_end(chars: &[char], i: usize) -> Option<usize> {
    let quote = chars[i];
    let mut j = i + 1;
    while j < chars.len() && chars[j] != '\n' {
        match chars[j] {
            '\\' => j += 1,
            c if c == quote => return Some(j + 1),
            _ => {}
        }
        j += 1;
    }
    None
}

/// Where Next (`forward`) or Prev Parameter puts the caret (`SPEC-0080` v3 behaviour 7): the
/// start of the argument after or before the one the caret at char `caret` of `text` is in, in
/// the innermost call around the caret. `None` without a round bracket around the caret, or
/// without a further argument in that direction: the key then stays the editor's.
///
/// Arguments are what the commas outside nested brackets and outside string literals of one
/// line separate; an argument starts at its first character that is no white space.
pub fn parameter_step(text: &str, caret: usize, forward: bool) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    // The round bracket that opens the innermost call around the caret.
    let mut depth = 0usize;
    let mut open = None;
    for i in (0..caret).rev() {
        match chars[i] {
            ')' | ']' | '}' => depth += 1,
            '(' if depth == 0 => {
                open = Some(i);
                break;
            }
            '(' | '[' | '{' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let open = open?;
    // The starts of its arguments, and the index of the one the caret is in.
    let mut starts: Vec<usize> = Vec::new();
    let mut current = 0usize;
    let mut depth = 0usize;
    let mut begin = open + 1;
    let mut i = open + 1;
    let mut closed = false;
    // An argument starts at its first character that is no white space; an empty place is
    // entered at its end.
    let start_of = |from: usize, to: usize| {
        (from..to)
            .find(|&k| !chars[k].is_whitespace())
            .unwrap_or(to)
    };
    while i < chars.len() {
        match chars[i] {
            '"' | '\'' => {
                if let Some(end) = literal_end(&chars, i) {
                    i = end;
                    continue;
                }
            }
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth > 0 => depth -= 1,
            ')' | ']' | '}' => {
                closed = true;
                break;
            }
            ',' if depth == 0 => {
                starts.push(start_of(begin, i));
                begin = i + 1;
                if caret > i {
                    current = starts.len();
                }
            }
            _ => {}
        }
        i += 1;
    }
    let end = if closed { i } else { chars.len() };
    // An empty list has no argument; an empty last place after a comma is one.
    if starts.is_empty() && (begin..end).all(|k| chars[k].is_whitespace()) {
        return None;
    }
    starts.push(start_of(begin, end));
    let target = if forward {
        current.checked_add(1).filter(|n| *n < starts.len())?
    } else {
        current.checked_sub(1)?
    };
    Some(starts[target])
}

/// The windows Activate Next Window and Activate Previous Window go through (`SPEC-0094` v6
/// behaviour 17): the project windows in the order they were opened, then every other window
/// of the application (a tool window in a window of its own) as the toolkit lists them.
pub fn window_cycle<T: Copy + PartialEq>(project: &[T], open: &[T]) -> Vec<T> {
    let mut all: Vec<T> = project
        .iter()
        .copied()
        .filter(|w| open.contains(w))
        .collect();
    for w in open {
        if !all.contains(w) {
            all.push(*w);
        }
    }
    all
}

/// What Show Source says when the selected row is no file or usage.
pub const NOTHING_TO_SHOW_SOURCE_OF: &str = "Select a file or a usage to show its source";

/// The title of the list of a tool window's tabs (`SPEC-0167` v7 behaviour 19).
pub fn tab_list_title(tool_window: &str) -> String {
    format!("Tabs of {tool_window}")
}

/// The file kinds New Scratch File offers, with their extensions (`SPEC-0059` v9.6
/// behaviour 28).
pub const SCRATCH_KINDS: &[(&str, &str)] = &[
    ("Plain text", "txt"),
    ("CSS", "css"),
    ("Go", "go"),
    ("HTML", "html"),
    ("HTTP request", "http"),
    ("Java", "java"),
    ("JavaScript", "js"),
    ("JSON", "json"),
    ("Kotlin", "kt"),
    ("Markdown", "md"),
    ("Python", "py"),
    ("Rust", "rs"),
    ("Shell script", "sh"),
    ("SQL", "sql"),
    ("TypeScript", "ts"),
    ("XML", "xml"),
    ("YAML", "yaml"),
];

/// The kind New Scratch File selects first: the one of the active file, else plain text.
pub fn scratch_kind_for(active: Option<&Path>) -> usize {
    active
        .and_then(Path::extension)
        .and_then(|e| e.to_str())
        .and_then(|e| {
            SCRATCH_KINDS
                .iter()
                .position(|(_, ext)| ext.eq_ignore_ascii_case(e))
        })
        .unwrap_or(0)
}

/// The name of the next scratch file with `extension`: `scratch.<ext>`, then `scratch_1.<ext>`,
/// `scratch_2.<ext>` …, the first that `taken` does not say exists.
pub fn scratch_name(extension: &str, taken: impl Fn(&str) -> bool) -> String {
    let first = format!("scratch.{extension}");
    if !taken(&first) {
        return first;
    }
    (1..)
        .map(|n| format!("scratch_{n}.{extension}"))
        .find(|name| !taken(name))
        .unwrap_or(first)
}

/// Creates an empty scratch file with `extension` in `dir`, which is created when missing; an
/// existing file is never overwritten.
pub fn create_scratch(dir: &Path, extension: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    loop {
        let name = scratch_name(extension, |n| dir.join(n).exists());
        let path = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => return Ok(path),
            // Another window took the name in between: the next one.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// What Go to Mnemonic says without a bookmark that has one.
pub const NO_MNEMONIC_BOOKMARKS: &str = "No bookmark has a mnemonic";

/// What Copy… says when not exactly one item is selected in the Project tree.
pub const ONE_TO_COPY: &str = "Select one file or folder to copy it";

/// The toolbar toggle of the TODO tool window a grouping action flips, by the id
/// `TodoToolView::press` takes (`SPEC-0334` behaviour 25).
pub fn todo_grouping(id: &str) -> Option<&'static str> {
    Some(match id {
        "TodoViewGroupByShowModules" => "group-modules",
        "TodoViewGroupByShowPackages" => "group-packages",
        "TodoViewGroupByFlattenPackage" => "flatten",
        _ => return None,
    })
}

/// One target of Quick Definition: where it is and the source shown for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickTarget {
    /// The file's path relative to the project, or absolute outside it.
    pub shown_path: String,
    /// The lines shown.
    pub source: Source,
}

/// The blocks of the documentation popup for the targets of Quick Definition (`SPEC-0081` v4
/// behaviour 6): for each of the first [`MAX_TARGETS`] a grey `path:line` and the source, a cut
/// declaration ending with `…`, a rule between targets, and the number of targets left out.
pub fn quick_definition_blocks(targets: &[QuickTarget]) -> Vec<crate::doc_model::DocBlock> {
    use crate::doc_model::DocBlock;
    let mut blocks = Vec::new();
    for (i, t) in targets.iter().take(MAX_TARGETS).enumerate() {
        if i > 0 {
            blocks.push(DocBlock::Rule);
        }
        blocks.push(DocBlock::Owner(format!(
            "{}:{}",
            t.shown_path,
            t.source.line + 1
        )));
        let mut text = t.source.text.clone();
        if t.source.truncated {
            text.push_str("\n…");
        }
        blocks.push(DocBlock::Code(text));
    }
    if targets.len() > MAX_TARGETS {
        blocks.push(DocBlock::Footer(format!(
            "{} more not shown; Go to Implementation(s) lists all",
            targets.len() - MAX_TARGETS
        )));
    }
    blocks
}

/// What Quick Definition says when the server names no target in a file of the project;
/// `outside` are the library classes it named instead.
pub fn quick_definition_nothing(outside: &[String]) -> String {
    if outside.is_empty() {
        "Cannot find a definition to show.".into()
    } else {
        format!(
            "The definition is in the library class {}; Quick Definition shows project files only",
            outside.join(", ")
        )
    }
}

/// The title of the chooser of Go to Test (`SPEC-0071` v10 behaviour 15): the tests of a source
/// file, or the subjects of a test.
pub fn goto_test_title(file_name: &str, is_test: bool, n: usize) -> String {
    let what = if is_test { "test subject" } else { "test" };
    format!("Choose {what} for {file_name} ({n})")
}

/// What Go to Test says without a candidate; `is_test` is `None` for a file whose language has
/// no naming rule.
pub fn goto_test_nothing(file_name: &str, is_test: Option<bool>) -> String {
    match is_test {
        Some(false) => format!("No test found for {file_name}"),
        Some(true) => format!("No test subject found for {file_name}"),
        None => format!("Go to Test knows no naming rule for {file_name}"),
    }
}

/// The title of the chooser of Go to Related (`SPEC-0071` v10 behaviour 17).
pub fn related_title(file_name: &str, n: usize) -> String {
    format!("Choose related file of {file_name} ({n})")
}

/// What Go to Related says without a related file; `known` is whether the model knows a
/// relation for files of this kind.
pub fn related_nothing(file_name: &str, known: bool) -> String {
    if known {
        format!("No related files found for {file_name}")
    } else {
        format!("No related files are known for {file_name}")
    }
}

/// The rows of a chooser of files, in the order given: each file with its first line.
pub fn file_rows(root: &Path, files: &[PathBuf]) -> Vec<crate::usages_model::Usage> {
    files
        .iter()
        .map(|file| crate::usages_model::Usage {
            place: crate::usages_model::place(file.clone(), 0, 0),
            shown_path: file
                .strip_prefix(root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| file.display().to_string()),
            text: crate::usages_model::disk_line(file, 0)
                .map(|t| crate::completion_model::one_line(&t))
                .unwrap_or_default(),
            owner: None,
        })
        .collect()
}

/// The action whose shortcut flips "Show edited only" inside the Recent Files popup (`SPEC-0069`
/// v3 behaviour 9).
pub const EDITED_ONLY_ACTION: &str = "SwitcherRecentEditedChangedToggleCheckBox";

/// The keymap action that moves the Switcher's selection down (`SPEC-0059` v9.10 behaviour 27h).
pub const SWITCHER_ITERATE: &str = "SwitcherIterateItems";

/// Whether the single keystroke `stroke` is a shortcut of `action` in `keymap`.
pub fn is_shortcut_of(
    keymap: &ferris_idea_ui::keymap::Keymap,
    action: &str,
    stroke: &ferris_idea_ui::keymap::Keystroke,
) -> bool {
    keymap.shortcuts(action).iter().any(|s| {
        matches!(s, ferris_idea_ui::keymap::Shortcut::Keyboard { first, second: None } if first == stroke)
    })
}

/// The commit menu's entry an action of the Git log chooses (`SPEC-0086` v11 behaviour 20).
pub fn log_entry(id: &str) -> Option<CommitAction> {
    match id {
        "Git.Reword.Commit" => Some(CommitAction::EditMessage),
        "Git.New.Branch.In.Log" => Some(CommitAction::NewBranch),
        _ => None,
    }
}

/// Why Compare Two Files cannot run on a selection.
pub const COMPARE_NEEDS_TWO: &str = "Select two files in the Project tree to compare them";

/// The two files Compare Two Files shows, left and right: the selection in tree order when it is
/// exactly two files (`SPEC-0084` v4 behaviour 15).
pub fn compare_pair(selected: &[PathBuf]) -> Result<(PathBuf, PathBuf), &'static str> {
    match selected {
        [left, right] if left.is_file() && right.is_file() => Ok((left.clone(), right.clone())),
        _ => Err(COMPARE_NEEDS_TWO),
    }
}

/// The column titles of a comparison: the file names, or the paths relative to `root` when the
/// names are equal.
pub fn compare_titles(root: &Path, left: &Path, right: &Path) -> (String, String) {
    let name = |p: &Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let relative = |p: &Path| p.strip_prefix(root).unwrap_or(p).display().to_string();
    if name(left) == name(right) {
        (relative(left), relative(right))
    } else {
        (name(left), name(right))
    }
}

/// Folder and file name the save dialog of Save As starts with (`SPEC-0059` v9.3 behaviour 26).
pub fn save_as_start(file: &Path, root: &Path) -> (PathBuf, String) {
    let dir = file
        .parent()
        .filter(|d| d.is_dir())
        .unwrap_or(root)
        .to_path_buf();
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    (dir, name)
}

/// What Jump to Last Tool Window says when no tool window had the focus yet.
pub const NO_LAST_TOOL_WINDOW: &str = "No tool window had the focus yet";

/// The tool window that had the keyboard focus last (`SPEC-0167` v5 behaviour 16).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LastToolWindow(Option<ToolWindow>);

impl LastToolWindow {
    /// Notes which tool window has the focus now, if any; losing the focus changes nothing.
    pub fn observe(&mut self, focused: Option<ToolWindow>) {
        if focused.is_some() {
            self.0 = focused;
        }
    }

    /// The tool window to jump to.
    pub fn get(self) -> Option<ToolWindow> {
        self.0
    }
}

// The eighth package (`workbench_keymap_eighth.rs`).

/// The keymap action that selects the editor tab right of the active one (`SPEC-0059` v9.7
/// behaviour 29).
pub const NEXT_EDITOR_TAB: &str = "NextEditorTab";
/// The keymap action that selects the editor tab left of it.
pub const PREVIOUS_EDITOR_TAB: &str = "PreviousEditorTab";

/// The keymap action that highlights the signature above in parameter info (`SPEC-0080` v4
/// behaviour 8).
pub const OVERLOAD_UP: &str = "MethodOverloadSwitchUp";
/// The keymap action that highlights the signature below.
pub const OVERLOAD_DOWN: &str = "MethodOverloadSwitchDown";

/// Whether the actions bound to a keystroke hold the overload switch down (`Some(true)`) or up
/// (`Some(false)`).
pub fn overload_action(bound: &[&str]) -> Option<bool> {
    if bound.contains(&OVERLOAD_DOWN) {
        Some(true)
    } else if bound.contains(&OVERLOAD_UP) {
        Some(false)
    } else {
        None
    }
}

/// The signature highlighted after a step `down` (or up) from `at` among `count`, wrapping;
/// `None` with fewer than two signatures.
pub fn overload_step(count: usize, at: usize, down: bool) -> Option<usize> {
    if count < 2 {
        return None;
    }
    let at = at.min(count - 1);
    Some(if down {
        (at + 1) % count
    } else {
        (at + count - 1) % count
    })
}

/// The keymap action that lists every file of the selected commit (`SPEC-0086` v12 behaviour
/// 21).
pub const SHOW_ALL_AFFECTED: &str = "VcsHistory.ShowAllAffected";
/// What the status bar says for a commit without changed files.
pub const NO_AFFECTED_FILES: &str = "The selected commit changes no files";

/// The rows of the list of a commit's files: the change letter and the path.
pub fn affected_rows(changes: &[ferris_idea_vcs::CommitChange]) -> Vec<String> {
    use ferris_idea_vcs::ChangeKind;
    changes
        .iter()
        .map(|c| {
            let letter = match c.kind {
                ChangeKind::Added => 'A',
                ChangeKind::Deleted => 'D',
                ChangeKind::Modified => 'M',
                ChangeKind::Renamed => 'R',
                ChangeKind::TypeChange => 'T',
                ChangeKind::Conflict => 'U',
            };
            format!("{letter}  {}", c.path)
        })
        .collect()
}

/// The title of that list: the short id and the subject.
pub fn affected_title(short_id: &str, summary: &str) -> String {
    format!("Files of {short_id}: {summary}")
}

/// What the status bar says where Unwrap finds no block (`SPEC-0056` v5 behaviour 21).
pub const NOTHING_TO_UNWRAP: &str = "Nothing to unwrap here";
/// The title of the list of blocks.
pub const UNWRAP_TITLE: &str = "Unwrap/Remove";

/// What the status bar says on a line without gutter icons (`SPEC-0294` v5 behaviour 12).
pub const NO_GUTTER_ICONS: &str = "No gutter icons on this line";

/// What the status bar says when Inspect Current File has no file (`SPEC-0098` v10 behaviour
/// 20).
pub const INSPECT_FILE_NEEDS_FILE: &str = "Inspect Current File needs a file open in the editor.";
/// The title of the list of inspections (behaviour 21).
pub const RUN_INSPECTION_TITLE: &str = "Run Inspection by Name";

/// What the status bar says where Recompile finds no module (`SPEC-0100` v5 behaviour 22).
pub const NOTHING_TO_COMPILE: &str =
    "Nothing to compile: the file is in no Maven, Gradle, Cargo or Go module";

/// What the status bar says when class name completion finds no type (`SPEC-0072` v13
/// behaviour 48).
pub const NO_CLASS_NAMES: &str = "No class names to suggest";

// The ninth package (`workbench_keymap_ninth.rs`).

/// What the status bar says while Safe Delete searches (`SPEC-0095` v12 behaviour 15a).
pub fn safe_delete_looking(name: &str) -> String {
    format!("Safe Delete: looking for usages of `{name}`…")
}

/// `n` and `noun`, with an `s` for more than one.
fn counted(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// The title of Safe Delete's list of usages (behaviour 15d): `noun` is `usage`, `text
/// occurrence` or, for a file, `mention` (behaviour 16).
pub fn safe_delete_title(name: &str, n: usize, noun: &str) -> String {
    format!("Safe Delete {name}: {}", counted(n, noun))
}

/// The first row of Safe Delete's list, selected first: nothing is deleted.
pub const SAFE_DELETE_CANCEL: &str = "Cancel";
/// The second row: deletes in spite of the usages.
pub const SAFE_DELETE_ANYWAY: &str = "Delete anyway";

/// The rows of Safe Delete's list: Cancel, Delete anyway, then one row per usage (its file, the
/// 0-based line and the line's text).
pub fn safe_delete_rows(root: &Path, usages: &[(PathBuf, usize, String)]) -> Vec<String> {
    let mut rows = vec![SAFE_DELETE_CANCEL.to_owned(), SAFE_DELETE_ANYWAY.to_owned()];
    rows.extend(usages.iter().map(|(path, line, text)| {
        let shown = path
            .strip_prefix(root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.display().to_string());
        format!(
            "{shown}:{}  {}",
            line + 1,
            crate::completion_model::one_line(text.trim())
        )
    }));
    rows
}

/// The key that takes an edit back, as the status bar names it.
fn undo_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘Z"
    } else {
        "Ctrl+Z"
    }
}

/// What the status bar says after Safe Delete deleted the `kind` `name` (behaviour 15c): in an
/// open file (`closed` is `None`) or in the file `closed` names; `left` usages stay.
pub fn safe_deleted(kind: &str, name: &str, closed: Option<&str>, left: usize) -> String {
    let mut line = match closed {
        None => format!("Deleted {kind} `{name}`; {} brings it back", undo_key()),
        Some(file) => format!("Deleted {kind} `{name}` in {file}; Local History has the old text"),
    };
    if left > 0 {
        line.push_str(&format!("; {} left behind", counted(left, "usage")));
    }
    line
}

/// What the status bar says when the declaration's file changed since the search (behaviour
/// 15e).
pub const SAFE_DELETE_CHANGED: &str =
    "The file changed since the search; nothing was deleted, run Safe Delete again";
/// What it says for a document of a remote project.
pub const SAFE_DELETE_REMOTE: &str = "Safe Delete is not available for a remote document";

/// The options of Reformat File… (`SPEC-0075` v6 behaviour 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReformatOption {
    /// Only the selection.
    Selection,
    /// Only the lines added or changed against HEAD.
    ChangedLines,
    /// Optimize Imports after formatting.
    OptimizeImports,
}

impl ReformatOption {
    /// The row's label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Selection => "Selected text only",
            Self::ChangedLines => "Only changed lines (Git)",
            Self::OptimizeImports => "Optimize imports",
        }
    }
}

/// The options offered: the selection only with one, the changed lines only for a file under
/// version control, Optimize imports always (behaviours 8a–8c).
pub fn reformat_file_rows(selection: bool, versioned: bool) -> Vec<ReformatOption> {
    let mut rows = Vec::new();
    if selection {
        rows.push(ReformatOption::Selection);
    }
    if versioned {
        rows.push(ReformatOption::ChangedLines);
    }
    rows.push(ReformatOption::OptimizeImports);
    rows
}

/// The title of the list of options.
pub fn reformat_file_title(name: &str) -> String {
    format!("Reformat File: {name}")
}

/// The runs of lines to format for "Only changed lines" (behaviour 8b): the added and changed
/// lines of `changes`, 0-based first and last, runs that touch merged, only deleted lines left
/// out, clipped to `within` (0-based first and last line of a selection) when given.
pub fn changed_ranges(
    changes: &[ferris_idea_vcs::LineChange],
    within: Option<(usize, usize)>,
) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = changes
        .iter()
        .filter(|c| c.kind != ferris_idea_vcs::LineChangeKind::Deleted && c.new_end >= c.new_start)
        .map(|c| (c.new_start - 1, c.new_end - 1))
        .filter_map(|(a, b)| match within {
            None => Some((a, b)),
            Some((lo, hi)) => (a <= hi && b >= lo).then(|| (a.max(lo), b.min(hi))),
        })
        .collect();
    runs.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (a, b) in runs {
        match merged.last_mut() {
            Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}

/// What the status bar says when "Only changed lines" finds none.
pub const NO_CHANGED_LINES: &str = "No changed lines to format";

/// The formatting part of the status line after Reformat File… with `ranges` runs of changed
/// lines (`None` for the whole file or the selection) and whether the text changed.
pub fn reformat_file_line(ranges: Option<usize>, changed: bool) -> String {
    match (ranges, changed) {
        (Some(0), _) => NO_CHANGED_LINES.to_owned(),
        (Some(n), true) => format!("Reformatted {}", counted(n, "changed range")),
        (None, true) => "Reformatted".to_owned(),
        (_, false) => "Already formatted.".to_owned(),
    }
}

/// The title of the chooser of Header/Source (`SPEC-0071` v12 behaviour 19).
pub fn header_source_title(file_name: &str, is_header: bool, n: usize) -> String {
    let what = if is_header { "source" } else { "header" };
    format!("Choose {what} for {file_name} ({n})")
}

/// What Header/Source says without a candidate; `is_header` is `None` for a file of another
/// language.
pub fn header_source_nothing(file_name: &str, is_header: Option<bool>) -> String {
    match is_header {
        Some(false) => format!("No header found for {file_name}"),
        Some(true) => format!("No source found for {file_name}"),
        None => format!(
            "Header/Source applies to C, C++ and Objective-C files; {file_name} is none of them"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0095 AK-20 (v12): the rows of Safe Delete's list and its status lines.
    #[test]
    fn safe_delete_rows_and_messages() {
        let root = Path::new("/p");
        assert_eq!(
            safe_delete_rows(
                root,
                &[(PathBuf::from("/p/src/b.rs"), 2, "    total();".into())]
            ),
            ["Cancel", "Delete anyway", "src/b.rs:3  total();"]
        );
        assert_eq!(
            safe_delete_title("total", 1, "usage"),
            "Safe Delete total: 1 usage"
        );
        assert_eq!(
            safe_delete_title("pricing.py", 3, "mention"),
            "Safe Delete pricing.py: 3 mentions"
        );
        assert_eq!(
            safe_deleted("function", "total", Some("src/b.rs"), 2),
            "Deleted function `total` in src/b.rs; Local History has the old text; 2 usages left behind"
        );
        assert!(safe_deleted("function", "total", None, 0).ends_with("brings it back"));
        assert_eq!(
            safe_delete_looking("total"),
            "Safe Delete: looking for usages of `total`…"
        );
    }

    /// SPEC-0075 AK-6 (v6): the options offered, the runs of changed lines and the status
    /// lines.
    #[test]
    fn reformat_file_rows_ranges_and_lines() {
        use ReformatOption::*;
        use ferris_idea_vcs::{LineChange, LineChangeKind};
        assert_eq!(
            reformat_file_rows(true, true),
            [Selection, ChangedLines, OptimizeImports]
        );
        assert_eq!(reformat_file_rows(false, false), [OptimizeImports]);
        assert_eq!(
            reformat_file_rows(false, true),
            [ChangedLines, OptimizeImports]
        );
        let change = |kind, new_start, new_end| LineChange {
            kind,
            new_start,
            new_end,
            old_start: 1,
            old_end: 1,
        };
        let changes = [
            change(LineChangeKind::Modified, 2, 3),
            change(LineChangeKind::Added, 4, 4),
            change(LineChangeKind::Deleted, 7, 6),
            change(LineChangeKind::Added, 10, 12),
        ];
        assert_eq!(changed_ranges(&changes, None), [(1, 3), (9, 11)]);
        assert_eq!(changed_ranges(&changes, Some((3, 9))), [(3, 3), (9, 9)]);
        assert_eq!(changed_ranges(&changes, Some((5, 8))), []);
        assert_eq!(
            reformat_file_line(Some(2), true),
            "Reformatted 2 changed ranges"
        );
        assert_eq!(reformat_file_line(Some(0), false), NO_CHANGED_LINES);
        assert_eq!(reformat_file_line(None, false), "Already formatted.");
        assert_eq!(reformat_file_title("Shop.java"), "Reformat File: Shop.java");
    }

    /// SPEC-0071 AK-14 (v12): the titles and messages of Header/Source.
    #[test]
    fn header_source_titles_and_messages() {
        assert_eq!(
            header_source_title("cart.cpp", false, 2),
            "Choose header for cart.cpp (2)"
        );
        assert_eq!(
            header_source_title("cart.h", true, 2),
            "Choose source for cart.h (2)"
        );
        assert_eq!(
            header_source_nothing("cart.cpp", Some(false)),
            "No header found for cart.cpp"
        );
        assert_eq!(
            header_source_nothing("cart.h", Some(true)),
            "No source found for cart.h"
        );
        assert!(header_source_nothing("cart.rs", None).starts_with("Header/Source applies"));
    }

    /// SPEC-0080 AK-6 (v4): the overloads step and wrap; one signature has no step; the keys of
    /// both bundled keymaps hold the actions.
    #[test]
    fn overloads_step_and_wrap() {
        use ferris_idea_ui::keymap::{Keymap, Keystroke};
        assert_eq!(overload_step(3, 0, true), Some(1));
        assert_eq!(overload_step(3, 2, true), Some(0));
        assert_eq!(overload_step(3, 0, false), Some(2));
        assert_eq!(overload_step(3, 9, false), Some(1));
        assert_eq!(overload_step(1, 0, true), None);
        assert_eq!(overload_step(0, 0, false), None);
        for (keymap, down, up) in [
            (Keymap::bundled_macos(), "alt DOWN", "alt UP"),
            (Keymap::bundled_default(), "control DOWN", "control UP"),
        ] {
            let bound = |k: &str| keymap.actions_for(&Keystroke::parse(k).unwrap(), None);
            assert_eq!(overload_action(&bound(down)), Some(true), "{down}");
            assert_eq!(overload_action(&bound(up)), Some(false), "{up}");
        }
        assert_eq!(overload_action(&["EditorSelectWord"]), None);
        assert_eq!(scope_of(OVERLOAD_UP), None);
    }

    /// SPEC-0086 AK-34 (v12): a commit's files as rows; the action belongs to the Git log.
    #[test]
    fn affected_rows_name_letter_and_path() {
        use ferris_idea_vcs::{ChangeKind, CommitChange};
        let change = |path: &str, kind| CommitChange {
            path: path.into(),
            kind,
        };
        assert_eq!(
            affected_rows(&[
                change("src/a.rs", ChangeKind::Modified),
                change("new.txt", ChangeKind::Added),
                change("gone.txt", ChangeKind::Deleted),
                change("moved.rs", ChangeKind::Renamed),
            ]),
            ["M  src/a.rs", "A  new.txt", "D  gone.txt", "R  moved.rs"]
        );
        assert!(affected_rows(&[]).is_empty());
        assert_eq!(affected_title("1a2b3c4", "Fix"), "Files of 1a2b3c4: Fix");
        assert_eq!(scope_of(SHOW_ALL_AFFECTED), Some(Scope::Log));
        assert_eq!(scope_of(NEXT_EDITOR_TAB), None);
    }

    /// A folder of its own below the system's temporary folder, removed at the end.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(test: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "ferris-idea-keymap-actions-{test}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    /// SPEC-0084 AK-10, SPEC-0063 AK-11, SPEC-0068 AK-19, SPEC-0086 AK-33.
    #[test]
    fn scoped_actions_run_first_and_only_in_their_focus() {
        // F2 on macOS: Next Error, the branch rename and the reword.
        let f2 = ids(&[
            "GotoNextError",
            "Git.Rename.Local.Branch",
            "Git.Reword.Commit",
        ]);
        assert_eq!(
            order_for_focus(f2.clone(), |_| false),
            ids(&["GotoNextError"])
        );
        assert_eq!(
            order_for_focus(f2.clone(), |s| s == Scope::Log),
            ids(&["Git.Reword.Commit", "GotoNextError"])
        );
        assert_eq!(
            order_for_focus(f2, |s| s == Scope::Branches),
            ids(&["Git.Rename.Local.Branch", "GotoNextError"])
        );
        // ⌘D: duplicate, the Commit window's diff and Compare Two Files.
        let d = ids(&["EditorDuplicate", "Diff.ShowDiff", "CompareTwoFiles"]);
        assert_eq!(
            order_for_focus(d.clone(), |s| s == Scope::Tree),
            ids(&["CompareTwoFiles", "EditorDuplicate"])
        );
        assert_eq!(
            order_for_focus(d, |s| s == Scope::Commit),
            ids(&["Diff.ShowDiff", "EditorDuplicate"])
        );
        // The viewer's stepping has no scope: it runs whenever a viewer is open.
        assert_eq!(scope_of("NextDiff"), None);
        assert_eq!(scope_of("PreviousDiff"), None);
        assert_eq!(scope_of("Diff.ApplyLeftSide"), Some(Scope::Diff));
        // SPEC-0084 v5: the viewer's own actions run by key only while it has the focus.
        for id in [
            "Diff.ApplyRightSide",
            "Diff.FocusOppositePane",
            "Diff.NextChange",
            "Diff.PrevChange",
            "Diff.ShowSettingsPopup",
        ] {
            assert_eq!(scope_of(id), Some(Scope::Diff), "{id}");
        }
        assert_eq!(scope_of("Git.CreateNewBranch"), None);
        // ⌃M on macOS: the Commit window's and the Find tool window's Group by Module.
        let m = ids(&["ChangesView.GroupBy.Module", "UsageGrouping.Module"]);
        assert_eq!(order_for_focus(m.clone(), |_| false), ids(&[]));
        assert_eq!(
            order_for_focus(m.clone(), |s| s == Scope::Find),
            ids(&["UsageGrouping.Module"])
        );
        assert_eq!(
            order_for_focus(m, |s| s == Scope::Commit),
            ids(&["ChangesView.GroupBy.Module"])
        );
    }

    /// SPEC-0063 AK-12 (v10): ⌃M is Commit Message History in the message field and Group By
    /// Module in the rest of the Commit window, in both bundled keymaps; nothing elsewhere.
    #[test]
    fn the_message_history_key_belongs_to_the_message_field() {
        use ferris_idea_ui::keymap::{Keymap, Keystroke, Shortcut};
        assert_eq!(scope_of(MESSAGE_HISTORY), Some(Scope::CommitMessage));
        assert_eq!(commit_op(MESSAGE_HISTORY), Some(CommitOp::MessageHistory));
        let chord = Keystroke::parse("control M").unwrap();
        for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
            assert!(keymap.shortcuts(MESSAGE_HISTORY).iter().any(|s| matches!(
                s,
                Shortcut::Keyboard { first, second: None } if *first == chord
            )));
            let bound = || -> Vec<String> {
                keymap
                    .actions_for(&chord, None)
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            };
            // In the field: the window around it has the focus too.
            let in_field = order_for_focus(bound(), |s| {
                matches!(s, Scope::Commit | Scope::CommitMessage)
            });
            assert_eq!(in_field.first().map(String::as_str), Some(MESSAGE_HISTORY));
            let in_list = order_for_focus(bound(), |s| s == Scope::Commit);
            assert!(!in_list.iter().any(|a| a == MESSAGE_HISTORY));
            let elsewhere = order_for_focus(bound(), |_| false);
            assert!(!elsewhere.iter().any(|a| a == MESSAGE_HISTORY));
        }
    }

    /// SPEC-0028 AK-12 (v6): of the actions of a chord, only those of the focused text input
    /// itself go before the input's editing actions; the actions of the part around it do not.
    #[test]
    fn only_the_inputs_own_actions_go_before_its_editing() {
        use ferris_idea_ui::keymap::{Keymap, Keystroke};
        let bound = |keymap: &Keymap, chord: &str| -> Vec<String> {
            keymap
                .actions_for(&Keystroke::parse(chord).unwrap(), None)
                .into_iter()
                .map(str::to_owned)
                .collect()
        };
        let in_field = |s: Scope| matches!(s, Scope::Commit | Scope::CommitMessage | Scope::Trees);
        let mac = Keymap::bundled_macos();
        // ⌃M: the history, although the chord is also Move Caret to Matching Brace and the
        // grouping of three tool windows.
        let m = bound(&mac, "control M");
        assert!(m.iter().any(|a| a == "EditorMatchBrace"), "{m:?}");
        assert_eq!(input_actions(m.clone(), in_field), ids(&[MESSAGE_HISTORY]));
        // Not in the file list of the Commit window, in another input or anywhere else.
        assert_eq!(input_actions(m.clone(), |s| s == Scope::Commit), ids(&[]));
        assert_eq!(input_actions(m, |_| false), ids(&[]));
        // The other ⌃-letter chords belong to no input: the field's editing and the window
        // around it keep them (⌃P is the caret up, ⌃O the commit options).
        for letter in 'A'..='Z' {
            if letter == 'M' {
                continue;
            }
            let chord = format!("control {letter}");
            assert_eq!(
                input_actions(bound(&mac, &chord), |_| true),
                ids(&[]),
                "{chord}"
            );
        }
        // The only part that is an input today.
        for scope in [
            Scope::Diff,
            Scope::Commit,
            Scope::Branches,
            Scope::Log,
            Scope::Tree,
            Scope::Find,
            Scope::Todo,
            Scope::Trees,
            Scope::Run,
        ] {
            assert!(!scope.is_text_input(), "{scope:?}");
        }
        assert!(Scope::CommitMessage.is_text_input());
    }

    /// SPEC-0028 v6 behaviour 11: the ⌃-letter chords the bundled macOS keymap binds to an action
    /// of a part of the window, as the specification lists them.
    #[test]
    fn the_control_letter_chords_of_the_parts_are_the_listed_ones() {
        use ferris_idea_ui::keymap::{Keymap, Keystroke};
        let mac = Keymap::bundled_macos();
        let mut found: Vec<(Scope, char)> = Vec::new();
        for letter in 'A'..='Z' {
            let chord = Keystroke::parse(&format!("control {letter}")).unwrap();
            for action in mac.actions_for(&chord, None) {
                if let Some(scope) = scope_of(action)
                    && !found.contains(&(scope, letter))
                {
                    found.push((scope, letter));
                }
            }
        }
        let of = |scope: Scope| -> String {
            found
                .iter()
                .filter(|(s, _)| *s == scope)
                .map(|(_, l)| *l)
                .collect()
        };
        assert_eq!(of(Scope::CommitMessage), "M");
        assert_eq!(of(Scope::Commit), "MOP");
        // ⌃D, ⌃I, ⌃O, ⌃R and ⌃W since SPEC-0293 v9 (SPEC-0028 v6.2).
        assert_eq!(of(Scope::Find), "DIMOPRSTW");
        assert_eq!(of(Scope::Todo), "FMP");
        for scope in [
            Scope::Diff,
            Scope::Branches,
            Scope::Log,
            Scope::Tree,
            Scope::Trees,
            // Send EOF is ⌘D on macOS (SPEC-0028 v6.3): no ⌃-letter chord.
            Scope::Run,
        ] {
            assert_eq!(of(scope), "", "{scope:?}");
        }
    }

    /// SPEC-0087 AK-12 (v10): Send EOF is an action of the Run tool window; its key of both
    /// bundled keymaps runs it first there and not at all elsewhere, where the key duplicates
    /// a line; what the status bar says for each answer of the window.
    #[test]
    fn send_eof_is_an_action_of_the_run_window() {
        use ferris_idea_ui::keymap::{Keymap, Keystroke};
        assert_eq!(SEND_EOF, "SendEOF");
        assert_eq!(scope_of(SEND_EOF), Some(Scope::Run));
        for (keymap, key) in [
            (Keymap::bundled_macos(), "meta D"),
            (Keymap::bundled_default(), "control D"),
        ] {
            let bound = || -> Vec<String> {
                keymap
                    .actions_for(&Keystroke::parse(key).unwrap(), None)
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            };
            assert!(bound().iter().any(|a| a == SEND_EOF), "{key}");
            assert!(bound().iter().any(|a| a == "EditorDuplicate"), "{key}");
            let in_run = order_for_focus(bound(), |s| s == Scope::Run);
            assert_eq!(in_run[0], SEND_EOF, "{key}");
            let elsewhere = order_for_focus(bound(), |_| false);
            assert!(!elsewhere.iter().any(|a| a == SEND_EOF), "{key}");
            assert!(elsewhere.iter().any(|a| a == "EditorDuplicate"), "{key}");
            // In the Project tree the key stays Compare Two Files, not Send EOF.
            let in_tree = order_for_focus(bound(), |s| s == Scope::Tree);
            assert!(!in_tree.iter().any(|a| a == SEND_EOF), "{key}");
        }
        assert_eq!(eof_message(Some(true)), "End of input sent to the program");
        assert_eq!(
            eof_message(Some(false)),
            "The program's input is already closed"
        );
        assert_eq!(
            eof_message(None),
            "No program is running in the selected Run tab"
        );
    }

    /// SPEC-0061 AK-v16: the background tasks that can be stopped, their rows and what the
    /// status bar says after one was told to stop.
    #[test]
    fn background_tasks_have_rows_in_a_fixed_order() {
        let titles: Vec<&str> = BackgroundTask::ALL.iter().map(|t| t.title()).collect();
        assert_eq!(
            titles,
            [
                "Cloning a repository",
                "Opening a remote project",
                "Finding usages",
                "Scanning the project for TODO items",
                "Running a console statement",
                "AI Assistant is answering"
            ]
        );
        let clone = BackgroundTask::CloneRepository;
        assert_eq!(
            clone.label(Some(" Receiving objects: 12 % ")),
            "Cloning a repository: Receiving objects: 12 %"
        );
        assert_eq!(clone.label(Some("  ")), "Cloning a repository");
        assert_eq!(BackgroundTask::TodoScan.label(None), titles[3]);
        assert_eq!(
            BackgroundTask::FindUsages.stopped(),
            "Stopped: Finding usages"
        );
        // The action has no part of the window: its key acts wherever the focus is.
        assert_eq!(scope_of(STOP_BACKGROUND_PROCESSES), None);
        assert_eq!(scope_of(GOTO_CUSTOM_REGION), None);
        assert_eq!(scope_of(EXPRESSION_TYPE_INFO), None);
        assert_ne!(NO_BACKGROUND_TASKS, NO_CUSTOM_REGIONS);
        assert_eq!(STOP_BACKGROUND_TITLE, "Stop Background Processes");
        assert_eq!(CUSTOM_REGION_TITLE, "Custom Folding");
    }

    /// SPEC-0335 AK-7 (behaviour 16): the changelist actions are operations of the Commit tool
    /// window; their keys of both bundled keymaps run them first there and not at all elsewhere.
    #[test]
    fn changelist_actions_run_in_the_commit_window() {
        use ferris_idea_ui::keymap::{Keymap, Keystroke, Shortcut};
        for (id, op) in [
            (NEW_CHANGELIST, ListOp::New),
            (EDIT_CHANGELIST, ListOp::Edit),
            (SET_ACTIVE_CHANGELIST, ListOp::SetActive),
            (DELETE_CHANGELIST, ListOp::Delete),
            (MOVE_TO_CHANGELIST, ListOp::Move),
        ] {
            assert_eq!(scope_of(id), Some(Scope::Commit), "{id}");
            assert_eq!(commit_op(id), Some(CommitOp::Changelist(op)), "{id}");
        }
        // Lines of one file in different lists are not carried out.
        assert_eq!(commit_op("Vcs.MoveChangedLinesToChangelist"), None);
        let mac = Keymap::bundled_macos();
        let other = Keymap::bundled_default();
        for (keymap, chord, id) in [
            (&mac, "shift meta M", MOVE_TO_CHANGELIST),
            (&other, "shift alt M", MOVE_TO_CHANGELIST),
            (&mac, "F2", EDIT_CHANGELIST),
            (&other, "F2", EDIT_CHANGELIST),
            (&mac, "shift F6", EDIT_CHANGELIST),
            (&other, "shift F6", EDIT_CHANGELIST),
            (&mac, "control SPACE", SET_ACTIVE_CHANGELIST),
            (&other, "control SPACE", SET_ACTIVE_CHANGELIST),
        ] {
            let chord = Keystroke::parse(chord).unwrap();
            assert!(
                keymap.shortcuts(id).iter().any(|s| matches!(
                    s,
                    Shortcut::Keyboard { first, second: None } if *first == chord
                )),
                "{id} {chord:?}"
            );
            let bound = || -> Vec<String> {
                keymap
                    .actions_for(&chord, None)
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            };
            let in_window = order_for_focus(bound(), |s| s == Scope::Commit);
            assert_eq!(in_window.first().map(String::as_str), Some(id), "{chord:?}");
            // In the message field the window around it has the focus too.
            let in_field = order_for_focus(bound(), |s| {
                matches!(s, Scope::Commit | Scope::CommitMessage)
            });
            assert!(in_field.iter().any(|a| a == id), "{chord:?}");
            let elsewhere = order_for_focus(bound(), |_| false);
            assert!(!elsewhere.iter().any(|a| a == id), "{chord:?}");
        }
        // No key for New and Delete.
        for id in [NEW_CHANGELIST, DELETE_CHANGELIST] {
            assert!(mac.shortcuts(id).is_empty() && other.shortcuts(id).is_empty());
        }
    }

    /// SPEC-0275 AK-30 (v13, behaviour 32): `+`, `-` and `*` are the tree node actions only in a
    /// focused tree; elsewhere (the terminal, a field) no action takes them and they are typed.
    #[test]
    fn tree_node_keys_need_a_focused_tree() {
        use ferris_idea_ui::keymap::{Keymap, Keystroke};
        for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
            for (chord, id) in [
                ("ADD", crate::tree_nav::EXPAND_NODE),
                ("SUBTRACT", crate::tree_nav::COLLAPSE_NODE),
                ("MULTIPLY", crate::tree_nav::EXPAND_FULLY),
            ] {
                let bound: Vec<String> = keymap
                    .actions_for(&Keystroke::parse(chord).unwrap(), None)
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
                assert!(bound.iter().any(|a| a == id), "{chord}: {bound:?}");
                let elsewhere = order_for_focus(bound.clone(), |_| false);
                assert!(!elsewhere.iter().any(|a| a == id), "{chord}: {elsewhere:?}");
                let in_tree = order_for_focus(bound, |s| s == Scope::Trees);
                assert_eq!(in_tree.first().map(String::as_str), Some(id), "{chord}");
            }
        }
    }

    /// SPEC-0275 AK-31 (v13): ⌘− is Collapse All in a tree and Collapse in the editor.
    #[test]
    fn expand_all_and_collapse_all_need_a_focused_tree() {
        use ferris_idea_ui::keymap::{Keymap, Keystroke};
        let mac = Keymap::bundled_macos();
        let bound = |text: &str| -> Vec<String> {
            mac.actions_for(&Keystroke::parse(text).unwrap(), None)
                .into_iter()
                .map(str::to_owned)
                .collect()
        };
        for (chord, all, region) in [
            ("meta MINUS", COLLAPSE_ALL, "CollapseRegion"),
            ("meta ADD", EXPAND_ALL, "ExpandRegion"),
            ("meta EQUALS", EXPAND_ALL, "ExpandRegion"),
        ] {
            let actions = bound(chord);
            assert!(actions.iter().any(|a| a == all), "{chord}: {actions:?}");
            let elsewhere = order_for_focus(actions.clone(), |_| false);
            assert!(!elsewhere.iter().any(|a| a == all), "{elsewhere:?}");
            assert!(elsewhere.iter().any(|a| a == region), "{elsewhere:?}");
            let in_tree = order_for_focus(actions, |s| s == Scope::Trees);
            assert_eq!(in_tree[0], all, "{in_tree:?}");
        }
        assert_eq!(scope_of(EXPAND_ALL), Some(Scope::Trees));
        assert_eq!(scope_of(COLLAPSE_ALL), Some(Scope::Trees));
        // ⇧↩ opens in the right split only from the Project tree.
        let enter = bound("shift ENTER");
        assert!(enter.iter().any(|a| a == OPEN_IN_RIGHT_SPLIT), "{enter:?}");
        assert!(
            !order_for_focus(enter.clone(), |_| false)
                .iter()
                .any(|a| a == OPEN_IN_RIGHT_SPLIT)
        );
        assert_eq!(
            order_for_focus(enter, |s| s == Scope::Tree)[0],
            OPEN_IN_RIGHT_SPLIT
        );
    }

    /// SPEC-0275 AK-31 (v13): the menu bar's folding items, taken with their keys, belong to the
    /// tree that has the focus.
    #[test]
    fn the_menu_bars_folding_keys_reach_a_focused_tree() {
        assert_eq!(for_focused_tree("CollapseRegion"), Some(COLLAPSE_ALL));
        assert_eq!(for_focused_tree("ExpandRegion"), Some(EXPAND_ALL));
        for other in [
            "CollapseAllRegions",
            "ExpandAllRegions",
            EXPAND_ALL,
            "$Copy",
        ] {
            assert_eq!(for_focused_tree(other), None, "{other}");
        }
    }

    /// SPEC-0275 AK-32 (v13).
    #[test]
    fn file_path_shows_the_active_file_or_the_trees_selection() {
        let (active, selected) = (Path::new("/w/src/a.rs"), Path::new("/w/docs"));
        assert_eq!(
            file_path_target(false, Some(active), Some(selected)),
            Some(active.to_path_buf())
        );
        assert_eq!(
            file_path_target(true, Some(active), Some(selected)),
            Some(selected.to_path_buf())
        );
        // An extra node of the tree has no path: the active file is shown.
        assert_eq!(
            file_path_target(true, Some(active), None),
            Some(active.to_path_buf())
        );
        assert_eq!(file_path_target(false, None, Some(selected)), None);
        assert_eq!(file_path_target(true, None, None), None);
    }

    /// SPEC-0275 AK-32 (v13).
    #[test]
    fn new_in_the_same_place_takes_the_folder_of_the_active_file() {
        assert_eq!(
            same_place_folder(Some(Path::new("/w/src/inner/a.rs"))),
            Some(PathBuf::from("/w/src/inner"))
        );
        assert_eq!(same_place_folder(None), None);
    }

    /// SPEC-0334 AK-10: the grouping actions of the TODO window name a button of its toolbar
    /// and belong to its focus; on a shared chord the focused window's action is the one that
    /// runs.
    #[test]
    fn todo_grouping_actions_name_their_toggle() {
        for (id, toggle) in [
            ("TodoViewGroupByShowModules", "group-modules"),
            ("TodoViewGroupByShowPackages", "group-packages"),
            ("TodoViewGroupByFlattenPackage", "flatten"),
        ] {
            assert_eq!(scope_of(id), Some(Scope::Todo), "{id}");
            assert_eq!(todo_grouping(id), Some(toggle), "{id}");
            let name = format!("todo-{toggle}");
            let control = ferris_idea_ui::tooltip::TODO_TOOLBAR
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("{name} is a button"));
            assert_eq!(control.action, Some(id), "{name}");
        }
        assert_eq!(todo_grouping("ActivateTODOToolWindow"), None);
        assert_eq!(scope_of("ActivateTODOToolWindow"), None);
        let m = ids(&[
            "ChangesView.GroupBy.Module",
            "UsageGrouping.Module",
            "TodoViewGroupByShowModules",
        ]);
        assert_eq!(order_for_focus(m.clone(), |_| false), ids(&[]));
        assert_eq!(
            order_for_focus(m.clone(), |s| s == Scope::Todo),
            ids(&["TodoViewGroupByShowModules"])
        );
        assert_eq!(
            order_for_focus(m, |s| s == Scope::Find),
            ids(&["UsageGrouping.Module"])
        );
        // Their chords in the macOS keymap, with what else is bound there.
        let mac = ferris_idea_ui::keymap::Keymap::bundled_macos();
        for (chord, id) in [
            ("control M", "TodoViewGroupByShowModules"),
            ("control P", "TodoViewGroupByShowPackages"),
            ("control F", "TodoViewGroupByFlattenPackage"),
        ] {
            let stroke = ferris_idea_ui::keymap::Keystroke::parse(chord).expect("a chord");
            let all: Vec<String> = mac
                .actions_for(&stroke, None)
                .into_iter()
                .map(str::to_owned)
                .collect();
            assert!(all.iter().any(|a| a == id), "{chord}: {all:?}");
            let focused = order_for_focus(all.clone(), |s| s == Scope::Todo);
            assert_eq!(focused.first().map(String::as_str), Some(id), "{chord}");
            assert!(
                !order_for_focus(all, |_| false).iter().any(|a| a == id),
                "{chord}"
            );
        }
    }

    /// SPEC-0293 AK-11 (v6).
    #[test]
    fn usage_grouping_actions_name_their_toggle() {
        let toggles: Vec<&str> = [
            "UsageGrouping.UsageType",
            "UsageGrouping.Module",
            "UsageGrouping.Directory",
            "UsageGrouping.FileStructure",
        ]
        .into_iter()
        .map(|id| {
            assert_eq!(scope_of(id), Some(Scope::Find), "{id}");
            usage_grouping(id).unwrap()
        })
        .collect();
        assert_eq!(
            toggles,
            [
                "group-kind",
                "group-module",
                "group-package",
                "group-structure"
            ]
        );
        // Every toggle is a button of the window's toolbar.
        for toggle in toggles {
            let name = format!("find-usages-{toggle}");
            assert!(
                ferris_idea_ui::tooltip::FIND_USAGES_TOOLBAR
                    .iter()
                    .any(|c| c.name == name),
                "{name}"
            );
        }
    }

    /// SPEC-0080 AK-5 (v3): Next and Prev Parameter go to the starts of the arguments of the
    /// innermost call around the caret.
    #[test]
    fn parameter_steps_go_to_the_starts_of_arguments() {
        // The caret is where `|` stands; the result is shown the same way.
        let step = |marked: &str, forward: bool| -> Option<String> {
            let caret = marked.chars().position(|c| c == '|').unwrap();
            let text: String = marked.chars().filter(|c| *c != '|').collect();
            parameter_step(&text, caret, forward).map(|to| {
                let mut out: Vec<char> = text.chars().collect();
                out.insert(to, '|');
                out.into_iter().collect()
            })
        };
        let some = |s: &str| Some(s.to_owned());
        assert_eq!(step("f(a|, bb, c)", true), some("f(a, |bb, c)"));
        assert_eq!(step("f(a, b|b, c)", true), some("f(a, bb, |c)"));
        assert_eq!(step("f(a, bb, |c)", true), None, "no further argument");
        assert_eq!(step("f(a, bb, c|)", false), some("f(a, |bb, c)"));
        assert_eq!(step("f(a, |bb, c)", false), some("f(|a, bb, c)"));
        assert_eq!(step("f(|a, bb, c)", false), None);
        // One argument, none, and no call at all.
        assert_eq!(step("f(a|)", true), None);
        assert_eq!(step("f(|)", true), None);
        assert_eq!(step("f( | )", false), None);
        assert_eq!(step("let x| = 1;", true), None);
        assert_eq!(step("f(a)|", true), None, "behind the call");
        // The innermost call; nested brackets and commas in them belong to one argument.
        assert_eq!(step("f(g(x|, y), z)", true), some("f(g(x, |y), z)"));
        assert_eq!(step("f(g(x, y|), z)", true), None);
        assert_eq!(step("f(g(x, y)|, z)", true), some("f(g(x, y), |z)"));
        assert_eq!(step("f([1, 2|], z)", true), some("f([1, 2], |z)"));
        assert_eq!(step("f(a, |m[i, j], z)", true), some("f(a, m[i, j], |z)"));
        // A comma in a string literal separates nothing; an apostrophe alone is no literal.
        assert_eq!(step("f(|\"a, b\", c)", true), some("f(\"a, b\", |c)"));
        assert_eq!(step("f(|'a', b)", true), some("f('a', |b)"));
        assert_eq!(step("f::<'a>(|x, y)", true), some("f::<'a>(x, |y)"));
        // An empty place after a comma is an argument; the call may still be open.
        assert_eq!(step("f(a|, )", true), some("f(a, |)"));
        assert_eq!(step("f(a|,\n  b", true), some("f(a,\n  |b"));
        // The keys are the keymap's.
        assert_eq!(parameter_action(&["EditorTab", NEXT_PARAMETER]), Some(true));
        assert_eq!(parameter_action(&[PREV_PARAMETER]), Some(false));
        assert_eq!(parameter_action(&["EditorTab"]), None);
        let mac = ferris_idea_ui::keymap::Keymap::bundled_macos();
        let k = |text: &str| ferris_idea_ui::keymap::Keystroke::parse(text).unwrap();
        assert_eq!(
            parameter_action(&mac.actions_for(&k("TAB"), None)),
            Some(true)
        );
        assert_eq!(
            parameter_action(&mac.actions_for(&k("shift TAB"), None)),
            Some(false)
        );
    }

    /// SPEC-0094 AK-15 (v6, behaviour 17): every window is in the cycle once, project windows
    /// first in opening order.
    #[test]
    fn every_window_is_in_the_cycle_once() {
        // Project windows 3, 1 and 5 were opened in this order; 5 is closed, 9 is another one.
        assert_eq!(window_cycle(&[3, 1, 5], &[1, 9, 3]), [3, 1, 9]);
        assert_eq!(window_cycle(&[], &[2, 4]), [2, 4]);
        assert_eq!(window_cycle::<u8>(&[7], &[]), Vec::<u8>::new());
        let all = window_cycle(&[3, 1], &[1, 9, 3]);
        use crate::run_debug_actions::step_window;
        assert_eq!(step_window(&all, 1, true), Some(9));
        assert_eq!(step_window(&all, 9, true), Some(3), "wraps");
        assert_eq!(step_window(&all, 3, false), Some(9));
        assert_eq!(step_window(&[4], 4, true), None);
    }

    /// SPEC-0059 AK-16 (v9.6): scratch files get the first free name; nothing is overwritten.
    #[test]
    fn scratch_files_get_the_next_free_name() {
        assert_eq!(scratch_name("py", |_| false), "scratch.py");
        assert_eq!(scratch_name("py", |n| n == "scratch.py"), "scratch_1.py");
        assert_eq!(
            scratch_name("py", |n| n == "scratch.py" || n == "scratch_1.py"),
            "scratch_2.py"
        );
        assert_eq!(SCRATCH_KINDS[0], ("Plain text", "txt"));
        assert_eq!(scratch_kind_for(None), 0);
        assert_eq!(scratch_kind_for(Some(Path::new("/p/a.unknown"))), 0);
        let python = scratch_kind_for(Some(Path::new("/p/tool.PY")));
        assert_eq!(SCRATCH_KINDS[python], ("Python", "py"));
        // Every extension once.
        let mut extensions: Vec<&str> = SCRATCH_KINDS.iter().map(|k| k.1).collect();
        extensions.sort_unstable();
        extensions.dedup();
        assert_eq!(extensions.len(), SCRATCH_KINDS.len());

        let dir = std::env::temp_dir().join(format!("ferris-idea-scratch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = create_scratch(&dir.join("scratches"), "sql").unwrap();
        assert_eq!(first.file_name().unwrap(), "scratch.sql");
        std::fs::write(&first, "select 1;").unwrap();
        let second = create_scratch(&dir.join("scratches"), "sql").unwrap();
        assert_eq!(second.file_name().unwrap(), "scratch_1.sql");
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "select 1;");
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "");
        assert_eq!(tab_list_title("Terminal"), "Tabs of Terminal");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SPEC-0293 AK-14 (v9): the filters, nested folders, module groups, Exclude, Include and
    /// the export are actions of the Find tool window, each with a button of its toolbar or a
    /// key of the bundled keymaps.
    #[test]
    fn usage_view_actions_name_their_change() {
        let changes: Vec<&str> = [
            "UsageFiltering.ReadAccess",
            "UsageFiltering.WriteAccess",
            "UsageFiltering.Imports",
            "UsageGrouping.DirectoryStructure",
            "UsageGrouping.FlattenModules",
        ]
        .into_iter()
        .map(|id| {
            assert_eq!(scope_of(id), Some(Scope::Find), "{id}");
            assert_eq!(usage_grouping(id), None, "{id}");
            usage_view_change(id).unwrap()
        })
        .collect();
        assert_eq!(
            changes,
            [
                "show-read",
                "show-write",
                "show-imports",
                "group-dirs",
                "flatten-modules"
            ]
        );
        for change in changes {
            let name = format!("find-usages-{change}");
            let control = ferris_idea_ui::tooltip::FIND_USAGES_TOOLBAR
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("{name}"));
            assert!(control.action.is_some(), "{name}");
        }
        assert_eq!(usage_view_change(USAGE_EXCLUDE), Some("exclude"));
        assert_eq!(usage_view_change(USAGE_INCLUDE), Some("include"));
        for id in [USAGE_EXCLUDE, USAGE_INCLUDE, EXPORT_TO_TEXT_FILE] {
            assert_eq!(scope_of(id), Some(Scope::Find), "{id}");
        }
        // The keys: the bundled macOS keymap binds the filters and groupings to ⌃R, ⌃W, ⌃I,
        // ⌃D and ⌃O, Include to ⇧⌫; Exclude has Delete and Backspace from Ferris IDEA.
        let mac = ferris_idea_ui::keymap::Keymap::bundled_macos();
        let k = |text: &str| ferris_idea_ui::keymap::Keystroke::parse(text).unwrap();
        let is_shortcut_of = |keymap: &ferris_idea_ui::keymap::Keymap, id: &str, key| {
            keymap.actions_for(&key, None).contains(&id)
        };
        for (id, key) in [
            ("UsageFiltering.ReadAccess", "control R"),
            ("UsageFiltering.WriteAccess", "control W"),
            ("UsageFiltering.Imports", "control I"),
            ("UsageGrouping.DirectoryStructure", "control D"),
            ("UsageGrouping.FlattenModules", "control O"),
            (EXPORT_TO_TEXT_FILE, "control O"),
            (USAGE_INCLUDE, "shift BACK_SPACE"),
            (USAGE_EXCLUDE, "BACK_SPACE"),
            (USAGE_EXCLUDE, "DELETE"),
        ] {
            assert!(is_shortcut_of(&mac, id, k(key)), "{id} on {key}");
        }
        // In the Find tool window its actions go first; elsewhere they are not tried at all.
        let on = |focused: bool| {
            order_for_focus(
                vec!["Run".into(), "UsageFiltering.ReadAccess".into()],
                |s| focused && s == Scope::Find,
            )
        };
        assert_eq!(on(true), ["UsageFiltering.ReadAccess", "Run"]);
        assert_eq!(on(false), ["Run"]);
        assert!(usage_view_passes_on("UsageGrouping.FlattenModules"));
        assert!(!usage_view_passes_on(USAGE_EXCLUDE));
        assert_ne!(
            usage_view_unchanged(USAGE_EXCLUDE),
            usage_view_unchanged(USAGE_INCLUDE)
        );
        assert_eq!(export_file_name("apply(int)"), "usages-of-apply_int.txt");
        assert_eq!(export_file_name("?"), "usages.txt");
        assert_eq!(
            exported(2, Path::new("/tmp/u.txt")),
            "Exported 2 usages to /tmp/u.txt"
        );
    }

    /// SPEC-0081 AK-6 (v4).
    #[test]
    fn quick_definition_blocks_name_each_target() {
        use crate::doc_model::DocBlock;
        let target = |i: usize, truncated: bool| QuickTarget {
            shown_path: format!("src/Impl{i}.java"),
            source: Source {
                line: 4 + i,
                end_line: 6 + i,
                text: format!("int apply{i}() {{\n    return {i};\n}}"),
                truncated,
            },
        };
        assert_eq!(
            quick_definition_blocks(&[target(0, false)]),
            [
                DocBlock::Owner("src/Impl0.java:5".into()),
                DocBlock::Code("int apply0() {\n    return 0;\n}".into()),
            ]
        );
        let seven: Vec<QuickTarget> = (0..7).map(|i| target(i, i == 1)).collect();
        let blocks = quick_definition_blocks(&seven);
        // Five targets with four rules between them, and the count of the rest.
        assert_eq!(blocks.len(), 5 * 2 + 4 + 1);
        assert_eq!(blocks[2], DocBlock::Rule);
        assert_eq!(blocks[3], DocBlock::Owner("src/Impl1.java:6".into()));
        assert_eq!(
            blocks[4],
            DocBlock::Code("int apply1() {\n    return 1;\n}\n…".into())
        );
        assert_eq!(
            blocks.last(),
            Some(&DocBlock::Footer(
                "2 more not shown; Go to Implementation(s) lists all".into()
            ))
        );
        assert!(quick_definition_blocks(&[]).is_empty());
        assert_eq!(
            quick_definition_nothing(&[]),
            "Cannot find a definition to show."
        );
        assert!(quick_definition_nothing(&["java.util.List".into()]).contains("java.util.List"));
    }

    /// SPEC-0071 AK-11 and AK-13 (v10).
    #[test]
    fn goto_test_titles_and_messages() {
        assert_eq!(
            goto_test_title("Cart.java", false, 2),
            "Choose test for Cart.java (2)"
        );
        assert_eq!(
            goto_test_title("CartTest.java", true, 2),
            "Choose test subject for CartTest.java (2)"
        );
        assert_eq!(
            goto_test_nothing("Cart.java", Some(false)),
            "No test found for Cart.java"
        );
        assert_eq!(
            goto_test_nothing("CartTest.java", Some(true)),
            "No test subject found for CartTest.java"
        );
        assert_eq!(
            goto_test_nothing("cart.rs", None),
            "Go to Test knows no naming rule for cart.rs"
        );
        assert_eq!(
            related_title("index.html", 2),
            "Choose related file of index.html (2)"
        );
        assert_eq!(
            related_nothing("index.html", true),
            "No related files found for index.html"
        );
        assert_eq!(
            related_nothing("main.rs", false),
            "No related files are known for main.rs"
        );
        // The rows of a chooser keep the order given and show each file's first line.
        let dir = Scratch::new("file-rows");
        let b = dir.path().join("b_test.py");
        let a = dir.path().join("sub").join("a_test.py");
        std::fs::create_dir_all(a.parent().unwrap()).unwrap();
        std::fs::write(&b, "import cart\n").unwrap();
        std::fs::write(&a, "  # tests\n").unwrap();
        let rows = file_rows(dir.path(), &[b.clone(), a.clone()]);
        assert_eq!(
            rows.iter()
                .map(|r| (r.shown_path.as_str(), r.text.as_str(), r.place.line))
                .collect::<Vec<_>>(),
            [
                ("b_test.py", "import cart", 0),
                ("sub/a_test.py", "# tests", 0)
            ]
        );
        assert_eq!(rows[1].place.path, a);
    }

    /// SPEC-0069 AK-5 (v3).
    #[test]
    fn the_edited_only_key_is_the_actions_shortcut() {
        use ferris_idea_ui::keymap::{Keymap, Keystroke};
        let k = |text: &str| Keystroke::parse(text).unwrap();
        let mac = Keymap::bundled_macos();
        assert!(is_shortcut_of(&mac, EDITED_ONLY_ACTION, &k("meta E")));
        // ⇧⌘E is Recent Locations, ⌘L another action's key.
        assert!(!is_shortcut_of(
            &mac,
            EDITED_ONLY_ACTION,
            &k("shift meta E")
        ));
        assert!(!is_shortcut_of(&mac, EDITED_ONLY_ACTION, &k("meta L")));
        // The chord that opens Recent Files, in either keymap.
        assert!(is_shortcut_of(&mac, "RecentFiles", &k("meta E")));
        let default = Keymap::bundled_default();
        assert!(is_shortcut_of(
            &default,
            EDITED_ONLY_ACTION,
            &k("control E")
        ));
        assert!(!is_shortcut_of(&default, EDITED_ONLY_ACTION, &k("meta E")));
        // Recently Changed Files has no key in the bundled keymaps.
        assert!(mac.shortcuts("RecentChangedFiles").is_empty());
        assert!(default.shortcuts("RecentChangedFiles").is_empty());
    }

    /// SPEC-0063 AK-11.
    #[test]
    fn commit_actions_name_their_operation() {
        assert_eq!(commit_op("Diff.ShowDiff"), Some(CommitOp::ShowDiff));
        assert_eq!(
            commit_op("Vcs.ToggleAmendCommitMode"),
            Some(CommitOp::ToggleAmend)
        );
        assert_eq!(
            commit_op("ChangesView.GroupBy.Directory"),
            Some(CommitOp::GroupByDirectory)
        );
        assert_eq!(
            commit_op("ChangesView.GroupBy.Module"),
            Some(CommitOp::GroupByModule)
        );
        assert_eq!(
            commit_op("ChangesView.ShowCommitOptions"),
            Some(CommitOp::Options)
        );
        // Changelists of whole files are carried out since SPEC-0335; lines of one file in
        // different lists are not.
        assert_eq!(
            commit_op("ChangesView.Move"),
            Some(CommitOp::Changelist(ListOp::Move))
        );
        assert_eq!(commit_op("Vcs.MoveChangedLinesToChangelist"), None);
        for id in [
            "Diff.ShowDiff",
            "Vcs.ToggleAmendCommitMode",
            "ChangesView.GroupBy.Directory",
            "ChangesView.GroupBy.Module",
            "ChangesView.ShowCommitOptions",
        ] {
            assert_eq!(scope_of(id), Some(Scope::Commit), "{id}");
        }
    }

    /// SPEC-0086 AK-33.
    #[test]
    fn log_actions_name_their_menu_entry() {
        assert_eq!(
            log_entry("Git.Reword.Commit"),
            Some(CommitAction::EditMessage)
        );
        assert_eq!(
            log_entry("Git.New.Branch.In.Log"),
            Some(CommitAction::NewBranch)
        );
        assert_eq!(log_entry("Git.CreateNewBranch"), None);
        assert_eq!(scope_of("Git.Reword.Commit"), Some(Scope::Log));
        assert_eq!(scope_of("Git.New.Branch.In.Log"), Some(Scope::Log));
    }

    /// SPEC-0084 AK-10.
    #[test]
    fn compare_pair_is_two_files_in_tree_order() {
        let dir = Scratch::new("compare");
        let a = dir.path().join("a.txt");
        let b = dir.path().join("sub").join("a.txt");
        let c = dir.path().join("c.txt");
        std::fs::create_dir_all(b.parent().unwrap()).unwrap();
        for f in [&a, &b, &c] {
            std::fs::write(f, "x\n").unwrap();
        }
        assert_eq!(
            compare_pair(&[a.clone(), c.clone()]),
            Ok((a.clone(), c.clone()))
        );
        // One file, three files and a folder are refused.
        assert_eq!(
            compare_pair(std::slice::from_ref(&a)),
            Err(COMPARE_NEEDS_TWO)
        );
        assert_eq!(
            compare_pair(&[a.clone(), b.clone(), c.clone()]),
            Err(COMPARE_NEEDS_TWO)
        );
        assert_eq!(
            compare_pair(&[a.clone(), dir.path().join("sub")]),
            Err(COMPARE_NEEDS_TWO)
        );
        // Names, or relative paths when the names are equal.
        assert_eq!(
            compare_titles(dir.path(), &a, &c),
            ("a.txt".to_owned(), "c.txt".to_owned())
        );
        let (left, right) = compare_titles(dir.path(), &a, &b);
        assert_eq!(left, "a.txt");
        assert_eq!(PathBuf::from(right), Path::new("sub").join("a.txt"));
    }

    /// SPEC-0059 AK-14.
    #[test]
    fn save_as_starts_at_the_file() {
        let dir = Scratch::new("save-as");
        let file = dir.path().join("notes.md");
        std::fs::write(&file, "").unwrap();
        assert_eq!(
            save_as_start(&file, Path::new("/project")),
            (dir.path().to_path_buf(), "notes.md".to_owned())
        );
        // A file whose folder is gone starts in the project folder.
        assert_eq!(
            save_as_start(&dir.path().join("gone").join("x.rs"), dir.path()),
            (dir.path().to_path_buf(), "x.rs".to_owned())
        );
    }

    /// SPEC-0167 AK-8.
    #[test]
    fn last_tool_window_is_the_one_focused_last() {
        let mut last = LastToolWindow::default();
        assert_eq!(last.get(), None);
        last.observe(None);
        assert_eq!(last.get(), None);
        last.observe(Some(ToolWindow::Terminal));
        last.observe(None);
        assert_eq!(last.get(), Some(ToolWindow::Terminal));
        last.observe(Some(ToolWindow::Project));
        last.observe(None);
        assert_eq!(last.get(), Some(ToolWindow::Project));
    }
}
