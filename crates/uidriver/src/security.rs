//! The security rules of external driving (`SPEC-0319` behaviours 32-39, ADR-0027 section 5):
//! who acts, which rights an action needs, what is reserved for the user, the emergency stop and
//! the pause while the user's own input comes in.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::action::{Action, KeyPress};
use crate::error::{ErrorCode, UiError};
use crate::tree::{Effect, Reserved, Tree, UiNode};

/// The right every external client needs (behaviour 34).
pub const UI_RIGHT: &str = "ui";

/// What the right `ui` allows, in words.
pub const UI_MEANING: &str =
    "operate the IDECraft window: read its interface, click, type and press keys in it";

/// How long the user's own input pauses external clients (behaviour 37).
pub const USER_PAUSE: Duration = Duration::from_secs(3);

/// After this long without a call, a client's driver session ends (`SPEC-0319` v4 behaviour 37b).
pub const SESSION_IDLE: Duration = Duration::from_secs(60);

/// How long the target is highlighted before input is delivered (behaviour 36).
pub const HIGHLIGHT: Duration = Duration::from_millis(300);

/// Who acts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Actor {
    /// The window's own self-test, in process.
    SelfTest,
    /// An external client: an MCP client of the window's HTTP server or `idecraft drive`.
    External {
        /// Its name as it gave it (`claude-code 2.1`).
        client: String,
        /// Its MCP session.
        session: String,
    },
}

impl Actor {
    /// Whether the security rules apply.
    pub fn is_external(&self) -> bool {
        matches!(self, Self::External { .. })
    }

    /// The origin synthesised events carry (behaviour 36): `selftest` or `driver:<client>`.
    pub fn origin(&self) -> String {
        match self {
            Self::SelfTest => "selftest".into(),
            Self::External { client, .. } => format!("driver:{client}"),
        }
    }

    /// The name shown in the marking.
    pub fn client(&self) -> &str {
        match self {
            Self::SelfTest => "self-test",
            Self::External { client, .. } => client,
        }
    }
}

