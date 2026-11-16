//! The reference's main menus as data (`SPEC-0124`): labels, action ids and submenus. The menu bar
//! of `idecraft-workbench` shows them; Search Everywhere titles an action with its menu label
//! (`SPEC-0031` v3), so "Split Right" finds `SplitVertically` (hand test 39).

/// One entry of a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// Label and action id.
    Action(&'static str, &'static str),
    /// A line between groups.
    Separator,
    /// A submenu.
    Sub(&'static str, &'static [Entry]),
}

use Entry::{Action as A, Separator as S, Sub};

/// Title of the Window menu's submenu of tool window layouts (`SPEC-0333` behaviour 12).
pub const LAYOUTS: &str = "Layouts";

const TOOL_WINDOWS: &[Entry] = &[
    A("Project", "ActivateProjectToolWindow"),
    A("Commit", "ActivateCommitToolWindow"),
    A("Git", "ActivateVersionControlToolWindow"),
    A("Run", "ActivateRunToolWindow"),
    A("Debug", "ActivateDebugToolWindow"),
    A("Problems", "ActivateProblemsViewToolWindow"),
    A("Terminal", "ActivateTerminalToolWindow"),
    A("Services", "ActivateServicesToolWindow"),
    A("Database", "ActivateDatabaseToolWindow"),
    A("Plugins", "ActivatePluginsToolWindow"),
    A("Structure", "ActivateStructureToolWindow"),
    A("Find", "ActivateFindToolWindow"),
    // SPEC-0334 behaviour 13.
    A("TODO", "ActivateTODOToolWindow"),
    // SPEC-0061 v17 behaviour 64.
    A("Notifications", "ActivateNotificationsToolWindow"),
    S,
    A("Maven", "ActivateMavenToolWindow"),
    A("Gradle", "ActivateGradleToolWindow"),
    A("npm", "ActivatenpmToolWindow"),
    A("Endpoints", "ActivateEndpointsToolWindow"),
    A("Spring", "Spring.ToolWindow"),
    A("Python Packages", "ActivatePythonPackagesToolWindow"),
    A("Containers", "Containers.ToolWindow"),
    A("Profiler", "Profiler.ToolWindow"),
    A("Remote Host", "Remote.ToolWindow"),
    A("AI Chat", "IDECraft.AiChat"),
];

