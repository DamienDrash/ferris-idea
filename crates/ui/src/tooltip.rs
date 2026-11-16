//! Tooltips of the application window (`SPEC-0294`): the registry of icon-only controls with their
//! titles and actions, the tooltip text made from the keymap in use, the texts of gutter icons and
//! stripe buttons, and what the window shows and declares for its inspection (MCP `ui_tooltips`, the
//! window self-test). The shared view that draws a tooltip is [`view`] (feature `view`).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::keymap::Keymap;
use crate::theme::Rgba;
use crate::tool_windows::Rect;

/// How long the pointer rests on a control before its tooltip appears (behaviour 6; the toolkit's
/// fixed delay).
pub const SHOW_DELAY_MS: u64 = 500;

/// Gap between the pointer's tip and the top of the tooltip box, so the pointer never covers the
/// text (behaviour 7).
pub const POINTER_GAP: f32 = 20.0;

/// Widest tooltip box; longer lines end in "…" (behaviour 8).
pub const MAX_WIDTH: f32 = 400.0;

/// Longest detail line kept, in chars (a long condition or log message).
pub const MAX_DETAIL_CHARS: usize = 120;

/// One control with a tooltip: its name (the element id, unique in its area), the title and the
/// action whose shortcut follows the title (behaviours 2 and 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Control {
    /// Element id and name in the inspection.
    pub name: &'static str,
    /// What the control does.
    pub title: &'static str,
    /// The action it runs, for its shortcut.
    pub action: Option<&'static str>,
    /// A second line: what the control shows or what a click does.
    pub detail: Option<&'static str>,
}

impl Control {
    /// A control without action or detail.
    pub const fn new(name: &'static str, title: &'static str) -> Self {
        Self {
            name,
            title,
            action: None,
            detail: None,
        }
    }

    /// With the action whose shortcut follows the title.
    pub const fn action(mut self, action: &'static str) -> Self {
        self.action = Some(action);
        self
    }

    /// With a second line.
    pub const fn detail(mut self, detail: &'static str) -> Self {
        self.detail = Some(detail);
        self
    }
}

const fn c(name: &'static str, title: &'static str) -> Control {
    Control::new(name, title)
}

/// The main toolbar (`SPEC-0061`, `SPEC-0087`).
pub const MAIN_TOOLBAR: &[Control] = &[
    c("project-widget", "Recent Projects").detail("Click to open or switch projects"),
    c("branch-widget", "Git Branches").detail("Click for branches and Git actions"),
    c("run-widget", "Run/Debug Configurations").action("ChooseRunConfiguration"),
    c("run-button", "Run").action("Run"),
    c("stop-button", "Stop").action("Stop"),
    // SPEC-0061 v4 behaviour 14.
    c("debug-button", "Debug").action("Debug"),
    // SPEC-0061 v5 behaviour 19.
    c("run-more", "More Actions").detail("Profile, run or debug the current file"),
    c("search-everywhere-button", "Search Everywhere").action("SearchEverywhere"),
    c("settings-button", "Settings").action("ShowSettings"),
];

/// The header of a tool window (`SPEC-0061` v4 behaviours 9 and 11).
pub const TOOL_WINDOW_HEADER: &[Control] = &[
    c("tool-window-options", "Options").detail("Move or resize the window"),
    c("tool-window-hide", "Hide").action("HideActiveWindow"),
    c("project-view-chooser", "Choose View").detail("Project or Project Files"),
    c("project-new", "New").action("NewElement"),
    c("project-locate", "Select Opened File")
        .detail("Shows the file of the active tab in the tree"),
    c("project-expand-all", "Expand All"),
    c("project-collapse-all", "Collapse All"),
    c("tool-window-more", "More Tool Windows")
        .detail("Every tool window, also those without a button"),
];

/// The Debug tool window's toolbar (`SPEC-0194` v4 behaviour 11).
pub const DEBUG_TOOLBAR: &[Control] = &[
    c("debug-rerun", "Rerun").action("Rerun"),
    c("debug-resume", "Resume Program").action("Resume"),
    c("debug-pause", "Pause Program"),
    c("debug-stop", "Stop").action("Stop"),
    c("debug-step-over", "Step Over").action("StepOver"),
    c("debug-step-into", "Step Into").action("StepInto"),
    c("debug-step-out", "Step Out").action("StepOut"),
    c("debug-run-to-cursor", "Run to Cursor").action("RunToCursor"),
    c("debug-view-breakpoints", "View Breakpoints…").action("ViewBreakpoints"),
    c("debug-mute-breakpoints", "Mute Breakpoints"),
    c("debug-more", "More").detail("Force steps, Smart Step Into, Reset Frame and more"),
    // The console's side bar and the expression field (SPEC-0091 v12 behaviours 36, 38).
    c("debug-console-up", "Up the Stack Trace").action("PreviousOccurence"),
    c("debug-console-down", "Down the Stack Trace").action("NextOccurence"),
    c("debug-console-wrap", "Soft-Wrap"),
    c("debug-console-end", "Scroll to the End"),
    c("debug-console-clear", "Clear All"),
    c("debug-fragment", "Code Fragment Mode")
        .detail("Several statements; Enter starts a new line, ⌘↩ evaluates"),
];