/// Effects of keymap actions by action id, derived from the shared action ids (behaviour 35).
/// Unknown ids are undeclared: an external client cannot press keys whose effect nobody stated.
pub fn effects_of_action(id: &str) -> Option<Vec<Effect>> {
    use Effect::*;
    // Reading the clipboard is reserved; it is answered before effects (see `reserved_action`).
    let explicit = match id {
        "Back"
        | "Forward"
        | "$SelectAll"
        | "$Copy"
        | "CopyPaths"
        | "CopyReference"
        // Copy Path/Reference… and its rows write the clipboard, like Copy; a caret per
        // selected line changes no text (SPEC-0332 behaviour 16).
        | "CopyReferencePopupGroup"
        | "CopyAbsolutePath"
        | "CopyFileName"
        | "CopyContentRootPath"
        | "CopyPathWithLineNumber"
        | "CopySourceRootPath"
        | "EditorAddCaretPerSelectedLine"
        | "Find"
        | "Replace"
        | "FindNext"
        | "FindPrevious"
        | "FindInPath"
        | "HideAllWindows"
        | "CloseContent"
        | "NextTab"
        | "PreviousTab"
        | "SplitVertically"
        | "SplitHorizontally"
        | "Unsplit"
        | "MoveEditorToOppositeTabGroup"
        | "UnsplitAll"
        | "SplitAndMoveRight"
        | "SplitAndMoveDown"
        | "NextSplitter"
        | "PrevSplitter"
        | "ReopenClosedTab"
        | "PinActiveEditorTab"
        | "CloseAllEditorsButActive"
        | "CloseAllEditors"
        | "CloseAllToTheLeft"
        | "CloseAllToTheRight"
        | "CloseAllUnmodifiedEditors"
        | "CloseAllUnpinnedEditors"
        | "Switcher"
        | "TabList"
        | "MaximizeEditorInSplit"
        | "IDECraft.ConfigureEditorTabs"
        | "ShowIntentionActions"
        | "QuickJavaDoc"
        | "QuickImplementations"
        // The fifth package of keymap actions, named here because their ids fall under no
        // prefix rule below or under one that would leave them undeclared: overwrite mode
        // changes how typing works and no file by itself, the folds and the declaration shown
        // at the top of the editor are view state (SPEC-0057 v6, SPEC-0083 v7 behaviour 17).
        | "EditorToggleInsertState"
        | "EditorContextInfo"
        | "CollapseBlock"
        | "CollapseSelection"
        // Folding the documentation comments (SPEC-0083 v8 behaviour 23).
        | "CollapseDocComments"
        | "ExpandDocComments"
        // The grouping toggles of the Find tool window (SPEC-0293 v6 behaviour 18).
        | "UsageGrouping.UsageType"
        | "UsageGrouping.Module"
        | "UsageGrouping.Directory"
        | "UsageGrouping.FileStructure"
        // Its filters, nested folders, module groups, Exclude and Include change what the
        // window shows and nothing else (SPEC-0293 v9 behaviour 27).
        | "UsageFiltering.ReadAccess"
        | "UsageFiltering.WriteAccess"
        | "UsageFiltering.Imports"
        | "UsageGrouping.DirectoryStructure"
        | "UsageGrouping.FlattenModules"
        | "UsageView.Exclude"
        | "UsageView.Include"
        // The sixth package of keymap actions that move the caret, the selection of a list or
        // the front window and change no file (SPEC-0080 v3, SPEC-0094 v6, SPEC-0275 v18
        // behaviour 45, SPEC-0167 v7, SPEC-0065 v6).
        | "NextParameter"
        | "PrevParameter"
        // The eighth package: the editor tabs, the overloads of parameter info, the files of a
        // commit, the gutter icon tooltip and the two inspection actions change no file
        // (SPEC-0059 v9.7, SPEC-0080 v4, SPEC-0086 v12, SPEC-0294 v5, SPEC-0098 v10).
        | "NextEditorTab"
        | "PreviousEditorTab"
        | "MethodOverloadSwitchUp"
        | "MethodOverloadSwitchDown"
        | "VcsHistory.ShowAllAffected"
        | "EditorShowGutterIconTooltip"
        | "CodeInspection.OnEditor"
        | "RunInspection"
        // The first and the last row of the focused list, and the selection extended to them
        // (SPEC-0275 v19 behaviours 47 and 48); the chooser of the file's custom regions moves
        // the caret (SPEC-0083 v9 behaviour 24). Type Info falls under a prefix rule below.
        | "List-selectFirstRow"
        | "List-selectLastRow"
        | "List-selectFirstRowExtendSelection"
        | "List-selectLastRowExtendSelection"
        | "GotoCustomRegion"
        | "NextWindow"
        | "PreviousWindow"
        | "ViewSource"
        | "ShowContent"
        | "ShowFilterPopup"
        | "ShowTypeBookmarks"
        // The TODO tool window and its grouping toggles (SPEC-0334 behaviour 26).
        | "ActivateTODOToolWindow"
        | "TodoViewGroupByShowModules"
        | "TodoViewGroupByShowPackages"
        | "TodoViewGroupByFlattenPackage"
        // The tree keys and the right split (SPEC-0275 v13 behaviours 32 and 36), the query
        // history list (SPEC-0093 v5 behaviour 16).
        | "ExpandTreeNode"
        | "CollapseTreeNode"
        | "FullyExpandTreeNode"
        | "OpenInRightSplit"
        | "Console.History.Browse"
        // Recent Files with only the changed ones, and its toggle (SPEC-0069 v3 behaviour 9).
        | "RecentChangedFiles"
        | "SwitcherRecentEditedChangedToggleCheckBox"
        // Iterate Recent Files moves the Switcher's selection (SPEC-0059 v9.10 behaviour 27h).
        | "SwitcherIterateItems"
        | "CallHierarchy"
        | "TypeHierarchy"
        | "MethodHierarchy"
        | "ToggleFullScreen"
        | "ToggleDistractionFreeMode"
        | "ShowSettings"
        | "ShowProjectStructureSettings"
        // Force Refresh reads files, Git and run configurations and writes nothing; Header/Source
        // opens a file (SPEC-0059 v9.8 behaviour 30, SPEC-0071 v12 behaviour 19).
        | "ForceRefresh"
        | "SwitchHeaderSource"
        | "ChooseRunConfiguration"
        | "ChooseDebugConfiguration"
        | "ShowNavBar"
        | "SelectAllOccurrences"
        | "SelectNextOccurrence"
        | "UnselectPreviousOccurrence"
        // Commit Message History opens a list in the Commit tool window; neither a file nor
        // the repository changes (SPEC-0063 v10 behaviour 22).
        | "Vcs.ShowMessageHistory"
        | "EditorEscape" => vec![None],
        "$Cut"
        | "$Delete"
        | "EditorBackSpace"
        | "EditorEnter"
        | "EditorTab"
        | "EditorDuplicate"
        | "EditorJoinLines"
        | "EditorDeleteLine"
        | "EditorIndentSelection"
        | "EditorUnindentSelection"
        | "EditorToggleCase"
        // Named here: ids that begin with `Editor` and are not listed count as moving the caret.
        | "EditorSortLines"
        | "EditorReverseLines"
        | "EditorTranspose"
        | "ConvertIndentsToSpaces"
        | "ConvertIndentsToTabs"
        // Cut up to Line End and Auto-Indent Lines change the text (SPEC-0057 v6). The font size
        // actions change the user's settings file; without this line the `Editor` prefix would
        // declare them as changing nothing (SPEC-0129 v4 behaviour 8).
        | "EditorCutLineEnd"
        | "AutoIndentLines"
        | "EditorIncreaseFontSizeGlobal"
        | "EditorDecreaseFontSizeGlobal"
        | "EditorSplitLine"
        | "EditorStartNewLine"
        | "EditorCompleteStatement"
        | "CommentByLineComment"
        | "CommentByBlockComment"
        | "ReformatCode"
        | "OptimizeImports"
        | "SaveAll"
        | "SaveAs"
        | "RenameElement"
        | "Refactorings.QuickListPopupAction"
        | "ExtractMethod"
        | "ExtractInterface"
        | "ExtractSuperclass"
        | "MembersPullUp"
        | "IntroduceField"
        | "IntroduceVariable"
        | "Inline"
        | "Move"
        | "SafeDelete"
        // Reformat File… formats and optimizes imports (SPEC-0075 v6 behaviour 8); without
        // this line the `Show` prefix would declare it as changing nothing.
        | "ShowReformatFileDialog"
        | "Generate"
        | "SurroundWith"
        | "DelegateMethods"
        | "InsertLiveTemplate"
        | "CodeCompletion"
        | "HippieCompletion"
        | "HippieBackwardCompletion"
        | "EditorChooseLookupItem"
        | "EditorChooseLookupItemReplace"
        // Class name completion and inserting with a dot insert a suggestion, Unwrap and
        // Introduce Parameter change the code (SPEC-0072 v13, SPEC-0056 v5, SPEC-0095 v11).
        | "EditorChooseLookupItemDot"
        | "ClassNameCompletion"
        | "Unwrap"
        | "IntroduceParameter"
        | "SmartTypeCompletion"
        | "MoveLineUp"
        | "MoveLineDown"
        | "MoveStatementUp"
        | "MoveStatementDown"
        | "MoveElementLeft"
        | "MoveElementRight"
        | "NewElement"
        | "NewElementSamePlace"
        | "NewFile"
        | "$Undo"
        | "$Redo" => vec![Write],
        // Tool window layouts (SPEC-0333 behaviour 14): the default layout shows nothing new;
        // a saved one can show the Terminal, which starts a shell; saving writes the user's
        // settings.
        "RestoreFactoryDefaultLayout" => vec![None],
        "RestoreDefaultLayout" => vec![Run],
        "StoreDefaultLayout" | "StoreNewLayout" => vec![Write],
        _ if id.starts_with("IDECraft.Layout.Apply:") => vec![Run],
        _ if id.starts_with("IDECraft.Layout.Rename:")
            || id.starts_with("IDECraft.Layout.Delete:") =>
        {
            vec![Write]
        }
        // Taking a change over in the diff viewer writes the other side's file (SPEC-0084 v5
        // behaviour 23).
        "Diff.ApplyLeftSide" | "Diff.ApplyRightSide" => vec![Write],
        // The export of the Find tool window writes the chosen file, a scratch file is
        // created in the user's scratches folder, Copy… creates a file or folder (SPEC-0293
        // v9 behaviour 26, SPEC-0059 v9.6 behaviour 28, SPEC-0275 v18 behaviour 46).
        "ExportToTextFile" | "NewScratchFile" | "CopyElement" => vec![Write],
        // The viewer's other actions change no file: the current pane, the file shown and the
        // settings popup. The popup is not covered by the `Show` prefix below (`Diff.`).
        "Diff.FocusOppositePane"
        | "Diff.NextChange"
        | "Diff.PrevChange"
        | "Diff.ShowSettingsPopup" => vec![None],
        // Changelists are kept in a file of the workspace (SPEC-0335 behaviour 25).
        "ChangesView.NewChangeList"
        | "ChangesView.Rename"
        | "ChangesView.SetDefault"
        | "ChangesView.RemoveChangeList"
        | "ChangesView.Move" => vec![Write],
        // Bookmarks are kept in a file of the workspace (SPEC-0331 behaviour 27).
        "ToggleBookmark" | "ToggleBookmarkWithMnemonic" => vec![Write],
        "ToggleBookmark0" | "ToggleBookmark1" | "ToggleBookmark2" | "ToggleBookmark3"
        | "ToggleBookmark4" | "ToggleBookmark5" | "ToggleBookmark6" | "ToggleBookmark7"
        | "ToggleBookmark8" | "ToggleBookmark9" => vec![Write],
        "CheckinProject"
        | "Vcs.RollbackChangedLines"
        | "ChangesView.Revert"
        | "Git.CreateNewBranch"
        | "Git.Rename.Local.Branch"
        | "Git.Reword.Commit"
        | "Vcs.ToggleAmendCommitMode"
        | "Git.Stash"
        | "Git.Unstash"
        | "Git.Branches" => {
            vec![VcsWrite]
        }
        "Vcs.UpdateProject" | "Vcs.Push" | "Git.Fetch" | "Git.Pull" => vec![VcsWrite, Remote],
        "Run"
        | "Rerun"
        | "RunClass"
        | "RunAnything"
        | "RerunTests"
        | "RunCoverage"
        | "RunToCursor"
        | "ForceRunToCursor"
        | "Stop"
        | "StopBackgroundProcesses"
        // Recompile runs the module's build tool (SPEC-0100 v5 behaviour 22).
        | "Compile"
        // The end of input lets a program of the Run tool window go on (SPEC-0087 v10
        // behaviour 18): it changes what a started program does, as Stop does.
        | "SendEOF"
        | "Terminal.NewTab"
        | "ActivateTerminalToolWindow" => vec![Run],
        "Debug"
        | "DebugClass"
        | "XDebugger.AttachToProcess"
        | "ToggleLineBreakpoint"
        | "XDebugger.SetValue"
        | "XDebugger.NewWatch"
        | "Resume"
        | "Pause"
        | "StepOver"
        | "StepInto"
        | "StepOut"
        | "EvaluateExpression"
        // Quick Evaluate Expression runs code in the paused program (SPEC-0091 v19).
        | "QuickEvaluateExpression" => vec![Debug],
        // File Path starts the system's file manager (SPEC-0275 v13 behaviour 34).
        "ShowFilePath" => vec![Run],
        "Exit" | "$Quit" | "IDECraft.Quit" => vec![Quit],
        // The file properties rewrite a file or its permissions (SPEC-0336 behaviour 22).
        "ChangeFileEncodingAction"
        | "ToggleReadOnlyAttribute"
        | "ConvertToWindowsLineSeparators"
        | "ConvertToUnixLineSeparators"
        | "ConvertToMacLineSeparators"
        | "AddBom"
        | "RemoveBom"
        // The file type actions write the project's `.idecraft/file-types.toml` (SPEC-0004 v5
        // behaviour 13).
        | "OverrideFileTypeAction"
        | "ReverteOverrideFileTypeAction"
        | "AssociateWithFileType" => vec![Write],
        // Closing project windows ends their runs and may drop unsaved files like quitting
        // (SPEC-0094 v3 behaviour 10).
        "CloseProject" | "CloseAllProjects" | "CloseOtherProjects" => vec![Quit],
        _ => Vec::new(),
    };
    if !explicit.is_empty() {
        return Some(explicit);
    }

    let none = || Some(vec![None]);
    let starts = |p: &str| id.starts_with(p);
    if starts("Activate") && id.ends_with("ToolWindow")
        || starts("Goto")
        || starts("GoTo")
        || starts("SearchEverywhere")
        || starts("Editor")
            && !id.contains("Paste")
            && !id.contains("Delete")
            && !id.contains("BackSpace")
            && !id.contains("Enter")
            && !id.contains("Tab")
            && !id.contains("Join")
            && !id.contains("Duplicate")
            && !id.contains("Indent")
            && !id.contains("Toggle")
            && !id.contains("Cut")
            && !id.contains("Split")
            && !id.contains("Case")
        || starts("Find")
        || starts("Show")
        || starts("Quick")
        || starts("Hide")
        || starts("Next")
        || starts("Previous")
        || starts("SelectIn")
        || starts("FileStructure")
        || starts("RecentFiles")
        || starts("RecentLocations")
        || starts("TextSearchAction")
        // The endpoint search of Search Everywhere (SPEC-0067 v9 behaviour 20).
        || id == "GotoUrlAction"
        || starts("StructureViewPopupMenu")
        || starts("ParameterInfo")
        || starts("ExpressionTypeInfo")
        || starts("CollapseAll")
        || starts("ExpandAll")
        || starts("CollapseRegion")
        || starts("ExpandRegion")
        || starts("ExpandToLevel")
        || starts("JumpToLastWindow")
        || starts("JumpToLastChange")
        || starts("JumpToNextChange")
        || starts("MethodUp")
        || starts("MethodDown")
        || starts("MaximizeToolWindow")
        || starts("ResizeToolWindow")
    {
        return none();
    }
    Option::None
}