/// The menus after the application menu, in the reference's order (`SPEC-0124` v3, hand test 50).
/// Within File, Edit, Navigate, Refactor and Window the items the open platform's main-menu groups
/// also hold stand in those groups' order, with their separators and action texts (`SPEC-0124` v8
/// behaviour 11; intellij-community, `PlatformActions.xml`, `LangActions.xml` and the module
/// descriptors named in `docs/legal/PROVENANCE.md`).
pub const MENUS: &[(&str, &[Entry])] = &[
    (
        "File",
        &[
            A("New…", "NewElement"),
            // SPEC-0124 v13 behaviour 15.
            A("New Scratch File…", "NewScratchFile"),
            S,
            A("Open…", "OpenFile"),
            A("Recent Projects…", "ManageRecentProjects"),
            // SPEC-0124 v10 behaviour 13: in the open platform's group, after the recent projects.
            A("Close Project", "CloseProject"),
            A("Close All Projects", "CloseAllProjects"),
            A("Close Other Projects", "CloseOtherProjects"),
            A("Close Tab", "CloseContent"),
            S,
            A("Remote Development…", "Remote.OpenProject"),
            S,
            A("Settings…", "ShowSettings"),
            // SPEC-0124 v8 behaviour 11: the Plugins page of Settings.
            A("Plugins…", "IDECraft.SettingsPlugins"),
            // SPEC-0124 v12 behaviour 14: the open platform's group of file properties, right
            // after the settings (SPEC-0336).
            Sub(
                "File Properties",
                &[
                    A("File Encoding", "ChangeFileEncodingAction"),
                    A("Toggle Read-Only Attribute", "ToggleReadOnlyAttribute"),
                    Sub(
                        "Line Separators",
                        &[
                            A("CRLF - Windows (\\r\\n)", "ConvertToWindowsLineSeparators"),
                            A("LF - Unix and macOS (\\n)", "ConvertToUnixLineSeparators"),
                            A("CR - Classic Mac OS (\\r)", "ConvertToMacLineSeparators"),
                        ],
                    ),
                ],
            ),
            S,
            Sub(
                "Local History",
                &[A("Show History", "LocalHistory.ShowHistory")],
            ),
            S,
            A("Save All", "SaveAll"),
            // SPEC-0059 v9.3 behaviours 25 and 26.
            A("Save As…", "SaveAs"),
            A("Reload All from Disk", "Synchronize"),
            // SPEC-0124 v16 behaviour 18, SPEC-0059 v9.8 behaviour 30.
            A("Force Refresh", "ForceRefresh"),
        ],
    ),
    (
        "Edit",
        &[
            A("Undo", "$Undo"),
            A("Redo", "$Redo"),
            S,
            A("Cut", "$Cut"),
            A("Copy", "$Copy"),
            // The small editing commands of SPEC-0332, at the places of SPEC-0124 v9 behaviour 12.
            A("Copy Path/Reference…", "CopyReferencePopupGroup"),
            A("Paste", "$Paste"),
            A("Paste from History…", "PasteMultiple"),
            A("Delete", "$Delete"),
            S,
            // Acts on the open search bar (SPEC-0124 v8 behaviour 11).
            A("Search in Selection Only", "ToggleInSelection"),
            Sub(
                "Find",
                &[
                    A("Find…", "Find"),
                    A("Replace…", "Replace"),
                    A("Find Next Occurrence", "FindNext"),
                    A("Find Previous Occurrence", "FindPrevious"),
                    S,
                    A("Find in Files…", "FindInPath"),
                    A("Replace in Files…", "ReplaceInPath"),
                ],
            ),
            Sub(
                "Find Usages",
                &[
                    A("Find Usages", "FindUsages"),
                    A("Show Usages", "ShowUsages"),
                    S,
                    A("Find Usages in File", "FindUsagesInFile"),
                    A("Highlight Usages in File", "HighlightUsagesInFile"),
                ],
            ),
            S,
            A("Column Selection Mode", "EditorToggleColumnMode"),
            A("Select All", "$SelectAll"),
            A(
                "Add Carets to Ends of Selected Lines",
                "EditorAddCaretPerSelectedLine",
            ),
            A("Add Selection for Next Occurrence", "SelectNextOccurrence"),
            A("Unselect Occurrence", "UnselectPreviousOccurrence"),
            A("Select All Occurrences", "SelectAllOccurrences"),
            A("Extend Selection", "EditorSelectWord"),
            A("Shrink Selection", "EditorUnSelectWord"),
            S,
            A("Toggle Case", "EditorToggleCase"),
            A("Join Lines", "EditorJoinLines"),
            A("Duplicate Line or Selection", "EditorDuplicate"),
            A("Sort Lines", "EditorSortLines"),
            A("Reverse Lines", "EditorReverseLines"),
            A("Transpose", "EditorTranspose"),
            A("Delete Line", "EditorDeleteLine"),
            S,
            A("Indent Selection", "EditorIndentSelection"),
            A("Unindent Line or Selection", "EditorUnindentSelection"),
            Sub(
                "Convert Indents",
                &[
                    A("To Spaces", "ConvertIndentsToSpaces"),
                    A("To Tabs", "ConvertIndentsToTabs"),
                ],
            ),
            S,
            // SPEC-0124 v7 behaviour 10: the reference has this submenu near the end of Edit.
            Sub(
                "Bookmarks",
                &[
                    A("Toggle Bookmark", "ToggleBookmark"),
                    A(
                        "Toggle Bookmark with Mnemonic",
                        "ToggleBookmarkWithMnemonic",
                    ),
                    S,
                    A("Show Bookmarks", "ShowBookmarks"),
                    // SPEC-0124 v13 behaviour 15.
                    A("Go to Mnemonic…", "ShowTypeBookmarks"),
                    S,
                    A("Go to Next Bookmark", "GotoNextBookmark"),
                    A("Go to Previous Bookmark", "GotoPreviousBookmark"),
                ],
            ),
        ],
    ),
    (
        "View",
        &[
            Sub("Tool Windows", TOOL_WINDOWS),
            Sub("Appearance", &[A("Tool Window Bars", "ViewToolButtons")]),
            S,
            A("Quick Documentation", "QuickJavaDoc"),
            // SPEC-0081 v4 behaviour 6.
            A("Quick Definition", "QuickImplementations"),
            A("Parameter Info", "ParameterInfo"),
            // SPEC-0124 v14 behaviour 16, SPEC-0077 v7 behaviour 6.
            A("Type Info", "ExpressionTypeInfo"),
            A("Error Description", "ShowErrorDescription"),
            A("Recent Files", "RecentFiles"),
            // SPEC-0069 v3 behaviour 9.
            A("Recently Changed Files", "RecentChangedFiles"),
            S,
            A("Split Right", "SplitVertically"),
            A("Split Down", "SplitHorizontally"),
            A("Split and Move Right", "SplitAndMoveRight"),
            A("Split and Move Down", "SplitAndMoveDown"),
            A("Move Tab to Opposite Group", "MoveEditorToOppositeTabGroup"),
            A("Unsplit", "Unsplit"),
            A("Unsplit All", "UnsplitAll"),
            A("Goto Next Splitter", "NextSplitter"),
            A("Goto Previous Splitter", "PrevSplitter"),
            A("Next Tab", "NextTab"),
            A("Previous Tab", "PreviousTab"),
            S,
            A("Quick Switch Scheme…", "QuickChangeScheme"),
            A("Compact Mode", "ToggleCompactMode"),
        ],
    ),
    (
        "Navigate",
        &[
            A("Back", "Back"),
            A("Forward", "Forward"),
            S,
            A("Search Everywhere", "SearchEverywhere"),
            S,
            A("Class…", "GotoClass"),
            A("File…", "GotoFile"),
            A("Symbol…", "GotoSymbol"),
            A("Action…", "GotoAction"),
            // Search Everywhere on its Text tab (SPEC-0124 v8 behaviour 11).
            A("Text…", "TextSearchAction"),
            // Search Everywhere on its Endpoints tab (SPEC-0124 v10, SPEC-0067 v9 behaviour 20).
            A("Endpoint…", "GotoUrlAction"),
            A("Line:Column…", "GotoLine"),
            // SPEC-0124 v14 behaviour 16, SPEC-0083 v9 behaviour 24.
            A("Custom Folding…", "GotoCustomRegion"),
            S,
            A("Next Highlighted Error", "GotoNextError"),
            A("Previous Highlighted Error", "GotoPreviousError"),
            // SPEC-0071 v9 behaviour 14.
            A("Last Edit Location", "JumpToLastChange"),
            A("Next Edit Location", "JumpToNextChange"),
            S,
            Sub(
                "Navigate in File",
                &[
                    A("Next Method", "MethodDown"),
                    A("Previous Method", "MethodUp"),
                ],
            ),
            S,
            // SPEC-0059 v9.3 behaviour 24.
            A("Select in Project Tree", "SelectIn"),
            A("Jump to Navigation Bar", "ShowNavBar"),
            S,
            A("Declaration or Usages", "GotoDeclaration"),
            A("Implementation(s)", "GotoImplementation"),
            A("Type Declaration", "GotoTypeDeclaration"),
            A("Super Method", "GotoSuperMethod"),
            // SPEC-0071 v10 behaviours 15 and 17.
            A("Related Symbol…", "GotoRelated"),
            A("Test", "GotoTest"),
            // SPEC-0124 v16 behaviour 18, SPEC-0071 v12 behaviour 19.
            A("Header/Source", "SwitchHeaderSource"),
            S,
            A("File Structure", "FileStructurePopup"),
            A("Type Hierarchy", "TypeHierarchy"),
            A("Method Hierarchy", "MethodHierarchy"),
            A("Call Hierarchy", "CallHierarchy"),
            S,
            A("Spring Bean", "Spring.GotoBean"),
        ],
    ),
    (
        "Code",
        &[
            A("Code Completion", "CodeCompletion"),
            // SPEC-0124 v15 behaviour 17, SPEC-0072 v13 behaviour 48, SPEC-0056 v5 behaviour 20.
            A("Class Name Completion", "ClassNameCompletion"),
            // SPEC-0124 v17 behaviour 19, SPEC-0072 v14 behaviour 50.
            A("Insert Live Template…", "InsertLiveTemplate"),
            A("Complete Current Statement", "EditorCompleteStatement"),
            A("Show Context Actions", "ShowIntentionActions"),
            A("Override Methods…", "OverrideMethods"),
            A("Implement Methods…", "ImplementMethods"),
            A("Generate…", "Generate"),
            A("Surround With…", "SurroundWith"),
            // SPEC-0124 v15 behaviour 17, SPEC-0056 v5 behaviour 21.
            A("Unwrap/Remove…", "Unwrap"),
            A("Delegate Methods…", "DelegateMethods"),
            S,
            A("Comment with Line Comment", "CommentByLineComment"),
            A("Comment with Block Comment", "CommentByBlockComment"),
            A("Reformat Code", "ReformatCode"),
            // SPEC-0124 v16 behaviour 18, SPEC-0075 v6 behaviour 8.
            A("Reformat File…", "ShowReformatFileDialog"),
            A("Update Copyright…", "UpdateCopyright"),
            A("Open Resource Bundle Editor", "OpenResourceBundleEditor"),
            A("Optimize Imports", "OptimizeImports"),
            A("Reformat with Prettier", "ReformatWithPrettierAction"),
            A("Fix ESLint Problems", "Javascript.Linters.EsLint.Fix"),
            A("Move Statement Up", "MoveStatementUp"),
            A("Move Statement Down", "MoveStatementDown"),
            A("Move Line Up", "MoveLineUp"),
            A("Move Line Down", "MoveLineDown"),
            S,
            Sub(
                "Folding",
                &[
                    // SPEC-0083 v8 behaviour 23: every folding action, in groups.
                    A("Expand", "ExpandRegion"),
                    A("Collapse", "CollapseRegion"),
                    S,
                    A("Expand Recursively", "ExpandRegionRecursively"),
                    A("Collapse Recursively", "CollapseRegionRecursively"),
                    S,
                    A("Expand All", "ExpandAllRegions"),
                    A("Collapse All", "CollapseAllRegions"),
                    S,
                    Sub(
                        "Expand to Level",
                        &[
                            A("1", "ExpandToLevel1"),
                            A("2", "ExpandToLevel2"),
                            A("3", "ExpandToLevel3"),
                            A("4", "ExpandToLevel4"),
                            A("5", "ExpandToLevel5"),
                        ],
                    ),
                    Sub(
                        "Expand All to Level",
                        &[
                            A("1", "ExpandAllToLevel1"),
                            A("2", "ExpandAllToLevel2"),
                            A("3", "ExpandAllToLevel3"),
                            A("4", "ExpandAllToLevel4"),
                            A("5", "ExpandAllToLevel5"),
                        ],
                    ),
                    S,
                    A("Expand Doc Comments", "ExpandDocComments"),
                    A("Collapse Doc Comments", "CollapseDocComments"),
                    S,
                    A("Fold Selection / Remove Region", "CollapseSelection"),
                    A("Fold Code Block", "CollapseBlock"),
                ],
            ),
            S,
            A("Inspect Code…", "InspectCode"),
            // SPEC-0124 v15 behaviour 17, SPEC-0098 v10 behaviours 20 and 21.
            A("Inspect Current File", "CodeInspection.OnEditor"),
            A("Run Inspection by Name…", "RunInspection"),
        ],
    ),
    (
        "Refactor",
        &[
            A("Refactor This…", "Refactorings.QuickListPopupAction"),
            A("Rename…", "RenameElement"),
            A("Change Signature…", "ChangeSignature"),
            S,
            // SPEC-0124 v8 behaviour 11: the open platform's group, with the refactorings IDECraft has.
            Sub(
                "Extract/Introduce",
                &[
                    A("Introduce Variable…", "IntroduceVariable"),
                    A("Introduce Constant…", "IntroduceConstant"),
                    A("Introduce Field…", "IntroduceField"),
                    // SPEC-0124 v15 behaviour 17, SPEC-0095 v11 behaviour 14.
                    A("Introduce Parameter…", "IntroduceParameter"),
                    S,
                    A("Extract Method…", "ExtractMethod"),
                    S,
                    A("Extract Interface…", "ExtractInterface"),
                    A("Extract Superclass…", "ExtractSuperclass"),
                ],
            ),
            A("Inline…", "Inline"),
            S,
            A("Move…", "Move"),
            // SPEC-0124 v13 behaviour 15.
            A("Copy…", "CopyElement"),
            // SPEC-0124 v16 behaviour 18, SPEC-0095 v12 behaviours 15 and 16.
            A("Safe Delete…", "SafeDelete"),
            S,
            A("Pull Members Up…", "MembersPullUp"),
        ],
    ),
    // SPEC-0124 v3 behaviour 5: the build actions IDECraft has (SPEC-0100 behaviours 9 and 18).
    (
        "Build",
        &[
            A("Build Project", "CompileDirty"),
            // SPEC-0124 v15 behaviour 17, SPEC-0100 v5 behaviour 22.
            A("Recompile", "Compile"),
            S,
            A("Sync Build Changes", "ExternalSystem.ProjectRefreshAction"),
        ],
    ),
    (
        "Run",
        &[
            A("Run", "Run"),
            A("Debug", "Debug"),
            A("Run…", "ChooseRunConfiguration"),
            // SPEC-0087 v8 behaviour 16.
            A("Debug…", "ChooseDebugConfiguration"),
            A("Edit Configurations…", "editRunConfigurations"),
            A("Run Current File", "RunClass"),
            A("Debug Current File", "DebugClass"),
            A("Attach to Process…", "XDebugger.AttachToProcess"),
            A("Profile", "Profiler.ProfileSelected"),
            A("Profile Memory", "Profiler.ProfileMemory"),
            A("Attach Profiler to Process…", "Profiler.AttachToProcess"),
            // SPEC-0088 v4 behaviour 11.
            A("Rerun Tests", "RerunTests"),
            A("Stop", "Stop"),
            // SPEC-0124 v14 behaviour 16, SPEC-0061 v16 behaviour 62.
            A("Stop Background Processes…", "StopBackgroundProcesses"),
            S,
            A("Step Over", "StepOver"),
            A("Step Into", "StepInto"),
            A("Step Out", "StepOut"),
            A("Resume Program", "Resume"),
            A("Pause Program", "Pause"),
            A("Run to Cursor", "RunToCursor"),
            A("Evaluate Expression…", "EvaluateExpression"),
            // SPEC-0124 v15 behaviour 17, SPEC-0091 v19 behaviour 44.
            A("Quick Evaluate Expression", "QuickEvaluateExpression"),
            S,
            A("Toggle Line Breakpoint", "ToggleLineBreakpoint"),
            // SPEC-0127 v8 behaviour 18.
            A(
                "Toggle Temporary Line Breakpoint",
                "ToggleTemporaryLineBreakpoint",
            ),
            A("View Breakpoints…", "ViewBreakpoints"),
            A("Mute Breakpoints", "XDebugger.MuteBreakpoints"),
        ],
    ),
    (
        "Tools",
        &[
            A("New Terminal Tab", "Terminal.NewTab"),
            A("Split Terminal Right", "Terminal.SplitVertically"),
            A("Split Terminal Down", "Terminal.SplitHorizontally"),
            A("Rename Terminal Session", "Terminal.RenameSession"),
            Sub(
                "AI Assistant",
                &[
                    A("AI Chat", "IDECraft.AiChat"),
                    A("Explain Code", "IDECraft.AiExplainCode"),
                    A(
                        "Explain Terminal Output with AI",
                        "IDECraft.AiExplainTerminal",
                    ),
                    A(
                        "Attach Terminal Output to Chat",
                        "IDECraft.AiAttachTerminalCommand",
                    ),
                    A("Share Terminal Tab with Chat", "IDECraft.AiShareTerminal"),
                    A("Attach Run Output to Chat", "IDECraft.AiAttachRun"),
                    A("Attach Problems to Chat", "IDECraft.AiAttachProblems"),
                    A("Attach Git Diff to Chat", "IDECraft.AiAttachGitDiff"),
                ],
            ),
            A("Remote Host", "Remote.ToolWindow"),
            A("SSH Terminal", "Remote.OpenTerminal"),
            // The emergency stop of UI automation (SPEC-0319 behaviour 37).
            A("Stop UI Automation", "IDECraft.StopUiAutomation"),
            S,
            // The request at the caret of a `.http` file (SPEC-0096 v4 behaviour 7).
            Sub(
                "HTTP Client",
                &[
                    A("Send Request at Caret", "Console.Execute.Multiline"),
                    // Every request of the file, one after another (SPEC-0096 v5 behaviour 8).
                    A("Run All Requests in File", "IDECraft.HttpRunAllRequests"),
                ],
            ),
            Sub(
                "Database",
                &[
                    A("Explain Plan", "Sql.ExplainPlan"),
                    A("Explain Analyze", "Sql.ExplainAnalyze"),
                    A("Compare Schemas", "Sql.CompareSchemas"),
                    A("Compare With…", "Sql.CompareObjects"),
                    A(
                        "Report a Problem with This Database…",
                        "Database.ReportProblem",
                    ),
                    A("Export Connection…", "Database.ExportConnection"),
                    A("Open Migration Script", "Sql.OpenMigrationScript"),
                    A("Swap Comparison Sides", "Sql.SwapComparison"),
                    A("Cancel Schema Read", "Sql.CancelComparison"),
                    A(
                        "Include Destructive Changes…",
                        "Sql.IncludeDestructiveChanges",
                    ),
                    A("Show ER Diagram", "Sql.ShowErDiagram"),
                    A("Export Data…", "Sql.ExportData"),
                    A("New Console", "Console.Open"),
                    A("Choose Data Source", "Sql.ChooseDataSource"),
                    A(
                        "Ask Again for Unsafe Changes",
                        "Sql.AskAgainForUnsafeChanges",
                    ),
                ],
            ),
            Sub(
                "Python",
                &[
                    A("Select Interpreter…", "Python.SelectInterpreter"),
                    A("New Virtual Environment", "Python.CreateVirtualEnv"),
                ],
            ),
        ],
    ),
    (
        "Git",
        &[
            A("Commit…", "CheckinProject"),
            A("Push…", "Vcs.Push"),
            A("Update Project…", "Vcs.UpdateProject"),
            A("Pull…", "Git.Pull"),
            A("Fetch", "Git.Fetch"),
            S,
            A("Branches…", "Git.Branches"),
            // SPEC-0068 v9 behaviour 32.
            A("New Branch…", "Git.CreateNewBranch"),
            A("Stash Changes…", "Git.Stash"),
            A("Unstash Changes…", "Git.Unstash"),
            A("Interactively Rebase…", "Git.Interactive.Rebase"),
            A("Resolve Conflicts…", "Git.ResolveConflicts"),
            A("Continue", "Git.Continue"),
            A("Abort", "Git.Abort"),
            A("Skip Commit", "Git.Skip"),
            S,
            Sub("Current File", &[A("Annotate with Git Blame", "Annotate")]),
            S,
            A("VCS Operations…", "Vcs.QuickListPopupAction"),
        ],
    ),
    // SPEC-0124 v3 behaviour 6. On macOS the system adds the open windows to a menu of this name.
    (
        "Window",
        &[
            A("Minimize", "MinimizeCurrentWindow"),
            A("Zoom", "ZoomCurrentWindow"),
            // SPEC-0061 v12 behaviour 58; SPEC-0094 v2 behaviour 5.
            A("Toggle Full Screen", "ToggleFullScreen"),
            S,
            // The named tool window layouts (SPEC-0333 behaviour 12), where the open platform
            // has its layouts group. The menu bar puts the saved layouts after Default.
            Sub(
                LAYOUTS,
                &[
                    A("Default", "RestoreFactoryDefaultLayout"),
                    S,
                    A("Restore Current Layout", "RestoreDefaultLayout"),
                    A("Save Changes in Current Layout", "StoreDefaultLayout"),
                    A("Save Current Layout as New…", "StoreNewLayout"),
                ],
            ),
            S,
            // The focused tool window (SPEC-0167 behaviour 6, SPEC-0061 v6 behaviour 34).
            Sub(
                "Active Tool Window",
                &[
                    A("Hide Active Tool Window", "HideActiveWindow"),
                    A("Hide All Tool Windows", "HideAllWindows"),
                    // SPEC-0167 v5 behaviours 16 and 18.
                    A("Jump to Last Tool Window", "JumpToLastWindow"),
                    A("Close Active Tab", "CloseActiveTab"),
                    S,
                    A("Stretch to Left", "ResizeToolWindowLeft"),
                    A("Stretch to Right", "ResizeToolWindowRight"),
                    A("Stretch to Top", "ResizeToolWindowUp"),
                    A("Stretch to Bottom", "ResizeToolWindowDown"),
                    A("Maximize Tool Window", "MaximizeToolWindow"),
                ],
            ),
            // Editor tabs (SPEC-0059 v9 behaviour 19).
            Sub(
                "Editor Tabs",
                &[
                    A("Reopen Closed Tab", "ReopenClosedTab"),
                    A("Pin Active Tab", "PinActiveEditorTab"),
                    S,
                    A("Close Other Tabs", "CloseAllEditorsButActive"),
                    A("Close All Tabs", "CloseAllEditors"),
                    A("Close Tabs to the Left", "CloseAllToTheLeft"),
                    A("Close Tabs to the Right", "CloseAllToTheRight"),
                    A("Close Unmodified Tabs", "CloseAllUnmodifiedEditors"),
                    A("Close All but Pinned", "CloseAllUnpinnedEditors"),
                    S,
                    A("Switcher", "Switcher"),
                    A("Show Hidden Tabs", "TabList"),
                    A("Maximize Editor", "MaximizeEditorInSplit"),
                    S,
                    A("Configure Editor Tabs…", "IDECraft.ConfigureEditorTabs"),
                ],
            ),
            S,
            // SPEC-0094 v2 behaviour 5; after the submenus, as in the open platform's Window group
            // (SPEC-0124 v8).
            A("Next Project Window", "NextProjectWindow"),
            A("Previous Project Window", "PreviousProjectWindow"),
            // Every window of the application (SPEC-0094 v6 behaviour 17, SPEC-0124 v13
            // behaviour 15).
            A("Activate Next Window", "NextWindow"),
            A("Activate Previous Window", "PreviousWindow"),
        ],
    ),
    (
        "Help",
        &[
            A("Find Action…", "GotoAction"),
            S,
            Sub(
                "Diagnostics",
                &[A("Keyboard Diagnostics", "IDECraft.KeyboardDiagnostics")],
            ),
        ],
    ),
];