/// The Run tool window (`SPEC-0087`, `SPEC-0088`).
pub const RUN_TOOLBAR: &[Control] = &[
    // SPEC-0087 v6 behaviour 12.
    c("run-rerun", "Rerun").action("Rerun"),
    c("run-stop", "Stop").action("Stop"),
    c("run-scroll-end", "Scroll to End").detail("Follows new output while on"),
    c("run-soft-wrap", "Soft-Wrap"),
    c("run-clear", "Clear").detail("Empties the console of this tab, not the run"),
    c("run-window-more", "More").detail("Copy Command Line, Open Run Configuration"),
    c("show-passed", "Show Passed"),
    // SPEC-0088 v3 behaviours 7–9.
    c("test-rerun-failed", "Rerun Failed Tests")
        .detail("Runs only the tests that failed in the last run"),
    c("test-sort", "Sort").detail("As run, alphabetically or by duration"),
    c("test-expand-all", "Expand All"),
    c("test-collapse-all", "Collapse All"),
    c("test-history", "Test History").detail("The last 10 runs of this configuration"),
    c("run-tab-close", "Close Tab"),
];

/// The Find tool window of Find Usages (`SPEC-0293` behaviour 7).
pub const FIND_USAGES_TOOLBAR: &[Control] = &[
    c("find-usages-rerun", "Rerun Find Usages"),
    c("find-usages-stop", "Stop").action("Stop"),
    c("find-usages-previous", "Previous Occurrence").action("PreviousOccurence"),
    c("find-usages-next", "Next Occurrence").action("NextOccurence"),
    c("find-usages-expand", "Expand All").action("ExpandAll"),
    c("find-usages-collapse", "Collapse All").action("CollapseAll"),
    c("find-usages-group", "Group by File"),
    // Their keys act while the window has the focus (SPEC-0293 v6 behaviour 18).
    c("find-usages-group-kind", "Group by Usage Kind").action("UsageGrouping.UsageType"),
    c("find-usages-group-module", "Group by Module").action("UsageGrouping.Module"),
    c("find-usages-group-package", "Group by Package").action("UsageGrouping.Directory"),
    c("find-usages-group-structure", "Group by Class and Method")
        .action("UsageGrouping.FileStructure"),
    // The filters, the nested folders, the module groups and the export (SPEC-0293 v9
    // behaviours 22–24 and 26); their keys act while the window has the focus.
    c("find-usages-show-read", "Show Read Access").action("UsageFiltering.ReadAccess"),
    c("find-usages-show-write", "Show Write Access").action("UsageFiltering.WriteAccess"),
    c("find-usages-show-imports", "Show Import Statements").action("UsageFiltering.Imports"),
    c("find-usages-group-dirs", "Directory Structure").action("UsageGrouping.DirectoryStructure"),
    c("find-usages-flatten-modules", "Flatten Modules").action("UsageGrouping.FlattenModules"),
    c("find-usages-export", "Export to Text File").action("ExportToTextFile"),
    c("find-usages-preview", "Preview Source"),
    c("find-usages-close", "Close"),
    c("find-usages-hide", "Hide").action("HideActiveWindow"),
];

/// The toolbar of the TODO tool window (`SPEC-0334` behaviours 16, 17, 19–21).
pub const TODO_TOOLBAR: &[Control] = &[
    c("todo-previous", "Previous Occurrence").action("PreviousOccurence"),
    c("todo-next", "Next Occurrence").action("NextOccurence"),
    c("todo-filter-all", "Show All").detail("Shows the items of every pattern"),
    c("todo-filter-todo", "Show todo Only").detail("Shows only the items of the pattern todo"),
    c("todo-filter-fixme", "Show fixme Only").detail("Shows only the items of the pattern fixme"),
    // Their keys act while the window has the focus (SPEC-0334 behaviour 25).
    c("todo-group-modules", "Group by Modules").action("TodoViewGroupByShowModules"),
    c("todo-group-packages", "Group by Packages").action("TodoViewGroupByShowPackages"),
    c("todo-flatten", "Flatten Packages").action("TodoViewGroupByFlattenPackage"),
    c("todo-expand", "Expand All"),
    c("todo-collapse", "Collapse All"),
    c("todo-preview", "Preview Source"),
];

/// Find in Files (`SPEC-0066`) and the editor's find bar (`SPEC-0065`).
pub const FIND_BAR: &[Control] = &[
    c("fif-case", "Match Case"),
    c("fif-words", "Words"),
    c("fif-regex", "Regex"),
    c("fif-close", "Close"),
    c("find-case", "Match Case"),
    c("find-words", "Words"),
    c("find-regex", "Regex"),
    c("find-prev", "Previous Occurrence").action("FindPrevious"),
    c("find-next", "Next Occurrence").action("FindNext"),
    c("find-close", "Close"),
    // SPEC-0065 v4: the find bar's further controls.
    c("find-replace-toggle", "Show Replace Field").action("Replace"),
    c("find-history", "Recent Searches"),
    c("find-clear", "Clear"),
    c("find-newline", "New Line"),
    c(
        "find-filter",
        "Search In Selection, Comments, String Literals",
    ),
    c("find-more", "More"),
    c("replace-preserve-case", "Preserve Case"),
    c("replace-exclude", "Exclude"),
    // SPEC-0066 v3: Find in Files.
    c("fif-pin", "Pin Window (Open in Find Window)"),
    c("fif-preview", "Preview"),
    c("fif-filter", "Comments and String Literals"),
    c("fif-newline", "New Line"),
    // SPEC-0067 v5: Search Everywhere.
    c("ew-scope", "Include Non-Project Items"),
    c("ew-preview", "Preview"),
    c("ew-filter", "Filter"),
    c("ew-find-window", "Open in Find Window"),
    c("ew-case", "Match Case"),
    c("ew-words", "Words"),
    c("ew-regex", "Regex"),
    c("ew-right-split", "Open in Right Split"),
];

