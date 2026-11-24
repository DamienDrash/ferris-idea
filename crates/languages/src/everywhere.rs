//! Search everywhere over files, symbols and actions (`SPEC-0031`), shared by the window and MCP.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use ferris_idea_core::{hump_match, hump_words_match, rank};
use ferris_idea_ui::keymap::Keymap;
use serde::{Deserialize, Serialize};

use crate::LanguageId;
use crate::syntax::{Definition, SyntaxDocument};

/// Largest file scanned for symbols (behaviour 5).
const MAX_SYMBOL_FILE: u64 = 1024 * 1024;

/// A file's size and modification time: its cached definitions are valid while both stay.
type Stamp = (u64, Option<SystemTime>);

/// Definitions per absolute path, kept between queries (`SPEC-0260` behaviour 5).
type DefinitionCache = HashMap<PathBuf, (Stamp, Arc<Vec<Definition>>)>;

/// Entries kept at most; the cache starts over above this (many projects in one session).
const MAX_CACHED_FILES: usize = 200_000;

fn cache() -> &'static Mutex<DefinitionCache> {
    static CACHE: OnceLock<Mutex<DefinitionCache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

/// The definitions of each of `files` (`None` for files without a grammar, too large or
/// unreadable). Files unchanged since an earlier query come from the cache; the others are parsed
/// on all cores. Stops early when `cancel` is set.
fn definitions(
    root: &Path,
    files: &[PathBuf],
    cancel: Option<&AtomicBool>,
) -> Vec<Option<Arc<Vec<Definition>>>> {
    let cancelled = || cancel.is_some_and(|c| c.load(Ordering::Relaxed));
    let mut out: Vec<Option<Arc<Vec<Definition>>>> = vec![None; files.len()];
    // (index, absolute path, language, tsx, stamp) of each file to parse.
    let mut missing = Vec::new();
    {
        let cache = cache().lock().unwrap_or_else(|e| e.into_inner());
        for (i, f) in files.iter().enumerate() {
            let full = root.join(f);
            let Ok(meta) = std::fs::metadata(&full) else {
                continue;
            };
            let Some(language) = LanguageId::detect(f).filter(|_| meta.len() <= MAX_SYMBOL_FILE)
            else {
                continue;
            };
            let stamp = (meta.len(), meta.modified().ok());
            match cache.get(&full) {
                Some((s, d)) if *s == stamp => out[i] = Some(d.clone()),
                _ => {
                    let tsx = f.extension().is_some_and(|e| e == "tsx");
                    missing.push((i, full, language, tsx, stamp));
                }
            }
        }
    }
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
    let chunk = missing.len().div_ceil(threads).max(1);
    let parsed: Vec<(usize, PathBuf, Stamp, Arc<Vec<Definition>>)> = std::thread::scope(|scope| {
        let workers: Vec<_> = missing
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    let mut done = Vec::new();
                    for (i, full, language, tsx, stamp) in part {
                        if cancelled() {
                            break;
                        }
                        let Ok(text) = std::fs::read_to_string(full) else {
                            continue;
                        };
                        let Ok(doc) = SyntaxDocument::new(*language, *tsx, &text) else {
                            continue;
                        };
                        done.push((*i, full.clone(), *stamp, Arc::new(doc.definitions())));
                    }
                    done
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap_or_default())
            .collect()
    });
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() + parsed.len() > MAX_CACHED_FILES {
        cache.clear();
    }
    for (i, full, stamp, defs) in parsed {
        out[i] = Some(defs.clone());
        cache.insert(full, (stamp, defs));
    }
    out
}

/// Which categories to search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Categories {
    /// Workspace files.
    pub files: bool,
    /// Class-like definitions only: classes, interfaces, enums, structs, traits, types (`SPEC-0031`
    /// v4 behaviour 8).
    pub classes: bool,
    /// Definitions from the tags queries.
    pub symbols: bool,
    /// Keymap actions.
    pub actions: bool,
    /// Lines of workspace files containing the query (v4 behaviour 9).
    pub text: bool,
}

impl Categories {
    /// No category; spread it with the wanted ones (`..Categories::NONE`).
    pub const NONE: Self = Self {
        files: false,
        classes: false,
        symbols: false,
        actions: false,
        text: false,
    };

    /// Files, symbols, actions and text: the "All" tab.
    pub const ALL: Self = Self {
        files: true,
        classes: false,
        symbols: true,
        actions: true,
        text: true,
    };
}

/// A file found by name or path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileHit {
    /// Workspace-relative path.
    pub path: String,
    /// Char positions of the file name (not the path) that matched, for highlighting.
    #[serde(default, skip_serializing)]
    pub positions: Vec<usize>,
}

/// A symbol definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolHit {
    /// Name of the symbol.
    pub name: String,
    /// `class`, `function`, `method`, …
    pub kind: String,
    /// Workspace-relative path.
    pub path: String,
    /// 0-based line.
    pub line: usize,
    /// Where the symbol lives, as the reference names it: the Java package (`lab.shop.model`), the
    /// Python module (`py.shop.order`), else the file's path (v4 behaviour 5).
    pub container: String,
    /// Parameter list of a function or method from its line: `(String, long)` for Java (types),
    /// `(id, totalCents)` elsewhere (names); `None` for other kinds or when it spans lines.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<String>,
    /// Char positions of the name that matched the query, for highlighting.
    #[serde(default, skip_serializing)]
    pub positions: Vec<usize>,
}

/// A line of a workspace file that contains the query (v4 behaviour 9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextHit {
    /// Workspace-relative path.
    pub path: String,
    /// 0-based line.
    pub line: usize,
    /// 0-based char column of the match.
    pub column: usize,
    /// Length of the match in chars (on this line).
    pub length: usize,
    /// The line's text, without the line break.
    pub text: String,
}

/// Actions Ferris IDEA carries out that have no shortcut in the keymaps, so Search Everywhere still finds
/// them (`SPEC-0085`).
pub const UNBOUND_ACTIONS: &[&str] = &[
    // Add Comment of the project room (SPEC-0323 v8 behaviour 54): the editor's menu only.
    "Room.AddComment",
    // Folding the documentation comments has no key in the keymaps (SPEC-0083 v8 behaviour 23).
    "ExpandDocComments",
    "CollapseDocComments",
    // Cut up to Line End has a key in the macOS keymap only (SPEC-0057 v6 behaviour 24).
    "EditorCutLineEnd",
    // The file properties (SPEC-0336 behaviour 20): no keymap binds them.
    "ChangeFileEncodingAction",
    "ToggleReadOnlyAttribute",
    "ConvertToWindowsLineSeparators",
    "ConvertToUnixLineSeparators",
    "ConvertToMacLineSeparators",
    "AddBom",
    "RemoveBom",
    // View › Compare with Clipboard (SPEC-0124 v18 behaviour 20): no keymap binds it.
    "CompareClipboardWithSelection",
    // The file type actions (SPEC-0004 v5 behaviour 13): no keymap binds them.
    "OverrideFileTypeAction",
    "ReverteOverrideFileTypeAction",
    "AssociateWithFileType",
    // The changelist actions the keymaps bind no key to (SPEC-0335 behaviour 16).
    "ChangesView.NewChangeList",
    "ChangesView.RemoveChangeList",
    // Activate Next and Previous Window have keys in the macOS keymap only (SPEC-0094 v6
    // behaviour 17).
    "NextWindow",
    "PreviousWindow",
    // Closing project windows (SPEC-0094 v3 behaviour 6).
    "CloseProject",
    "CloseAllProjects",
    "CloseOtherProjects",
    // The named tool window layouts (SPEC-0333 behaviour 13).
    "RestoreFactoryDefaultLayout",
    "StoreDefaultLayout",
    "StoreNewLayout",
    "LocalHistory.ShowHistory",
    // The small editing commands the keymaps bind no key to (SPEC-0332 behaviour 15).
    "EditorSortLines",
    "EditorReverseLines",
    "EditorTranspose",
    "ConvertIndentsToSpaces",
    "ConvertIndentsToTabs",
    "CopyReferencePopupGroup",
    "CopyAbsolutePath",
    "CopyFileName",
    "CopyContentRootPath",
    // The path with the caret's line and from the source folder (SPEC-0332 v2 behaviour 15).
    "CopyPathWithLineNumber",
    "CopySourceRootPath",
    // Run › Edit Configurations… (SPEC-0327 behaviour 1).
    "editRunConfigurations",
    // Full screen has a key in the macOS keymap only (SPEC-0061 v12 behaviour 58).
    "ToggleFullScreen",
    // Delegate Methods (SPEC-0122 v4 behaviour 10).
    "DelegateMethods",
    // Next and previous usage have a key in the macOS keymap only (SPEC-0295 v2 behaviour 15).
    "GotoNextElementUnderCaretUsage",
    "GotoPrevElementUnderCaretUsage",
    // Next Edit Location (SPEC-0071 v9 behaviour 14).
    "JumpToNextChange",
    // The next and previous bookmark of the file (SPEC-0331 behaviour 19).
    "GotoNextBookmark",
    "GotoPreviousBookmark",
    // Recently Changed Files (SPEC-0069 v3 behaviour 9).
    "RecentChangedFiles",
    // The TODO tool window: the keymaps bind no key to it (SPEC-0334 behaviour 13).
    "ActivateTODOToolWindow",
    // The Notifications tool window: the keymaps bind no key to it (SPEC-0061 v17 behaviour 64).
    "ActivateNotificationsToolWindow",
    // Class refactorings (SPEC-0320).
    "ExtractInterface",
    "ExtractSuperclass",
    "MembersPullUp",
    // Annotate with Git Blame (SPEC-0277 behaviour 1).
    "Annotate",
    "ActivateDatabaseToolWindow",
    "InspectCode",
    "Terminal.NewTab",
    "ActivateEndpointsToolWindow",
    "Git.ResolveConflicts",
    "ActivateMavenToolWindow",
    "ActivateGradleToolWindow",
    "ActivatenpmToolWindow",
    "Debugger.AddToWatch",
    // SPEC-0194 v3 behaviour 8.
    "XDebugger.MuteBreakpoints",
    // SPEC-0127 v15 behaviours 31 and 32.
    "ToggleBreakpointEnabled",
    "Debugger.RemoveAllBreakpoints",
    "Debugger.RemoveAllBreakpointsInFile",
    "Git.Stash",
    "Git.Unstash",
    "Git.Interactive.Rebase",
    "Git.Continue",
    "Git.Abort",
    "Git.Skip",
    "Vcs.Push",
    "Vcs.UpdateProject",
    "Git.Fetch",
    "Git.Branches",
    "ChangeLaf",
    "Sql.ExplainPlan",
    "Sql.ExplainAnalyze",
    "Sql.CompareSchemas",
    "Sql.CompareObjects",
    "Database.ReportProblem",
    "Database.ExportConnection",
    "Sql.OpenMigrationScript",
    "Sql.SwapComparison",
    "Sql.CancelComparison",
    "Sql.IncludeDestructiveChanges",
    "Sql.ShowErDiagram",
    "Sql.ExportData",
    // Menu actions without a shortcut in one of the keymaps (IMP-0013).
    "OpenFile",
    "ManageRecentProjects",
    "ToggleCompactMode",
    "UpdateCopyright",
    "DebugClass",
    "Pause",
    "Git.Pull",
    "Terminal.SplitVertically",
    "Terminal.SplitHorizontally",
    "Terminal.RenameSession",
    "Console.Open",
    "Sql.ChooseDataSource",
    "Sql.AskAgainForUnsafeChanges",
    "SplitVertically",
    "SplitHorizontally",
    "Unsplit",
    "UnsplitAll",
    "SplitAndMoveRight",
    "SplitAndMoveDown",
    "MoveEditorToOppositeTabGroup",
    // Editor tabs and groups (SPEC-0059 v9 behaviour 19, SPEC-0112 v5).
    "NextSplitter",
    "PrevSplitter",
    "ReopenClosedTab",
    "PinActiveEditorTab",
    "CloseAllEditorsButActive",
    "CloseAllEditors",
    "CloseAllToTheLeft",
    "CloseAllToTheRight",
    "CloseAllUnmodifiedEditors",
    "CloseAllUnpinnedEditors",
    "Switcher",
    "TabList",
    "MaximizeEditorInSplit",
    "FerrisIDEA.ConfigureEditorTabs",
    "ViewToolButtons",
    "Spring.ToolWindow",
    "Spring.GotoBean",
    "FerrisIDEA.AiChat",
    "FerrisIDEA.AiExplainCode",
    "FerrisIDEA.AiExplainTerminal",
    "FerrisIDEA.AiAttachTerminalSelection",
    "FerrisIDEA.AiAttachTerminalCommand",
    "FerrisIDEA.AiAttachTerminalLines",
    "FerrisIDEA.AiShareTerminal",
    "FerrisIDEA.AiExplainRun",
    "FerrisIDEA.AiAttachRun",
    "FerrisIDEA.AiExplainTest",
    "FerrisIDEA.AiAttachTest",
    "FerrisIDEA.AiAttachProblems",
    "FerrisIDEA.AiAttachGitDiff",
    "ActivatePythonPackagesToolWindow",
    "Python.SelectInterpreter",
    "Python.CreateVirtualEnv",
    "Containers.ToolWindow",
    "Profiler.ToolWindow",
    "Profiler.ProfileSelected",
    "Profiler.ProfileMemory",
    "Profiler.AttachToProcess",
    "Remote.ToolWindow",
    "Remote.OpenTerminal",
    // Remote Development… (SPEC-0317 v3 behaviour 1).
    "Remote.OpenProject",
    // Share Project… (SPEC-0323 v6 behaviour 37).
    "Remote.ShareProject",
    // Join Project by Link… (SPEC-0323 v7 behaviour 48).
    "Remote.JoinByLink",
    // Room History… (SPEC-0323 v9 behaviour 61).
    "Room.ShowHistory",
    "Javascript.Linters.EsLint.Fix",
    "ActivatePluginsToolWindow",
    "OpenResourceBundleEditor",
    "FerrisIDEA.KeyboardDiagnostics",
    // File › Plugins… (SPEC-0124 v8 behaviour 11).
    "FerrisIDEA.SettingsPlugins",
    // File › Manage IDE Settings (SPEC-0105 v9 behaviour 14).
    "ImportSettings",
    "ExportSettings",
    // The emergency stop of UI automation (SPEC-0319 behaviour 37).
    "FerrisIDEA.StopUiAutomation",
    // Every request of a `.http` file (SPEC-0096 v5 behaviour 8).
    "FerrisIDEA.HttpRunAllRequests",
    "About",
    // Window › Minimize and Zoom have keys only in the macOS keymap (SPEC-0124 v3 behaviour 6).
    "MinimizeCurrentWindow",
    "ZoomCurrentWindow",
    // The endpoint search: the bundled keymaps do not bind it (SPEC-0067 v9 behaviour 20).
    "GotoUrlAction",
];