/// Whether a keymap action is reserved for the user: anything that reads the system clipboard.
pub fn reserved_action(id: &str) -> Option<Reserved> {
    (id.contains("Paste") || id == "PasteMultiple").then_some(Reserved::Clipboard)
}

/// One key of a `press` with the effects of its keymap actions and why it is reserved, if it is.
pub type KeyFact = (KeyPress, Option<Vec<Effect>>, Option<Reserved>);

/// The rights a call has (behaviour 34): `ui` and the effect rights of the workspace's grant file
/// and of this session's answers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grants(pub BTreeSet<String>);

impl Grants {
    /// Grants with these names.
    pub fn of(names: &[&str]) -> Self {
        Self(names.iter().map(|s| (*s).to_owned()).collect())
    }

    /// Whether `right` is granted.
    pub fn has(&self, right: &str) -> bool {
        self.0.contains(right)
    }
}

/// Checks an external action on `element` (or a key press without one) before any input is
/// delivered (behaviour 35). Returns the effects that the action has. In-process actors pass.
pub fn authorize(
    actor: &Actor,
    action: &Action,
    element: Option<&UiNode>,
    key_effects: &[KeyFact],
    grants: &Grants,
) -> Result<Vec<Effect>, UiError> {
    if !actor.is_external() {
        return Ok(element.and_then(|e| e.effects.clone()).unwrap_or_default());
    }
    if !grants.has(UI_RIGHT) {
        return Err(UiError::not_granted(UI_RIGHT, UI_MEANING));
    }
    let reserved = |why: Reserved, what: String| {
        UiError::new(
            ErrorCode::Reserved,
            format!("reserved: {what} is reserved for the user ({})", why.name()),
            "the user does this in the window; nothing was delivered",
        )
    };
    let mut effects: BTreeSet<Effect> = BTreeSet::new();
    if let Some(e) = element {
        if let Some(why) = e.reserved {
            return Err(reserved(why, describe(e)));
        }
        if e.secret
            && matches!(
                action,
                Action::Type { .. } | Action::SetValue { .. } | Action::Press { .. }
            )
        {
            return Err(reserved(
                Reserved::Secret,
                format!("typing into {}", describe(e)),
            ));
        }
        match &e.effects {
            Some(list) => effects.extend(list.iter().copied()),
            None if action_needs_effects(action) => {
                return Err(UiError::new(
                    ErrorCode::Undeclared,
                    format!(
                        "undeclared: {} declares no effect for its action, so external clients may not operate it",
                        describe(e)
                    ),
                    "the view must annotate the element with its effects (SPEC-0319 behaviour 35)",
                ));
            }
            None => {}
        }
    }
    for (key, key_effects, why) in key_effects {
        if key.is_paste() {
            return Err(reserved(
                Reserved::Clipboard,
                format!("`{}` (Paste)", key.to_text()),
            ));
        }
        if let Some(why) = why {
            return Err(reserved(*why, format!("`{}`", key.to_text())));
        }
        match key_effects {
            Some(list) => effects.extend(list.iter().copied()),
            None => {
                return Err(UiError::new(
                    ErrorCode::Undeclared,
                    format!(
                        "undeclared: the effect of `{}` is not declared, so external clients may not press it",
                        key.to_text()
                    ),
                    "press keys whose keymap action has a declared effect, or type text into a field",
                ));
            }
        }
    }
    if effects.contains(&Effect::Trust) {
        return Err(reserved(Reserved::Trust, "trusting the project".into()));
    }
    for effect in &effects {
        if let Some(right) = effect.right()
            && !grants.has(right)
        {
            return Err(UiError::not_granted(right, effect.meaning()));
        }
    }
    Ok(effects.into_iter().collect())
}