/// Every action id the menus use.
pub fn menu_actions() -> Vec<&'static str> {
    fn walk(items: &[Entry], out: &mut Vec<&'static str>) {
        for e in items {
            match e {
                Entry::Action(_, id) => out.push(id),
                Entry::Sub(_, sub) => walk(sub, out),
                Entry::Separator => {}
            }
        }
    }
    let mut out = Vec::new();
    for (_, items) in MENUS {
        walk(items, &mut out);
    }
    out
}

/// The label and the menu (or submenu) name of action `id` in the menus, the first place it
/// appears: `SplitVertically` → `("Split Right", "View")`.
pub fn menu_title(id: &str) -> Option<(&'static str, &'static str)> {
    fn walk(
        menu: &'static str,
        items: &'static [Entry],
        id: &str,
    ) -> Option<(&'static str, &'static str)> {
        items.iter().find_map(|e| match e {
            Entry::Action(label, i) if *i == id => Some((*label, menu)),
            Entry::Sub(name, sub) => walk(name, sub, id),
            _ => None,
        })
    }
    MENUS.iter().find_map(|(name, items)| walk(name, items, id))
}

/// The buttons right of the run configuration in the main toolbar, in order; `stop-button` only
/// while something runs (`SPEC-0061` v4 behaviour 14, v5 behaviour 19).
pub const MAIN_TOOLBAR_RIGHT: &[&str] = &[
    "run-widget",
    "run-button",
    "debug-button",
    "stop-button",
    "run-more",
    "search-everywhere-button",
    "settings-button",
];