/// A keymap action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionHit {
    /// Action id, e.g. `GotoDeclaration`.
    pub id: String,
    /// The reference's title (see [`ACTION_TITLES`]), else the id as words, e.g. `Goto Declaration`.
    pub name: String,
    /// The menu group the reference shows next to the title, e.g. `Local History`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// First keyboard shortcut or gesture in the platform's display, if any; mouse shortcuts are
    /// not shown (v4 behaviour 4).
    pub shortcut: Option<String>,
    /// Char positions of the title that matched, for highlighting.
    #[serde(default, skip_serializing)]
    pub positions: Vec<usize>,
}

/// Actions whose title in the reference differs from their id as words: id, title and group. Search
/// Everywhere matches and shows the title, so "Show History" finds `LocalHistory.ShowHistory`
/// first, as Find Action does in the reference (hand test 6 of round 4).
pub const ACTION_TITLES: &[(&str, &str, &str)] = &[
    // SPEC-0323 v8 behaviour 54: the project room's comments.
    ("Room.AddComment", "Add Comment", "Editor"),
    // SPEC-0127 v15 behaviour 32: not in a menu.
    (
        "Debugger.RemoveAllBreakpoints",
        "Remove All Breakpoints",
        "Run",
    ),
    (
        "Debugger.RemoveAllBreakpointsInFile",
        "Remove All Breakpoints in File",
        "Run",
    ),
    // The default layout is titled "Default" in its menu; alone that says too little (SPEC-0333
    // behaviour 13).
    (
        "RestoreFactoryDefaultLayout",
        "Restore Default Layout",
        "Layouts",
    ),
    ("LocalHistory.ShowHistory", "Show History", "Local History"),
    // The byte order mark actions have no menu item (SPEC-0336 behaviour 20).
    ("AddBom", "Add BOM", "File Properties"),
    ("RemoveBom", "Remove BOM", "File Properties"),
    // The file type actions (SPEC-0004 v5 behaviour 13); the open platform groups the first two
    // as "Mark File As".
    (
        "OverrideFileTypeAction",
        "Override File Type",
        "Mark File As",
    ),
    (
        "ReverteOverrideFileTypeAction",
        "Revert File Type Override",
        "Mark File As",
    ),
    (
        "AssociateWithFileType",
        "Associate with File Type…",
        "File Types",
    ),
    // The rows of the copy popup as actions of their own (SPEC-0332 behaviour 13).
    ("CopyAbsolutePath", "Absolute Path", "Copy Path/Reference…"),
    ("CopyFileName", "File Name", "Copy Path/Reference…"),
    (
        "CopyContentRootPath",
        "Path from Content Root",
        "Copy Path/Reference…",
    ),
    (
        "CopyPathWithLineNumber",
        "Path with Line Number",
        "Copy Path/Reference…",
    ),
    (
        "CopySourceRootPath",
        "Path from Source Root",
        "Copy Path/Reference…",
    ),
    ("CopyReference", "Copy Reference", "Copy Path/Reference…"),
    ("Annotate", "Annotate with Git Blame", "Git"),
    ("ChooseRunConfiguration", "Run…", "Run"),
    // Build Project with the build tools window's Before/After Build tasks (SPEC-0100 v3).
    ("CompileDirty", "Build Project", "Build"),
    ("ChooseDebugConfiguration", "Debug…", "Run"),
    ("Resume", "Resume Program", "Run"),
    (
        "SelectNextOccurrence",
        "Add Selection for Next Occurrence",
        "Edit",
    ),
    ("UnselectPreviousOccurrence", "Unselect Occurrence", "Edit"),
    ("SelectAllOccurrences", "Select All Occurrences", "Edit"),
    ("ActivateProblemsViewToolWindow", "Problems", "Tool Windows"),
    // The grouping toggles of the TODO tool window (SPEC-0334 behaviour 24).
    ("TodoViewGroupByShowModules", "Group by Modules", "TODO"),
    ("TodoViewGroupByShowPackages", "Group by Packages", "TODO"),
    ("TodoViewGroupByFlattenPackage", "Flatten Packages", "TODO"),
    (
        "FerrisIDEA.KeyboardDiagnostics",
        "Keyboard Diagnostics",
        "Diagnostics",
    ),
    ("FerrisIDEA.StopUiAutomation", "Stop UI Automation", "Tools"),
    // D3b (hand test 28): the theme switcher under its menu title, and "theme" finds it.
    ("QuickChangeScheme", "Quick Switch Scheme…", "View"),
    ("ChangeLaf", "Theme…", "Quick Switch Scheme"),
    ("About", "About Ferris IDEA", "Help"),
    ("CommentByLineComment", "Comment with Line Comment", "Code"),
    (
        "CommentByBlockComment",
        "Comment with Block Comment",
        "Code",
    ),
    ("ShowSettings", "Settings…", "File"),
    // SPEC-0295 behaviour 8.
    (
        "HighlightUsagesInFile",
        "Highlight Usages in File",
        "Find Usages",
    ),
    // The second package of actions from the keymap coverage (SPEC-0124 v6 behaviour 9).
    ("FindUsagesInFile", "Find Usages in File", "Find Usages"),
    (
        "GotoNextElementUnderCaretUsage",
        "Next Highlighted Usage",
        "Navigate",
    ),
    (
        "GotoPrevElementUnderCaretUsage",
        "Previous Highlighted Usage",
        "Navigate",
    ),
    ("HippieCompletion", "Cyclic Expand Word", "Code"),
    (
        "HippieBackwardCompletion",
        "Cyclic Expand Word (Backward)",
        "Code",
    ),
    (
        "EditorChooseLookupItem",
        "Insert Suggestion",
        "Completion List",
    ),
    (
        "EditorChooseLookupItemReplace",
        "Replace with Suggestion",
        "Completion List",
    ),
    ("SearchEverywhere.NextTab", "Next Tab", "Search Everywhere"),
    (
        "SearchEverywhere.PrevTab",
        "Previous Tab",
        "Search Everywhere",
    ),
    (
        "SearchEverywhere.SelectItem",
        "Open Selected Item",
        "Search Everywhere",
    ),
    // The actions of a focused part have no menu bar item; their group names the part
    // (SPEC-0124 v4 behaviour 7).
    ("NextDiff", "Next Difference", "Diff"),
    ("PreviousDiff", "Previous Difference", "Diff"),
    // The titles of the open platform (SPEC-0084 v5 behaviours 18–22); `Diff.NextChange` and
    // `Diff.PrevChange` go to another file, not to another difference.
    ("Diff.ApplyLeftSide", "Accept Left Side", "Diff"),
    ("Diff.ApplyRightSide", "Accept Right Side", "Diff"),
    (
        "Diff.FocusOppositePane",
        "Select Opposite Diff Pane",
        "Diff",
    ),
    ("Diff.NextChange", "Compare Next File", "Diff"),
    ("Diff.PrevChange", "Compare Previous File", "Diff"),
    (
        "Diff.ShowSettingsPopup",
        "Show Diff Settings Popup…",
        "Diff",
    ),
    ("CompareTwoFiles", "Compare Two Files", "Diff"),
    ("Diff.ShowDiff", "Show Diff", "Commit"),
    ("Vcs.ToggleAmendCommitMode", "Amend Last Commit", "Commit"),
    (
        "ChangesView.GroupBy.Directory",
        "Group By Directory",
        "Commit",
    ),
    ("ChangesView.GroupBy.Module", "Group By Module", "Commit"),
    ("ChangesView.ShowCommitOptions", "Commit Options", "Commit"),
    // SPEC-0063 v10 behaviour 22.
    ("Vcs.ShowMessageHistory", "Commit Message History", "Commit"),
    // The changelist actions with the open platform's titles (SPEC-0335 behaviour 16).
    ("ChangesView.NewChangeList", "New Changelist…", "Commit"),
    ("ChangesView.Rename", "Edit Changelist…", "Commit"),
    ("ChangesView.SetDefault", "Set Active Changelist", "Commit"),
    (
        "ChangesView.RemoveChangeList",
        "Delete Changelist",
        "Commit",
    ),
    ("ChangesView.Move", "Move to Another Changelist…", "Commit"),
    ("Git.Rename.Local.Branch", "Rename Branch…", "Git"),
    ("Git.Reword.Commit", "Edit Commit Message…", "Git"),
    ("Git.New.Branch.In.Log", "New Branch at Commit…", "Git"),
    // The grouping toggles of the Find tool window (SPEC-0293 v6 behaviour 18).
    ("UsageGrouping.UsageType", "Group by Usage Kind", "Find"),
    ("UsageGrouping.Module", "Group by Module", "Find"),
    ("UsageGrouping.Directory", "Group by Package", "Find"),
    (
        "UsageGrouping.FileStructure",
        "Group by Class and Method",
        "Find",
    ),
    // In-file navigation (SPEC-0071 v9 behaviour 14).
    ("MethodUp", "Previous Method", "Navigate in File"),
    ("MethodDown", "Next Method", "Navigate in File"),
    ("JumpToLastChange", "Last Edit Location", "Navigate"),
    ("JumpToNextChange", "Next Edit Location", "Navigate"),
    // Beside Move Statement in the Code menu (SPEC-0056 v4 behaviour 19).
    ("MoveElementLeft", "Move Element Left", "Code"),
    ("MoveElementRight", "Move Element Right", "Code"),
    // Folding with everything inside and down to a level (SPEC-0083 v6 behaviour 14).
    ("ExpandRegionRecursively", "Expand Recursively", "Folding"),
    (
        "CollapseRegionRecursively",
        "Collapse Recursively",
        "Folding",
    ),
    ("ExpandAllToLevel1", "Expand All to Level 1", "Folding"),
    ("ExpandToLevel1", "Expand to Level 1", "Folding"),
    ("ExpandAllToLevel2", "Expand All to Level 2", "Folding"),
    ("ExpandToLevel2", "Expand to Level 2", "Folding"),
    ("ExpandAllToLevel3", "Expand All to Level 3", "Folding"),
    ("ExpandToLevel3", "Expand to Level 3", "Folding"),
    ("ExpandAllToLevel4", "Expand All to Level 4", "Folding"),
    ("ExpandToLevel4", "Expand to Level 4", "Folding"),
    ("ExpandAllToLevel5", "Expand All to Level 5", "Folding"),
    ("ExpandToLevel5", "Expand to Level 5", "Folding"),
    // The documentation comments only (SPEC-0083 v8 behaviours 20 and 23).
    ("ExpandDocComments", "Expand Doc Comments", "Folding"),
    ("CollapseDocComments", "Collapse Doc Comments", "Folding"),
    // The fifth package of keymap actions: editor actions with the texts of the open platform's
    // actions (SPEC-0057 v6 behaviours 24–27, SPEC-0083 v7 behaviour 17, SPEC-0129 v4
    // behaviour 8).
    ("EditorCutLineEnd", "Cut up to Line End", "Edit"),
    ("EditorToggleInsertState", "Toggle Insert/Overwrite", "Edit"),
    ("AutoIndentLines", "Auto-Indent Lines", "Code"),
    ("EditorContextInfo", "Context Info", "View"),
    ("CollapseBlock", "Fold Code Block", "Folding"),
    (
        "CollapseSelection",
        "Fold Selection / Remove Region",
        "Folding",
    ),
    (
        "EditorIncreaseFontSizeGlobal",
        "Increase Font Size in All Editors",
        "View",
    ),
    (
        "EditorDecreaseFontSizeGlobal",
        "Decrease Font Size in All Editors",
        "View",
    ),
    // The sixth package of keymap actions, with the texts of the open platform's actions. The
    // Find tool window (SPEC-0293 v9 behaviours 22–26):
    ("UsageFiltering.ReadAccess", "Show Read Access", "Find"),
    ("UsageFiltering.WriteAccess", "Show Write Access", "Find"),
    ("UsageFiltering.Imports", "Show Import Statements", "Find"),
    (
        "UsageGrouping.DirectoryStructure",
        "Directory Structure",
        "Find",
    ),
    (
        "UsageGrouping.FlattenModules",
        "Flatten Modules in Usage View",
        "Find",
    ),
    ("UsageView.Exclude", "Exclude", "Find"),
    ("UsageView.Include", "Include", "Find"),
    ("ExportToTextFile", "Export to Text File", "Find"),
    // Search Everywhere (SPEC-0067 v11 behaviour 22), parameter info (SPEC-0080 v3
    // behaviour 7), trees (SPEC-0275 v18 behaviour 45), tool windows (SPEC-0167 v7 behaviour
    // 19) and the find bar (SPEC-0065 v6 behaviour 20). The actions with a menu item have
    // their titles from the menu (SPEC-0124 v13 behaviour 15).
    (
        "SearchEverywhere.NavigateToNextGroup",
        "Next Group Element",
        "Search Everywhere",
    ),
    (
        "SearchEverywhere.NavigateToPrevGroup",
        "Prev Group Element",
        "Search Everywhere",
    ),
    ("NextParameter", "Next Parameter", "Code"),
    ("PrevParameter", "Prev Parameter", "Code"),
    ("ViewSource", "Show Source", "Tree"),
    ("ShowContent", "Show List of Tabs", "Window"),
    ("ShowFilterPopup", "Show Filter Popup", "Find"),
    // The eighth package of keymap actions without a menu item (SPEC-0124 v15 behaviour 17):
    // the editor tabs (SPEC-0059 v9.7 behaviour 29), the overloads of parameter info
    // (SPEC-0080 v4 behaviour 8), inserting with a dot (SPEC-0072 v13 behaviour 49), the files of
    // a commit (SPEC-0086 v12 behaviour 21) and the gutter icon tooltip (SPEC-0294 v5 behaviour
    // 12).
    ("NextEditorTab", "Next Editor Tab", "Window"),
    ("PreviousEditorTab", "Previous Editor Tab", "Window"),
    ("MethodOverloadSwitchUp", "Previous Method Overload", "Code"),
    ("MethodOverloadSwitchDown", "Next Method Overload", "Code"),
    (
        "EditorChooseLookupItemDot",
        "Choose Lookup Item and Insert Dot",
        "Code",
    ),
    (
        "VcsHistory.ShowAllAffected",
        "Show All Affected Files",
        "Git",
    ),
    (
        "EditorShowGutterIconTooltip",
        "Show Gutter Icon Tooltip",
        "Editor",
    ),
    // The seventh package of keymap actions, with the texts of the open platform's actions:
    // the first and last row of the focused list (SPEC-0275 v19 behaviours 47 and 48) and the
    // end of input for the program of the Run tool window (SPEC-0087 v10 behaviour 18). Type
    // Info, Custom Folding… and Stop Background Processes… have their titles from the menu
    // (SPEC-0124 v14 behaviour 16).
    ("List-selectFirstRow", "Select First Row", "List"),
    ("List-selectLastRow", "Select Last Row", "List"),
    (
        "List-selectFirstRowExtendSelection",
        "Extend Selection to First Row",
        "List",
    ),
    (
        "List-selectLastRowExtendSelection",
        "Extend Selection to Last Row",
        "List",
    ),
    ("SendEOF", "Send EOF", "Run"),
    // The fourth package of keymap actions. Trees (SPEC-0275 v13 behaviours 32–36):
    ("ExpandTreeNode", "Expand Tree Node", "Tree"),
    ("CollapseTreeNode", "Collapse Tree Node", "Tree"),
    ("FullyExpandTreeNode", "Fully Expand Tree Node", "Tree"),
    ("ExpandAll", "Expand All", "Tree"),
    ("CollapseAll", "Collapse All", "Tree"),
    ("OpenInRightSplit", "Open in Right Split", "Tree"),
    ("ShowFilePath", "File Path", "Navigate"),
    ("NewElementSamePlace", "New in This Directory…", "File"),
    // The Text tab of Search Everywhere (SPEC-0067 v8 behaviour 19).
    ("TextSearchAction", "Text…", "Navigate"),
    // The Endpoints tab of Search Everywhere (SPEC-0067 v9 behaviour 20).
    ("GotoUrlAction", "Endpoint…", "Navigate"),
    // The query history of the SQL console (SPEC-0093 v5 behaviour 16).
    ("Console.History.Browse", "Browse Query History", "Database"),
    // The recent searches of the find bar (SPEC-0065 v5 behaviour 19).
    ("ShowSearchHistory", "Search History", "Find"),
    // Bookmarks (SPEC-0331 behaviour 26), grouped as their submenu of Edit (SPEC-0124 v7).
    ("ToggleBookmark", "Toggle Bookmark", "Bookmarks"),
    (
        "ToggleBookmarkWithMnemonic",
        "Toggle Bookmark with Mnemonic",
        "Bookmarks",
    ),
    ("ShowBookmarks", "Show Bookmarks", "Bookmarks"),
    ("GotoNextBookmark", "Go to Next Bookmark", "Bookmarks"),
    (
        "GotoPreviousBookmark",
        "Go to Previous Bookmark",
        "Bookmarks",
    ),
    ("GotoBookmark0", "Go to Bookmark 0", "Bookmarks"),
    ("GotoBookmark1", "Go to Bookmark 1", "Bookmarks"),
    ("GotoBookmark2", "Go to Bookmark 2", "Bookmarks"),
    ("GotoBookmark3", "Go to Bookmark 3", "Bookmarks"),
    ("GotoBookmark4", "Go to Bookmark 4", "Bookmarks"),
    ("GotoBookmark5", "Go to Bookmark 5", "Bookmarks"),
    ("GotoBookmark6", "Go to Bookmark 6", "Bookmarks"),
    ("GotoBookmark7", "Go to Bookmark 7", "Bookmarks"),
    ("GotoBookmark8", "Go to Bookmark 8", "Bookmarks"),
    ("GotoBookmark9", "Go to Bookmark 9", "Bookmarks"),
    ("ToggleBookmark0", "Toggle Bookmark 0", "Bookmarks"),
    ("ToggleBookmark1", "Toggle Bookmark 1", "Bookmarks"),
    ("ToggleBookmark2", "Toggle Bookmark 2", "Bookmarks"),
    ("ToggleBookmark3", "Toggle Bookmark 3", "Bookmarks"),
    ("ToggleBookmark4", "Toggle Bookmark 4", "Bookmarks"),
    ("ToggleBookmark5", "Toggle Bookmark 5", "Bookmarks"),
    ("ToggleBookmark6", "Toggle Bookmark 6", "Bookmarks"),
    ("ToggleBookmark7", "Toggle Bookmark 7", "Bookmarks"),
    ("ToggleBookmark8", "Toggle Bookmark 8", "Bookmarks"),
    ("ToggleBookmark9", "Toggle Bookmark 9", "Bookmarks"),
];