/// Whether the action does something to the element beyond showing a pointer over it.
fn action_needs_effects(action: &Action) -> bool {
    !matches!(
        action,
        Action::Hover { .. } | Action::ScrollIntoView | Action::Scroll { .. }
    )
}

/// Refuses every action of an external client while a modal question reserved for the user is
/// shown in the window (`SPEC-0319` v6 behaviour 35, UISEC-14): whatever it pressed would reach
/// the question, whose answers are the user's. In-process actors pass.
pub fn refuse_while_reserved_modal(actor: &Actor, tree: &Tree) -> Result<(), UiError> {
    if !actor.is_external() {
        return Ok(());
    }
    match tree.reserved_modal() {
        Some(m) => Err(question_open(&describe(m), m.reserved)),
        None => Ok(()),
    }
}

/// The refusal while a question reserved for the user is open in the window; `what` names it.
pub fn question_open(what: &str, why: Option<Reserved>) -> UiError {
    UiError::new(
        ErrorCode::Reserved,
        format!(
            "reserved: {what} is open in the window; its answers are reserved for the user ({})",
            why.unwrap_or(Reserved::Decision).name()
        ),
        "the user answers it in the window; nothing was delivered",
    )
}

fn describe(e: &UiNode) -> String {
    let mut s = crate::tree::role_name(e.role());
    if let Some(n) = e.name() {
        s.push_str(&format!(" {n:?}"));
    }
    if let Some(t) = e.test_id() {
        s.push_str(&format!(" #{t}"));
    }
    s
}