/// The Git log tool window (`SPEC-0086`).
pub const GIT_LOG_TOOLBAR: &[Control] = &[
    c("git-log-regex", "Regex"),
    c("git-log-case", "Match Case"),
    c("git-log-filter-remove", "Remove This Filter"),
    c("git-log-clear-filters", "Clear All Filters"),
    // The branches pane's action bar (`SPEC-0086` v7 behaviour 16).
    c("git-log-branch-new", "New Branch…"),
    c("git-log-branch-checkout", "Checkout Selected Branch"),
    c("git-log-branch-fetch", "Fetch"),
    c("git-log-branch-expand", "Expand All").action("ExpandAll"),
    c("git-log-branch-collapse", "Collapse All").action("CollapseAll"),
    // Favourites (`SPEC-0086` v10 behaviour 19).
    c("git-log-branch-favorite", "Mark/Unmark as Favorite"),
    c("git-log-branch-only-favorites", "Show Only Favorites"),
];

/// The Commit tool window (`SPEC-0063` v5 behaviour 9).
pub const COMMIT_TOOLBAR: &[Control] = &[
    c("commit-tool-refresh", "Refresh").action("Refresh"),
    c("commit-tool-rollback", "Rollback…").action("ChangesView.Revert"),
    c("commit-tool-diff", "Show Diff").action("Diff.ShowDiff"),
    c("commit-tool-group", "Group By"),
    c("commit-tool-expand", "Expand All").action("ExpandAll"),
    c("commit-tool-collapse", "Collapse All").action("CollapseAll"),
    c("commit-tool-options", "Commit Options"),
    // The changelist actions (`SPEC-0335` behaviour 16).
    c("commit-tool-changelists", "Changelists"),
];

/// The Database tool window (`SPEC-0010`).
pub const DATABASE_TOOLBAR: &[Control] = &[
    c("database-new", "New Data Source…"),
    c("database-edit", "Data Source Properties…"),
    c("database-refresh", "Refresh").action("Refresh"),
    // SPEC-0089 v5 behaviour 11.
    c("database-ddl", "Show DDL")
        .action("IDECraft.Database.ShowDdl")
        .detail("The selected table or view as SQL in a read-only tab"),
    // SPEC-0089 v5 behaviour 10.
    c("database-speed-search", "Speed Search")
        .detail("Type in the tree to find a name; ↑/↓ next match, Esc closes"),
    // SPEC-0089 v6 behaviours 13–14.
    c("database-console", "New Query Console")
        .action("Console.Open")
        .detail("A console bound to the selected connection"),
    c("database-table-data", "Edit Data").detail("The selected table's rows in a data editor"),
    c("database-doc", "Quick Documentation")
        .action("QuickJavaDoc")
        .detail("Columns, keys and indexes of the selection, from what the tree read"),
    c("database-filter", "Filter Schemas")
        .detail("Show only the schemas of the connection's schema filter, or all"),
    c("database-collapse", "Collapse All").action("CollapseAll"),
];

/// The bar above a SQL console (`SPEC-0093` v3 behaviours 9–13).
pub const SQL_TOOLBAR: &[Control] = &[
    c("sql-cancel", "Cancel Running Statement")
        .detail("Asks the database to end the statement that runs now"),
    c("sql-tx", "Transaction Mode").detail(
        "Auto commits every statement; Manual keeps one transaction open until Commit or Rollback",
    ),
    c("sql-commit", "Commit").detail("Commits the console's open transaction"),
    c("sql-rollback", "Rollback").detail("Rolls the console's open transaction back"),
    c("sql-history", "History")
        .detail("Statements run on this connection, newest first; a click puts one at the caret"),
];

/// The Services tool window: result tabs and the pager (`SPEC-0012`).
pub const SERVICES_TOOLBAR: &[Control] = &[
    c("services-first", "First Page"),
    c("services-previous", "Previous Page"),
    c("services-next", "Next Page"),
    c("services-last", "Last Page"),
    c("services-pin", "Pin Tab"),
    c("services-close", "Close Tab"),
    // SPEC-0099 v3 behaviour 10.
    c("services-view-table", "Table View"),
    c("services-view-tree", "Tree View").detail("One node per row, its columns as children"),
    c("services-view-text", "Text View").detail("The page as a plain-text table to copy"),
    // SPEC-0190 v5 behaviour 3: the toolbar's actions that do not fit.
    c("services-more", "More").detail("Toolbar actions that do not fit the window"),
];

/// The Terminal tool window: tabs and its find bar (`SPEC-0062`, `SPEC-0206`).
pub const TERMINAL_TOOLBAR: &[Control] = &[
    c("terminal-new-tab", "New Tab"),
    c("terminal-tab-close", "Close Tab"),
    c("terminal-tab-ai", "Shared with the AI chat"),
    c("terminal-search-case", "Match Case"),
    c("terminal-search-prev", "Previous Occurrence").action("FindPrevious"),
    c("terminal-search-next", "Next Occurrence").action("FindNext"),
    c("terminal-search-close", "Close"),
];

/// The Problems tool window (`SPEC-0070`).
pub const PROBLEMS_TOOLBAR: &[Control] = &[
    c("problems-hide", "Hide").action("HideActiveWindow"),
    c(
        "problems-ai-attach",
        "Attach the selected problem, else all listed, to the AI chat",
    ),
    // The File tab's toolbar (SPEC-0092 v3 behaviour 8).
    c("problems-group", "Group by Inspection"),
    c("problems-sort-name", "Sort by Name").detail("Otherwise by severity, then line"),
    c("problems-show-warnings", "Show Warnings"),
    c("problems-show-weak", "Show Weak Warnings"),
    c("problems-quick-fixes", "Show Quick Fixes")
        .action("ShowIntentionActions")
        .detail("Opens the selected problem and its context actions"),
    // SPEC-0092 v4 behaviour 9.
    c("problems-preview", "Open Editor Preview")
        .detail("Shows the code of the selected problem next to the list"),
];