/// The more-actions menu after the run widget (`SPEC-0061` v5 behaviour 19). OBS-0020 recorded
/// the reference's content: running with coverage, profiling, and running with parameters; IDECraft
/// offers its own actions on the selected configuration without a button of their own (coverage
/// and the parameter dialog belong to the run configurations, F-077).
pub const RUN_WIDGET_MORE: &[Entry] = &[
    A("Profile", "Profiler.ProfileSelected"),
    A("Profile Memory", "Profiler.ProfileMemory"),
    S,
    A("Run Current File", "RunClass"),
    A("Debug Current File", "DebugClass"),
];

/// The Settings button's menu at the right end of the main toolbar (`SPEC-0061` v6 behaviour 28,
/// OBS-0020): the entries the reference has and IDECraft supports.
pub const SETTINGS_BUTTON_MENU: &[Entry] = &[
    A("Settings…", "ShowSettings"),
    A("Plugins…", "IDECraft.SettingsPlugins"),
    S,
    A("Theme…", "QuickChangeScheme"),
    A("Keymap…", "IDECraft.SettingsKeymap"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(name: &str) -> &'static [Entry] {
        MENUS
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, e)| *e)
            .unwrap()
    }

    fn labels(entries: &[Entry]) -> Vec<&'static str> {
        entries
            .iter()
            .map(|e| match e {
                Entry::Action(label, _) | Entry::Sub(label, _) => *label,
                Entry::Separator => "-",
            })
            .collect()
    }

    /// SPEC-0124 v3 AK-5 (hand test 50, MB1 to MB3): Build between Refactor and Run, Git after
    /// Tools, Window between Git and Help; what Build and Window hold, and what left View.
    /// SPEC-0124 v10, SPEC-0067 AK-10 (v9): Navigate holds the endpoint search after Text….
    #[test]
    fn endpoint_search_is_in_navigate() {
        assert_eq!(menu_title("GotoUrlAction"), Some(("Endpoint…", "Navigate")));
        let navigate = labels(menu("Navigate"));
        let at = navigate.iter().position(|l| *l == "Text…").unwrap();
        assert_eq!(navigate[at + 1..at + 3], ["Endpoint…", "Line:Column…"]);
    }

    /// SPEC-0124 AK-6 (v4).
    #[test]
    fn keymap_coverage_actions_are_in_their_menus() {
        for (id, label, place) in [
            ("SaveAs", "Save As…", "File"),
            ("Synchronize", "Reload All from Disk", "File"),
            ("SelectIn", "Select in Project Tree", "Navigate"),
            ("Git.CreateNewBranch", "New Branch…", "Git"),
            (
                "JumpToLastWindow",
                "Jump to Last Tool Window",
                "Active Tool Window",
            ),
            ("CloseActiveTab", "Close Active Tab", "Active Tool Window"),
            // SPEC-0333 behaviour 8: the title of the open platform, in the Layouts submenu.
            ("RestoreDefaultLayout", "Restore Current Layout", "Layouts"),
        ] {
            assert_eq!(menu_title(id), Some((label, place)), "{id}");
        }
        // Save As and Reload follow Save All; New Branch follows Branches.
        let file = labels(menu("File"));
        let at = file.iter().position(|l| *l == "Save All").unwrap();
        assert_eq!(file[at + 1..at + 3], ["Save As…", "Reload All from Disk"]);
        let git = labels(menu("Git"));
        let at = git.iter().position(|l| *l == "Branches…").unwrap();
        assert_eq!(git[at + 1], "New Branch…");
    }

    /// SPEC-0096 v4 AK-6: the request at the caret has an entry under Tools › HTTP Client, whose
    /// label is also its title in Find Action.
    #[test]
    fn the_request_at_the_caret_has_a_menu_entry() {
        assert_eq!(
            menu_title("Console.Execute.Multiline"),
            Some(("Send Request at Caret", "HTTP Client"))
        );
        // SPEC-0096 v5 AK-9.
        assert_eq!(
            menu_title("IDECraft.HttpRunAllRequests"),
            Some(("Run All Requests in File", "HTTP Client"))
        );
    }

    /// SPEC-0124 AK-7 (v5), SPEC-0071 AK-13 (v10), SPEC-0069 AK-5 (v3).
    #[test]
    fn navigation_actions_from_the_keymap_are_in_their_menus() {
        for (id, label, place) in [
            ("QuickImplementations", "Quick Definition", "View"),
            ("GotoRelated", "Related Symbol…", "Navigate"),
            ("GotoTest", "Test", "Navigate"),
            ("RecentChangedFiles", "Recently Changed Files", "View"),
        ] {
            assert_eq!(menu_title(id), Some((label, place)), "{id}");
        }
        let view = labels(menu("View"));
        let at = view.iter().position(|l| *l == "Recent Files").unwrap();
        assert_eq!(view[at + 1], "Recently Changed Files");
        let at = view
            .iter()
            .position(|l| *l == "Quick Documentation")
            .unwrap();
        assert_eq!(view[at + 1], "Quick Definition");
        let navigate = labels(menu("Navigate"));
        let at = navigate.iter().position(|l| *l == "Super Method").unwrap();
        assert_eq!(navigate[at + 1..at + 3], ["Related Symbol…", "Test"]);
    }

    /// SPEC-0334 AK-10: View › Tool Windows lists TODO after Find.
    #[test]
    fn todo_is_in_view_tool_windows() {
        let find = TOOL_WINDOWS
            .iter()
            .position(|e| *e == A("Find", "ActivateFindToolWindow"))
            .expect("Find is listed");
        assert_eq!(TOOL_WINDOWS[find + 1], A("TODO", "ActivateTODOToolWindow"));
        assert_eq!(
            menu_title("ActivateTODOToolWindow"),
            Some(("TODO", "Tool Windows"))
        );
    }

    /// SPEC-0061 AK-v17b: View › Tool Windows lists Notifications after TODO.
    #[test]
    fn notifications_is_in_view_tool_windows() {
        let todo = TOOL_WINDOWS
            .iter()
            .position(|e| *e == A("TODO", "ActivateTODOToolWindow"))
            .expect("TODO is listed");
        assert_eq!(
            TOOL_WINDOWS[todo + 1],
            A("Notifications", "ActivateNotificationsToolWindow")
        );
        assert_eq!(
            menu_title("ActivateNotificationsToolWindow"),
            Some(("Notifications", "Tool Windows"))
        );
    }

    /// SPEC-0124 AK-9 (v7): Edit ends with the Bookmarks submenu and its five actions; Navigate
    /// holds no bookmark action.
    #[test]
    fn closing_projects_is_a_group_in_file() {
        let file = menu("File");
        let at = file
            .iter()
            .position(|e| *e == A("Recent Projects…", "ManageRecentProjects"))
            .expect("Recent Projects… in File");
        assert_eq!(
            file[at + 1..at + 4],
            [
                A("Close Project", "CloseProject"),
                A("Close All Projects", "CloseAllProjects"),
                A("Close Other Projects", "CloseOtherProjects"),
            ]
        );
        assert_eq!(
            menu_title("CloseOtherProjects"),
            Some(("Close Other Projects", "File"))
        );
    }

    /// SPEC-0124 AK-13 (v12), SPEC-0336 AK-9: File Properties follows Plugins… and holds the
    /// encoding, the read-only toggle and the three line separators in the open group's order.
    #[test]
    fn file_properties_is_a_submenu_of_file() {
        let file = menu("File");
        let at = file
            .iter()
            .position(|e| *e == A("Plugins…", "IDECraft.SettingsPlugins"))
            .expect("Plugins… in File");
        let Entry::Sub("File Properties", items) = file[at + 1] else {
            panic!("File Properties follows Plugins…: {:?}", file[at + 1]);
        };
        assert_eq!(file[at + 2], S);
        assert_eq!(
            items[..2],
            [
                A("File Encoding", "ChangeFileEncodingAction"),
                A("Toggle Read-Only Attribute", "ToggleReadOnlyAttribute"),
            ]
        );
        let [_, _, Entry::Sub("Line Separators", separators)] = items else {
            panic!("Line Separators is the last item: {items:?}");
        };
        assert_eq!(
            **separators,
            [
                A("CRLF - Windows (\\r\\n)", "ConvertToWindowsLineSeparators"),
                A("LF - Unix and macOS (\\n)", "ConvertToUnixLineSeparators"),
                A("CR - Classic Mac OS (\\r)", "ConvertToMacLineSeparators"),
            ]
        );
        assert_eq!(
            menu_title("ChangeFileEncodingAction"),
            Some(("File Encoding", "File Properties"))
        );
        assert_eq!(
            menu_title("ConvertToUnixLineSeparators"),
            Some(("LF - Unix and macOS (\\n)", "Line Separators"))
        );
    }

    /// SPEC-0124 AK-9 (v7): Edit ends with the Bookmarks submenu and its five actions; Navigate
    /// holds no bookmark action.
    #[test]
    fn bookmarks_are_a_submenu_at_the_end_of_edit() {
        let edit = menu("Edit");
        let Some(Entry::Sub("Bookmarks", items)) = edit.last() else {
            panic!("Edit ends with {:?}", edit.last());
        };
        assert_eq!(edit[edit.len() - 2], Entry::Separator);
        assert_eq!(
            **items,
            [
                A("Toggle Bookmark", "ToggleBookmark"),
                A(
                    "Toggle Bookmark with Mnemonic",
                    "ToggleBookmarkWithMnemonic"
                ),
                S,
                A("Show Bookmarks", "ShowBookmarks"),
                // SPEC-0124 v13 behaviour 15.
                A("Go to Mnemonic…", "ShowTypeBookmarks"),
                S,
                A("Go to Next Bookmark", "GotoNextBookmark"),
                A("Go to Previous Bookmark", "GotoPreviousBookmark"),
            ]
        );
        assert_eq!(
            menu_title("ToggleBookmark"),
            Some(("Toggle Bookmark", "Bookmarks"))
        );
        let bookmark_actions = menu_actions()
            .into_iter()
            .filter(|id| id.contains("Bookmark"))
            .count();
        // Six since Go to Mnemonic… (`ShowTypeBookmarks`, SPEC-0124 v13 behaviour 15).
        assert_eq!(bookmark_actions, 6, "only in Edit › Bookmarks");
    }

    /// SPEC-0124 AK-11 (v9): the small editing commands of SPEC-0332 stand in Edit where the open
    /// Edit group has their ids.
    #[test]
    fn small_editing_commands_are_in_edit() {
        let wanted = [
            "Copy",
            "Copy Path/Reference…",
            "Paste",
            "Select All",
            "Add Carets to Ends of Selected Lines",
            "Add Selection for Next Occurrence",
            "Duplicate Line or Selection",
            "Sort Lines",
            "Reverse Lines",
            "Transpose",
            "Delete Line",
            "Unindent Line or Selection",
            "Convert Indents",
            "Bookmarks",
        ];
        assert_eq!(order_of("Edit", &wanted), wanted);
        assert_eq!(
            *submenu("Edit", "Convert Indents"),
            [
                A("To Spaces", "ConvertIndentsToSpaces"),
                A("To Tabs", "ConvertIndentsToTabs"),
            ]
        );
        for (id, title, group) in [
            ("CopyReferencePopupGroup", "Copy Path/Reference…", "Edit"),
            (
                "EditorAddCaretPerSelectedLine",
                "Add Carets to Ends of Selected Lines",
                "Edit",
            ),
            ("EditorSortLines", "Sort Lines", "Edit"),
            ("EditorReverseLines", "Reverse Lines", "Edit"),
            ("EditorTranspose", "Transpose", "Edit"),
            ("ConvertIndentsToSpaces", "To Spaces", "Convert Indents"),
            ("ConvertIndentsToTabs", "To Tabs", "Convert Indents"),
        ] {
            assert_eq!(menu_title(id), Some((title, group)), "{id}");
            let times = menu_actions().into_iter().filter(|a| *a == id).count();
            assert_eq!(times, 1, "{id}");
        }
    }

    /// The labels of `name` that are in `wanted`, in the menu's order.
    fn order_of(name: &str, wanted: &[&str]) -> Vec<&'static str> {
        labels(menu(name))
            .into_iter()
            .filter(|l| wanted.contains(l))
            .collect()
    }

    fn submenu(name: &str, title: &str) -> &'static [Entry] {
        menu(name)
            .iter()
            .find_map(|e| match e {
                Entry::Sub(n, items) if *n == title => Some(*items),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name} has no submenu {title}"))
    }

    /// SPEC-0124 AK-10 (v8): the items the open platform's File, Edit, Navigate, Refactor and
    /// Window groups also hold stand in those groups' order, under the titles of behaviour 11.
    #[test]
    fn items_of_the_open_platform_groups_stand_in_their_order() {
        for (name, open_order) in [
            (
                "File",
                &[
                    "New…",
                    "Open…",
                    "Recent Projects…",
                    "Remote Development…",
                    "Plugins…",
                    "File Properties",
                    "Local History",
                    "Save All",
                    "Reload All from Disk",
                ][..],
            ),
            (
                "Edit",
                &[
                    "Undo",
                    "Redo",
                    "Cut",
                    "Copy",
                    "Paste",
                    "Delete",
                    "Search in Selection Only",
                    "Find",
                    "Find Usages",
                    "Column Selection Mode",
                    "Select All",
                    "Extend Selection",
                    "Shrink Selection",
                    "Toggle Case",
                    "Join Lines",
                    "Duplicate Line or Selection",
                    "Indent Selection",
                    "Unindent Line or Selection",
                    "Bookmarks",
                ],
            ),
            (
                "Navigate",
                &[
                    "Back",
                    "Forward",
                    "Search Everywhere",
                    "Class…",
                    "File…",
                    "Symbol…",
                    "Text…",
                    "Line:Column…",
                    "Next Highlighted Error",
                    "Previous Highlighted Error",
                    "Last Edit Location",
                    "Next Edit Location",
                    "Navigate in File",
                    "Select in Project Tree",
                    "Jump to Navigation Bar",
                    "Declaration or Usages",
                    "Implementation(s)",
                    "Type Declaration",
                    "Super Method",
                    "File Structure",
                    "Type Hierarchy",
                    "Method Hierarchy",
                    "Call Hierarchy",
                ],
            ),
            (
                "Refactor",
                &[
                    "Refactor This…",
                    "Rename…",
                    "Extract/Introduce",
                    "Inline…",
                    "Move…",
                ],
            ),
            (
                "Window",
                &[
                    "Minimize",
                    "Zoom",
                    "Layouts",
                    "Active Tool Window",
                    "Editor Tabs",
                    "Next Project Window",
                    "Previous Project Window",
                ],
            ),
        ] {
            assert_eq!(order_of(name, open_order), open_order, "{name}");
        }
    }

    /// SPEC-0124 AK-10 (v8): the items v8 adds or moves run these actions, the submenus hold what
    /// behaviour 11 names, and an action has one menu item.
    /// SPEC-0083 v8 AK-24: Code › Folding holds every folding action.
    #[test]
    fn folding_actions_are_in_code_folding() {
        let Some(Sub(_, folding)) = menu("Code").iter().find(|e| matches!(e, Sub("Folding", _)))
        else {
            panic!("Code has no Folding submenu");
        };
        assert_eq!(
            labels(folding),
            [
                "Expand",
                "Collapse",
                "-",
                "Expand Recursively",
                "Collapse Recursively",
                "-",
                "Expand All",
                "Collapse All",
                "-",
                "Expand to Level",
                "Expand All to Level",
                "-",
                "Expand Doc Comments",
                "Collapse Doc Comments",
                "-",
                "Fold Selection / Remove Region",
                "Fold Code Block",
            ]
        );
        let actions = menu_actions();
        for id in [
            "ExpandRegionRecursively",
            "CollapseRegionRecursively",
            "ExpandDocComments",
            "CollapseDocComments",
            "CollapseSelection",
            "CollapseBlock",
        ] {
            assert!(actions.contains(&id), "{id}");
        }
        for level in 1..=5 {
            for id in [
                format!("ExpandToLevel{level}"),
                format!("ExpandAllToLevel{level}"),
            ] {
                assert!(actions.contains(&id.as_str()), "{id}");
                assert_eq!(
                    menu_title(&id).map(|(label, _)| label.to_owned()),
                    Some(level.to_string())
                );
            }
        }
    }

    #[test]
    fn items_added_from_the_reference_comparison() {
        for (id, label, place) in [
            ("IDECraft.SettingsPlugins", "Plugins…", "File"),
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
            ("ExtractMethod", "Extract Method…", "Extract/Introduce"),
            (
                "ExtractInterface",
                "Extract Interface…",
                "Extract/Introduce",
            ),
        ] {
            assert_eq!(menu_title(id), Some((label, place)), "{id}");
        }
        assert_eq!(
            labels(submenu("Edit", "Find Usages")),
            [
                "Find Usages",
                "Show Usages",
                "-",
                "Find Usages in File",
                "Highlight Usages in File"
            ]
        );
        assert_eq!(
            labels(submenu("Navigate", "Navigate in File")),
            ["Next Method", "Previous Method"]
        );
        assert_eq!(
            labels(submenu("Refactor", "Extract/Introduce")),
            [
                "Introduce Variable…",
                "Introduce Constant…",
                "Introduce Field…",
                "Introduce Parameter…",
                "-",
                "Extract Method…",
                "-",
                "Extract Interface…",
                "Extract Superclass…"
            ]
        );
        assert_eq!(labels(submenu("File", "Local History")), ["Show History"]);
        // File Structure left View for Navigate.
        assert!(!labels(menu("View")).contains(&"File Structure"));
        // Apart from Find Action (Navigate and Help), the AI chat and Remote Host (each a tool
        // window and a Tools item), an action has one menu item.
        let mut ids = menu_actions();
        ids.sort_unstable();
        let twice: Vec<&str> = ids
            .windows(2)
            .filter(|w| w[0] == w[1])
            .map(|w| w[0])
            .collect();
        assert_eq!(
            twice,
            ["GotoAction", "IDECraft.AiChat", "Remote.ToolWindow"]
        );
    }

    #[test]
    fn menu_bar_order_build_and_window() {
        let names: Vec<&str> = MENUS.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            [
                "File", "Edit", "View", "Navigate", "Code", "Refactor", "Build", "Run", "Tools",
                "Git", "Window", "Help"
            ]
        );
        assert_eq!(
            menu("Build"),
            [
                A("Build Project", "CompileDirty"),
                A("Recompile", "Compile"),
                S,
                A("Sync Build Changes", "ExternalSystem.ProjectRefreshAction"),
            ]
        );
        assert_eq!(
            labels(menu("Window")),
            [
                "Minimize",
                "Zoom",
                "Toggle Full Screen",
                "-",
                "Layouts",
                "-",
                "Active Tool Window",
                "Editor Tabs",
                "-",
                "Next Project Window",
                "Previous Project Window",
                // SPEC-0124 v13 behaviour 15.
                "Activate Next Window",
                "Activate Previous Window"
            ]
        );
        let sub = |name: &str| {
            menu("Window")
                .iter()
                .find_map(|e| match e {
                    Entry::Sub(n, items) if *n == name => Some(*items),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(
            labels(sub("Active Tool Window")),
            [
                "Hide Active Tool Window",
                "Hide All Tool Windows",
                "Jump to Last Tool Window",
                "Close Active Tab",
                "-",
                "Stretch to Left",
                "Stretch to Right",
                "Stretch to Top",
                "Stretch to Bottom",
                "Maximize Tool Window"
            ]
        );
        let tabs = sub("Editor Tabs");
        assert_eq!(tabs.len(), 15);
        assert!(tabs.contains(&A("Reopen Closed Tab", "ReopenClosedTab")));
        assert!(tabs.contains(&A("Maximize Editor", "MaximizeEditorInSplit")));
        // View keeps the tool windows and the split actions (SPEC-0112 behaviour 7), not these.
        let view = labels(menu("View"));
        assert!(view.contains(&"Tool Windows") && view.contains(&"Split Right"));
        for moved in [
            "Editor Tabs",
            "Hide Active Tool Window",
            "Hide All Tool Windows",
        ] {
            assert!(!view.contains(&moved), "{moved}");
        }
        // No two separators in a row, none at a menu's start or end.
        fn tidy(entries: &[Entry]) -> bool {
            entries.first() != Some(&S)
                && entries.last() != Some(&S)
                && entries.windows(2).all(|w| w != [S, S])
                && entries.iter().all(|e| match e {
                    Entry::Sub(_, sub) => tidy(sub),
                    _ => true,
                })
        }
        assert!(MENUS.iter().all(|(_, e)| tidy(e)));
        // Find Action's group: the menu, or the submenu.
        assert_eq!(menu_title("CompileDirty"), Some(("Build Project", "Build")));
        assert_eq!(
            menu_title("MinimizeCurrentWindow"),
            Some(("Minimize", "Window"))
        );
        assert_eq!(
            menu_title("HideAllWindows"),
            Some(("Hide All Tool Windows", "Active Tool Window"))
        );
    }

    /// SPEC-0333 AK-5: the fixed entries of Window › Layouts, with the ids and texts of the open
    /// platform's layout actions; the saved layouts are added by the menu bar.
    #[test]
    fn layouts_are_a_submenu_of_window() {
        assert_eq!(
            submenu("Window", LAYOUTS),
            [
                A("Default", "RestoreFactoryDefaultLayout"),
                S,
                A("Restore Current Layout", "RestoreDefaultLayout"),
                A("Save Changes in Current Layout", "StoreDefaultLayout"),
                A("Save Current Layout as New…", "StoreNewLayout"),
            ]
        );
        for (id, label) in [
            ("RestoreFactoryDefaultLayout", "Default"),
            ("StoreDefaultLayout", "Save Changes in Current Layout"),
            ("StoreNewLayout", "Save Current Layout as New…"),
        ] {
            assert_eq!(menu_title(id), Some((label, LAYOUTS)), "{id}");
        }
        // The static entries are the ones the model gives without saved layouts.
        let model: Vec<(String, String)> = crate::window_layouts::Layouts::default()
            .menu()
            .into_iter()
            .filter_map(|item| match item {
                crate::window_layouts::MenuItem::Action { label, id, .. } => Some((label, id)),
                _ => None,
            })
            .collect();
        let fixed: Vec<(String, String)> = submenu("Window", LAYOUTS)
            .iter()
            .filter_map(|e| match e {
                Entry::Action(label, id) => Some(((*label).to_owned(), (*id).to_owned())),
                _ => None,
            })
            .collect();
        assert_eq!(model, fixed);
    }

    /// SPEC-0127 AK-13 (v8), SPEC-0087 AK-10 (v8), SPEC-0088 AK-8 (v4), SPEC-0094 AK-4 (v2),
    /// SPEC-0124 AK-14 (v13): the menu items of the sixth keymap package stand at their places.
    #[test]
    fn sixth_package_items_are_in_their_menus() {
        for (id, label, place) in [
            ("NewScratchFile", "New Scratch File…", "File"),
            ("ShowTypeBookmarks", "Go to Mnemonic…", "Bookmarks"),
            ("CopyElement", "Copy…", "Refactor"),
            ("NextWindow", "Activate Next Window", "Window"),
            ("PreviousWindow", "Activate Previous Window", "Window"),
        ] {
            assert_eq!(menu_title(id), Some((label, place)), "{id}");
        }
        let after = |entries: &'static [Entry], label: &str| {
            let all = labels(entries);
            all[all.iter().position(|l| *l == label).unwrap() + 1]
        };
        assert_eq!(after(menu("File"), "New…"), "New Scratch File…");
        assert_eq!(after(menu("Refactor"), "Move…"), "Copy…");
        assert_eq!(
            after(submenu("Edit", "Bookmarks"), "Show Bookmarks"),
            "Go to Mnemonic…"
        );
        let window = labels(menu("Window"));
        assert_eq!(
            window[window.len() - 3..],
            [
                "Previous Project Window",
                "Activate Next Window",
                "Activate Previous Window"
            ]
        );
    }

    /// SPEC-0124 AK-15 (v14): the menu items of the seventh keymap package stand at their
    /// places.
    #[test]
    fn seventh_package_items_are_in_their_menus() {
        for (id, label, place) in [
            ("ExpressionTypeInfo", "Type Info", "View"),
            ("GotoCustomRegion", "Custom Folding…", "Navigate"),
            (
                "StopBackgroundProcesses",
                "Stop Background Processes…",
                "Run",
            ),
        ] {
            assert_eq!(menu_title(id), Some((label, place)), "{id}");
        }
        let after = |entries: &'static [Entry], label: &str| {
            let all = labels(entries);
            all[all.iter().position(|l| *l == label).unwrap() + 1]
        };
        assert_eq!(after(menu("View"), "Parameter Info"), "Type Info");
        assert_eq!(after(menu("Navigate"), "Line:Column…"), "Custom Folding…");
        assert_eq!(after(menu("Run"), "Stop"), "Stop Background Processes…");
        // The actions of a list and of the Run tool window have no menu item.
        for id in [
            "List-selectFirstRow",
            "List-selectLastRow",
            "List-selectFirstRowExtendSelection",
            "List-selectLastRowExtendSelection",
            "SendEOF",
        ] {
            assert_eq!(menu_title(id), None, "{id}");
        }
    }

    /// SPEC-0124 AK-18 (v17): Insert Live Template… stands after Class Name Completion in Code;
    /// the template and Switcher keys of the tenth package have no item.
    #[test]
    fn tenth_package_items_are_in_their_menus() {
        assert_eq!(
            menu_title("InsertLiveTemplate"),
            Some(("Insert Live Template…", "Code"))
        );
        let all = labels(menu("Code"));
        let at = all
            .iter()
            .position(|l| *l == "Class Name Completion")
            .unwrap();
        assert_eq!(all[at + 1], "Insert Live Template…");
        for id in [
            "NextTemplateVariable",
            "PreviousTemplateVariable",
            "SwitcherIterateItems",
        ] {
            assert_eq!(menu_title(id), None, "{id}");
        }
    }

    /// SPEC-0124 AK-17 (v16): the menu items of the ninth keymap package stand after the items
    /// named.
    #[test]
    fn ninth_package_items_are_in_their_menus() {
        for (id, label, place) in [
            ("ForceRefresh", "Force Refresh", "File"),
            ("ShowReformatFileDialog", "Reformat File…", "Code"),
            ("SwitchHeaderSource", "Header/Source", "Navigate"),
            ("SafeDelete", "Safe Delete…", "Refactor"),
        ] {
            assert_eq!(menu_title(id), Some((label, place)), "{id}");
        }
        let after = |entries: &'static [Entry], label: &str| {
            let all = labels(entries);
            all[all.iter().position(|l| *l == label).unwrap() + 1]
        };
        assert_eq!(after(menu("File"), "Reload All from Disk"), "Force Refresh");
        assert_eq!(after(menu("Code"), "Reformat Code"), "Reformat File…");
        assert_eq!(after(menu("Navigate"), "Test"), "Header/Source");
        assert_eq!(after(menu("Refactor"), "Copy…"), "Safe Delete…");
    }

    /// SPEC-0124 AK-16 (v15): the menu items of the eighth keymap package stand at their
    /// places; the package's actions of a popup or a tool window have none.
    #[test]
    fn eighth_package_items_are_in_their_menus() {
        for (id, label, place) in [
            ("ClassNameCompletion", "Class Name Completion", "Code"),
            (
                "EditorCompleteStatement",
                "Complete Current Statement",
                "Code",
            ),
            ("Unwrap", "Unwrap/Remove…", "Code"),
            ("CodeInspection.OnEditor", "Inspect Current File", "Code"),
            ("RunInspection", "Run Inspection by Name…", "Code"),
            (
                "IntroduceParameter",
                "Introduce Parameter…",
                "Extract/Introduce",
            ),
            ("Compile", "Recompile", "Build"),
            (
                "QuickEvaluateExpression",
                "Quick Evaluate Expression",
                "Run",
            ),
        ] {
            assert_eq!(menu_title(id), Some((label, place)), "{id}");
        }
        let after = |entries: &'static [Entry], label: &str| {
            let all = labels(entries);
            all[all.iter().position(|l| *l == label).unwrap() + 1]
        };
        assert_eq!(
            after(menu("Code"), "Code Completion"),
            "Class Name Completion"
        );
        assert_eq!(
            after(menu("Code"), "Insert Live Template…"),
            "Complete Current Statement"
        );
        assert_eq!(after(menu("Code"), "Surround With…"), "Unwrap/Remove…");
        assert_eq!(after(menu("Code"), "Inspect Code…"), "Inspect Current File");
        assert_eq!(
            after(menu("Code"), "Inspect Current File"),
            "Run Inspection by Name…"
        );
        assert_eq!(after(menu("Build"), "Build Project"), "Recompile");
        assert_eq!(
            after(menu("Run"), "Evaluate Expression…"),
            "Quick Evaluate Expression"
        );
        for id in [
            "NextEditorTab",
            "PreviousEditorTab",
            "MethodOverloadSwitchUp",
            "MethodOverloadSwitchDown",
            "EditorChooseLookupItemDot",
            "VcsHistory.ShowAllAffected",
            "EditorShowGutterIconTooltip",
        ] {
            assert_eq!(menu_title(id), None, "{id}");
        }
    }

    /// SPEC-0061 AK-v12: the run, debug and window actions taken from the keymap are in their
    /// menus, each beside the action it belongs to.
    #[test]
    fn run_debug_and_window_actions_are_in_their_menus() {
        for (id, label, place) in [
            ("ChooseDebugConfiguration", "Debug…", "Run"),
            ("RerunTests", "Rerun Tests", "Run"),
            (
                "ToggleTemporaryLineBreakpoint",
                "Toggle Temporary Line Breakpoint",
                "Run",
            ),
            ("ToggleFullScreen", "Toggle Full Screen", "Window"),
            ("NextProjectWindow", "Next Project Window", "Window"),
            ("PreviousProjectWindow", "Previous Project Window", "Window"),
        ] {
            assert_eq!(menu_title(id), Some((label, place)), "{id}");
        }
        let run = labels(menu("Run"));
        let after = |label: &str| run[run.iter().position(|l| *l == label).unwrap() + 1];
        assert_eq!(after("Run…"), "Debug…");
        assert_eq!(after("Rerun Tests"), "Stop");
        assert_eq!(
            after("Toggle Line Breakpoint"),
            "Toggle Temporary Line Breakpoint"
        );
        // Full screen follows Zoom; the two project window actions stand before the two that
        // step through every window, which end the menu (SPEC-0124 v8, v13 behaviour 15).
        let window = labels(menu("Window"));
        let at = window.iter().position(|l| *l == "Zoom").unwrap();
        assert_eq!(window[at + 1], "Toggle Full Screen");
        assert_eq!(
            window[window.len() - 4..window.len() - 2],
            ["Next Project Window", "Previous Project Window"]
        );
    }

    /// SPEC-0061 v5 AK-13: the more-actions button after Debug and Stop, before Search Everywhere;
    /// its entries run actions the menus know.
    #[test]
    fn run_widget_more_actions() {
        let at = |id: &str| MAIN_TOOLBAR_RIGHT.iter().position(|b| *b == id).unwrap();
        assert!(at("debug-button") < at("run-more") && at("stop-button") < at("run-more"));
        assert_eq!(at("run-more") + 1, at("search-everywhere-button"));
        let known = menu_actions();
        let actions: Vec<&str> = RUN_WIDGET_MORE
            .iter()
            .filter_map(|e| match e {
                Entry::Action(_, id) => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(
            actions,
            [
                "Profiler.ProfileSelected",
                "Profiler.ProfileMemory",
                "RunClass",
                "DebugClass"
            ]
        );
        assert!(actions.iter().all(|a| known.contains(a)));
    }

    /// SPEC-0061 v6 AK-19: the Settings button opens a menu; Settings and Theme are menu actions.
    #[test]
    fn settings_button_menu() {
        let known = menu_actions();
        let actions: Vec<&str> = SETTINGS_BUTTON_MENU
            .iter()
            .filter_map(|e| match e {
                Entry::Action(_, id) => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(
            actions,
            [
                "ShowSettings",
                "IDECraft.SettingsPlugins",
                "QuickChangeScheme",
                "IDECraft.SettingsKeymap"
            ]
        );
        assert!(known.contains(&"ShowSettings") && known.contains(&"QuickChangeScheme"));
    }
}