/// The emergency stop counter of this process (behaviour 37). The MCP server withdraws grants of
/// `ui` given for one session when it changed.
static STOP_GENERATION: AtomicU64 = AtomicU64::new(0);

/// How many emergency stops this process has seen.
pub fn stop_generation() -> u64 {
    STOP_GENERATION.load(Ordering::SeqCst)
}

/// One external client driving a window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Driving {
    /// The client's name.
    pub client: String,
    /// Its session.
    pub session: String,
    /// The window it drives.
    pub window: u64,
    /// Its last call.
    #[serde(skip)]
    pub last_call: Option<Instant>,
}

/// Why driving stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopSource {
    /// Esc pressed by the user while the marking was shown.
    Escape,
    /// The Stop button of the marking.
    StopButton,
    /// The menu item "Stop UI Automation".
    Menu,
    /// The MCP tool `ui_driver_stop`.
    Mcp,
    /// `idecraft drive stop`.
    Cli,
}

/// The state of external driving in the window process.
#[derive(Debug, Default)]
pub struct Supervisor {
    /// Clients driving now, by session.
    sessions: BTreeMap<String, Driving>,
    /// When the user's own input last came in.
    last_user_input: Option<Instant>,
    /// The last stop.
    last_stop: Option<StopSource>,
}

impl Supervisor {
    /// A supervisor with nobody driving.
    pub fn new() -> Self {
        Self::default()
    }

    /// Notes that `actor` drives `window` at `now`.
    pub fn driving(&mut self, actor: &Actor, window: u64, now: Instant) {
        if let Actor::External { client, session } = actor {
            self.sessions.insert(
                session.clone(),
                Driving {
                    client: client.clone(),
                    session: session.clone(),
                    window,
                    last_call: Some(now),
                },
            );
        }
    }

    /// Ends the sessions without a call since [`SESSION_IDLE`] (the client went away); sessions
    /// in `busy` wait for a call and count as active. A client that comes back is listed again.
    pub fn expire(&mut self, now: Instant, busy: &BTreeSet<String>) -> Vec<Driving> {
        let mut gone = Vec::new();
        self.sessions.retain(|session, d| {
            if busy.contains(session) {
                d.last_call = Some(now);
                return true;
            }
            let idle = d
                .last_call
                .is_some_and(|t| now.saturating_duration_since(t) >= SESSION_IDLE);
            if idle {
                gone.push(d.clone());
            }
            !idle
        });
        gone
    }