/// Close, hide and reload buttons of the other tool windows and views: Debug, Build Tools,
/// Endpoints, Hierarchy, the ER diagram and the plugins' panels.
pub const OTHER_TOOL_WINDOWS: &[Control] = &[
    c("debug-watch-remove", "Remove Watch"),
    c("build-tools-reload", "Reload All Projects"),
    // The build tools window's toolbar (SPEC-0100 v2 behaviours 5–7).
    c("build-tools-execute", "Execute Build Command")
        .detail("Runs a Maven goal or Gradle task line in the selected module"),
    c("build-tools-offline", "Toggle Offline Mode")
        .detail("Maven -o, Gradle --offline: only the local caches are used"),
    c("build-tools-skip-tests", "Toggle 'Skip Tests' Mode")
        .detail("Maven -DskipTests: tests are compiled but not run"),
    c("build-tools-expand", "Expand All"),
    c("build-tools-collapse", "Collapse All"),
    // SPEC-0100 v3 behaviours 13–16.
    c("build-tools-sources", "Download Sources and Documentation")
        .detail("Fetches the sources and Javadoc of the Maven and Gradle libraries"),
    c("build-tools-analyze", "Analyze Dependencies").detail(
        "Conflicts and every library of the selected module with the paths that bring it in",
    ),
    c("build-tools-diagram", "Show Dependencies Diagram")
        .detail("Draws the resolved tree; Export keeps it as Mermaid, SVG or PNG"),
    c("build-tools-link", "Link Build File")
        .detail("Reads a pom.xml or Gradle build file, also in an ignored folder"),
    c("build-tools-unlink", "Unlink Module")
        .detail("Leaves the selected module and the modules inside it out of the project"),
    // The badge over a changed build file (SPEC-0100 v2 behaviour 9).
    c("build-sync", "Sync Build Changes")
        .action("ExternalSystem.ProjectRefreshAction")
        .detail(
            "The build file changed: read the project model again and resolve its dependencies",
        ),
    c("build-sync-dismiss", "Hide Until the Next Change"),
    c("endpoints-reload", "Refresh"),
    c("hierarchy-close", "Close"),
    c("hierarchy-sort", "Sort Alphabetically"),
    c("hierarchy-expand", "Expand All"),
    c("hierarchy-collapse", "Collapse All"),
    c("hierarchy-refresh", "Refresh"),
    c("structure-inherited", "Show Inherited Members"),
    c("structure-anonymous", "Show Anonymous Classes"),
    c("structure-lambdas", "Show Lambdas"),
    c("er-close", "Close Diagram"),
    c("plugin-panels-refresh", "Refresh"),
    // The Notifications tool window's toolbar (SPEC-0061 v17 behaviour 64).
    c("notifications-clear-all", "Clear All").detail("Removes every notification from the list"),
];

/// Editor tabs (`SPEC-0059`) and the problem indicator in the editor's corner (`SPEC-0070` v4
/// behaviour 3).
pub const EDITOR_TABS: &[Control] = &[
    c("tab-close", "Close").action("CloseContent"),
    c("problem-indicator", "Problems")
        .action("ActivateProblemsViewToolWindow")
        .detail("Click to open the Problems tool window"),
    c("problem-previous", "Previous Highlighted Error").action("GotoPreviousError"),
    c("problem-next", "Next Highlighted Error").action("GotoNextError"),
];

/// Status bar widgets (`SPEC-0061` behaviour 5, behaviour 5).
pub const STATUS_BAR: &[Control] = &[
    c("status-caret", "Go to Line:Column…")
        .action("GotoLine")
        .detail("Caret line and column"),
    c("status-line-separator", "Line Separator").detail("How lines of this file end"),
    c("status-encoding", "File Encoding").detail("The encoding the file is read and saved in"),
    c("status-indent", "Indentation").detail("Indent of this file"),
    c("column-mode-widget", "Column Selection Mode")
        .action("EditorToggleColumnMode")
        .detail("Click to turn it off"),
    c("overwrite-mode-widget", "Overwrite Mode")
        .action("EditorToggleInsertState")
        .detail("Click to turn it off"),
    c("python-interpreter-widget", "Python Interpreter").detail("Click to choose another"),
];

/// One area of the window with its declared controls.
#[derive(Debug, Clone, Copy)]
pub struct Area {
    /// Name of the area.
    pub name: &'static str,
    /// Its controls.
    pub controls: &'static [Control],
}