/// Title and group of a tool window action: `ActivatenpmToolWindow` → `npm`,
/// `Containers.ToolWindow` → `Containers`, both in "Tool Windows" (hand test 23).
fn tool_window_title(id: &str) -> Option<String> {
    let name = id
        .strip_prefix("Activate")
        .and_then(|r| r.strip_suffix("ToolWindow"))
        .or_else(|| id.strip_suffix(".ToolWindow"))
        .filter(|n| !n.is_empty())?;
    let name = name.strip_suffix("View").unwrap_or(name);
    Some(action_words(name))
}

/// Title and group of an action: from [`ACTION_TITLES`], else its menu label and menu
/// ([`ferris_idea_ui::menus::menu_title`]), else its id as words without a group.
pub fn action_title(id: &str) -> (String, Option<&'static str>) {
    match ACTION_TITLES.iter().find(|(i, _, _)| *i == id) {
        Some((_, title, group)) => ((*title).to_owned(), Some(*group)),
        None => match (ferris_idea_ui::menus::menu_title(id), tool_window_title(id)) {
            // The menu label is the title, as in the reference (hand test 39: "Split Right").
            (Some((label, menu)), _) => (label.to_owned(), Some(menu)),
            (None, Some(title)) => (title, Some("Tool Windows")),
            // Editor actions read without the id's `Editor` prefix, as their menu titles do
            // (`EditorDeleteToWordEnd` → "Delete To Word End"; v4 behaviour 4, OBS-0023 L16).
            (None, None) => {
                let words = action_words(id);
                match words.strip_prefix("Editor ") {
                    Some(rest) if !rest.is_empty() => (rest.to_owned(), None),
                    _ => (words, None),
                }
            }
        },
    }
}

/// Results per category, best first; a category not searched is `None` (behaviour 6).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Everywhere {
    /// Files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileHit>>,
    /// Class-like symbols (v4 behaviour 8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classes: Option<Vec<SymbolHit>>,
    /// Symbols.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols: Option<Vec<SymbolHit>>,
    /// Actions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<ActionHit>>,
    /// Lines of text (v4 behaviour 9).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<Vec<TextHit>>,
    /// The value of a query that is an arithmetic expression, `12*7` → `84` (v4 behaviour 10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calculator: Option<String>,
    /// 0-based line asked for with `name:line` (v4 behaviour 7).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// 0-based column asked for with `name:line:column`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    /// What the files category left out, in one line: `2 folders not listed (libraries and
    /// ignored files)` (v5 behaviour 12). Set by the caller that knows the listing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_listed: Option<String>,
}

/// Words of an action id: `GotoDeclaration` → `Goto Declaration`.
pub fn action_words(id: &str) -> String {
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for c in id.chars() {
        if c == '.' || c == '_' {
            out.push(' ');
        } else {
            if c.is_uppercase() && prev.is_some_and(|p| p.is_lowercase()) {
                out.push(' ');
            }
            out.push(c);
        }
        prev = Some(c);
    }
    out
}

fn top<T>(mut found: Vec<(i64, String, T)>, limit: usize) -> Vec<T> {
    found.sort_by(|a, b| rank((a.0, &a.1), (b.0, &b.1)));
    found.into_iter().take(limit).map(|(_, _, t)| t).collect()
}

/// Options of the text category (v4 behaviour 9), as the find bar's toggles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextOptions {
    /// Distinguish upper and lower case.
    pub match_case: bool,
    /// Whole words only.
    pub whole_words: bool,
    /// The query is a regular expression.
    pub regex: bool,
}

/// What Search Everywhere looks through.
#[derive(Debug, Clone, Copy)]
pub struct Sources<'a> {
    /// Workspace root the files are relative to.
    pub root: &'a Path,
    /// Workspace files.
    pub files: &'a [PathBuf],
    /// Keymap whose actions are offered.
    pub keymap: &'a Keymap,
    /// Further actions as `(id, title)`, e.g. those of installed plugins.
    pub extra_actions: &'a [(String, String)],
    /// Recently opened files (workspace-relative), most recent first: their hits rank higher, and
    /// an empty query lists them (v4 behaviour 6).
    pub recent: &'a [PathBuf],
    /// Options of the text category.
    pub text_options: TextOptions,
    /// The files the files category searches when they are not `files` (`SPEC-0031` v5 behaviour
    /// 12): the project's own sources and the open files. Recently opened files are searched in
    /// any case. `None`: `files`. Classes, symbols and text keep `files`.
    pub file_names: Option<&'a [PathBuf]>,
}

/// Splits `name:line` and `name:line:column` (1-based) off a query (v4 behaviour 7).
pub fn split_line_suffix(query: &str) -> (&str, Option<(usize, Option<usize>)>) {
    let mut parts = query.rsplitn(3, ':');
    let last = parts.next().unwrap_or_default();
    let Ok(last_n) = last.parse::<usize>() else {
        return (query, None);
    };
    match (parts.next(), parts.next()) {
        (Some(mid), Some(head)) if !head.is_empty() => match mid.parse::<usize>() {
            Ok(line) => (
                head,
                Some((line.saturating_sub(1), Some(last_n.saturating_sub(1)))),
            ),
            Err(_) => (
                &query[..query.len() - last.len() - 1],
                Some((last_n.saturating_sub(1), None)),
            ),
        },
        (Some(head), None) if !head.is_empty() => (head, Some((last_n.saturating_sub(1), None))),
        _ => (query, None),
    }
}

/// The value of an arithmetic expression with `+ - * / % ^` and parentheses, `None` when the query is
/// not one or has no operator (v4 behaviour 10).
pub fn calculate(query: &str) -> Option<String> {
    struct P<'a> {
        s: &'a [u8],
        at: usize,
        ops: usize,
    }
    impl P<'_> {
        fn skip(&mut self) {
            while self.s.get(self.at).is_some_and(|c| c.is_ascii_whitespace()) {
                self.at += 1;
            }
        }
        fn peek(&mut self) -> Option<u8> {
            self.skip();
            self.s.get(self.at).copied()
        }
        fn expr(&mut self) -> Option<f64> {
            let mut v = self.term()?;
            while let Some(op @ (b'+' | b'-')) = self.peek() {
                self.at += 1;
                self.ops += 1;
                let r = self.term()?;
                v = if op == b'+' { v + r } else { v - r };
            }
            Some(v)
        }
        fn term(&mut self) -> Option<f64> {
            let mut v = self.factor()?;
            while let Some(op @ (b'*' | b'/' | b'%')) = self.peek() {
                self.at += 1;
                self.ops += 1;
                let r = self.factor()?;
                v = match op {
                    b'*' => v * r,
                    _ if r == 0.0 => return None,
                    b'/' => v / r,
                    _ => v % r,
                };
            }
            Some(v)
        }
        fn factor(&mut self) -> Option<f64> {
            match self.peek()? {
                b'-' => {
                    self.at += 1;
                    Some(-self.factor()?)
                }
                b'+' => {
                    self.at += 1;
                    self.factor()
                }
                _ => {
                    let base = self.atom()?;
                    if self.peek() == Some(b'^') {
                        self.at += 1;
                        self.ops += 1;
                        return Some(base.powf(self.factor()?));
                    }
                    Some(base)
                }
            }
        }
        fn atom(&mut self) -> Option<f64> {
            if self.peek()? == b'(' {
                self.at += 1;
                let v = self.expr()?;
                if self.peek()? != b')' {
                    return None;
                }
                self.at += 1;
                return Some(v);
            }
            let start = self.at;
            while self
                .s
                .get(self.at)
                .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
            {
                self.at += 1;
            }
            std::str::from_utf8(&self.s[start..self.at])
                .ok()?
                .parse()
                .ok()
        }
    }
    let mut p = P {
        s: query.trim().as_bytes(),
        at: 0,
        ops: 0,
    };
    let v = p.expr()?;
    if p.peek().is_some() || p.ops == 0 || !v.is_finite() {
        return None;
    }
    Some(if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    })
}

