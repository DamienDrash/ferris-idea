# SPEC-0124: Menu bar

- **Version:** 19 · **Date:** 2026-11-15 (v19: shortcuts shown after the active keyboard layout,
  behaviour 3a, AK-20, OBS-0055 M6; v18: the View, Navigate and Edit items of OBS-0055 with the
  shortcuts the reference shows, behaviour 20, AK-19); 2026-11-15 (v17: the menu item of the tenth keymap package, behaviour 19, AK-18); 2026-11-14 (v16: the menu items of the ninth keymap package, behaviour 18, AK-17; ninth package of docs/research/KEYMAP_COVERAGE.md; v15: the menu items of the eighth keymap package, behaviour 17, AK-16; eighth package of docs/research/KEYMAP_COVERAGE.md; v14: the menu items of the seventh keymap package, behaviour 16, AK-15; v13: the menu items of the sixth keymap package, behaviour 15, AK-14; v1: 2026-11-02, v2: 2026-11-03; v4: the actions added from the keymap coverage, behaviour 7; v5: the navigation actions from the keymap coverage, behaviour 8; v6 is the second keymap package (PR #679), written on another branch and not in this text until that branch is merged; v7: the Bookmarks submenu of Edit, behaviour 10; v8: File, Edit, Navigate, Refactor and Window follow the open platform's main-menu groups, behaviour 11; v9 is written on another branch (PR 690); v10: closing project windows in File, behaviour 13; v11: "Not offered" no longer names what has been built since — closing projects (v10, SPEC-0094 v3), the endpoint search (SPEC-0067 v9), named layouts (SPEC-0333) and the HTTP client's menu entry (SPEC-0096 v4); v12: the File Properties submenu of File, behaviour 14, SPEC-0336)
- **Sources:** GEN (the reference has an application menu bar with File, Edit, View, Navigate,
  Code, Refactor, Build, Run, Git/VCS, Tools, Window, Help; items show their shortcuts; F-087),
  OSS (action ids and their shortcuts from the ported keymaps; menu item captions of the platform
  actions), STD (macOS: the application menu is named after `CFBundleName`; a key equivalent goes
  to the key window before the main menu), found in hand testing on 2026-11-02 (project owner: the menu
  bar was missing, the reference's showed instead), IMP-0013; v3: hand test 50 of 2026-11-13
  (agent run, bundle from `main` @ `dda6dc23`; findings MB1 to MB3 in the status file: in a project
  with a Git repository the reference's bar reads File, Edit, View, Navigate, Code, Refactor, Build,
  Run, Tools, Git, Window, Help, and VCS stands in Git's place in a project without one), OBS-0024
  (Build Project is started from the Build menu), DOC (the reference's help names its tab actions
  under Window › Editor Tabs and the actions on the focused tool window under Window › Active Tool
  Window), STD (macOS: a Window menu with Minimize and Zoom, to which the system adds the
  application's open windows), SPEC-0059 v9 behaviour 19, SPEC-0061 v6 behaviour 34, SPEC-0100
  behaviours 9 and 18, SPEC-0167 behaviour 6; v7: an observation of the reference's menu bar on
  2026-11-13 (macOS, user interface only; to be added to OBS-0040): Edit has a submenu titled
  Bookmarks, after Macros and before Emoji & Symbols, near the end of the menu; Navigate has no
  bookmark items; the submenu's own items were not read. SPEC-0331 (the bookmark actions); v8:
  OSS — the main-menu action groups and action texts of the open platform, `JetBrains/intellij-community`
  at the pinned commit `d4018fb` of APACHE_PORTING.md: `platform/platform-impl/resources/idea/PlatformActions.xml`
  (the groups of File, Edit, Navigate and Window), `platform/platform-impl/resources/idea/LangActions.xml`
  (the Refactor group and what the language layer adds to Edit and Navigate),
  `platform/usageView-impl/resources/intellij.platform.usageView.impl.actions.xml` (the usage search group),
  `platform/navbar/frontend/resources/intellij.platform.navbar.frontend.xml`,
  `platform/structure-view-impl/resources/intellij.platform.structureView.xml`,
  `platform/lvcs-impl/resources/intellij.platform.lvcs.impl.xml`,
  `platform/lang-impl/resources/intellij.platform.lang.impl.actions.xml` (the hierarchy group), and the texts
  in `platform/platform-resources-en/src/messages/ActionsBundle.properties`,
  `platform/navbar/frontend/resources/messages/NavBarFrontendBundle.properties` and
  `platform/lvcs-impl/resources/messages/LocalHistoryBundle.properties` (PROVENANCE.md: logic template, no
  code); OBS-0041 (findings in our own words on where functions sit in the reference's menus; it is the
  reason for this version, not the source of any order or title); GEN; v9: SPEC-0332 (the actions),
  OSS — the places of their ids in the Edit group of `PlatformActions.xml` and their texts in
  `ActionsBundle.properties`, same commit
- **Feature matrix rows:** F-087

## Purpose
Every action can be found and started from the menu bar, with its shortcut shown, as in the
reference.

## Behaviour
1. **Application menu:** named "IDECraft" (the binary embeds an `Info.plist` with
   `CFBundleName`, so the name is right without an app bundle), with Settings…, Services and
   Quit IDECraft (⌘Q). No update checks or licence entries (ADR-0007). [STD]
2. **Menus (v3):** File, Edit (with a Find submenu), View (with a Tool Windows submenu listing
   every tool window), Navigate, Code (with a Folding submenu), Refactor, Build, Run, Tools (Database
   and Python submenus), Git, Window, Help — in the order hand test 50 found in the reference, and
   with its captions, holding only actions IDECraft carries out (a unit test checks every item has a
   handler). A chosen item runs its action the way Search Everywhere does: in the editor, else in
   the workbench; an action that does not apply here says so in the status bar. [GEN, OSS, hand
   test 50 MB1]
3. **Shortcuts:** each item shows the first single-keystroke shortcut of its action in the keymap in
   use, including the user's changes; changing a shortcut in Settings updates the menu at once.
   A shortcut runs its action once: the window handles the key first and the menu only acts when
   no view took it. [OSS, STD]
3a. **v19 – After the keyboard layout:** on macOS a shortcut whose key the active layout moves
   to another key shows that key, as macOS localizes shortcuts of its own applications: on a German
   layout Back and Forward show ⌘Ö and ⌘Ä instead of ⌘[ and ⌘].
   The table of moved keys is the one the platform reports for the layout
   (Apple's localization, which is not the key in the same position); letters, digits and named
   keys stay. The labels are set again when the layout changes. The keymap binds characters, not
   key codes: a key pressed with ⌘, ⌃ or ⌥ is first looked up as the layout types it and, where
   the keymap knows no action for that, as the US key the layout's key stands in for (SPEC-0071 v4
   behaviour 6), so ⌘Ö on a German layout runs Back. Until v18 the menu showed the US keys
   (OBS-0055 M6). [OBS, STD]
4. **Export Data… (v2):** Tools › Database › Export Data… opens the export dialog of SPEC-0021 v2
   for the result shown in the Services window (the selected rows when several are selected); with
   no result shown, the status bar says to run a query first. Like every menu item it is also found
   by Search Everywhere's actions. [IMP-0013]
5. **Build menu (v3):** between Refactor and Run, with the two build actions IDECraft has, under
   the names and shortcuts they already have: Build Project (`CompileDirty`, SPEC-0100 behaviour
   18) and, after a separator, Sync Build Changes (`ExternalSystem.ProjectRefreshAction`, SPEC-0100
   behaviour 9). Until v3 neither was in a menu; nothing moves out of Run or Tools. The tasks of a
   build tool are started from the build tools window (SPEC-0100 behaviours 2 and 10), which
   View › Tool Windows opens; they have no menu item. [hand test 50 MB2, OBS-0024]
6. **Window menu (v3):** between Git and Help: Minimize (`MinimizeCurrentWindow`, ⌘M) and Zoom
   (`ZoomCurrentWindow`, ⌃⌘=), which minimise and zoom the window the way its title bar buttons
   do; a submenu Active Tool Window with Hide Active Tool Window and Hide All Tool Windows
   (SPEC-0167 behaviour 6; until v3 in View) and, after a separator, Stretch to Left, Stretch to
   Right, Stretch to Top, Stretch to Bottom and Maximize Tool Window (SPEC-0061 behaviour 34, the
   names of the tool window header's menu; until v3 in no menu); the submenu Editor Tabs with the
   tab actions of SPEC-0059 behaviour 19 (until v3 in View). On macOS the system lists the open
   project windows at the end of this menu and brings the chosen one to the front. Find Action
   shows these actions with their new place: the submenu's name, or Window. [hand test 50 MB3,
   DOC, STD]
7. **Actions from the keymap coverage (v4):** File gets Save As… (`SaveAs`) and Reload All from Disk (`Synchronize`) after Save All (SPEC-0059 v9.3); Navigate gets Select in Project Tree (`SelectIn`); Git gets New Branch… (`Git.CreateNewBranch`) after Branches… (SPEC-0068 v9); Window › Active Tool Window gets Jump to Last Tool Window (`JumpToLastWindow`) and Close Active Tab (`CloseActiveTab`), and Window gets Restore Default Layout (`RestoreDefaultLayout`) after the two submenus (SPEC-0167 v5). The actions that act on a focused part — the diff viewer's (SPEC-0084 v4), the Commit tool window's (SPEC-0063 v9), the branch popup's rename (SPEC-0068 v9) and the Git log's (SPEC-0086 v11) — get no menu bar item; Find Action lists them with the name of their part (Diff, Commit, Git) as the group. [GEN]
8. **Navigation actions from the keymap coverage (v5):** Navigate gets Related Symbol… (`GotoRelated`) and Test (`GotoTest`) after Super Method (SPEC-0071 v10); View gets Quick Definition (`QuickImplementations`) after Quick Documentation (SPEC-0081 v4) and Recently Changed Files (`RecentChangedFiles`) after Recent Files (SPEC-0069 v3). An item shows no shortcut on a chord its action yields to another action (behaviour 3 otherwise): Test shows none in the macOS keymap, where ⇧⌘T is Reopen Closed Tab (SPEC-0071 v10 behaviour 16). The grouping toggles of the Find tool window (SPEC-0293 v6) get no menu bar item; Find Action lists them with the group Find. [GEN]
9. **More actions from the keymap coverage (v6):** none of them gets a menu bar item. Find Action lists Find Usages in File (`FindUsagesInFile`, SPEC-0293 v7) under Edit beside Highlight Usages in File; Next Highlighted Usage and Previous Highlighted Usage (`GotoNextElementUnderCaretUsage`, `GotoPrevElementUnderCaretUsage`, SPEC-0295 v2) under Navigate; Cyclic Expand Word and Cyclic Expand Word (Backward) (`HippieCompletion`, `HippieBackwardCompletion`, SPEC-0057 v5) under Code beside Code Completion. The actions of a part that is only there for a moment are grouped by that part, as in behaviour 7: Insert Suggestion and Replace with Suggestion (`EditorChooseLookupItem`, `EditorChooseLookupItemReplace`, SPEC-0072 v11) under Completion List; Next Tab, Previous Tab and Open Selected Item (`SearchEverywhere.NextTab`, `SearchEverywhere.PrevTab`, `SearchEverywhere.SelectItem`, SPEC-0067 v7) under Search Everywhere. [GEN]
10. **Bookmarks (v7):** Edit ends with a submenu Bookmarks, after a separator — the place the reference has it, whose neighbours there (Macros before, Emoji & Symbols after) IDECraft does not have. It holds Toggle Bookmark (`ToggleBookmark`) and Toggle Bookmark with Mnemonic (`ToggleBookmarkWithMnemonic`), a separator, Show Bookmarks (`ShowBookmarks`), a separator, Go to Next Bookmark (`GotoNextBookmark`) and Go to Previous Bookmark (`GotoPreviousBookmark`), with the titles of SPEC-0331 behaviour 26 and the shortcuts of the keymap (F3, ⌥F3, ⌘F3 on macOS; next and previous have none). The contents of the reference's submenu are not observed yet; these five are IDECraft's choice from SPEC-0331's actions. The actions for a digit (`ToggleBookmark0` … `ToggleBookmark9`, `GotoBookmark0` … `GotoBookmark9`) get no menu item; Navigate gets no bookmark item. Find Action shows every bookmark action with the group Bookmarks (until v7: Navigate), as behaviour 6 does for other submenus. [observation of 2026-11-13, GEN]
11. **File, Edit, Navigate, Refactor and Window after the open platform's groups (v8):** These five menus hold IDECraft's actions in the arrangement of the open platform's main-menu groups: an action the open group holds stands where that group has it relative to the other actions IDECraft has, under the open action text, and a separator stands where the open group has one between two items IDECraft has. An action or title the open sources do not have is IDECraft's own choice and is marked so. An action IDECraft cannot carry out gets no item — none that does nothing, none that is always disabled; "Not offered" below names what is missing. Each item below is marked [OSS] with the file that defines its place (the texts are from `ActionsBundle.properties` unless another bundle is named) or [ours]. `PlatformActions.xml`, `LangActions.xml` and the module files are those of the Sources line.
    - **File:** New… [ours], Open…, Recent Projects… (the place of the open recent-projects group [OSS, `PlatformActions.xml`]; one action with a dialog instead of a submenu, hence the ellipsis [ours]), Close Tab [ours], a separator, Remote Development… (the open group has an empty slot for remote development here [OSS, `PlatformActions.xml`]; action and title are SPEC-0317's [ours]), a separator, Settings…, Plugins… (the text and the place after Settings of the open plugins action [OSS, `PlatformActions.xml`]; IDECraft's action is `IDECraft.SettingsPlugins`, the Plugins page of Settings, SPEC-0061 behaviour 28; new in v8), a separator, the submenu Local History (the open group and its text, placed before Save All between separators [OSS, `intellij.platform.lvcs.impl.xml`, `LocalHistoryBundle.properties`]) with Show History (`LocalHistory.ShowHistory`; titled as in IDECraft's context menus, without the ellipsis of the open text [ours]; until v8 an item "Local History…" of File), a separator, Save All, Save As… [ours], Reload All from Disk [OSS, `PlatformActions.xml`]. Until v8 Remote Development… stood before Recent Projects… and the save actions before Local History.
    - **Edit:** Undo, Redo, a separator, Cut, Copy, Paste, Paste from History… (in the open group inside a paste submenu; flat here [ours]), Delete, a separator, Search in Selection Only (`ToggleInSelection`: restricts the open search bar to the selection or lifts the restriction, SPEC-0065; without an open search bar it does not apply and says so, behaviour 2. Place and text are those of the open platform's action for the same function, whose id differs [OSS, `PlatformActions.xml`]; new in the menu), the submenu Find (unchanged), the submenu Find Usages after it [OSS, `LangActions.xml`] with Find Usages (`FindUsages`), Show Usages (`ShowUsages`), a separator, Find Usages in File (`FindUsagesInFile`) and Highlight Usages in File (`HighlightUsagesInFile`) [OSS, `intellij.platform.usageView.impl.actions.xml`] (SPEC-0293, SPEC-0295; new in the menu, behaviour 9 no longer holds for the last two), a separator, Column Selection Mode, Select All [OSS, `PlatformActions.xml`], Add Selection for Next Occurrence, Unselect Occurrence, Select All Occurrences (the open group has these three in its Find submenu; here they stay with the selection actions [ours]), Extend Selection (`EditorSelectWord`) and Shrink Selection (`EditorUnSelectWord`) (last in the selection group [OSS, `LangActions.xml`]; new in the menu), a separator, Toggle Case, Join Lines, Duplicate Line or Selection [OSS, `PlatformActions.xml`], Delete Line [ours], a separator, Indent Selection (`EditorIndentSelection`) before Unindent Line or Selection (`EditorUnindentSelection`) [OSS, `LangActions.xml`, `PlatformActions.xml`; new in the menu], a separator, the submenu Bookmarks (behaviour 10). Until v8 Select All stood before Column Selection Mode and Duplicate before Join Lines and Toggle Case.
    - **Navigate:** Back, Forward, a separator, Search Everywhere, a separator, Class…, File…, Symbol… [OSS, `PlatformActions.xml`, `LangActions.xml`], Action… [ours], Text… (`TextSearchAction`, after Symbol… [OSS, `LangActions.xml`]: opens Search Everywhere on its Text tab, SPEC-0067, as Class… opens it on Classes; the action is new in v8 and has the shortcut of the keymaps), Line:Column… (`GotoLine`; the open main-menu text [OSS, `PlatformActions.xml`]; until v8 titled Line/Column…), a separator, Next Highlighted Error, Previous Highlighted Error (until v8 at the end of the menu), Last Edit Location (`JumpToLastChange`) and Next Edit Location (`JumpToNextChange`) [OSS, `PlatformActions.xml`] (SPEC-0071 v9 behaviour 14; new in the menu), a separator, the submenu Navigate in File with Next Method (`MethodDown`) and Previous Method (`MethodUp`) [OSS, `PlatformActions.xml`] (SPEC-0071 v9; new in the menu), a separator, Select in Project Tree (`SelectIn`; the open place [OSS, `LangActions.xml`]; IDECraft's title, because the action selects the file at once and offers no targets [ours]), Jump to Navigation Bar (`ShowNavBar`, SPEC-0061; after the select action [OSS, `intellij.platform.navbar.frontend.xml`, `NavBarFrontendBundle.properties`]; new in the menu), a separator, Declaration or Usages, Implementation(s), Type Declaration, Super Method [OSS, `LangActions.xml`], Related Symbol…, Test (behaviour 8; the open group has Test first [ours]), a separator, File Structure (`FileStructurePopup`; in the code group before the hierarchies [OSS, `intellij.platform.structureView.xml`]; until v8 in View), Type Hierarchy, Method Hierarchy, Call Hierarchy [OSS, `intellij.platform.lang.impl.actions.xml`], a separator, Spring Bean [ours].
    - **Navigate (v10):** Endpoint… (`GotoUrlAction`) stands after Text…: it opens Search Everywhere on its Endpoints tab (SPEC-0067 v9 behaviour 20). Title and place are IDECraft's own [ours]; the item shows no shortcut, since the bundled keymaps do not bind the id. Find Action lists it as Endpoint… with the group Navigate. With it the search popup for endpoints is no longer among the functions "Not offered" names under Navigation (`menus::tests::endpoint_search_is_in_navigate`, `menu_bar::tests::every_menu_action_is_carried_out_somewhere`, `every_menu_action_is_found_by_search_everywhere`).
    - **Refactor:** Refactor This…, Rename…, Change Signature…, a separator, the submenu Extract/Introduce with Introduce Variable…, Introduce Constant…, Introduce Field…, a separator, Extract Method…, a separator, Extract Interface… and Extract Superclass… (until v8 items of Refactor itself), Inline…, a separator, Move… (until v8 before the Introduce items), a separator, Pull Members Up… — the open Refactor group in its order with its separators, less the refactorings IDECraft does not have [OSS, `LangActions.xml`].
    - **Window:** Minimize, Zoom [OSS, `PlatformActions.xml`], Toggle Full Screen (the open platform has it under View › Appearance; it stays here, SPEC-0061 v12 [ours]), a separator, Restore Default Layout (at the place of the open layouts group, which holds the action of this id under another text; IDECraft's title and no submenu for one item, SPEC-0167 [ours]; until v8 at the end), a separator, the submenus Active Tool Window and Editor Tabs (unchanged; their place after the layouts [OSS, `PlatformActions.xml`]), a separator, Next Project Window, Previous Project Window (last in the open group [OSS, `PlatformActions.xml`]; until v8 after Toggle Full Screen).
    - **Tools:** unchanged and IDECraft's own [ours]; its items are not compared with the open platform's Tools group.
    - **Find Action** shows each of these actions with its menu title and, as its group, the menu or the submenu it is in now (behaviour 6): Find Usages for the four usage actions, Navigate in File for the two method steps, Extract/Introduce for the six refactorings, Local History for Show History, Navigate for File Structure.
12. **The small editing commands in Edit (v9):** Edit gets the actions of SPEC-0332 at the places their ids have in the open Edit group, under the open texts [OSS, `PlatformActions.xml`, `ActionsBundle.properties`]: Copy Path/Reference… (`CopyReferencePopupGroup`) after Copy and before Paste; Add Carets to Ends of Selected Lines (`EditorAddCaretPerSelectedLine`) after Select All, before the three occurrence selections that IDECraft keeps there; Sort Lines (`EditorSortLines`), Reverse Lines (`EditorReverseLines`) and Transpose (`EditorTranspose`) after Duplicate Line or Selection, before Delete Line [ours]; the submenu Convert Indents with To Spaces (`ConvertIndentsToSpaces`) and To Tabs (`ConvertIndentsToTabs`) after Unindent Line or Selection. Copy Path/Reference… opens its list (SPEC-0332 behaviour 13); the open group has a submenu-like group there that opens a popup too. Add Carets to Ends of Selected Lines shows ⌥⇧G. Find Action shows the two conversions with the group Convert Indents and the others with Edit (behaviour 6).

13. **Closing projects (v10):** File holds Close Project (`CloseProject`), Close All Projects
    (`CloseAllProjects`) and Close Other Projects (`CloseOtherProjects`) right after Recent
    Projects…, in the same group, as the open platform's File group holds them (behaviour 11;
    SPEC-0094 v3 behaviours 6 to 10). Find Action titles them with these labels under File. [OSS]
14. **File properties (v12):** File holds the submenu File Properties directly after Plugins…,
    before the separator that precedes Local History — the place of the open platform's group of
    this name [OSS, `PlatformActions.xml`; texts from `ActionsBundle.properties`]. It holds File
    Encoding (`ChangeFileEncodingAction`), Toggle Read-Only Attribute (`ToggleReadOnlyAttribute`)
    and the submenu Line Separators with CRLF - Windows (\r\n)
    (`ConvertToWindowsLineSeparators`), LF - Unix and macOS (\n) (`ConvertToUnixLineSeparators`)
    and CR - Classic Mac OS (\r) (`ConvertToMacLineSeparators`), in the open group's order; the
    separator names are those of `ApplicationBundle.properties`. The open group's item for a
    file's type has no counterpart (SPEC-0336, out of scope). Find Action also lists Add BOM
    (`AddBom`) and Remove BOM (`RemoveBom`) under File Properties; they have no menu item, as in
    the open main menu. What the actions do: SPEC-0336. [OSS]
15. **Sixth keymap package (v13):** four menus gain the actions of the sixth package of
    docs/research/KEYMAP_COVERAGE.md that have a place in the open platform's menus: File holds
    New Scratch File… (`NewScratchFile`, SPEC-0059 v9.6 behaviour 28) directly after New…; Edit ›
    Bookmarks holds Go to Mnemonic… (`ShowTypeBookmarks`, SPEC-0331 v3 behaviour 33) after Show
    Bookmarks; Refactor holds Copy… (`CopyElement`, SPEC-0275 v18 behaviour 46) after Move…;
    Window ends with Activate Next Window and Activate Previous Window (`NextWindow`,
    `PreviousWindow`, SPEC-0094 v6 behaviour 17) after the project window entries. The other
    actions of the package belong to one part of the window (the Find tool window, Search
    Everywhere, parameter info, a tree, a tool window's tabs, the find bar) and are reached by
    their keys, their toolbar buttons and Find Action. [OSS: titles from
    `ActionsBundle.properties`, groups from `PlatformActions.xml` and `LangActions.xml`]
16. **Seventh keymap package (v14):** three menus gain an action of the seventh package of
    docs/research/KEYMAP_COVERAGE.md: View holds Type Info (`ExpressionTypeInfo`, SPEC-0077 v7
    behaviour 6) after Parameter Info; Navigate holds Custom Folding… (`GotoCustomRegion`,
    SPEC-0083 v9 behaviour 24) after Line:Column…; Run holds Stop Background Processes…
    (`StopBackgroundProcesses`, SPEC-0061 v16 behaviour 62) after Stop. The other actions of
    the package belong to one part of the window and have no menu item: the four actions of
    a list (`List-selectFirstRow`, `List-selectLastRow` and the two that extend the
    selection, SPEC-0275 v19 behaviours 47 and 48) act in the focused list by their keys, and
    Send EOF (`SendEOF`, SPEC-0087 v10 behaviour 18) by its key in the Run tool window. Find
    Action lists Send EOF in both keymaps and the four list actions where the keymap gives
    them a chord, which is the macOS keymap; chosen from Find Action a list action has no
    list to act on and does nothing. [OSS: titles from `ActionsBundle.properties`; the places
    beside Parameter Info, Line:Column… and Stop are IDECraft's, the open platform's groups
    were not read for them]
17. **Eighth keymap package (v15):** Code gains Class Name Completion and Complete Current
    Statement after Code Completion, Unwrap/Remove… after Surround With…, and Inspect Current
    File and Run Inspection by Name… after Inspect Code…; Refactor › Extract/Introduce gains
    Introduce Parameter… after Introduce Field…; Build gains Recompile after Build Project; Run
    gains Quick Evaluate Expression after Evaluate Expression…. The actions of a popup or a tool
    window (the editor tabs, the overloads of parameter info, inserting with a dot, the files of
    a commit, the gutter icon tooltip) have no menu item; Find Action finds them by their
    titles. [OSS: titles from `ActionsBundle.properties`, groups from `PlatformActions.xml` and
    `LangActions.xml`]
18. **Ninth keymap package (v16):** File gains Force Refresh (`ForceRefresh`, SPEC-0059 v9.8
    behaviour 30) after Reload All from Disk; Code gains Reformat File… (`ShowReformatFileDialog`,
    SPEC-0075 v6 behaviour 8) after Reformat Code; Navigate gains Header/Source
    (`SwitchHeaderSource`, SPEC-0071 v12 behaviour 19) after Test; Refactor gains Safe Delete…
    (`SafeDelete`, SPEC-0095 v12 behaviours 15 and 16) after Copy…. Find Action finds the four
    by these titles. [OSS: titles from `ActionsBundle.properties`; Header/Source is IDECraft's
    title, the places beside the named items are IDECraft's]
19. **Tenth keymap package (v17):** Code gains Insert Live Template… (`InsertLiveTemplate`,
    SPEC-0072 v14 behaviour 50) after Class Name Completion; Find Action finds it by this title.
    Next and Previous Template Variable and Iterate Recent Files are keys of a template and of the
    Switcher and get no item. [OSS: title from `ActionsBundle.properties`; the place is IDECraft's]

20. **Items of OBS-0055 (v18):** the menus gain items the reference shows with a shortcut
    (OBS-0055 M3–M5) for actions IDECraft carries out, in the open groups' order, under the open
    texts unless noted [OSS: `PlatformActions.xml` ViewMenu and GoToMenu, `LangActions.xml`
    QuickActions, CodeEditorViewGroup and GoToCodeGroup, `ActionsBundle.properties`]:
    - **View:** Quick Definition (`QuickImplementations`) now stands before Quick Documentation,
      as in the open QuickActions group; Context Info (`EditorContextInfo`, ⌃⇧Q) after Type Info,
      before Error Description; then, each group after a separator, Jump to Source (`EditSource`,
      ⌘↓); Recent Files, Recently Changed Files and Recent Locations (`RecentLocations`, ⇧⌘E);
      Compare With… (`CompareTwoFiles`, ⌘D; the reference's title, the open text is "Compare
      Files") and Compare with Clipboard (`CompareClipboardWithSelection`); Quick Switch Scheme…
      and Compact Mode; Increase Font Size in All Editors and Decrease Font Size in All Editors
      (`EditorIncreaseFontSizeGlobal`, `EditorDecreaseFontSizeGlobal`, ⌃⇧. and ⌃⇧, in the macOS
      keymap). The split and tab actions follow after a separator [ours: the open View menu does
      not hold them]. Jump to Source opens the Project tree's selected file in the editor and gives
      it the focus while the tree has the focus, as ⌘↓ does there, and else gives the active
      editor the focus [ours]; Compare With… compares the two files selected in the tree
      (SPEC-0084); Compare with Clipboard compares the selection or file with the clipboard.
    - **Navigate:** Previous Occurrence and Next Occurrence (`PreviousOccurence`,
      `NextOccurence`, ⌥⌘↑ and ⌥⌘↓) after the Navigate in File submenu, as the open GoToMenu has
      them; OBS-0055 names them as bookmark occurrences, the open texts are used. File Path
      (`ShowFilePath`, ⌥⌘F12) after a separator below Header/Source, before File Structure.
    - **Edit:** Duplicate Line or Selection is titled Duplicate Line, as the reference shows it
      [OBS], and shows no shortcut: ⌘D (Ctrl+D) shows at View › Compare With…, as in the
      reference. The keymap keeps ⌘D on both actions; in the editor ⌘D still duplicates, in the
      Project tree it compares (`keymap_actions`, SPEC-0059). A menu item leaves a chord to the
      item named in `menu_bar::CHORD_SHOWN_ELSEWHERE`; behaviour 3 holds otherwise.
    - **Not added:** Recent Changes (⌥⇧C in the reference): IDECraft has no action for the
      list of recent local-history changes. Reset Font Size in All Editors: left out by SPEC-0129
      v4 (no value to go back to). Show Siblings, Quick Type Definition and the Bidi submenu:
      IDECraft has no such actions. Emoji & Symbols (⌃⌘Space, OBS-0055 M5) is the system's
      panel, not an IDECraft action.
    - **Visible consequences of named deviations:** Navigate › Test shows no shortcut in the
      macOS keymap because ⇧⌘T is Reopen Closed Tab (SPEC-0059, behaviour 8; OBS-0055 M1).
      Navigate › Endpoint… shows no shortcut: the reference's ⇧⌘# (German keyboard) is not in
      the bundled keymap files (OBS-0055 M2). Shortcuts after the German layout (OBS-0055 M6):
      behaviour 3a (v19).

## Acceptance criteria
- **AK-1** Keystroke conversion; every menu action has a handler; v2: data export is in the menu,
  and every menu action is found by Search Everywhere with either platform's keymap
  (`menu_bar::tests`).
- **AK-2** Self-test: the menu shows the keymap's shortcuts; a menu action dispatched through the
  toolkit opens Search Everywhere.
- **AK-3** Checked on screen (2026-11-02): the menu bar shows IDECraft and its menus with shortcuts,
  View → Tool Windows → Terminal opens the terminal, ⌘1 toggles the project window once.
- **AK-4** Hand test (round 4): every menu next to the reference's. Hand test 50 (2026-11-13) did
  not pass: MB1 to MB3, answered by v3; to be repeated.
- **AK-5 (v3)** The menus are File, Edit, View, Navigate, Code, Refactor, Build, Run, Tools, Git,
  Window, Help in this order; Build holds Build Project and Sync Build Changes; Window holds
  Minimize, Zoom, Active Tool Window and Editor Tabs with the entries of behaviour 6, and View no
  longer holds them (`menus::tests`). Find Action titles Sync Build Changes with Build and Minimize
  with Window (`everywhere::tests`). The self-test checks the names and the order of the installed
  menus (`menu_steps`).
- **AK-6 (v4)** The new items are in their menus and every one is carried out and found (`menus::tests::keymap_coverage_actions_are_in_their_menus`, `menu_bar::tests::every_menu_action_is_carried_out_somewhere`, `every_menu_action_is_found_by_search_everywhere`); the part's actions have their titles (`everywhere::tests::part_actions_have_titles_and_groups`).
- **AK-7 (v5)** The four items are in their menus after their neighbours, every one is carried out and found, and Test has no key equivalent in the macOS keymap (`menus::tests::navigation_actions_from_the_keymap_are_in_their_menus`, `menu_bar::tests::every_menu_action_is_carried_out_somewhere`, `every_menu_action_is_found_by_search_everywhere`, `keymap::tests::goto_test_yields_the_chord_of_reopen_closed_tab`).
- **AK-8 (v6)** The actions of behaviour 9 have their titles and groups and a key in the macOS keymap; the default keymap has a key for all but the two usage steps, which Find Action offers without one (`everywhere::tests::second_package_actions_have_titles`).
- **AK-9 (v7)** Edit ends with a separator and the submenu Bookmarks holding the five items of behaviour 10 in that order; no other menu holds a bookmark action; every one is carried out and found; Find Action groups the bookmark actions under Bookmarks (`menus::tests::bookmarks_are_a_submenu_at_the_end_of_edit`, `menu_bar::tests::every_menu_action_is_carried_out_somewhere`, `every_menu_action_is_found_by_search_everywhere`, `everywhere::tests::bookmark_actions_have_titles_and_shortcuts`). In the window, next to the reference's submenu: open (hand test).
- **AK-10 (v8)** In File, Edit, Navigate, Refactor and Window the items the open platform's groups also hold stand in those groups' order under the titles of behaviour 11; the new and moved items run the actions named there, the submenus Find Usages, Navigate in File, Extract/Introduce and Local History hold what behaviour 11 names with its separators, View no longer holds File Structure, and apart from Find Action, AI Chat and Remote Host no action has two menu items (`menus::tests::items_of_the_open_platform_groups_stand_in_their_order`, `items_added_from_the_reference_comparison`). Every item is carried out and found with either keymap (`menu_bar::tests::every_menu_action_is_carried_out_somewhere`, `every_menu_action_is_found_by_search_everywhere`); Find Action titles and groups them as their menus do (`everywhere::tests::reference_comparison_items_have_titles_and_groups`). In the window, and Text… opening the Text tab: open (hand test).
- **AK-11 (v9)** Edit holds the seven items of behaviour 12 at their places and Convert Indents its two; every one is carried out and found with either keymap (`menus::tests::small_editing_commands_are_in_edit`, `menu_bar::tests::every_menu_action_is_carried_out_somewhere`, `every_menu_action_is_found_by_search_everywhere`). In the window next to the reference: open (hand test).
- **AK-12 (v10, behaviour 13)** The three items follow Recent Projects… in its group and
  are carried out and found (`menus::tests::closing_projects_is_a_group_in_file`,
  `menu_bar::tests::every_menu_action_is_carried_out_somewhere`,
  `every_menu_action_is_found_by_search_everywhere`).
- **AK-13 (v12, behaviour 14)** File Properties follows Plugins… in File and holds its items in
  the order of behaviour 14; every one is carried out and found
  (`menus::tests::file_properties_is_a_submenu_of_file`,
  `menu_bar::tests::every_menu_action_is_carried_out_somewhere`,
  `every_menu_action_is_found_by_search_everywhere`,
  `everywhere::tests::file_property_actions_have_titles`). In the window next to the reference:
  open (hand test).
- **AK-14 (v13, behaviour 15)** The five items stand in their menus at the places named and run
  their actions (`menus::tests::sixth_package_items_are_in_their_menus`); Find Action lists
  every action of the package with its title and group and its key in both bundled keymaps
  (`everywhere::tests::sixth_package_actions_have_titles`).
- **AK-15 (v14, behaviour 16)** The three items stand in their menus after the items named,
  and the list actions and Send EOF have no menu item
  (`menus::tests::seventh_package_items_are_in_their_menus`); every menu item is carried out
  and found with either keymap (`menu_bar::tests::every_menu_action_is_carried_out_somewhere`,
  `every_menu_action_is_found_by_search_everywhere`); Find Action titles and groups the
  eight actions, with their keys in both bundled keymaps and none for the list actions in the
  default one (`everywhere::tests::seventh_package_actions_have_titles`). In the window next
  to the reference: open (hand test).
- **AK-16 (v15, behaviour 17)** The eight items stand in their menus at the places named, and the
  others of the package have none (`menus::tests::eighth_package_items_are_in_their_menus`);
  every action of the package has its title and group and its key in both bundled keymaps
  (`everywhere::tests::eighth_package_actions_have_titles`).
- **AK-17 (v16, behaviour 18)** The four items stand in their menus after the items named
  (`menus::tests::ninth_package_items_are_in_their_menus`); every one has its title and group
  and its key in both bundled keymaps (`everywhere::tests::ninth_package_actions_have_titles`)
  and is carried out and found (`menu_bar::tests::every_menu_action_is_carried_out_somewhere`,
  `every_menu_action_is_found_by_search_everywhere`).
- **AK-18 (v17, behaviour 19)** The item stands after Class Name Completion and the three keys have
  no item (`menus::tests::tenth_package_items_are_in_their_menus`); its effect is declared as a
  write, the keys' as none (`security::tests::tenth_package_actions_declare_their_effects`).

- **AK-19 (v18, behaviour 20)** The items stand in View, Navigate and Edit in the order named,
  with the separators (`menus::tests::view_navigate_and_edit_items_of_obs_0055`, and
  `items_of_the_open_platform_groups_stand_in_their_order` for Edit and Navigate); the macOS
  keymap gives the shortcuts named, ⌘D shows at Compare With… and not at Duplicate Line while the
  keymap still binds it to Duplicate (`menu_bar::tests::obs_0055_items_show_their_macos_shortcuts`);
  every item is carried out and found (`menu_bar::tests::every_menu_action_is_carried_out_somewhere`,
  `every_menu_action_is_found_by_search_everywhere`).

- **AK-20 (v19, behaviour 3a)** With the German layout's table, Back and Forward show ⌘Ö and ⌘Ä,
  Compare With… ⌘D and Jump to Source ⌘↓; ⇧⌘[ shows ⇧⌘Ö; a US layout keeps ⌘[; and the event
  of ⌘Ö (⌘Ä) on that layout resolves to Back (Forward) in the macOS keymap
  (`menu_bar::tests::menu_shortcuts_follow_the_keyboard_layout`). The real menu bar on a German
  layout is checked in a window (not part of the tests).

## Not offered (v11, v12)
Functions that have a main-menu item in the open platform's groups or, by OBS-0041, in the reference, and that IDECraft does not have. They get no menu item until an action exists; none is shown disabled.

- **Projects and files:** a dialog for the project's modules, SDKs and libraries; a menu action for a file's type (encoding, line separators and the read-only state are in File › File Properties since v12, SPEC-0336); file templates the user can save; printing and exporting files as HTML.
- **The IDE itself:** repairing the IDE and discarding its caches; exporting, importing and resetting settings; defaults for new projects; a power-saving mode.
- **Editing:** copying a reference of the symbol at the caret, a path with a line number or a path from a source root (the file's absolute path, name and path below the project folder are copied by SPEC-0332 behaviour 13); re-wrapping a paragraph (SPEC-0332 behaviour 14 says why); recording and replaying macros; escaping special characters for XML and HTML; editing delimited text as a table.
- **Navigation:** stepping through Emmet edit points; a popup of the file's parent folders; a hierarchy of included files.
- **Refactoring:** copying a file through a refactoring dialog (files are copied in the project tree, SPEC-0275); migrating packages and classes; refactorings for XML; moving string literals into resource bundles.
- **Windows:** a notifications list; a window for background processes.
- **Tools:** generating API documentation; installing a command line launcher; consoles and packaging helpers for Python, Groovy and Kotlin; scaffolding a plugin project; security and code quality analysis services.

## Out of scope
Hide/Show All in the application menu, enabling and disabling items by context, the in-window main
menu of Windows and Linux. A VCS menu for projects without a repository, Rebuild and a menu item
per build task, switching to the next or previous project window from the keyboard, storing and
restoring a window layout: no specification defines them yet.

## Known deviations from the reference
Menus list only what IDECraft can do; items that do not apply are not greyed out but answer in the
status bar. Exporting result data is in the menu bar, not only in the grid's context menu and a
shortcut (IMP-0013).

v3:
- **Git in every project.** The reference shows VCS in Git's place in a project without a
  repository. IDECraft shows Git there too: no specification defines the entries of a VCS menu.
- **Build** holds two entries. Which entries the reference's Build menu has was not observed beyond
  Build Project (OBS-0024); IDECraft has no compiler of its own and no Rebuild (SPEC-0100).
- **Window** holds only what IDECraft carries out. The split and tab-switching actions (Split
  Right, Unsplit, Next Tab and the others of SPEC-0112) stay in View, where SPEC-0112 behaviour 7
  puts them; where the reference lists them was not observed. Maximize Tool Window keeps this
  caption while a window is maximised; the tool window header's menu then reads Restore Tool
  Window Size.
- The entries of Build and Window, and the shortcuts the menus show, are not compared side by side
  yet.

v8 (arrangement after the open platform's groups):
- **IDECraft's own items and places** are marked [ours] in behaviour 11: Close Tab, Save As…, Paste from History… outside a paste submenu, the three occurrence selections outside Find, Delete Line, Action… in Navigate, Related Symbol… before Test, Spring Bean, Toggle Full Screen in Window, Restore Default Layout outside a layouts submenu, and all of Tools.
- **Titles of IDECraft's own.** New… and Recent Projects… open a popup and a dialog and carry an ellipsis; Show History has none, as in the context menus; Select in Project Tree says what the action does; Restore Default Layout and Remote Development… are the titles of SPEC-0167 and SPEC-0317. Titles do not change with what an action acts on (file or symbol, line or selection).
- **Submenus hold fewer actions** than the open groups: only those IDECraft carries out.
- **Search in Selection Only** is always in Edit and says in the status bar when no search bar is open; the open platform enables its action by context.
- View, Code, Build, Run, Git, Tools and Help are not arranged after the open platform's groups yet; the contents of Edit › Bookmarks (behaviour 10) can be taken from the open bookmarks group in the same way.
- Nothing of this version was compared with the reference on screen.

v9:
- The deviations of the new commands are SPEC-0332's. Nothing of this version was compared with the reference on screen.

v7:
- **Bookmarks** holds five actions chosen from SPEC-0331. Which items the reference's submenu has,
  their captions and their order were not observed (only the submenu's title and place); to be
  compared in the next observation run and corrected here.