/// Every area with declared controls (behaviour 9).
pub const AREAS: &[Area] = &[
    Area {
        name: "main toolbar",
        controls: MAIN_TOOLBAR,
    },
    Area {
        name: "debug",
        controls: DEBUG_TOOLBAR,
    },
    Area {
        name: "run",
        controls: RUN_TOOLBAR,
    },
    Area {
        name: "find usages",
        controls: FIND_USAGES_TOOLBAR,
    },
    Area {
        name: "find bar",
        controls: FIND_BAR,
    },
    Area {
        name: "git log",
        controls: GIT_LOG_TOOLBAR,
    },
    Area {
        name: "commit",
        controls: COMMIT_TOOLBAR,
    },
    Area {
        name: "database",
        controls: DATABASE_TOOLBAR,
    },
    Area {
        name: "sql console",
        controls: SQL_TOOLBAR,
    },
    Area {
        name: "services",
        controls: SERVICES_TOOLBAR,
    },
    Area {
        name: "terminal",
        controls: TERMINAL_TOOLBAR,
    },
    Area {
        name: "problems",
        controls: PROBLEMS_TOOLBAR,
    },
    Area {
        name: "todo",
        controls: TODO_TOOLBAR,
    },
    Area {
        name: "other tool windows",
        controls: OTHER_TOOL_WINDOWS,
    },
    Area {
        name: "editor tabs",
        controls: EDITOR_TABS,
    },
    Area {
        name: "status bar",
        controls: STATUS_BAR,
    },
    Area {
        name: "tool window header",
        controls: TOOL_WINDOW_HEADER,
    },
];

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// The control `name` of `area`; evaluated in a `const` block it makes a missing declaration a build
/// error (behaviour 9). Use [`declared!`](crate::declared).
pub const fn pick(area: &'static [Control], name: &str) -> &'static Control {
    let mut i = 0;
    while i < area.len() {
        if same(area[i].name, name) {
            return &area[i];
        }
        i += 1;
    }
    panic!("control without a declared tooltip (SPEC-0294 behaviour 9)")
}

/// The declared control `name` of the area `AREA` of [`crate::tooltip`], checked while building:
/// `declared!(DEBUG_TOOLBAR, "debug-rerun")`.
#[macro_export]
macro_rules! declared {
    ($area:ident, $name:literal) => {
        const { $crate::tooltip::pick($crate::tooltip::$area, $name) }
    };
}

/// The declared control called `name`, in any area.
pub fn find(name: &str) -> Option<&'static Control> {
    AREAS
        .iter()
        .flat_map(|a| a.controls.iter())
        .find(|c| c.name == name)
}

/// What a tooltip says: a title, the shortcut after it, detail lines below (behaviour 2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tip {
    /// Name of the action or what the control is.
    pub title: String,
    /// Shortcut in the keymap in use, empty without one.
    pub shortcut: String,
    /// Lines below the title.
    pub details: Vec<String>,
}

impl Tip {
    /// A title alone.
    pub fn plain(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// `title` with the first shortcut of `action` in `keymap` (behaviour 2).
    pub fn of_action(title: impl Into<String>, action: &str, keymap: &Keymap, macos: bool) -> Self {
        Self {
            title: title.into(),
            shortcut: keymap.shortcut_label(action, macos),
            details: Vec::new(),
        }
    }

    /// The tooltip of a declared control.
    pub fn of(control: &Control, keymap: &Keymap, macos: bool) -> Self {
        let mut tip = match control.action {
            Some(a) => Self::of_action(control.title, a, keymap, macos),
            None => Self::plain(control.title),
        };
        tip.details.extend(control.detail.map(str::to_owned));
        tip
    }

    /// With another line below, cut to [`MAX_DETAIL_CHARS`].
    pub fn with_detail(mut self, line: impl Into<String>) -> Self {
        let line: String = line.into();
        let line = if line.chars().count() > MAX_DETAIL_CHARS {
            let mut cut: String = line.chars().take(MAX_DETAIL_CHARS - 1).collect();
            cut.push('…');
            cut
        } else {
            line
        };
        self.details.push(line);
        self
    }

    /// The whole text: title, a space and the shortcut, each detail on its own line.
    pub fn text(&self) -> String {
        let mut out = self.title.clone();
        if !self.shortcut.is_empty() {
            out.push(' ');
            out.push_str(&self.shortcut);
        }
        for d in &self.details {
            out.push('\n');
            out.push_str(d);
        }
        out
    }
}

/// A tool window's stripe button: its title and the shortcut of its activation action (behaviour 3).
pub fn stripe(window: crate::tool_windows::ToolWindow, keymap: &Keymap, macos: bool) -> Tip {
    Tip::of_action(window.title(), window.action(), keymap, macos)
}

/// A ▶ run marker in the gutter (behaviour 4).
pub fn run_marker(keymap: &Keymap, macos: bool) -> Tip {
    Tip::of_action("Run", "RunClass", keymap, macos).with_detail("Click to run or debug")
}

/// A folding mark in the gutter (behaviour 4).
pub fn fold_mark(folded: bool) -> Tip {
    Tip::plain(if folded {
        "Click to unfold"
    } else {
        "Click to fold"
    })
}

/// What a breakpoint does besides stopping, for its tooltip.
#[derive(Debug, Clone, Copy, Default)]
pub struct BreakpointFacts<'a> {
    /// Switched off.
    pub disabled: bool,
    /// Condition expression.
    pub condition: Option<&'a str>,
    /// Pass count.
    pub hit_condition: Option<&'a str>,
    /// Message logged instead of stopping.
    pub log_message: Option<&'a str>,
    /// Why the running debug session could not bind it (`SPEC-0127` v5 behaviour 14).
    pub not_bound: Option<&'a str>,
}

/// A breakpoint in the gutter (behaviour 4).
pub fn breakpoint(facts: BreakpointFacts<'_>) -> Tip {
    let mut tip = Tip::plain(if facts.disabled {
        "Line breakpoint (disabled)"
    } else {
        "Line breakpoint"
    });
    if let Some(c) = facts.condition {
        tip = tip.with_detail(format!("Condition: {c}"));
    }
    if let Some(h) = facts.hit_condition {
        tip = tip.with_detail(format!("Pass count: {h}"));
    }
    if let Some(m) = facts.log_message {
        tip = tip.with_detail(format!("Logs: {m}"));
        tip = tip.with_detail("Does not suspend");
    }
    if let Some(r) = facts.not_bound {
        tip = tip.with_detail(format!("Not bound: {r}"));
    }
    tip.with_detail("Right click to edit")
}