    /// The clients driving `window` (the marking shows them).
    pub fn drivers_of(&self, window: u64) -> Vec<&Driving> {
        self.sessions
            .values()
            .filter(|d| d.window == window)
            .collect()
    }

    /// Every client driving.
    pub fn drivers(&self) -> Vec<&Driving> {
        self.sessions.values().collect()
    }

    /// Notes the user's own keyboard or pointer input.
    pub fn user_input(&mut self, now: Instant) {
        self.last_user_input = Some(now);
    }

    /// Whether external clients are paused at `now` (3 s after the user's last input).
    pub fn paused(&self, now: Instant) -> bool {
        self.last_user_input
            .is_some_and(|t| now.saturating_duration_since(t) < USER_PAUSE)
    }

    /// The emergency stop: ends every external session and counts the stop. The caller cancels
    /// pending calls, switches the setting off and logs it.
    pub fn stop(&mut self, source: StopSource) -> Vec<Driving> {
        STOP_GENERATION.fetch_add(1, Ordering::SeqCst);
        self.last_stop = Some(source);
        std::mem::take(&mut self.sessions).into_values().collect()
    }

    /// The last stop, if any.
    pub fn last_stop(&self) -> Option<StopSource> {
        self.last_stop
    }
}

/// The error of a call cancelled by the emergency stop.
pub fn stopped() -> UiError {
    UiError::new(
        ErrorCode::Cancelled,
        "cancelled: the user stopped UI automation",
        "only the user can switch it on again in Settings",
    )
}