/// Where a symbol of `path` lives (v4 behaviour 5): the Java/Kotlin package after a `java` or
/// `kotlin` source folder, the dotted module of a Python file, else the path itself.
pub fn container_of(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let parts: Vec<String> = path
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    match ext {
        "java" | "kt" => {
            let dirs = &parts[..parts.len().saturating_sub(1)];
            let after = dirs
                .iter()
                .rposition(|p| p == "java" || p == "kotlin")
                .map_or(0, |i| i + 1);
            dirs[after..].join(".")
        }
        "py" | "pyi" => {
            let mut module = parts.join(".");
            module.truncate(module.len() - ext.len() - 1);
            module
        }
        _ => parts.join("/"),
    }
}

/// The parameter list of a function or method defined at `name` on `line` (v4 behaviour 5): Java
/// and Kotlin show the types, other languages the names (`self`/`cls` left out); `None` when the list
/// does not close on the line.
pub fn params_of(line: &str, name: &str, path: &Path) -> Option<String> {
    let at = line.find(name)? + name.len();
    let rest = &line[at..];
    let rest = rest.trim_start();
    // Skip type parameters such as `<T>`.
    let rest = if rest.starts_with('<') {
        let close = rest.find('>')?;
        rest[close + 1..].trim_start()
    } else {
        rest
    };
    let inner = rest.strip_prefix('(')?;
    let mut depth = 0usize;
    let mut end = None;
    for (i, c) in inner.char_indices() {
        match c {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' if depth == 0 => {
                end = Some(i);
                break;
            }
            ')' | '>' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let inner = &inner[..end?];
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let mut params = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut pieces = Vec::new();
    for (i, c) in inner.char_indices() {
        match c {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                pieces.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    pieces.push(&inner[start..]);
    for p in pieces {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        let shown = match ext {
            "java" => {
                let words: Vec<&str> = p
                    .split_whitespace()
                    .filter(|w| !w.starts_with('@') && *w != "final")
                    .collect();
                if words.len() > 1 {
                    words[..words.len() - 1].join(" ")
                } else {
                    words.join(" ")
                }
            }
            "kt" => p
                .split_once(':')
                .map_or(p, |(_, t)| t)
                .split('=')
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned(),
            "c" | "h" | "cpp" | "cc" | "hpp" => p
                .split_whitespace()
                .last()
                .unwrap_or_default()
                .trim_start_matches('*')
                .to_owned(),
            _ => p
                .split([':', '='])
                .next()
                .unwrap_or_default()
                .trim()
                .trim_end_matches('?')
                .to_owned(),
        };
        if matches!(ext, "py" | "pyi") && params.is_empty() && (shown == "self" || shown == "cls") {
            continue;
        }
        params.push(shown);
    }
    Some(format!("({})", params.join(", ")))
}

/// Kinds the Classes category keeps (v4 behaviour 8).
fn class_like(kind: &str) -> bool {
    matches!(
        kind,
        "class" | "interface" | "enum" | "struct" | "trait" | "type" | "record" | "union"
    )
}

/// Reads a file for the text category: `None` for binary files and files over 10 MiB.
fn text_of(full: &Path) -> Option<String> {
    let meta = std::fs::metadata(full).ok()?;
    if meta.len() > ferris_idea_core::MAX_SEARCH_BYTES {
        return None;
    }
    let bytes = std::fs::read(full).ok()?;
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Searches the sources' files and actions for `query`, at most `limit` per category.
/// Stops early and returns what it has when `cancel` is set (a newer query replaced this one).
pub fn search_everywhere(
    sources: Sources<'_>,
    query: &str,
    categories: Categories,
    limit: usize,
    cancel: Option<&AtomicBool>,
) -> Everywhere {
    let Sources {
        root,
        files,
        keymap,
        extra_actions,
        recent,
        text_options,
        file_names,
    } = sources;
    // The files category: its own list and the recently opened files (v5 behaviour 12).
    let named: Vec<PathBuf>;
    let file_names: &[PathBuf] = match file_names {
        Some(list) => {
            let known: std::collections::HashSet<&PathBuf> = list.iter().collect();
            let extra: Vec<&PathBuf> = recent
                .iter()
                .filter(|r| !known.contains(r) && root.join(r).is_file())
                .collect();
            if extra.is_empty() {
                list
            } else {
                named = list.iter().chain(extra).cloned().collect();
                &named
            }
        }
        None => files,
    };
    let cancelled = || cancel.is_some_and(|c| c.load(Ordering::Relaxed));
    let (query, line) = split_line_suffix(query.trim());
    let query = query.trim();
    let mut out = Everywhere {
        line: line.map(|(l, _)| l),
        column: line.and_then(|(_, c)| c),
        ..Everywhere::default()
    };
    // Hits in recently opened files rank higher, the latest most (v4 behaviour 6).
    let recency = |f: &Path| -> i64 {
        recent
            .iter()
            .take(50)
            .position(|r| r == f)
            .map_or(0, |i| 60 - i as i64)
    };
    if query.is_empty() {
        if categories.files || categories.classes {
            let known: std::collections::HashSet<&PathBuf> =
                files.iter().chain(file_names).collect();
            out.files = Some(
                recent
                    .iter()
                    .filter(|r| known.contains(r) || files.is_empty())
                    .take(limit)
                    .map(|r| FileHit {
                        path: r.to_string_lossy().into_owned(),
                        positions: Vec::new(),
                    })
                    .collect(),
            );
        }
        return out;
    }
    if categories.files && categories.actions {
        out.calculator = calculate(query);
    }
    if categories.files {
        let mut found = Vec::new();
        for f in file_names {
            let path = f.to_string_lossy().into_owned();
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let hit = if query.contains('/') {
                hump_match(query, &path).map(|m| (m.score, Vec::new()))
            } else {
                // The file name is matched; a `/` in the query matches the whole path (behaviour 3).
                hump_match(query, &name).map(|m| (m.score + 50, m.positions))
            };
            if let Some((score, positions)) = hit {
                found.push((
                    score + recency(f),
                    path.clone(),
                    FileHit { path, positions },
                ));
            }
        }
        out.files = Some(top(found, limit));
    }
    if categories.symbols || categories.classes {
        let mut symbols = Vec::new();
        let mut classes = Vec::new();
        let known = if cancelled() {
            Vec::new()
        } else {
            definitions(root, files, cancel)
        };
        let qualified = query.contains('.');
        for (f, definitions) in files.iter().zip(known) {
            let Some(definitions) = definitions else {
                continue;
            };
            let container = container_of(f);
            let bonus = recency(f);
            for d in definitions.iter() {
                let hit = hump_match(query, &d.name)
                    .map(|m| (m.score, m.positions))
                    .or_else(|| {
                        qualified
                            .then(|| hump_match(query, &format!("{container}.{}", d.name)))
                            .flatten()
                            .map(|m| (m.score, Vec::new()))
                    });
                let Some((score, positions)) = hit else {
                    continue;
                };
                let hit = SymbolHit {
                    name: d.name.clone(),
                    kind: d.kind.clone(),
                    path: f.to_string_lossy().into_owned(),
                    line: d.line,
                    container: container.clone(),
                    params: None,
                    positions,
                };
                if categories.classes && class_like(&d.kind) {
                    classes.push((score + bonus, d.name.clone(), hit.clone()));
                }
                if categories.symbols {
                    symbols.push((score + bonus, d.name.clone(), hit));
                }
            }
        }
        let with_params = |mut hits: Vec<SymbolHit>| {
            let mut texts: HashMap<String, Option<String>> = HashMap::new();
            for h in &mut hits {
                if !matches!(h.kind.as_str(), "function" | "method" | "constructor") {
                    continue;
                }
                let text = texts
                    .entry(h.path.clone())
                    .or_insert_with(|| std::fs::read_to_string(root.join(&h.path)).ok());
                if let Some(line) = text.as_deref().and_then(|t| t.lines().nth(h.line)) {
                    h.params = params_of(line, &h.name, Path::new(&h.path));
                }
            }
            hits
        };
        if categories.classes {
            out.classes = Some(with_params(top(classes, limit)));
        }
        if categories.symbols {
            out.symbols = Some(with_params(top(symbols, limit)));
        }
    }
    if categories.actions {
        let macos = cfg!(target_os = "macos");
        let mut found = Vec::new();
        let unbound = UNBOUND_ACTIONS
            .iter()
            .filter(|id| keymap.shortcuts(id).is_empty())
            .map(|id| (*id, &[][..]));
        for (id, shortcuts) in keymap.actions().chain(unbound) {
            let (name, group) = action_title(id);
            // The title counts as much as the id; the better match ranks the action.
            // Words in any order (D50).
            let title = hump_words_match(query, &name);
            let score = [
                title.as_ref().map(|m| m.score),
                hump_words_match(query, id).map(|m| m.score),
            ]
            .into_iter()
            .flatten()
            .max();
            if let Some(score) = score {
                found.push((
                    score,
                    name.clone(),
                    ActionHit {
                        id: id.to_owned(),
                        name,
                        group: group.map(str::to_owned),
                        // Without a chord the action yields (SPEC-0071 v10 behaviour 16).
                        shortcut: shortcuts
                            .iter()
                            .filter(|s| keymap.shown_shortcuts(id).contains(s))
                            .find(|s| !matches!(s, ferris_idea_ui::keymap::Shortcut::Mouse { .. }))
                            .map(|s| s.display(macos)),
                        positions: title.map(|m| m.positions).unwrap_or_default(),
                    },
                ));
            }
        }
        // Actions of plugins (SPEC-0104): id and title.
        for (id, title) in extra_actions {
            let by_title = hump_words_match(query, title);
            if let Some(score) = by_title
                .as_ref()
                .map(|m| m.score)
                .or_else(|| hump_words_match(query, id).map(|m| m.score))
            {
                found.push((
                    score,
                    id.clone(),
                    ActionHit {
                        id: id.clone(),
                        name: title.clone(),
                        group: None,
                        shortcut: None,
                        positions: by_title.map(|m| m.positions).unwrap_or_default(),
                    },
                ));
            }
        }
        out.actions = Some(top(found, limit));
    }
    if categories.text && !cancelled() {
        out.text = Some(search_text(
            root,
            files,
            recent,
            query,
            text_options,
            limit,
            cancel,
        ));
    }
    out
}

/// The text category (v4 behaviour 9): lines containing `query`, recently opened files first, then
/// in path order; at most `limit` lines.
fn search_text(
    root: &Path,
    files: &[PathBuf],
    recent: &[PathBuf],
    query: &str,
    options: TextOptions,
    limit: usize,
    cancel: Option<&AtomicBool>,
) -> Vec<TextHit> {
    let q = ferris_idea_core::Query {
        pattern: query.to_owned(),
        regex: options.regex,
        match_case: options.match_case,
        whole_words: options.whole_words,
    };
    let Ok(finder) = ferris_idea_core::Finder::new(&q) else {
        return Vec::new();
    };
    let known: std::collections::HashSet<&PathBuf> = files.iter().collect();
    let first: Vec<&PathBuf> = recent.iter().filter(|r| known.contains(r)).collect();
    let order = first
        .iter()
        .copied()
        .chain(files.iter().filter(|f| !first.contains(f)));
    let mut out = Vec::new();
    for f in order {
        if out.len() >= limit || cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            break;
        }
        let Some(text) = text_of(&root.join(f)) else {
            continue;
        };
        let mut last_line = None;
        for m in finder.find(&text) {
            if last_line == Some(m.line) {
                continue;
            }
            last_line = Some(m.line);
            let line_len = m.line_text.chars().count();
            out.push(TextHit {
                path: f.to_string_lossy().into_owned(),
                line: m.line,
                column: m.column,
                length: (m.end - m.start).min(line_len.saturating_sub(m.column)),
                text: m.line_text,
            });
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0031 v4 (OBS-0023 L1–L5, L12–L14, L16): word starts, classes, text, `name:line`,
    /// recent files first, the calculator, containers and parameters.
    #[test]
    fn v4_categories_lines_recent_and_calculator() {
        assert_eq!(
            split_line_suffix("OrderService:12"),
            ("OrderService", Some((11, None)))
        );
        assert_eq!(
            split_line_suffix("Order.java:3:5"),
            ("Order.java", Some((2, Some(4))))
        );
        assert_eq!(split_line_suffix("a:b"), ("a:b", None));
        assert_eq!(split_line_suffix(":7"), (":7", None));
        assert_eq!(calculate("12*7").as_deref(), Some("84"));
        assert_eq!(calculate("(1+2)^2 / 4").as_deref(), Some("2.25"));
        assert_eq!(calculate("-3 + 10 % 4").as_deref(), Some("-1"));
        assert_eq!(calculate("1199"), None, "a number alone is no calculation");
        assert_eq!(calculate("1/0"), None);
        assert_eq!(calculate("order"), None);
        assert_eq!(
            container_of(Path::new("src/main/java/lab/shop/model/Order.java")),
            "lab.shop.model"
        );
        assert_eq!(container_of(Path::new("py/shop/order.py")), "py.shop.order");
        assert_eq!(
            container_of(Path::new("web/src/order.ts")),
            "web/src/order.ts"
        );
        let java = Path::new("A.java");
        assert_eq!(
            params_of(
                "    public Order createOrder(String id, long totalCents) {",
                "createOrder",
                java
            )
            .as_deref(),
            Some("(String, long)")
        );
        assert_eq!(
            params_of(
                "  static <T> List<T> of(final List<Map<K, V>> m) {",
                "of",
                java
            )
            .as_deref(),
            Some("(List<Map<K, V>>)")
        );
        assert_eq!(
            params_of(
                "export function createOrder(id: string, totalCents: number): OrderDto {",
                "createOrder",
                Path::new("a.ts")
            )
            .as_deref(),
            Some("(id, totalCents)")
        );
        assert_eq!(
            params_of(
                "    def apply_discount(self, percent: int = 5) -> None:",
                "apply_discount",
                Path::new("a.py")
            )
            .as_deref(),
            Some("(percent)")
        );
        assert_eq!(params_of("def f(a,", "f", Path::new("a.py")), None);

        let dir =
            std::env::temp_dir().join(format!("ferris-idea-everywhere-v4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/main/java/lab/shop/model")).unwrap();
        std::fs::create_dir_all(dir.join("src/main/java/lab/shop/service")).unwrap();
        std::fs::write(
            dir.join("src/main/java/lab/shop/model/Order.java"),
            "package lab.shop.model;\n\npublic class Order {\n    long totalCents;\n}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main/java/lab/shop/service/OrderService.java"),
            "package lab.shop.service;\n\npublic class OrderService {\n    public long calculateTotal() {\n        return 0; // order total\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main/java/lab/shop/service/ProductService.java"),
            "package lab.shop.service;\n\npublic class ProductService {\n    public long calculateTotal() {\n        return 1;\n    }\n}\n",
        )
        .unwrap();
        let files = vec![
            PathBuf::from("src/main/java/lab/shop/model/Order.java"),
            PathBuf::from("src/main/java/lab/shop/service/OrderService.java"),
            PathBuf::from("src/main/java/lab/shop/service/ProductService.java"),
        ];
        let keymap = Keymap::bundled_macos();
        let recent = vec![files[2].clone()];
        let sources = Sources {
            root: &dir,
            files: &files,
            keymap: &keymap,
            extra_actions: &[],
            recent: &recent,
            text_options: TextOptions::default(),
            file_names: None,
        };
        // Word starts: `order` finds no ProductService, and no action by inner letters.
        let all = search_everywhere(sources, "order", Categories::ALL, 8, None);
        let names: Vec<&str> = all
            .files
            .as_ref()
            .unwrap()
            .iter()
            .map(|f| f.path.as_str())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names.iter().all(|n| !n.contains("Product")));
        assert!(
            all.actions
                .as_ref()
                .unwrap()
                .iter()
                .all(|a| !a.name.contains("Decrease Font")),
            "{:?}",
            all.actions
        );
        // Text lines and their match.
        let text = all.text.unwrap();
        assert_eq!(text.len(), 3, "{text:?}");
        // The recent file's lines come first (it has none here), then path order.
        assert_eq!(text[0].path, files[0].to_string_lossy());
        let comment = text.iter().find(|t| t.line == 4).unwrap();
        assert_eq!((comment.column, comment.length), (21, 5));
        // Classes keep class-like kinds; `model.Ord` finds the class by its package.
        let classes = Categories {
            classes: true,
            ..Categories::NONE
        };
        let found = search_everywhere(sources, "model.Ord", classes, 8, None)
            .classes
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].name.as_str(), found[0].container.as_str()),
            ("Order", "lab.shop.model")
        );
        // `name:line` and the recent file first; methods carry their parameters.
        let symbols = Categories {
            symbols: true,
            ..Categories::NONE
        };
        let found = search_everywhere(sources, "calculatetotal:3", symbols, 8, None);
        assert_eq!(found.line, Some(2));
        let hits = found.symbols.unwrap();
        assert_eq!(hits[0].path, files[2].to_string_lossy());
        assert_eq!(hits[0].params.as_deref(), Some("()"));
        // An empty query lists the recent files; the calculator answers on All.
        let empty = search_everywhere(sources, "", Categories::ALL, 8, None);
        assert_eq!(empty.files.unwrap()[0].path, files[2].to_string_lossy());
        let calc = search_everywhere(sources, "12*7", Categories::ALL, 8, None);
        assert_eq!(calc.calculator.as_deref(), Some("84"));
        // Mouse shortcuts are not shown; editor actions read without their `Editor` prefix.
        let actions = Categories {
            actions: true,
            ..Categories::NONE
        };
        let found = search_everywhere(sources, "add or remove caret", actions, 8, None);
        let caret = found
            .actions
            .unwrap()
            .into_iter()
            .find(|a| a.id == "EditorAddOrRemoveCaret")
            .unwrap();
        assert_eq!(caret.name, "Add Or Remove Caret");
        assert!(
            caret
                .shortcut
                .as_deref()
                .is_none_or(|s| !s.starts_with("mouse"))
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn words_and_cancelled_search() {
        assert_eq!(action_words("GotoDeclaration"), "Goto Declaration");
        assert_eq!(action_words("Vcs.Push"), "Vcs Push");
        let dir =
            std::env::temp_dir().join(format!("ferris-idea-everywhere-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("main.rs"), "fn main() {}\nstruct Mainframe;\n").unwrap();
        let files = vec![PathBuf::from("main.rs")];
        let keymap = Keymap::bundled_default();
        let sources = Sources {
            root: &dir,
            files: &files,
            keymap: &keymap,
            extra_actions: &[],
            recent: &[],
            text_options: TextOptions::default(),
            file_names: None,
        };
        let all = search_everywhere(sources, "main", Categories::ALL, 5, None);
        assert_eq!(all.files.as_ref().unwrap()[0].path, "main.rs");
        let symbols = all.symbols.unwrap();
        assert_eq!((symbols[0].name.as_str(), symbols[0].line), ("main", 0));
        let stop = AtomicBool::new(true);
        let only = Categories {
            symbols: true,
            ..Categories::NONE
        };
        let cut = search_everywhere(sources, "main", only, 5, Some(&stop));
        assert_eq!(
            cut.symbols,
            Some(Vec::new()),
            "a cancelled search stops early"
        );
        assert!(cut.files.is_none() && cut.actions.is_none());
        // Definitions are cached per file and follow a change of the file (SPEC-0260).
        std::fs::write(dir.join("main.rs"), "fn main() {}\nstruct Mainline2;\n").unwrap();
        let again = search_everywhere(sources, "mainline", only, 5, None);
        assert_eq!(again.symbols.unwrap()[0].name, "Mainline2");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn first_action(query: &str) -> ActionHit {
        let files: Vec<PathBuf> = Vec::new();
        let keymap = Keymap::bundled_macos();
        let sources = Sources {
            root: Path::new("."),
            files: &files,
            keymap: &keymap,
            extra_actions: &[],
            recent: &[],
            text_options: TextOptions::default(),
            file_names: None,
        };
        let only = Categories {
            actions: true,
            ..Categories::NONE
        };
        let found = search_everywhere(sources, query, only, 8, None);
        found.actions.unwrap().remove(0)
    }

    /// D3b (hand tests 23 and 28) and D50 (hand tests 150, 181): Find Action shows the menu
    /// titles, finds the theme switcher by "theme" and ignores the order of the typed words.
    #[test]
    fn find_action_titles_and_word_order() {
        assert_eq!(first_action("quick switch scheme").id, "QuickChangeScheme");
        assert_eq!(first_action("theme").id, "ChangeLaf");
        assert_eq!(first_action("theme").name, "Theme…");
        let npm = first_action("npm");
        assert_eq!(
            (npm.name.as_str(), npm.group.as_deref()),
            ("npm", Some("Tool Windows"))
        );
        // SPEC-0124 v3 AK-5: the Build and Window menus title their actions.
        assert_eq!(
            action_title("ExternalSystem.ProjectRefreshAction"),
            ("Sync Build Changes".into(), Some("Build"))
        );
        assert_eq!(
            action_title("CompileDirty"),
            ("Build Project".into(), Some("Build"))
        );
        assert_eq!(
            action_title("MinimizeCurrentWindow"),
            ("Minimize".into(), Some("Window"))
        );
        assert_eq!(
            first_action("sync build changes").id,
            "ExternalSystem.ProjectRefreshAction"
        );
        assert_eq!(action_title("Containers.ToolWindow").0, "Containers");
        assert_eq!(action_title("ActivateProjectToolWindow").0, "Project");
        assert_eq!(
            first_action("comment with line comment").id,
            "CommentByLineComment"
        );
        assert_eq!(first_action("line comment with").id, "CommentByLineComment");
        assert_eq!(
            first_action("attach profiler to process").id,
            "Profiler.AttachToProcess"
        );
        assert_eq!(first_action("about").id, "About");
        // D8a, D8c (hand tests 39, 199): the split actions under their menu labels.
        assert_eq!(first_action("split right").id, "SplitVertically");
        assert_eq!(first_action("split down").id, "SplitHorizontally");
        assert_eq!(first_action("split and move right").id, "SplitAndMoveRight");
        assert_eq!(first_action("split vertically").id, "SplitVertically");
    }

    /// Hand test 6 of round 4: "Show History" in Find Action finds the local history of the file
    /// first, under the reference's title and group, with either platform's keymap.
    #[test]
    fn show_history_is_found_by_its_title() {
        let files: Vec<PathBuf> = Vec::new();
        for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
            let sources = Sources {
                root: Path::new("."),
                files: &files,
                keymap: &keymap,
                extra_actions: &[],
                recent: &[],
                text_options: TextOptions::default(),
                file_names: None,
            };
            for query in [
                "Show History",
                "show History",
                "showhistory",
                "local history",
            ] {
                let only = Categories {
                    actions: true,
                    ..Categories::NONE
                };
                let found = search_everywhere(sources, query, only, 8, None);
                let first = &found.actions.unwrap()[0];
                assert_eq!(first.id, "LocalHistory.ShowHistory", "query {query:?}");
                assert_eq!(first.name, "Show History");
                assert_eq!(first.group.as_deref(), Some("Local History"));
            }
        }
        // SPEC-0277 AK-3: Find Action finds Annotate with Git Blame by its title.
        for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
            let sources = Sources {
                root: Path::new("."),
                files: &files,
                keymap: &keymap,
                extra_actions: &[],
                recent: &[],
                text_options: TextOptions::default(),
                file_names: None,
            };
            for query in ["Annotate", "annotate with git blame", "git blame"] {
                let only = Categories {
                    actions: true,
                    ..Categories::NONE
                };
                let found = search_everywhere(sources, query, only, 8, None);
                let first = &found.actions.unwrap()[0];
                assert_eq!(first.id, "Annotate", "query {query:?}");
                assert_eq!(first.name, "Annotate with Git Blame");
                assert_eq!(first.group.as_deref(), Some("Git"));
            }
        }
        assert_eq!(action_title("Resume").0, "Resume Program");
        assert_eq!(
            action_title("GotoDeclaration"),
            ("Declaration or Usages".into(), Some("Navigate"))
        );
        assert_eq!(action_title("EditorScrollUp"), ("Scroll Up".into(), None));
        // Every titled action is one Search Everywhere offers.
        let mac = Keymap::bundled_macos();
        for (id, _, _) in ACTION_TITLES {
            assert!(
                !mac.shortcuts(id).is_empty() || UNBOUND_ACTIONS.contains(id),
                "{id} has a title but is not offered"
            );
        }
    }

    /// SPEC-0124 AK-10 (v8): Find Action titles the items of behaviour 11 as their menus do and
    /// groups them by the menu or submenu they are in; both keymaps offer them.
    #[test]
    fn reference_comparison_items_have_titles_and_groups() {
        for (id, title, group) in [
            ("FerrisIDEA.SettingsPlugins", "Plugins…", "File"),
            // SPEC-0105 AK-15 (v9).
            ("ImportSettings", "Import Settings…", "Manage IDE Settings"),
            ("ExportSettings", "Export Settings…", "Manage IDE Settings"),
            ("LocalHistory.ShowHistory", "Show History", "Local History"),
            ("ToggleInSelection", "Search in Selection Only", "Edit"),
            ("FindUsages", "Find Usages", "Find Usages"),
            ("ShowUsages", "Show Usages", "Find Usages"),
            ("FindUsagesInFile", "Find Usages in File", "Find Usages"),
            (
                "HighlightUsagesInFile",
                "Highlight Usages in File",
                "Find Usages",
            ),
            ("EditorSelectWord", "Extend Selection", "Edit"),
            ("EditorUnSelectWord", "Shrink Selection", "Edit"),
            ("EditorIndentSelection", "Indent Selection", "Edit"),
            (
                "EditorUnindentSelection",
                "Unindent Line or Selection",
                "Edit",
            ),
            ("TextSearchAction", "Text…", "Navigate"),
            ("GotoLine", "Line:Column…", "Navigate"),
            ("JumpToLastChange", "Last Edit Location", "Navigate"),
            ("JumpToNextChange", "Next Edit Location", "Navigate"),
            ("MethodUp", "Previous Method", "Navigate in File"),
            ("MethodDown", "Next Method", "Navigate in File"),
            ("ShowNavBar", "Jump to Navigation Bar", "Navigate"),
            ("FileStructurePopup", "File Structure", "Navigate"),
            (
                "IntroduceVariable",
                "Introduce Variable…",
                "Extract/Introduce",
            ),
            (
                "ExtractSuperclass",
                "Extract Superclass…",
                "Extract/Introduce",
            ),
            ("Move", "Move…", "Refactor"),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
                assert!(
                    !keymap.shortcuts(id).is_empty() || UNBOUND_ACTIONS.contains(&id),
                    "{id} is not offered"
                );
            }
        }
    }

    /// SPEC-0332 AK-7 and v2 AK-13: the small editing commands have their titles and groups;
    /// both keymaps offer them, and only Add Carets to Ends of Selected Lines and Copy Reference
    /// have a key.
    #[test]
    fn small_editing_commands_have_titles() {
        for (id, title, group) in [
            ("EditorSortLines", "Sort Lines", "Edit"),
            ("EditorReverseLines", "Reverse Lines", "Edit"),
            ("EditorTranspose", "Transpose", "Edit"),
            (
                "EditorAddCaretPerSelectedLine",
                "Add Carets to Ends of Selected Lines",
                "Edit",
            ),
            ("ConvertIndentsToSpaces", "To Spaces", "Convert Indents"),
            ("ConvertIndentsToTabs", "To Tabs", "Convert Indents"),
            ("CopyReferencePopupGroup", "Copy Path/Reference…", "Edit"),
            ("CopyAbsolutePath", "Absolute Path", "Copy Path/Reference…"),
            ("CopyFileName", "File Name", "Copy Path/Reference…"),
            (
                "CopyContentRootPath",
                "Path from Content Root",
                "Copy Path/Reference…",
            ),
            (
                "CopyPathWithLineNumber",
                "Path with Line Number",
                "Copy Path/Reference…",
            ),
            (
                "CopySourceRootPath",
                "Path from Source Root",
                "Copy Path/Reference…",
            ),
            ("CopyReference", "Copy Reference", "Copy Path/Reference…"),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
                let bound = !keymap.shortcuts(id).is_empty();
                assert_eq!(
                    bound,
                    matches!(id, "EditorAddCaretPerSelectedLine" | "CopyReference"),
                    "{id}"
                );
                assert!(
                    bound || UNBOUND_ACTIONS.contains(&id),
                    "{id} is not offered"
                );
            }
            assert_eq!(first_action(&title.to_lowercase()).id, id);
        }
        let keys: Vec<String> = Keymap::bundled_macos()
            .shortcuts("EditorAddCaretPerSelectedLine")
            .iter()
            .map(|s| s.display(true))
            .collect();
        assert_eq!(keys, ["⌥⇧G"]);
        let keys = |keymap: Keymap, mac: bool| -> Vec<String> {
            keymap
                .shortcuts("CopyReference")
                .iter()
                .map(|s| s.display(mac))
                .collect()
        };
        assert_eq!(keys(Keymap::bundled_macos(), true), ["⌥⇧⌘C"]);
        assert_eq!(keys(Keymap::bundled_default(), false), ["Ctrl+Alt+Shift+C"]);
    }

    /// SPEC-0124 AK-6 (v4): the actions of a focused part are titled and grouped by their part,
    /// the others by their menu, and both keymaps offer them.
    #[test]
    fn part_actions_have_titles_and_groups() {
        for (id, title, group) in [
            ("NextDiff", "Next Difference", "Diff"),
            ("PreviousDiff", "Previous Difference", "Diff"),
            ("Diff.ApplyLeftSide", "Accept Left Side", "Diff"),
            ("CompareTwoFiles", "Compare Two Files", "Diff"),
            ("Diff.ShowDiff", "Show Diff", "Commit"),
            ("Vcs.ToggleAmendCommitMode", "Amend Last Commit", "Commit"),
            (
                "ChangesView.GroupBy.Directory",
                "Group By Directory",
                "Commit",
            ),
            ("ChangesView.GroupBy.Module", "Group By Module", "Commit"),
            ("ChangesView.ShowCommitOptions", "Commit Options", "Commit"),
            ("Vcs.ShowMessageHistory", "Commit Message History", "Commit"),
            ("Git.Rename.Local.Branch", "Rename Branch…", "Git"),
            ("Git.Reword.Commit", "Edit Commit Message…", "Git"),
            ("Git.New.Branch.In.Log", "New Branch at Commit…", "Git"),
            ("Git.CreateNewBranch", "New Branch…", "Git"),
            (
                "JumpToLastWindow",
                "Jump to Last Tool Window",
                "Active Tool Window",
            ),
            ("CloseActiveTab", "Close Active Tab", "Active Tool Window"),
            ("RestoreDefaultLayout", "Restore Current Layout", "Layouts"),
            ("SelectIn", "Select in Project Tree", "Navigate"),
            ("Synchronize", "Reload All from Disk", "File"),
            ("SaveAs", "Save As…", "File"),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
                assert!(!keymap.shortcuts(id).is_empty(), "{id} has no shortcut");
            }
        }
    }

    /// SPEC-0335 AK-7 (behaviour 16): the changelist actions carry the open platform's titles in
    /// the group Commit; the two without a key are still offered.
    #[test]
    fn changelist_actions_have_titles() {
        for (id, title) in [
            ("ChangesView.NewChangeList", "New Changelist…"),
            ("ChangesView.Rename", "Edit Changelist…"),
            ("ChangesView.SetDefault", "Set Active Changelist"),
            ("ChangesView.RemoveChangeList", "Delete Changelist"),
            ("ChangesView.Move", "Move to Another Changelist…"),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some("Commit")), "{id}");
            assert!(
                !Keymap::bundled_macos().shortcuts(id).is_empty() || UNBOUND_ACTIONS.contains(&id),
                "{id} is not offered"
            );
        }
        for id in ["ChangesView.NewChangeList", "ChangesView.RemoveChangeList"] {
            assert!(Keymap::bundled_macos().shortcuts(id).is_empty(), "{id}");
            assert!(UNBOUND_ACTIONS.contains(&id), "{id}");
        }
    }

    /// SPEC-0333 AK-5 (behaviour 13): the layout actions have their titles in the group Layouts;
    /// the ones without a key are still offered.
    #[test]
    fn layout_actions_have_titles() {
        for (id, title) in [
            ("RestoreDefaultLayout", "Restore Current Layout"),
            ("RestoreFactoryDefaultLayout", "Restore Default Layout"),
            ("StoreDefaultLayout", "Save Changes in Current Layout"),
            ("StoreNewLayout", "Save Current Layout as New…"),
        ] {
            assert_eq!(
                action_title(id),
                (title.to_owned(), Some("Layouts")),
                "{id}"
            );
            assert!(
                !Keymap::bundled_macos().shortcuts(id).is_empty() || UNBOUND_ACTIONS.contains(&id),
                "{id} is not offered"
            );
        }
    }

    /// SPEC-0084 AK-17 (v5): the diff viewer's own actions carry the titles of the open platform
    /// in the group of their part, are found by them, and keep the keys of the bundled keymaps.
    #[test]
    fn diff_viewer_actions_have_titles_and_keys() {
        let (mac, other) = (Keymap::bundled_macos(), Keymap::bundled_default());
        let keys = |keymap: &Keymap, id: &str, mac: bool| -> Vec<String> {
            keymap
                .shortcuts(id)
                .iter()
                .map(|s| s.display(mac))
                .collect()
        };
        for (id, title, on_mac, elsewhere) in [
            (
                "Diff.ApplyLeftSide",
                "Accept Left Side",
                "⌃⌘→",
                "Ctrl+Alt+R",
            ),
            (
                "Diff.ApplyRightSide",
                "Accept Right Side",
                "⌃⌘←",
                "Ctrl+Alt+A",
            ),
            (
                "Diff.FocusOppositePane",
                "Select Opposite Diff Pane",
                "⌃⇧⇥",
                "Ctrl+Shift+Tab",
            ),
            (
                "Diff.NextChange",
                "Compare Next File",
                "⌃⇧→",
                "Alt+Shift+Right",
            ),
            (
                "Diff.PrevChange",
                "Compare Previous File",
                "⌃⇧←",
                "Alt+Shift+Left",
            ),
            (
                "Diff.ShowSettingsPopup",
                "Show Diff Settings Popup…",
                "⇧⌘D",
                "Ctrl+Shift+D",
            ),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some("Diff")), "{id}");
            assert_eq!(keys(&mac, id, true), [on_mac], "{id}");
            assert_eq!(keys(&other, id, false), [elsewhere], "{id}");
            assert_eq!(first_action(&title.to_lowercase()).id, id);
        }
    }

    /// SPEC-0336 AK-9: the seven file property actions are found by their titles under their
    /// groups, and no bundled keymap gives one a shortcut.
    #[test]
    fn file_property_actions_have_titles() {
        let (mac, other) = (Keymap::bundled_macos(), Keymap::bundled_default());
        for (id, title, group) in [
            (
                "ChangeFileEncodingAction",
                "File Encoding",
                "File Properties",
            ),
            (
                "ToggleReadOnlyAttribute",
                "Toggle Read-Only Attribute",
                "File Properties",
            ),
            (
                "ConvertToWindowsLineSeparators",
                "CRLF - Windows (\\r\\n)",
                "Line Separators",
            ),
            (
                "ConvertToUnixLineSeparators",
                "LF - Unix and macOS (\\n)",
                "Line Separators",
            ),
            (
                "ConvertToMacLineSeparators",
                "CR - Classic Mac OS (\\r)",
                "Line Separators",
            ),
            ("AddBom", "Add BOM", "File Properties"),
            ("RemoveBom", "Remove BOM", "File Properties"),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            assert!(UNBOUND_ACTIONS.contains(&id), "{id}");
            assert!(mac.shortcuts(id).is_empty(), "{id}");
            assert!(other.shortcuts(id).is_empty(), "{id}");
            assert_eq!(first_action(&title.to_lowercase()).id, id);
        }
    }

    /// SPEC-0004 AK-8 (v5): the file type actions are found by their titles, and no bundled
    /// keymap gives one a shortcut.
    #[test]
    fn file_type_actions_have_titles() {
        let (mac, other) = (Keymap::bundled_macos(), Keymap::bundled_default());
        for (id, title, group) in [
            (
                "OverrideFileTypeAction",
                "Override File Type",
                "Mark File As",
            ),
            (
                "ReverteOverrideFileTypeAction",
                "Revert File Type Override",
                "Mark File As",
            ),
            (
                "AssociateWithFileType",
                "Associate with File Type…",
                "File Types",
            ),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            assert!(UNBOUND_ACTIONS.contains(&id), "{id}");
            assert!(mac.shortcuts(id).is_empty(), "{id}");
            assert!(other.shortcuts(id).is_empty(), "{id}");
            assert_eq!(first_action(&title.to_lowercase()).id, id);
        }
    }

    /// SPEC-0331 AK-9: the bookmark actions have their titles, are found by them, and keep the
    /// shortcuts of the bundled keymap.
    #[test]
    fn bookmark_actions_have_titles_and_shortcuts() {
        let mac = Keymap::bundled_macos();
        let shortcut = |id: &str| -> Vec<String> {
            mac.shortcuts(id).iter().map(|s| s.display(true)).collect()
        };
        for (id, title, keys) in [
            ("ToggleBookmark", "Toggle Bookmark", "F3"),
            (
                "ToggleBookmarkWithMnemonic",
                "Toggle Bookmark with Mnemonic",
                "⌥F3",
            ),
            ("ShowBookmarks", "Show Bookmarks", "⌘F3"),
            ("GotoBookmark0", "Go to Bookmark 0", "⌃0"),
            ("GotoBookmark9", "Go to Bookmark 9", "⌃9"),
            ("ToggleBookmark0", "Toggle Bookmark 0", "⌃⇧0"),
            ("ToggleBookmark9", "Toggle Bookmark 9", "⌃⇧9"),
        ] {
            assert_eq!(
                action_title(id),
                (title.to_owned(), Some("Bookmarks")),
                "{id}"
            );
            assert_eq!(shortcut(id), [keys], "{id}");
            assert_eq!(first_action(&title.to_lowercase()).id, id);
        }
        for digit in 0..=9 {
            for (id, title) in [
                (
                    format!("GotoBookmark{digit}"),
                    format!("Go to Bookmark {digit}"),
                ),
                (
                    format!("ToggleBookmark{digit}"),
                    format!("Toggle Bookmark {digit}"),
                ),
            ] {
                assert_eq!(action_title(&id), (title, Some("Bookmarks")));
                assert_eq!(mac.shortcuts(&id).len(), 1, "{id}");
            }
        }
        // Without a shortcut in the keymap, and still offered.
        for (id, title) in [
            ("GotoNextBookmark", "Go to Next Bookmark"),
            ("GotoPreviousBookmark", "Go to Previous Bookmark"),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some("Bookmarks")));
            assert!(mac.shortcuts(id).is_empty());
            assert!(UNBOUND_ACTIONS.contains(&id));
            assert_eq!(first_action(&title.to_lowercase()).id, id);
        }
        // The tool window under its name.
        assert_eq!(
            action_title("ActivateBookmarksToolWindow"),
            ("Bookmarks".to_owned(), Some("Tool Windows"))
        );
        assert_eq!(shortcut("ActivateBookmarksToolWindow"), ["⌘2"]);
    }

    /// SPEC-0061 AK-v17b: the Notifications window has its title and is offered by Find Action
    /// without a key.
    #[test]
    fn notifications_action_has_its_title() {
        let id = "ActivateNotificationsToolWindow";
        assert_eq!(
            action_title(id),
            ("Notifications".to_owned(), Some("Tool Windows"))
        );
        assert!(Keymap::bundled_macos().shortcuts(id).is_empty());
        assert!(UNBOUND_ACTIONS.contains(&id));
        assert_eq!(first_action("notifications").id, id);
    }

    /// SPEC-0334 AK-10: the TODO window and its grouping toggles have their titles and are
    /// offered by Find Action, the toggles with the keys of the keymaps.
    #[test]
    fn todo_actions_have_titles() {
        let mac = Keymap::bundled_macos();
        let default = Keymap::bundled_default();
        assert_eq!(
            action_title("ActivateTODOToolWindow"),
            ("TODO".to_owned(), Some("Tool Windows"))
        );
        assert!(mac.shortcuts("ActivateTODOToolWindow").is_empty());
        assert!(UNBOUND_ACTIONS.contains(&"ActivateTODOToolWindow"));
        assert_eq!(first_action("todo").id, "ActivateTODOToolWindow");
        for (id, title, on_mac) in [
            ("TodoViewGroupByShowModules", "Group by Modules", "⌃M"),
            ("TodoViewGroupByShowPackages", "Group by Packages", "⌃P"),
            ("TodoViewGroupByFlattenPackage", "Flatten Packages", "⌃F"),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some("TODO")), "{id}");
            let keys: Vec<String> = mac.shortcuts(id).iter().map(|s| s.display(true)).collect();
            assert_eq!(keys, [on_mac], "{id}");
            assert_eq!(default.shortcuts(id).len(), 1, "{id}");
            assert_eq!(first_action(&title.to_lowercase()).id, id);
        }
    }

    /// SPEC-0293 AK-11 (v6), SPEC-0081 AK-6 (v4), SPEC-0071 AK-13 (v10), SPEC-0069 AK-5 (v3): the
    /// navigation actions taken from the keymap are titled as their buttons and menu items and
    /// offered by Find Action.
    #[test]
    fn navigation_actions_from_the_keymap_have_titles() {
        let mac = Keymap::bundled_macos();
        for (id, title, group) in [
            ("UsageGrouping.UsageType", "Group by Usage Kind", "Find"),
            ("UsageGrouping.Module", "Group by Module", "Find"),
            ("UsageGrouping.Directory", "Group by Package", "Find"),
            (
                "UsageGrouping.FileStructure",
                "Group by Class and Method",
                "Find",
            ),
            ("QuickImplementations", "Quick Definition", "View"),
            ("GotoRelated", "Related Symbol…", "Navigate"),
            ("GotoTest", "Test", "Navigate"),
            ("RecentChangedFiles", "Recently Changed Files", "View"),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            assert!(
                !mac.shortcuts(id).is_empty() || UNBOUND_ACTIONS.contains(&id),
                "{id} is not offered"
            );
        }
        // Go to Test is found, without the key that reopens a closed tab on macOS.
        let files: Vec<PathBuf> = Vec::new();
        let sources = Sources {
            root: Path::new("."),
            files: &files,
            keymap: &mac,
            extra_actions: &[],
            recent: &[],
            text_options: TextOptions::default(),
            file_names: None,
        };
        let only = Categories {
            actions: true,
            ..Categories::NONE
        };
        let found = search_everywhere(sources, "Test", only, 50, None);
        let test = found
            .actions
            .unwrap()
            .into_iter()
            .find(|a| a.id == "GotoTest")
            .expect("Go to Test is offered");
        assert_eq!(test.name, "Test");
        assert_eq!(test.group.as_deref(), Some("Navigate"));
        assert_eq!(test.shortcut, None);
    }

    /// SPEC-0067 AK-10 (v9, behaviour 20): the endpoint search has its title, is found by it
    /// with either keymap and has no shortcut.
    #[test]
    fn endpoint_search_has_its_title() {
        use ferris_idea_ui::keymap::Keymap;
        assert_eq!(
            action_title("GotoUrlAction"),
            ("Endpoint…".to_owned(), Some("Navigate"))
        );
        assert!(UNBOUND_ACTIONS.contains(&"GotoUrlAction"));
        for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
            assert!(keymap.shortcuts("GotoUrlAction").is_empty());
            let found = search_everywhere(
                Sources {
                    root: Path::new("/nonexistent"),
                    files: &[],
                    keymap: &keymap,
                    extra_actions: &[],
                    recent: &[],
                    text_options: TextOptions::default(),
                    file_names: None,
                },
                "Endpoint",
                Categories {
                    actions: true,
                    ..Categories::NONE
                },
                50,
                None,
            );
            let hit = found
                .actions
                .unwrap_or_default()
                .into_iter()
                .find(|a| a.id == "GotoUrlAction")
                .expect("the endpoint search among the actions");
            assert_eq!(hit.name, "Endpoint…");
            assert_eq!(hit.group.as_deref(), Some("Navigate"));
            assert_eq!(hit.shortcut, None);
        }
    }

    /// SPEC-0275 AK-32, SPEC-0067 AK-9, SPEC-0093 AK-12, SPEC-0065 AK-7 (fourth package): the
    /// actions have their titles and a shortcut in both bundled keymaps.
    #[test]
    fn fourth_package_actions_have_titles() {
        use ferris_idea_ui::keymap::Keymap;
        for (id, title, group) in [
            ("ExpandTreeNode", "Expand Tree Node", "Tree"),
            ("CollapseTreeNode", "Collapse Tree Node", "Tree"),
            ("FullyExpandTreeNode", "Fully Expand Tree Node", "Tree"),
            ("ExpandAll", "Expand All", "Tree"),
            ("CollapseAll", "Collapse All", "Tree"),
            ("OpenInRightSplit", "Open in Right Split", "Tree"),
            ("ShowFilePath", "File Path", "Navigate"),
            ("NewElementSamePlace", "New in This Directory…", "File"),
            ("TextSearchAction", "Text…", "Navigate"),
            ("Console.History.Browse", "Browse Query History", "Database"),
            ("ShowSearchHistory", "Search History", "Find"),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            for keymap in [Keymap::bundled_macos(), Keymap::bundled_default()] {
                assert!(!keymap.shortcuts(id).is_empty(), "{id} has no shortcut");
            }
        }
        // The editor's two mouse actions read as their ids do, like the other editor actions
        // (SPEC-0130 v2 behaviour 5; SPEC-0067 v4 behaviour 4).
        assert_eq!(
            action_title("EditorAddOrRemoveCaret"),
            ("Add Or Remove Caret".to_owned(), None)
        );
        assert_eq!(
            action_title("EditorCreateRectangularSelectionOnMouseDrag"),
            (
                "Create Rectangular Selection On Mouse Drag".to_owned(),
                None
            )
        );
    }

    /// SPEC-0124 v6 AK-8: the second package of actions from the keymap coverage is titled and
    /// grouped, and both bundled keymaps give every one a key.
    #[test]
    fn second_package_actions_have_titles() {
        for (id, title, group) in [
            ("FindUsagesInFile", "Find Usages in File", "Find Usages"),
            (
                "GotoNextElementUnderCaretUsage",
                "Next Highlighted Usage",
                "Navigate",
            ),
            (
                "GotoPrevElementUnderCaretUsage",
                "Previous Highlighted Usage",
                "Navigate",
            ),
            ("HippieCompletion", "Cyclic Expand Word", "Code"),
            (
                "HippieBackwardCompletion",
                "Cyclic Expand Word (Backward)",
                "Code",
            ),
            (
                "EditorChooseLookupItem",
                "Insert Suggestion",
                "Completion List",
            ),
            (
                "EditorChooseLookupItemReplace",
                "Replace with Suggestion",
                "Completion List",
            ),
            ("SearchEverywhere.NextTab", "Next Tab", "Search Everywhere"),
            (
                "SearchEverywhere.PrevTab",
                "Previous Tab",
                "Search Everywhere",
            ),
            (
                "SearchEverywhere.SelectItem",
                "Open Selected Item",
                "Search Everywhere",
            ),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            assert!(
                !Keymap::bundled_macos().shortcuts(id).is_empty(),
                "{id} has no shortcut on macOS"
            );
            // The default keymap leaves the two usage steps without a key; they are offered.
            assert!(
                !Keymap::bundled_default().shortcuts(id).is_empty()
                    || UNBOUND_ACTIONS.contains(&id),
                "{id} is not offered with the default keymap"
            );
        }
        assert!(
            Keymap::bundled_default()
                .shortcuts("GotoNextElementUnderCaretUsage")
                .is_empty()
        );
    }

    /// SPEC-0083 v6 AK-15: the editor actions taken from the keymap have a key in the macOS keymap
    /// (or are offered without one) and the title of their menu neighbours.
    #[test]
    fn editor_actions_from_the_keymap_have_titles() {
        let mac = Keymap::bundled_macos();
        for (id, title, group) in [
            ("ExpandRegionRecursively", "Expand Recursively", "Folding"),
            (
                "CollapseRegionRecursively",
                "Collapse Recursively",
                "Folding",
            ),
            ("ExpandAllToLevel1", "Expand All to Level 1", "Folding"),
            ("ExpandAllToLevel5", "Expand All to Level 5", "Folding"),
            ("ExpandToLevel1", "Expand to Level 1", "Folding"),
            ("ExpandToLevel5", "Expand to Level 5", "Folding"),
            // SPEC-0083 v8 AK-24.
            ("ExpandDocComments", "Expand Doc Comments", "Folding"),
            ("CollapseDocComments", "Collapse Doc Comments", "Folding"),
            ("MethodUp", "Previous Method", "Navigate in File"),
            ("MethodDown", "Next Method", "Navigate in File"),
            ("JumpToLastChange", "Last Edit Location", "Navigate"),
            ("JumpToNextChange", "Next Edit Location", "Navigate"),
            ("MoveElementLeft", "Move Element Left", "Code"),
            ("MoveElementRight", "Move Element Right", "Code"),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            assert!(
                !mac.shortcuts(id).is_empty() || UNBOUND_ACTIONS.contains(&id),
                "{id} is not offered"
            );
        }
        // Editor actions without a menu neighbour read as their id's words.
        for (id, title) in [
            ("EditorMatchBrace", "Match Brace"),
            ("EditorScrollToCenter", "Scroll To Center"),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), None));
            assert!(!mac.shortcuts(id).is_empty(), "{id}");
        }
        assert!(mac.shortcuts("JumpToNextChange").is_empty());
        // Their menu neighbours for comparison.
        assert_eq!(
            action_title("ExpandRegion"),
            ("Expand".into(), Some("Folding"))
        );
    }

    /// SPEC-0031 v5 behaviour 12, AK-12: the files category searches its own list and the
    /// recently opened files; symbols still come from all files.
    #[test]
    fn the_files_category_searches_its_own_list_and_recent_files() {
        let root = std::env::temp_dir().join(format!("ferris-idea-ew-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (rel, text) in [
            ("app/shop.py", "class Shop:\n    pass\n"),
            (".venv/lib/shop_helpers.py", "class ShopHelper:\n    pass\n"),
            ("node_modules/shop/index.js", "function shopIndex() {}\n"),
        ] {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        let all: Vec<PathBuf> = [
            "app/shop.py",
            ".venv/lib/shop_helpers.py",
            "node_modules/shop/index.js",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        let own = vec![PathBuf::from("app/shop.py")];
        let keymap = Keymap::bundled_default();
        let names = |recent: &[PathBuf], query: &str| {
            let sources = Sources {
                root: &root,
                files: &all,
                keymap: &keymap,
                extra_actions: &[],
                recent,
                text_options: TextOptions::default(),
                file_names: Some(&own),
            };
            let cats = Categories {
                files: true,
                symbols: true,
                ..Categories::NONE
            };
            let found = search_everywhere(sources, query, cats, 20, None);
            let files: Vec<String> = found.files.unwrap().into_iter().map(|h| h.path).collect();
            let symbols: Vec<String> = found
                .symbols
                .unwrap_or_default()
                .into_iter()
                .map(|h| h.name)
                .collect();
            (files, symbols)
        };
        let (files, symbols) = names(&[], "shop");
        assert_eq!(files, ["app/shop.py"]);
        // Symbols are not limited by the files category's list.
        assert!(symbols.contains(&"ShopHelper".to_owned()), "{symbols:?}");
        // A file opened recently is found although it is not the project's own; one that is gone
        // is not.
        let recent = vec![
            PathBuf::from(".venv/lib/shop_helpers.py"),
            PathBuf::from("gone/shop.py"),
        ];
        let (files, _) = names(&recent, "shop");
        assert_eq!(files.len(), 2, "{files:?}");
        assert!(files.contains(&".venv/lib/shop_helpers.py".to_owned()));
        // An empty query lists it too.
        let (files, _) = names(&recent, "");
        assert_eq!(files, [".venv/lib/shop_helpers.py"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0127 AK-13 (v8), SPEC-0087 AK-10 (v8), SPEC-0088 AK-8 (v4), SPEC-0094 AK-4 (v2),
    /// SPEC-0061 AK-v12: the run, debug and window actions taken from the keymap have the title
    /// and the group of their menu and the keys of the bundled keymaps; full screen, which has
    /// a key on macOS only, is offered without one in the default keymap.
    #[test]
    fn run_debug_and_window_actions_from_the_keymap() {
        let mac = Keymap::bundled_macos();
        let default = Keymap::bundled_default();
        let keys = |keymap: &Keymap, id: &str, macos: bool| -> Vec<String> {
            keymap
                .shortcuts(id)
                .iter()
                .map(|s| s.display(macos))
                .collect()
        };
        for (id, title, group, on_mac, on_default) in [
            (
                "ToggleTemporaryLineBreakpoint",
                "Toggle Temporary Line Breakpoint",
                "Run",
                &["⌥⇧⌘F8"][..],
                &["Ctrl+Alt+Shift+F8"][..],
            ),
            (
                "ChooseDebugConfiguration",
                "Debug…",
                "Run",
                &["⌃⌥D"],
                &["Alt+Shift+F9"],
            ),
            (
                "RerunTests",
                "Rerun Tests",
                "Run",
                &["⌃⌘R", "⌥⇧R"],
                &["Alt+Shift+R"],
            ),
            (
                "NextProjectWindow",
                "Next Project Window",
                "Window",
                &["⌥⌘`"],
                &["Ctrl+Alt+]"],
            ),
            (
                "PreviousProjectWindow",
                "Previous Project Window",
                "Window",
                &["⌥⇧⌘`"],
                &["Ctrl+Alt+["],
            ),
            (
                "ToggleFullScreen",
                "Toggle Full Screen",
                "Window",
                &["⌃⌘F"],
                &[],
            ),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            assert_eq!(keys(&mac, id, true), on_mac, "{id} in the macOS keymap");
            assert_eq!(
                keys(&default, id, false),
                on_default,
                "{id} in the default keymap"
            );
            assert!(
                !on_default.is_empty() || UNBOUND_ACTIONS.contains(&id),
                "{id} is not offered with the default keymap"
            );
        }
        // Their chords run nothing else in the macOS keymap.
        for (chord, id) in [
            ("alt shift meta F8", "ToggleTemporaryLineBreakpoint"),
            ("control alt D", "ChooseDebugConfiguration"),
            ("control meta R", "RerunTests"),
            ("alt shift R", "RerunTests"),
            ("alt meta BACK_QUOTE", "NextProjectWindow"),
            ("alt shift meta BACK_QUOTE", "PreviousProjectWindow"),
            ("control meta F", "ToggleFullScreen"),
        ] {
            let stroke = ferris_idea_ui::keymap::Keystroke::parse(chord).unwrap();
            assert_eq!(mac.actions_for(&stroke, None), [id], "{chord}");
        }
    }

    /// SPEC-0124 AK-15 (v14) and the specifications of the seventh package: its actions have
    /// their titles and groups, and their keys in the bundled keymaps. The four actions of a
    /// list have chords in the macOS keymap only; elsewhere Home and End are the list's own
    /// keys and Find Action does not offer the actions.
    #[test]
    fn seventh_package_actions_have_titles() {
        let (mac, default) = (Keymap::bundled_macos(), Keymap::bundled_default());
        for (id, title, group, mac_key, default_key) in [
            (
                "List-selectFirstRow",
                "Select First Row",
                "List",
                "alt UP",
                "",
            ),
            (
                "List-selectLastRow",
                "Select Last Row",
                "List",
                "alt DOWN",
                "",
            ),
            (
                "List-selectFirstRowExtendSelection",
                "Extend Selection to First Row",
                "List",
                "alt shift UP",
                "",
            ),
            (
                "List-selectLastRowExtendSelection",
                "Extend Selection to Last Row",
                "List",
                "alt shift DOWN",
                "",
            ),
            (
                "GotoCustomRegion",
                "Custom Folding…",
                "Navigate",
                "alt meta PERIOD",
                "control alt PERIOD",
            ),
            ("SendEOF", "Send EOF", "Run", "meta D", "control D"),
            (
                "StopBackgroundProcesses",
                "Stop Background Processes…",
                "Run",
                "shift meta F2",
                "control shift F2",
            ),
            (
                "ExpressionTypeInfo",
                "Type Info",
                "View",
                "control shift P",
                "control shift P",
            ),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            for (keymap, key) in [(&mac, mac_key), (&default, default_key)] {
                let bound = !keymap.shortcuts(id).is_empty();
                assert_eq!(bound, !key.is_empty(), "{id}: {key:?}");
                assert!(!UNBOUND_ACTIONS.contains(&id), "{id}");
                if !key.is_empty() {
                    let stroke =
                        ferris_idea_ui::keymap::Keystroke::parse(key).expect("a keystroke");
                    assert!(
                        keymap.actions_for(&stroke, None).contains(&id),
                        "{id} on {key}"
                    );
                }
            }
        }
    }

    /// SPEC-0124 AK-14 (v13) and the specifications of the sixth package: its actions have
    /// their titles and groups, and their keys in the bundled keymaps; Activate Next and
    /// Previous Window have keys in the macOS keymap only and are offered without one elsewhere.
    #[test]
    fn sixth_package_actions_have_titles() {
        let (mac, default) = (Keymap::bundled_macos(), Keymap::bundled_default());
        for (id, title, group, mac_key, default_key) in [
            (
                "UsageFiltering.ReadAccess",
                "Show Read Access",
                "Find",
                "control R",
                "control R",
            ),
            (
                "UsageFiltering.WriteAccess",
                "Show Write Access",
                "Find",
                "control W",
                "control W",
            ),
            (
                "UsageFiltering.Imports",
                "Show Import Statements",
                "Find",
                "control I",
                "control I",
            ),
            (
                "UsageGrouping.DirectoryStructure",
                "Directory Structure",
                "Find",
                "control D",
                "control alt D",
            ),
            (
                "UsageGrouping.FlattenModules",
                "Flatten Modules in Usage View",
                "Find",
                "control O",
                "control alt O",
            ),
            (
                "UsageView.Include",
                "Include",
                "Find",
                "shift BACK_SPACE",
                "INSERT",
            ),
            ("UsageView.Exclude", "Exclude", "Find", "DELETE", "DELETE"),
            (
                "ExportToTextFile",
                "Export to Text File",
                "Find",
                "control O",
                "alt O",
            ),
            (
                "SearchEverywhere.NavigateToNextGroup",
                "Next Group Element",
                "Search Everywhere",
                "meta DOWN",
                "control DOWN",
            ),
            (
                "SearchEverywhere.NavigateToPrevGroup",
                "Prev Group Element",
                "Search Everywhere",
                "meta UP",
                "control UP",
            ),
            ("NextParameter", "Next Parameter", "Code", "TAB", "TAB"),
            (
                "PrevParameter",
                "Prev Parameter",
                "Code",
                "shift TAB",
                "shift TAB",
            ),
            (
                "NextWindow",
                "Activate Next Window",
                "Window",
                "meta BACK_QUOTE",
                "",
            ),
            (
                "PreviousWindow",
                "Activate Previous Window",
                "Window",
                "shift meta BACK_QUOTE",
                "",
            ),
            (
                "ViewSource",
                "Show Source",
                "Tree",
                "meta ENTER",
                "control ENTER",
            ),
            ("CopyElement", "Copy…", "Refactor", "F5", "F5"),
            (
                "ShowContent",
                "Show List of Tabs",
                "Window",
                "control shift DOWN",
                "alt DOWN",
            ),
            (
                "ShowTypeBookmarks",
                "Go to Mnemonic…",
                "Bookmarks",
                "alt meta F3",
                "control shift F11",
            ),
            (
                "NewScratchFile",
                "New Scratch File…",
                "File",
                "shift meta N",
                "control alt shift INSERT",
            ),
            (
                "ShowFilterPopup",
                "Show Filter Popup",
                "Find",
                "control alt F",
                "control alt F",
            ),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            for (keymap, key) in [(&mac, mac_key), (&default, default_key)] {
                let bound = !keymap.shortcuts(id).is_empty();
                assert_eq!(bound, !key.is_empty(), "{id}: {key:?}");
                assert!(
                    bound || UNBOUND_ACTIONS.contains(&id),
                    "{id} is not offered"
                );
                if !key.is_empty() {
                    let stroke =
                        ferris_idea_ui::keymap::Keystroke::parse(key).expect("a keystroke");
                    assert!(
                        keymap.actions_for(&stroke, None).contains(&id),
                        "{id} on {key}"
                    );
                }
            }
        }
    }

    /// SPEC-0124 AK-17 (v16) and the specifications of the ninth package: its actions have
    /// their titles and groups from their menus and their keys in both bundled keymaps.
    #[test]
    fn ninth_package_actions_have_titles() {
        let (mac, default) = (Keymap::bundled_macos(), Keymap::bundled_default());
        for (id, title, group, mac_key, default_key) in [
            (
                "SafeDelete",
                "Safe Delete…",
                "Refactor",
                "meta DELETE",
                "alt DELETE",
            ),
            (
                "ShowReformatFileDialog",
                "Reformat File…",
                "Code",
                "alt shift meta L",
                "control alt shift L",
            ),
            (
                "ForceRefresh",
                "Force Refresh",
                "File",
                "alt shift meta R",
                "control shift F5",
            ),
            (
                "SwitchHeaderSource",
                "Header/Source",
                "Navigate",
                "F10",
                "F10",
            ),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            for (keymap, key) in [(&mac, mac_key), (&default, default_key)] {
                let stroke = ferris_idea_ui::keymap::Keystroke::parse(key).expect("a keystroke");
                assert!(
                    keymap.actions_for(&stroke, None).contains(&id),
                    "{id} on {key}"
                );
            }
        }
    }

    /// SPEC-0124 AK-16 (v15) and the specifications of the eighth package: its actions have
    /// their titles and groups (from the menus where they have an item) and their keys in both
    /// bundled keymaps; a chord of two keystrokes is checked by its first one.
    #[test]
    fn eighth_package_actions_have_titles() {
        let (mac, default) = (Keymap::bundled_macos(), Keymap::bundled_default());
        for (id, title, group, mac_key, default_key) in [
            (
                "NextEditorTab",
                "Next Editor Tab",
                "Window",
                "control shift RIGHT",
                "alt shift RIGHT",
            ),
            (
                "PreviousEditorTab",
                "Previous Editor Tab",
                "Window",
                "control shift LEFT",
                "alt shift LEFT",
            ),
            (
                "MethodOverloadSwitchUp",
                "Previous Method Overload",
                "Code",
                "alt UP",
                "control UP",
            ),
            (
                "MethodOverloadSwitchDown",
                "Next Method Overload",
                "Code",
                "alt DOWN",
                "control DOWN",
            ),
            (
                "EditorChooseLookupItemDot",
                "Choose Lookup Item and Insert Dot",
                "Code",
                "control PERIOD",
                "control PERIOD",
            ),
            (
                "ClassNameCompletion",
                "Class Name Completion",
                "Code",
                "control alt SPACE",
                "control alt SPACE",
            ),
            (
                "EditorCompleteStatement",
                "Complete Current Statement",
                "Code",
                "shift meta ENTER",
                "control shift ENTER",
            ),
            (
                "Unwrap",
                "Unwrap/Remove…",
                "Code",
                "shift meta DELETE",
                "control shift DELETE",
            ),
            (
                "CodeInspection.OnEditor",
                "Inspect Current File",
                "Code",
                "alt shift I",
                "alt shift I",
            ),
            (
                "RunInspection",
                "Run Inspection by Name…",
                "Code",
                "alt shift meta I",
                "control alt shift I",
            ),
            (
                "IntroduceParameter",
                "Introduce Parameter…",
                "Extract/Introduce",
                "alt meta P",
                "control alt P",
            ),
            (
                "Compile",
                "Recompile",
                "Build",
                "shift meta F9",
                "control shift F9",
            ),
            (
                "QuickEvaluateExpression",
                "Quick Evaluate Expression",
                "Run",
                "alt meta F8",
                "control alt F8",
            ),
            (
                "VcsHistory.ShowAllAffected",
                "Show All Affected Files",
                "Git",
                "control meta A",
                "alt shift A",
            ),
            (
                "EditorShowGutterIconTooltip",
                "Show Gutter Icon Tooltip",
                "Editor",
                "alt shift 6, T",
                "alt shift 6, T",
            ),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            for (keymap, key) in [(&mac, mac_key), (&default, default_key)] {
                assert!(!keymap.shortcuts(id).is_empty(), "{id}: {key:?}");
                let first = key.split(',').next().unwrap_or(key);
                if key.contains(',') {
                    continue;
                }
                let stroke = ferris_idea_ui::keymap::Keystroke::parse(first).expect("a keystroke");
                assert!(
                    keymap.actions_for(&stroke, None).contains(&id),
                    "{id} on {key}"
                );
            }
        }
    }

    /// SPEC-0057 v6 AK-23, SPEC-0083 v7 AK-18, SPEC-0129 v4 AK-7: the fifth package's actions have
    /// their titles and a key in both keymaps; Cut up to Line End has one in the macOS keymap
    /// only and is offered without one elsewhere.
    #[test]
    fn fifth_package_actions_have_titles() {
        let (mac, default) = (Keymap::bundled_macos(), Keymap::bundled_default());
        for (id, title, group, mac_key, default_key) in [
            (
                "EditorCutLineEnd",
                "Cut up to Line End",
                "Edit",
                "control K",
                "",
            ),
            (
                "EditorToggleInsertState",
                "Toggle Insert/Overwrite",
                "Edit",
                "INSERT",
                "INSERT",
            ),
            (
                "AutoIndentLines",
                "Auto-Indent Lines",
                "Code",
                "control alt I",
                "control alt I",
            ),
            (
                "EditorContextInfo",
                "Context Info",
                "View",
                "control shift Q",
                "alt Q",
            ),
            (
                "CollapseBlock",
                "Fold Code Block",
                "Folding",
                "shift meta PERIOD",
                "control shift PERIOD",
            ),
            (
                "CollapseSelection",
                "Fold Selection / Remove Region",
                "Folding",
                "meta PERIOD",
                "control PERIOD",
            ),
            (
                "EditorIncreaseFontSizeGlobal",
                "Increase Font Size in All Editors",
                "View",
                "control shift PERIOD",
                "alt shift PERIOD",
            ),
            (
                "EditorDecreaseFontSizeGlobal",
                "Decrease Font Size in All Editors",
                "View",
                "control shift COMMA",
                "alt shift COMMA",
            ),
        ] {
            assert_eq!(action_title(id), (title.to_owned(), Some(group)), "{id}");
            // The keys are named for the reader; the keymap tests hold their notation.
            for (keymap, key) in [(&mac, mac_key), (&default, default_key)] {
                let bound = !keymap.shortcuts(id).is_empty();
                assert_eq!(bound, !key.is_empty(), "{id}: {key:?}");
                assert!(
                    bound || UNBOUND_ACTIONS.contains(&id),
                    "{id} is not offered"
                );
            }
        }
    }
}