/// Colours of the tooltip box (behaviour 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooltipColors {
    /// Box background.
    pub background: Rgba,
    /// Box border.
    pub border: Rgba,
    /// Title text.
    pub text: Rgba,
    /// Shortcut and detail text.
    pub shortcut: Rgba,
}

/// The tooltip shown at the moment (behaviour 10).
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    /// Which tooltip view.
    pub id: u64,
    /// Its whole text ([`Tip::text`]).
    pub text: String,
    /// Where its box is painted, in window coordinates.
    pub bounds: Rect,
    /// Where the pointer was.
    pub pointer: (f32, f32),
    /// Size of the window's content.
    pub window: (f32, f32),
}

impl Shown {
    /// Whether the box lies inside the window and away from the pointer (behaviour 7).
    pub fn well_placed(&self) -> bool {
        let b = self.bounds;
        let (px, py) = self.pointer;
        let inside = b.x >= 0.
            && b.y >= 0.
            && b.x + b.w <= self.window.0 + 0.5
            && b.y + b.h <= self.window.1 + 0.5;
        let under = px >= b.x && px <= b.x + b.w && py >= b.y && py <= b.y + b.h;
        inside && !under
    }
}

/// What a control declares: a registry entry, or a text made for this moment.
#[derive(Debug, Clone)]
enum Declared {
    Control(&'static Control),
    Tip(Tip),
}

thread_local! {
    static SHOWN: RefCell<Option<Shown>> = const { RefCell::new(None) };
    static DECLARED: RefCell<BTreeMap<String, Declared>> = const { RefCell::new(BTreeMap::new()) };
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static EPOCH: AtomicU64 = AtomicU64::new(0);

/// A fresh id for a tooltip view.
pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Hides every shown tooltip: a key was pressed (behaviour 6).
pub fn dismiss_all() {
    EPOCH.fetch_add(1, Ordering::Relaxed);
}

static SUPPRESSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// While `on`, no tooltip draws: a menu is open, and a tooltip of the control under the pointer
/// would lie over it (`SPEC-0294` v2 behaviour 11, OBS-0022 re-check of L28).
///
/// The window sets it at the start of every frame, which also ends the holds of the last frame
/// (see [`hold_this_frame`]).
pub fn set_suppressed(on: bool) {
    SUPPRESSED.store(on, Ordering::Relaxed);
    HELD.store(false, Ordering::Relaxed);
}

static HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Holds tooltips back for the frame being drawn: a view inside the window draws a menu of its
/// own (the Run window's ⋮ menu), which the window's flag does not know about (`SPEC-0294` v4
/// behaviour 11). Views call it from `render` while their menu is open; tooltips draw after the
/// views of the frame.
pub fn hold_this_frame() {
    HELD.store(true, Ordering::Relaxed);
}

/// Whether tooltips are held back at the moment (a menu is open).
pub fn suppressed() -> bool {
    SUPPRESSED.load(Ordering::Relaxed) || HELD.load(Ordering::Relaxed)
}

/// Changes with every [`dismiss_all`]; a tooltip made before the change draws nothing.
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Relaxed)
}

/// Records the tooltip `id` as shown.
pub fn record_shown(shown: Shown) {
    SHOWN.with(|s| *s.borrow_mut() = Some(shown));
}

/// Forgets the tooltip `id` (it was hidden or dropped).
pub fn clear_shown(id: u64) {
    SHOWN.with(|s| {
        let mut s = s.borrow_mut();
        if s.as_ref().is_some_and(|x| x.id == id) {
            *s = None;
        }
    });
}

/// The tooltip shown at the moment, if any.
pub fn shown() -> Option<Shown> {
    SHOWN.with(|s| s.borrow().clone())
}

/// Notes that the declared control `control` was drawn with its tooltip.
pub fn declare_control(control: &'static Control) {
    DECLARED.with(|d| {
        let mut d = d.borrow_mut();
        if !matches!(d.get(control.name), Some(Declared::Control(c)) if std::ptr::eq(*c, control)) {
            d.insert(control.name.to_owned(), Declared::Control(control));
        }
    });
}

/// Notes that the control `name` was drawn with the tooltip `tip`.
pub fn declare(name: &str, tip: &Tip) {
    DECLARED.with(|d| {
        let mut d = d.borrow_mut();
        match d.get_mut(name) {
            Some(Declared::Tip(t)) if t == tip => {}
            _ => {
                d.insert(name.to_owned(), Declared::Tip(tip.clone()));
            }
        }
    });
}

/// Every control drawn with a tooltip so far, by name, with its text in `keymap` (behaviour 10).
pub fn declared(keymap: &Keymap, macos: bool) -> Vec<(String, String)> {
    DECLARED.with(|d| {
        d.borrow()
            .iter()
            .map(|(name, decl)| {
                let text = match decl {
                    Declared::Control(c) => Tip::of(c, keymap, macos).text(),
                    Declared::Tip(t) => t.text(),
                };
                (name.clone(), text)
            })
            .collect()
    })
}

/// The tooltip text the control `name` declares, if it was drawn.
pub fn declared_text(name: &str, keymap: &Keymap, macos: bool) -> Option<String> {
    DECLARED.with(|d| {
        d.borrow().get(name).map(|decl| match decl {
            Declared::Control(c) => Tip::of(c, keymap, macos).text(),
            Declared::Tip(t) => t.text(),
        })
    })
}