/// The error of a call paused by the user's own input.
pub fn interrupted() -> UiError {
    UiError::new(
        ErrorCode::Interrupted,
        "interrupted: the user is using the window",
        "try again 3 s after the user's last input",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0334 AK-10: showing the TODO window and regrouping its tree change nothing.
    #[test]
    fn todo_actions_declare_no_effect() {
        for id in [
            "ActivateTODOToolWindow",
            "TodoViewGroupByShowModules",
            "TodoViewGroupByShowPackages",
            "TodoViewGroupByFlattenPackage",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::None]), "{id}");
        }
    }

    /// SPEC-0333 AK-6: the default layout changes nothing outside the window; a saved layout
    /// can show the Terminal and start its shell; saving, renaming and deleting a layout write
    /// the user's settings.
    #[test]
    fn layout_actions_declare_their_effects() {
        assert_eq!(
            effects_of_action("RestoreFactoryDefaultLayout"),
            Some(vec![Effect::None])
        );
        for id in ["RestoreDefaultLayout", "IDECraft.Layout.Apply:Review"] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Run]), "{id}");
        }
        for id in [
            "StoreDefaultLayout",
            "StoreNewLayout",
            "IDECraft.Layout.Rename:Review",
            "IDECraft.Layout.Delete:Review",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Write]), "{id}");
        }
        // Only these prefixes: another id under the same name is undeclared.
        assert_eq!(
            effects_of_action("IDECraft.Layout.Export:Review"),
            Option::None
        );
    }

    /// SPEC-0331 AK-9: setting and removing a bookmark changes a file of the workspace; going
    /// to one, the list and the tool window change nothing.
    #[test]
    fn bookmark_actions_declare_their_effects() {
        let mut toggles = vec![
            "ToggleBookmark".to_owned(),
            "ToggleBookmarkWithMnemonic".into(),
        ];
        toggles.extend((0..=9).map(|d| format!("ToggleBookmark{d}")));
        for id in &toggles {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Write]), "{id}");
        }
        let mut shows = vec![
            "ShowBookmarks".to_owned(),
            "GotoNextBookmark".into(),
            "GotoPreviousBookmark".into(),
            "ActivateBookmarksToolWindow".into(),
        ];
        shows.extend((0..=9).map(|d| format!("GotoBookmark{d}")));
        for id in &shows {
            assert_eq!(effects_of_action(id), Some(vec![Effect::None]), "{id}");
        }
    }

    /// SPEC-0336 AK-9: the file property actions are declared as writes.
    #[test]
    fn file_property_actions_are_declared_as_writes() {
        for id in [
            "ChangeFileEncodingAction",
            "ToggleReadOnlyAttribute",
            "ConvertToWindowsLineSeparators",
            "ConvertToUnixLineSeparators",
            "ConvertToMacLineSeparators",
            "AddBom",
            "RemoveBom",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Write]), "{id}");
        }
    }

    /// SPEC-0004 AK-8 (v5): the file type actions are declared as writes.
    #[test]
    fn file_type_actions_are_declared_as_writes() {
        for id in [
            "OverrideFileTypeAction",
            "ReverteOverrideFileTypeAction",
            "AssociateWithFileType",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Write]), "{id}");
        }
    }

    /// SPEC-0094 v3 AK-7: closing project windows is declared as quitting.
    #[test]
    fn closing_projects_is_declared_as_quitting() {
        for id in ["CloseProject", "CloseAllProjects", "CloseOtherProjects"] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Quit]), "{id}");
        }
    }

    /// SPEC-0072 v11 AK-30, SPEC-0057 v5: the actions that change the text are declared as
    /// writes, the steps through usages and the popup's keys as without effect.
    #[test]
    fn second_package_actions_declare_their_effects() {
        for id in [
            "HippieCompletion",
            "HippieBackwardCompletion",
            "EditorChooseLookupItem",
            "EditorChooseLookupItemReplace",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Write]), "{id}");
        }
        for id in [
            "GotoNextElementUnderCaretUsage",
            "GotoPrevElementUnderCaretUsage",
            "FindUsagesInFile",
            "SearchEverywhere.NextTab",
            "SearchEverywhere.PrevTab",
            "SearchEverywhere.SelectItem",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::None]), "{id}");
        }
    }

    /// SPEC-0067 AK-10 (v9, behaviour 20): the endpoint search opens a popup and changes
    /// nothing.
    #[test]
    fn endpoint_search_declares_no_effect() {
        assert_eq!(effects_of_action("GotoUrlAction"), Some(vec![Effect::None]));
    }

    /// SPEC-0335 AK-7 (behaviour 25): the changelist actions write a state file of the project.
    #[test]
    fn changelist_actions_declare_write() {
        for id in [
            "ChangesView.NewChangeList",
            "ChangesView.Rename",
            "ChangesView.SetDefault",
            "ChangesView.RemoveChangeList",
            "ChangesView.Move",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Write]), "{id}");
        }
    }

    /// SPEC-0063 AK-12 (v10, behaviour 22): Commit Message History opens a list and changes
    /// nothing; the Amend switch beside it stays a change to the repository's next commit.
    #[test]
    fn commit_message_history_declares_no_effect() {
        assert_eq!(
            effects_of_action("Vcs.ShowMessageHistory"),
            Some(vec![Effect::None])
        );
        assert_eq!(
            effects_of_action("Vcs.ToggleAmendCommitMode"),
            Some(vec![Effect::VcsWrite])
        );
    }

    /// The specifications of the seventh package (SPEC-0275 AK-42 and AK-43 v19, SPEC-0083
    /// AK-30 v9, SPEC-0077 AK-12 v7, SPEC-0087 AK-12 v10, SPEC-0061 AK-v16): what each action
    /// changes is declared; the end of input and stopping background tasks act on what runs.
    #[test]
    fn seventh_package_actions_declare_their_effects() {
        for id in [
            "List-selectFirstRow",
            "List-selectLastRow",
            "List-selectFirstRowExtendSelection",
            "List-selectLastRowExtendSelection",
            "GotoCustomRegion",
            "ExpressionTypeInfo",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::None]), "{id}");
        }
        for id in ["SendEOF", "StopBackgroundProcesses"] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Run]), "{id}");
        }
    }

    /// SPEC-0124 AK-18 (v17) and the specifications of the tenth package: Insert Live Template
    /// inserts a template, the template and Switcher keys only move a selection.
    #[test]
    fn tenth_package_actions_declare_their_effects() {
        assert_eq!(
            effects_of_action("InsertLiveTemplate"),
            Some(vec![Effect::Write])
        );
        for id in [
            "NextTemplateVariable",
            "PreviousTemplateVariable",
            "SwitcherIterateItems",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::None]), "{id}");
        }
    }

    /// SPEC-0124 AK-17 (v16) and the specifications of the ninth package: Safe Delete and
    /// Reformat File… write, Force Refresh and Header/Source change nothing.
    #[test]
    fn ninth_package_actions_declare_their_effects() {
        for id in ["ForceRefresh", "SwitchHeaderSource"] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::None]), "{id}");
        }
        for id in ["SafeDelete", "ShowReformatFileDialog"] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Write]), "{id}");
        }
    }

    /// SPEC-0124 AK-16 (v15) and the specifications of the eighth package: what each action
    /// changes is declared; Recompile runs a build, Quick Evaluate runs code in the program.
    #[test]
    fn eighth_package_actions_declare_their_effects() {
        for id in [
            "NextEditorTab",
            "PreviousEditorTab",
            "MethodOverloadSwitchUp",
            "MethodOverloadSwitchDown",
            "VcsHistory.ShowAllAffected",
            "EditorShowGutterIconTooltip",
            "CodeInspection.OnEditor",
            "RunInspection",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::None]), "{id}");
        }
        for id in [
            "EditorChooseLookupItemDot",
            "ClassNameCompletion",
            "Unwrap",
            "IntroduceParameter",
            "EditorCompleteStatement",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Write]), "{id}");
        }
        assert_eq!(effects_of_action("Compile"), Some(vec![Effect::Run]));
        assert_eq!(
            effects_of_action("QuickEvaluateExpression"),
            Some(vec![Effect::Debug])
        );
    }

    /// SPEC-0293 AK-14 and AK-15 (v9) and the other specifications of the sixth package: what
    /// each action changes is declared; the export, a scratch file and Copy… write.
    #[test]
    fn sixth_package_actions_declare_their_effects() {
        for id in [
            "UsageFiltering.ReadAccess",
            "UsageFiltering.WriteAccess",
            "UsageFiltering.Imports",
            "UsageGrouping.DirectoryStructure",
            "UsageGrouping.FlattenModules",
            "UsageView.Exclude",
            "UsageView.Include",
            "SearchEverywhere.NavigateToNextGroup",
            "SearchEverywhere.NavigateToPrevGroup",
            "NextParameter",
            "PrevParameter",
            "NextWindow",
            "PreviousWindow",
            "ViewSource",
            "ShowContent",
            "ShowFilterPopup",
            "ShowTypeBookmarks",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::None]), "{id}");
        }
        for id in ["ExportToTextFile", "NewScratchFile", "CopyElement"] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Write]), "{id}");
        }
    }

    /// SPEC-0275 AK-32, SPEC-0093 AK-12 (fourth package): what the tree, popup and history
    /// actions do is declared; New creates files and File Path starts the file manager.
    #[test]
    fn fourth_package_actions_declare_their_effects() {
        for id in [
            "ExpandTreeNode",
            "CollapseTreeNode",
            "FullyExpandTreeNode",
            "ExpandAll",
            "CollapseAll",
            "OpenInRightSplit",
            "TextSearchAction",
            "Console.History.Browse",
            "ShowSearchHistory",
            "EditorAddOrRemoveCaret",
            "EditorCreateRectangularSelectionOnMouseDrag",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::None]), "{id}");
        }
        assert_eq!(
            effects_of_action("NewElementSamePlace"),
            Some(vec![Effect::Write])
        );
        assert_eq!(effects_of_action("ShowFilePath"), Some(vec![Effect::Run]));
    }

    /// SPEC-0084 AK-17 (v5): taking a change over in the diff viewer is declared as writing;
    /// stepping, the current pane, the next and previous file and the settings popup are not.
    #[test]
    fn diff_viewer_actions_declare_their_effects() {
        for id in ["Diff.ApplyLeftSide", "Diff.ApplyRightSide"] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Write]), "{id}");
        }
        for id in [
            "NextDiff",
            "PreviousDiff",
            "Diff.FocusOppositePane",
            "Diff.NextChange",
            "Diff.PrevChange",
            "Diff.ShowSettingsPopup",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::None]), "{id}");
        }
        // An action of the viewer nobody declared stays undeclared.
        assert_eq!(effects_of_action("Diff.AppendLeftSide"), Option::None);
    }

    /// SPEC-0332 AK-7: the commands that change the text are declared as writing; a caret per
    /// line and the copies of a path are not.
    #[test]
    fn small_editing_commands_declare_their_effects() {
        for id in [
            "EditorSortLines",
            "EditorReverseLines",
            "EditorTranspose",
            "ConvertIndentsToSpaces",
            "ConvertIndentsToTabs",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Write]), "{id}");
        }
        for id in [
            "EditorAddCaretPerSelectedLine",
            "CopyReferencePopupGroup",
            "CopyAbsolutePath",
            "CopyFileName",
            "CopyContentRootPath",
            // SPEC-0332 v2 AK-13.
            "CopyPathWithLineNumber",
            "CopySourceRootPath",
            "CopyReference",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::None]), "{id}");
            assert_eq!(reserved_action(id), None, "{id}");
        }
    }

    #[test]
    fn sessions_of_clients_that_went_away_end_after_the_limit() {
        let t0 = Instant::now();
        let actor = |s: &str| Actor::External {
            client: "c".into(),
            session: s.into(),
        };
        let mut sup = Supervisor::new();
        sup.driving(&actor("gone"), 1, t0);
        sup.driving(&actor("busy"), 1, t0);
        sup.driving(&actor("calling"), 1, t0 + SESSION_IDLE);
        sup.driving(&Actor::SelfTest, 1, t0);
        let busy = BTreeSet::from(["busy".to_owned()]);
        assert!(sup.expire(t0 + SESSION_IDLE / 2, &busy).is_empty());
        let gone = sup.expire(t0 + SESSION_IDLE, &busy);
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].session, "gone");
        let left: Vec<&str> = sup.drivers().iter().map(|d| d.session.as_str()).collect();
        assert_eq!(left, ["busy", "calling"]);
        // A client that comes back is listed again.
        sup.driving(&actor("gone"), 1, t0 + SESSION_IDLE * 2);
        assert_eq!(sup.drivers().len(), 3);
    }

    /// SPEC-0057 v6 AK-23, SPEC-0083 v7 AK-18, SPEC-0129 v4 AK-7: the fifth package's actions
    /// declare what they do — the text and the settings file are written, modes, folds and the
    /// context strip are not.
    #[test]
    fn fifth_package_actions_declare_their_effects() {
        for id in [
            "EditorCutLineEnd",
            "AutoIndentLines",
            "EditorIncreaseFontSizeGlobal",
            "EditorDecreaseFontSizeGlobal",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::Write]), "{id}");
            assert_eq!(reserved_action(id), Option::None, "{id}");
        }
        for id in [
            "EditorToggleInsertState",
            "EditorContextInfo",
            "CollapseBlock",
            "CollapseSelection",
            // SPEC-0083 v8 AK-24: every folding action changes the view only.
            "CollapseDocComments",
            "ExpandDocComments",
            "CollapseAllRegions",
            "ExpandRegionRecursively",
            "CollapseRegionRecursively",
            "ExpandAllToLevel1",
            "ExpandToLevel5",
        ] {
            assert_eq!(effects_of_action(id), Some(vec![Effect::None]), "{id}");
            assert_eq!(reserved_action(id), Option::None, "{id}");
        }
    }
}