/// The shared tooltip view on GPUI (behaviour 8).
#[cfg(feature = "view")]
pub mod view {
    use gpui::prelude::*;
    use gpui::{AnyView, App, Context, Window, canvas, div, px};

    use super::{
        MAX_WIDTH, POINTER_GAP, Shown, Tip, TooltipColors, clear_shown, epoch, next_id,
        record_shown,
    };
    use crate::theme::Rgba;
    use crate::tool_windows::Rect;

    fn color(c: Rgba) -> gpui::Hsla {
        gpui::rgba((c.rgb << 8) | u32::from(c.alpha)).into()
    }

    /// One tooltip: built when the pointer has rested on its control, dropped when it hides.
    pub struct TooltipView {
        id: u64,
        tip: Tip,
        colors: TooltipColors,
        epoch: u64,
    }

    impl TooltipView {
        /// A tooltip saying `tip` in `colors`, for `.tooltip(…)` of an element.
        pub fn build(tip: Tip, colors: TooltipColors, cx: &mut App) -> AnyView {
            cx.new(|_| Self {
                id: next_id(),
                tip,
                colors,
                epoch: epoch(),
            })
            .into()
        }
    }

    impl Render for TooltipView {
        fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            // A key was pressed since it appeared: it stays hidden (behaviour 6).
            // … and none draws over an open menu (behaviour 11).
            if epoch() != self.epoch || super::suppressed() {
                clear_shown(self.id);
                return div().into_any_element();
            }
            let k = self.colors;
            let id = self.id;
            let text = self.tip.text();
            let pointer = window.mouse_position();
            let pointer = (f32::from(pointer.x), f32::from(pointer.y));
            let size = window.viewport_size();
            let size = (f32::from(size.width), f32::from(size.height));
            let line = || div().whitespace_nowrap().overflow_hidden().text_ellipsis();
            // The gap above the box keeps it clear of the pointer; flipped above the pointer, the
            // box ends just over it (behaviour 7).
            div()
                .pt(px(POINTER_GAP))
                .child(
                    div()
                        .relative()
                        .max_w(px(MAX_WIDTH))
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(color(k.border))
                        .bg(color(k.background))
                        .text_color(color(k.text))
                        .text_size(px(12.))
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(line().child(self.tip.title.clone()))
                                .when(!self.tip.shortcut.is_empty(), |d| {
                                    d.child(
                                        line()
                                            .flex_none()
                                            .text_color(color(k.shortcut))
                                            .child(self.tip.shortcut.clone()),
                                    )
                                }),
                        )
                        .children(
                            self.tip
                                .details
                                .iter()
                                .map(|l| line().text_color(color(k.shortcut)).child(l.clone())),
                        )
                        .child(
                            canvas(
                                move |b, _, _| {
                                    record_shown(Shown {
                                        id,
                                        text: text.clone(),
                                        bounds: Rect {
                                            x: f32::from(b.origin.x),
                                            y: f32::from(b.origin.y),
                                            w: f32::from(b.size.width),
                                            h: f32::from(b.size.height),
                                        },
                                        pointer,
                                        window: size,
                                    })
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full(),
                        ),
                )
                .into_any_element()
        }
    }

    impl Drop for TooltipView {
        fn drop(&mut self) {
            clear_shown(self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_windows::ToolWindow;

    fn keymaps() -> [(Keymap, bool); 2] {
        [
            (Keymap::bundled_macos(), true),
            (Keymap::bundled_default(), false),
        ]
    }

    // SPEC-0294 AK-1, SPEC-0194 v4 AK-8.
    #[test]
    fn labels_follow_the_keymap_in_use() {
        let rerun = declared!(DEBUG_TOOLBAR, "debug-rerun");
        let mac = Keymap::bundled_macos();
        let default = Keymap::bundled_default();
        assert_eq!(Tip::of(rerun, &mac, true).text(), "Rerun ⌘R");
        assert_eq!(Tip::of(rerun, &default, false).text(), "Rerun Ctrl+F5");
        // Without a shortcut: the name alone.
        let pause = declared!(DEBUG_TOOLBAR, "debug-pause");
        assert_eq!(Tip::of(pause, &mac, true).text(), "Pause Program");
        // A user's own shortcut is shown at once.
        let mut user = crate::keymap::KeymapFile::user("mine", "Mac OS X 10.5+");
        user.set_action(
            "Rerun",
            Some(vec![
                crate::keymap::Shortcut::parse_keyboard("shift meta R").unwrap(),
            ]),
        );
        let changed = Keymap::bundled_macos().with_overrides(&user);
        assert_eq!(Tip::of(rerun, &changed, true).text(), "Rerun ⇧⌘R");
        // Details go on their own lines.
        let caret = declared!(STATUS_BAR, "status-caret");
        assert_eq!(
            Tip::of(caret, &mac, true).text(),
            "Go to Line:Column… ⌘L\nCaret line and column"
        );
    }

    // SPEC-0294 AK-2: every icon-only control of the toolbars is declared with a tooltip.
    #[test]
    fn every_declared_control_has_a_title_and_a_shortcut_for_its_action() {
        for area in AREAS {
            let mut names = std::collections::BTreeSet::new();
            for c in area.controls {
                assert!(!c.title.trim().is_empty(), "{} in {}", c.name, area.name);
                assert!(names.insert(c.name), "{} twice in {}", c.name, area.name);
                if let Some(a) = c.action {
                    assert!(
                        keymaps()
                            .iter()
                            .any(|(k, mac)| !k.shortcut_label(a, *mac).is_empty()),
                        "{a} of {} has no shortcut in either keymap",
                        c.name
                    );
                    for (k, mac) in keymaps() {
                        let tip = Tip::of(c, &k, mac);
                        assert!(tip.text().starts_with(c.title));
                    }
                }
            }
        }
        // Names are unique across areas, so the inspection can name a control alone.
        let all: Vec<&str> = AREAS
            .iter()
            .flat_map(|a| a.controls.iter().map(|c| c.name))
            .collect();
        let unique: std::collections::BTreeSet<_> = all.iter().collect();
        assert_eq!(all.len(), unique.len());
        // The debug toolbar has the ten buttons of SPEC-0194 v4 behaviour 11 and the ⋮ menu of
        // SPEC-0091 v11, and the console side bar and fragment button of v12.
        assert_eq!(DEBUG_TOOLBAR.len(), 17);
        assert_eq!(find("debug-more").map(|c| c.title), Some("More"));
        assert_eq!(find("debug-step-over").map(|c| c.title), Some("Step Over"));
    }

    // SPEC-0294 AK-3.
    #[test]
    fn stripe_buttons_name_their_window_and_shortcut() {
        let mac = Keymap::bundled_macos();
        let default = Keymap::bundled_default();
        assert_eq!(stripe(ToolWindow::Project, &mac, true).text(), "Project ⌘1");
        assert_eq!(
            stripe(ToolWindow::Project, &default, false).text(),
            "Project Alt+1"
        );
        for w in ToolWindow::ALL {
            let tip = stripe(w, &mac, true);
            assert_eq!(tip.title, w.title());
        }
    }

    // SPEC-0294 AK-4.
    #[test]
    fn gutter_icons_explain_themselves() {
        let mac = Keymap::bundled_macos();
        assert_eq!(
            run_marker(&mac, true).text(),
            "Run ⌃⇧R\nClick to run or debug"
        );
        assert_eq!(fold_mark(false).text(), "Click to fold");
        assert_eq!(fold_mark(true).text(), "Click to unfold");
        assert_eq!(
            breakpoint(BreakpointFacts::default()).text(),
            "Line breakpoint\nRight click to edit"
        );
        let full = breakpoint(BreakpointFacts {
            disabled: true,
            condition: Some("i > 3"),
            hit_condition: Some("2"),
            log_message: Some("i={i}"),
            not_bound: None,
        });
        assert_eq!(
            full.text(),
            "Line breakpoint (disabled)\nCondition: i > 3\nPass count: 2\nLogs: i={i}\nDoes not suspend\nRight click to edit"
        );
        let long = breakpoint(BreakpointFacts {
            condition: Some(&"x".repeat(300)),
            ..BreakpointFacts::default()
        });
        assert_eq!(long.details[0].chars().count(), MAX_DETAIL_CHARS);
        assert!(long.details[0].ends_with('…'));
    }

    // SPEC-0127 v5 AK-9: a breakpoint the session did not bind says why, before the edit hint.
    #[test]
    fn breakpoint_not_bound_line() {
        let tip = breakpoint(BreakpointFacts {
            condition: Some("n == 2"),
            not_bound: Some("no code at this line"),
            ..BreakpointFacts::default()
        });
        assert_eq!(
            tip.text(),
            "Line breakpoint\nCondition: n == 2\nNot bound: no code at this line\nRight click to edit"
        );
    }

    // SPEC-0294 AK-5 (the part without a window), behaviour 10.
    #[test]
    fn shown_and_declared_tooltips_are_kept_for_the_inspection() {
        let mac = Keymap::bundled_macos();
        declare_control(declared!(DEBUG_TOOLBAR, "debug-rerun"));
        declare("stripe-project", &stripe(ToolWindow::Project, &mac, true));
        assert_eq!(
            declared_text("debug-rerun", &mac, true).as_deref(),
            Some("Rerun ⌘R")
        );
        // A keymap change shows in the declared text at once.
        let default = Keymap::bundled_default();
        assert_eq!(
            declared_text("debug-rerun", &default, false).as_deref(),
            Some("Rerun Ctrl+F5")
        );
        assert!(
            declared(&mac, true)
                .iter()
                .any(|(n, t)| n == "stripe-project" && t == "Project ⌘1")
        );
        let id = next_id();
        let shown_tip = Shown {
            id,
            text: "Rerun ⌘R".into(),
            bounds: Rect {
                x: 10.,
                y: 40.,
                w: 80.,
                h: 22.,
            },
            pointer: (9., 19.),
            window: (800., 600.),
        };
        assert!(shown_tip.well_placed());
        // Under the pointer, or out of the window, is not.
        let under = Shown {
            pointer: (20., 50.),
            ..shown_tip.clone()
        };
        assert!(!under.well_placed());
        let outside = Shown {
            window: (60., 600.),
            ..shown_tip.clone()
        };
        assert!(!outside.well_placed());
        record_shown(shown_tip.clone());
        assert_eq!(shown(), Some(shown_tip));
        clear_shown(id + 1);
        assert!(shown().is_some(), "another tooltip's end leaves it");
        clear_shown(id);
        assert_eq!(shown(), None);
        let before = epoch();
        dismiss_all();
        assert_ne!(epoch(), before);
    }

    // SPEC-0294 v2 AK-9: while a menu is open, tooltips are held back.
    #[test]
    fn tooltips_are_held_back_while_a_menu_is_open() {
        set_suppressed(true);
        assert!(suppressed());
        set_suppressed(false);
        assert!(!suppressed());
        // SPEC-0294 v4 AK-11: a view's own menu holds them back for the frame; the next frame
        // without the menu shows them again.
        hold_this_frame();
        assert!(suppressed());
        set_suppressed(false);
        assert!(!suppressed());
    }
}
