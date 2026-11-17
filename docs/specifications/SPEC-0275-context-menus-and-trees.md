# SPEC-0275: Context menus, new files and shared tree behaviour

- **Version:** 21 · **Date:** 2026-11-15 (v21: `create_file` takes `text` and is a dry run unless `dry_run: false`, behaviour 13, SPEC-0006 v54); 2026-11-15 (v20: a secondary click inside a selection keeps it and the caret, behaviour 14b — OBS-0051 L8); 2026-11-14 (v19.1: outside a tree `+` and `*` are typed, behaviour 32, from the window self-test of 15:15: the terminal lost the `+` of `echo $((40+2))`); 2026-11-14 (v19: the first and the last row of a list — Home and End, and the keymap's `List-selectFirstRow` and `List-selectLastRow` — decided in one place for the lists with keyboard selection, and `List-selectFirstRowExtendSelection` and `List-selectLastRowExtendSelection` in the Project tree, behaviours 47 and 48, AK-42 and AK-43; seventh package of docs/research/KEYMAP_COVERAGE.md; v18: Show Source (`ViewSource`) opens the selection of the Project tree or of the Find tool window without moving the focus, and Copy… (`CopyElement`) copies the selected file under a new name, behaviours 45 and 46, AK-40 and AK-41; sixth package of docs/research/KEYMAP_COVERAGE.md; v17: the New popup opens with its first entry chosen, behaviour 44); 2026-11-14 (v16: the file operations of the project tree can be undone and redone from the tree, with a question first; a deleted item is kept inside the project's `.idecraft` folder for it; the MCP tool `file_operation_undo`; behaviours 37–43, AK-34 to AK-38, OBS-0042; v15: Path From Source Root among the tree's copy entries, behaviour 1, SPEC-0332 v2; v14: a click inside a popup never reaches what lies under it, behaviour 28b, AK-33; window check of 2026-11-14: a click on a row of Quick Switch Scheme also moved the editor's caret); 2026-11-13 (v13: the tree keys are keymap actions, Expand All and Collapse All of the focused tree, File Path, New in the same place and Open in Right Split, behaviours 32–36, docs/research/KEYMAP_COVERAGE.md; v12: a Maven or Gradle module in a sub-folder of a project without a build file of its own has its folder roles and compacted packages, SPEC-0048 v7 behaviour 10a; v11: packages are compacted and new files get their package also in source folders recognised from the files of a project without a build file, OBS-0031 finding T1, behaviours 3, 30 and 31; v10: packages in source folders compacted to one row, the option "Compact Middle Packages" and the MCP tool `project_tree`, OBS-0024 L3, behaviours 30 and 31); 2026-11-06 (v9: a submenu near the window edge moves into the window, behaviour 29; v2: Cut, Copy and Paste of files, behaviours 17–23;
  v3: several selected items in the project tree, the package of a pasted Java or Kotlin file,
  behaviours 24–27; v4: every popup closes on a click outside or when the window loses the focus,
  behaviour 28; v5: the editor's run and debug items name the class and method that run and show the
  shortcuts of the keymap in use, behaviour 14a; v6: the editor menu hides Cut and Copy without a
  selection and offers column selection, folding, analysis, Generate, run configurations, open-in
  and a comparison with the clipboard, behaviour 14b; v7: the keyboard focus is given back after
  popups closed while the window was inactive, OBS-0016 F034-8, behaviour 28a; v8: the Git submenu
  with the file's commit, rollback, history, diff and branch comparison, OBS-0022 L26, behaviour 14c)
- **Sources:** GEN (desktop IDEs open a context menu on a secondary click in the project tree, the
  editor and the database tree; the project tree's menu creates files and folders, renames and
  deletes; trees toggle a node with one click on its disclosure arrow and open leaves with a double
  click; arrow keys walk the tree), STD (macOS Human Interface Guidelines: a Control-click is a
  secondary click; a two-finger click on a trackpad is the secondary button), OSS (action ids and
  shortcuts from the bundled keymaps of `JetBrains/intellij-community`: `NewElement` ⌘N,
  `RenameElement` ⇧F6, `$Delete` ⌦, `CopyPaths`, `EditSource` ⌘↓/F4, `ExpandTreeNode` numpad +,
  `CollapseTreeNode` numpad −, `ShowIntentionActions` ⌥↩, `FindUsages` ⌥F7, `Generate` ⌘N in the
  editor), DOC (jetbrains.com/help: the project view's New menu lists File, Directory and typed
  files; the database explorer's menu has New › Data Source and Query Console, Jump to Query Console,
  Refresh, Properties), hand-test round 4 (no right click anywhere, no way to create a file),
  ADR-0008 (MCP), ADR-0009 (look and feel); v2: GEN (file managers and IDEs cut, copy and paste files
  and folders with ⌘X/⌘C/⌘V; a copy into the same folder gets a free name), OSS (keymap actions
  `$Cut`, `$Copy`, `$Paste`), DOC (jetbrains.com/help: pasting copied files opens a Copy dialog
  with a new name and the target folder; pasting cut files moves them), SPEC-0044 (Local History),
  SPEC-0114 (`move_file`); v3: GEN (file managers and IDE project views select several rows with
  ⇧-click, ⌘-click, ⇧↑/⇧↓ and ⌘A, and copy, move and delete them together), DOC (jetbrains.com/help:
  moving a class to another package with cut and paste runs the Move refactoring, which updates the
  package statement and references), STD (LSP `workspace/willRenameFiles`), SPEC-0114 (Move, F6);
  v4: GEN (desktop popups and menus close when the user clicks anywhere outside them or switches to
  another window or application), hand test round 5 Nr. 8 (the menu of a ▶ run marker stayed open
  after a click outside); v5: OBS-0011 item 5 (the reference names the run and debug items of a Java
  file after the class and its `main` method and shows the macOS bindings beside them), SPEC-0028 v3,
  SPEC-0122 behaviour 1 (⌃⇧R inside a test method runs only that method); v6: OBS-0011 item 5 (the
  reference's editor menu without a selection has no Cut and Copy, and has column selection, folding,
  analysis, Generate, further run kinds, open-in and a comparison with the clipboard), OSS (keymap
  action ids `EditorToggleColumnMode`, `ExpandRegion`, `CollapseRegion`, `ExpandAllRegions`,
  `CollapseAllRegions`, `InspectCode`, `Generate`, `ChooseRunConfiguration`,
  `CompareClipboardWithSelection`), SPEC-0083 (folding), SPEC-0098 (Inspect Code), SPEC-0122
  (Generate); v10: OBS-0024 item 1 and L3 (the reference shows a chain of packages below a source
  folder as one row with dots, with its default settings), GEN (IDE project views end such a row
  where a package has files or several sub-packages, and offer an option to switch it off),
  SPEC-0048 v3 behaviour 10 (which folders are source and test folders), ADR-0008
  v16: OBS-0042 (with the focus in the reference's project tree, undo asks before it takes back
  the last file operation — creating, renaming, moving, deleting a file — and redo before it
  makes it again; a deleted file comes back with its content; the tab of a file that goes
  closes), GEN (file managers undo file operations; a kept copy is restored only where it came
  from), SPEC-0044 (Local History and its folder), SPEC-0057 v8 behaviour 29 (where undo goes),
  SPEC-0053 (agent rights), ADR-0008, CLAUDE.md rule 12 (the more secure option)
- **Feature matrix rows:** F-001 (project tree), F-011 (undo and redo of file operations), F-002 (editor), F-020 (project tree), F-050 (database tree)

## Purpose
The user can right-click (two-finger click, Control-click) the project tree, the editor text and the
database tree and choose what to do there; creates files of the usual kinds, folders, renames and
deletes from the project tree or ⌘N; and every tree reacts to the mouse and the keyboard the same
way. Agents create, rename and delete files over MCP with the same rules.

## Behaviour
1. **Project tree menu:** a secondary click on a row selects it and opens a menu with New › (File,
   Directory, Java Class, Kotlin File, Python File, TypeScript File, JavaScript File, SQL File, HTTP
   Request), Copy Path/Reference › (Absolute Path, Path From Project Root, Path From Source Root, File
   Name; what each copies: SPEC-0332 v2 behaviours 13–13f), Rename…,
   Delete…, Reveal in Finder (Open Containing Folder elsewhere), Open in Terminal, Local History ›
   Show History, Git › (Commit…, Show Git Log, Branches…). Rename and Delete are off for the project
   folder, Show History for folders, Git outside a repository. A secondary click below the rows acts
   on the project folder. [GEN, DOC]
2. **Secondary click:** the right button, a two-finger trackpad click and, on macOS, a Control-click
   open a context menu; every context menu (project tree, editor, database tree, editor tabs, stripe
   buttons) uses the same rule. A click outside the menu closes it; ↑/↓ move, → opens a submenu,
   ← closes it, Enter chooses, Escape closes. Each item shows the shortcut of the keymap in use. [STD,
   GEN]
3. **New:** New › kind asks for a name in a small field near the top of the window; typed kinds add
   their extension unless the name has it; `a/b/c.txt` creates the folders on the way; Directory
   creates folders. A Java class gets `package …;` from its folder below a source root and a class of
   the file's name; a Kotlin file the `package` line. The source root is, in this order (v11): a
   folder `src/main/java`, `src/test/java`, `src/main/kotlin` or `src/test/kotlin` on the way; the
   source folder the files around say (SPEC-0048 v6 behaviour 14: the nearest folder from the new
   file's folder upwards with a Java or Kotlin file of the project's own whose folder ends in its
   declared package); a folder `src` on the way; a source folder derived for the project (the
   same behaviour). So a class created beside `app/code/lab/inventory/Inventory.java` with
   `package lab.inventory;` starts with that package, where before v11 it had none. ⌘N (`NewElement`) in the project tree and
   File › New… open the New menu for the selection; in the editor ⌘N stays `Generate`; without an
   editor ⌘N is New. [GEN, OSS, DOC]
4. **After creating:** the tree shows and selects the new file and the file opens in the editor; a
   new folder is selected. [GEN]
5. **Rename:** Rename… (⇧F6 in the tree) opens the field with the current name, the part before the
   extension selected; Enter renames in place. A file open with unsaved changes is not renamed (the
   field says why); an open file's tab follows the new name. [GEN, OSS]
6. **Delete:** Delete… (⌦ in the tree) asks first, naming the file, or the folder with the number of
   files and folders in it; Enter deletes, Escape keeps. Tabs of deleted files close. Deleted files
   keep a Local History revision (IMP-0275). The question is a native alert (SPEC-0281
   behaviour 6). [GEN, OSS]
7. **Names:** an empty name, `.`/`..`, a leading or trailing space, `\`, `:`, a control character,
   a segment over 255 bytes and, for a Java class, a name that is not a class name are refused with
   the reason; a name that exists is refused — nothing is overwritten. The field stays open with the
   reason until Escape. [GEN]
8. **Arrow:** one click on a node's disclosure arrow selects and toggles it, in every tree: Project,
   Database, the test tree, Build Tools, Call Hierarchy. [GEN]
9. **Row:** one click on a row selects it (and gives the tree the keyboard focus); a double click on
   a leaf opens it (file, table data, task), on a node with children toggles it. A table in the
   database tree opens its data on a double click (SPEC-0190). [GEN]
10. **Keys:** ↑/↓ move the selection, Home/End to the first/last row; → expands a collapsed node or
    moves to the first child of an expanded one; ← collapses an expanded node or moves to the parent;
    Enter opens a leaf and toggles a node; ⌘↓ (Ctrl+↓) and F4 open; numpad + expands and numpad −
    collapses (or goes to the parent). [OSS, GEN]
11. **One behaviour:** the trees share one toolkit-free implementation of behaviours 8–10; each view
    only carries out its commands (select, expand, collapse, open). Flat lists (Problems, Find
    results, Structure, Git log files, Services) keep single click selects, double click opens. [GEN]
12. **Self-test clicks:** the window self-test clicks controls with native mouse events posted to its
    own window (right button with the hardware's button number, Control-click, double click), so the
    toolkit's own translation is part of the test. Controls report their painted bounds only while
    the self-test runs. [GEN]
13. **MCP:** `create_file` (`dir`, `name`, `kind`; since SPEC-0006 v54 also `text`, and `dry_run`
    default true: without `dry_run: false` only a dry run), `rename_file` (`path`, `new_name`) and
    `delete_file` (`path`, `dry_run` default true: counts and the question only) apply behaviours 3,
    5, 6 and 7 with the same code; paths stay inside the workspace. [GEN, ADR-0008]
14. **Editor menu:** a secondary click on the text moves the caret there unless it is inside a
    selection, then opens Show Context Actions, Cut/Copy (off without a selection), Paste, Go To ›
    (Declaration or Usages, Implementation(s), Type Declaration), Find Usages, Refactor › (Rename…,
    Change Signature…, Extract Variable…, Extract Constant…, Extract Method…, Inline…), Reformat
    Code, Run/Debug '<file>' for a file that runs on its own, Local History › Show History, Git ›
    (Commit…, Show Git Log, Branches…). Items run the same actions as the menu bar. [GEN, OSS]
14b. **(v20) The selection stays (OBS-0051 L8):** a secondary click anywhere on a selected range,
    its first and its last character included, keeps every selection and the caret where they are,
    so the menu has Cut and Copy for it; only a click outside every selection moves the caret. A
    Control-click that reaches the editor as a primary click on macOS is a secondary click there too
    and never moves the caret before the menu opens. Until v20 such a click, or one on the edge of
    the selection, removed it and the menu lacked Cut and Copy. [GEN]
14a. **(v5) Run and debug items name what runs:** in a Java or Kotlin file whose top-level class has
    a `main` method, `<Class>.main()`; in a test class, the class; with the caret inside a test
    method, the Run item names `<method>()` because Run starts only that method there, while Debug
    keeps naming the class; any other runnable file keeps its file name. The shortcut beside every
    menu item is the first binding of its action in the keymap in use, written in the system's
    notation (SPEC-0028), so on macOS Run shows ⌃⇧R and Debug ⌃⇧D. [OBS-0011, SPEC-0122]
14b. **(v6) Editor menu contents:** Cut and Copy appear only while text is selected (they are left
    out, not greyed). After Paste comes a switch for column selection. Beside Go To and Find Usages
    there are a folding submenu (expand and collapse at the caret, expand all, collapse all), an
    analysis submenu (Inspect Code…, which inspects the project as the menu bar does, and opening
    the Problems tool window), Refactor and Generate…; after Run/Debug a submenu with the run-configuration chooser (⌃⌥R); then an open-in
    submenu (the file manager with the file selected, a terminal in the file's folder), Local
    History, Compare with Clipboard and Git. Compare with Clipboard opens the diff view with the
    clipboard text on the left and the selection (the whole file without one) on the right; with
    an empty clipboard it only says so in the status bar. Items whose action IDECraft cannot carry
    out for the file (Generate outside Java and Kotlin) say why in the status bar. The menu does not
    list kinds of runs or diagrams IDECraft lacks. [OBS-0011, OSS, GEN]
14c. **(v8) Git submenu of the editor and the project tree's file:** in this order: Annotate with
    Git Blame (Close Annotations while shown), then Commit File… (opens the Commit tool
    window with only this file ticked), Rollback… (asks whether to roll the file back to HEAD),
    Show History (the Git log filtered to this file,
    SPEC-0086 v5 behaviour 10), Show Diff (`Compare.SameVersion`: the diff of SPEC-0084 for the
    file), Compare with Branch… (SPEC-0084 v2 behaviour 11), then Commit…, Show Git Log and
    Branches… as before. The file items are greyed outside a repository; for a file Git does not
    track they say so in the status bar (Commit File… still opens the Commit tool window, where the
    file is unversioned). From the project tree they open the file first. Before v8 the submenu had
    only Annotate, Commit…, Show Git Log and Branches… [OBS-0022 item 9, L26]
15. **Database menu:** a secondary click on a row selects it (on the empty area: keeps the
    selection) and opens New › (Data Source…, Query Console), Jump to Query Console…, Refresh,
    Properties (Edit Data on a table), Copy (the name), Rename… (tables and columns), Delete… (a
    connection, asking as ⌦ does), Diagrams › Show Diagram. Items that need a selection are off
    without one. [DOC, GEN]
16. **Root cause recorded (hand-test round 4):** GPUI 0.2 on macOS already delivers a right click, a
    two-finger click and a Control-click as the secondary button (verified with native events); the
    project tree, the editor text and the database tree had no handler, so a right click there did
    nothing. GPUI takes the button from the event's button number, not its type: a right-button
    event with button number 0 (as synthesised by some tools) arrives as a primary click. [GEN]
17. **Cut, Copy, Paste:** the project tree's menu has Cut (`$Cut`), Copy (`$Copy`) and Paste
    (`$Paste`) with the keymap's shortcuts; Cut and Copy are off for the project folder, Paste is off
    while nothing is on the file clipboard. ⌘X, ⌘C and ⌘V act on the tree's selection while the tree
    has the focus, also when the menu bar takes the key first. [GEN, OSS]
18. **File clipboard:** Cut and Copy remember the selection and put its absolute path as text on
    the system clipboard. Paste pastes files only while the system clipboard still holds that text;
    once other text was copied, Paste in the tree pastes nothing and says so. [GEN]
19. **Target:** Paste goes into the selected folder, or into the folder of the selected file. A
    copy into a folder that has the name already gets a free name (`a_copy.txt`, `a_copy2.txt`,
    `src_copy`); nothing is ever overwritten (IMP-0279). [GEN, DOC]
20. **Name field:** Paste of one item opens the small name field titled "Copy a.txt to src" or
    "Move a.txt to src" with the free name proposed and the part before the extension selected;
    Enter pastes, Escape cancels. A folder into itself or below itself and the project folder are
    refused with the reason in the status bar; an existing or invalid name typed by hand is refused
    in the field, which stays open; a file open with unsaved changes is not moved. [GEN]
21. **Open tabs:** tabs of a moved file, or of files below a moved folder, follow to the new path.
    [GEN]
22. **Result:** the tree shows and selects the pasted item and keeps the focus. Folders copy with
    everything in them, links stay links. The Local History records the old paths before a move,
    then the new files, under a label "Moved … to …" / "Copied … to …" (IMP-0280); after a move the
    file clipboard is empty. [GEN]
23. **MCP:** `copy_file` (`path`, `target_dir`, `new_name`, `dry_run` default true) and `move_file`
    for folders or with `update_references: false` use the same code as Paste: never overwrite (the
    error names a free name), refuse a folder into itself, record the Local History; the dry run
    reports the target, the counts and a free name. [GEN, ADR-0008]
24. **Several selected items (v3):** in the project tree a click with ⇧ selects the rows from the
    anchor (the row clicked or reached last without ⇧) to the clicked row; a click with ⌘ (Ctrl
    elsewhere) adds the row to the selection or removes it and makes it the anchor; ⇧↑/⇧↓ extend
    the selection from the anchor one row at a time (and shrink it when going back); ⌘A (Ctrl+A
    elsewhere) selects every visible row. A click or an arrow key without these modifiers selects
    one row again. Every selected row is highlighted; the row last clicked or reached is the lead.
    Rows hidden by collapsing a folder leave the selection. A secondary click on a selected row keeps
    the selection; on another row it selects only that row. The rules are part of the shared
    toolkit-free tree behaviour (behaviour 11); the other trees keep one selection. [GEN]
25. **Operations on all selected items (v3):** Cut and Copy put every selected item on the file
    clipboard (the system clipboard text holds their absolute paths, one per line). Paste of several
    items opens no name field: each keeps its name, copies that meet a name get a free one, a move
    onto an existing name is refused with the item named. Every item is checked before anything is
    written; if one cannot be pasted, nothing is (IMP-0281). Afterwards the tree selects every
    pasted item. Delete asks once, naming how many items and how many files and folders go in all
    ("Delete 3 items: 5 files and 2 folders?"); every file goes to the Local History; tabs of deleted
    files close. Copy Path/Reference copies one line per item. Items inside a selected folder count
    once, with the folder. Rename stays for one item: it is off in the menu with several selected and
    ⇧F6 says to select one. New, Reveal, Open in Terminal, Show History and Annotate act on the row
    the menu was opened on. [GEN]
26. **Package of a pasted Java or Kotlin file (v3):** pasting a cut `.java` or `.kt` file (one item
    or several) into another folder with its name kept moves it as Move (F6, SPEC-0114) does: in the
    background, with the language server's edits for the move (for Java the package statement,
    imports and qualified references in other files) applied together with the move; open files
    change through their editors; the Local History records it as behaviour 22 says. Wherever the
    server's edits leave the moved file's package line as it was, IDECraft rewrites the line to the
    package of the new folder (as in behaviour 3: replaced, added before the first line of code, or
    removed in a source root's top folder) — but only when the line matched the package of the old
    folder, so a source layout IDECraft does not recognise is never "corrected". Kotlin servers in use offer no edits for a move, so for Kotlin only the
    package line follows and references in other files are not updated: rewriting imports by name
    without the server's knowledge of the symbols could change unrelated code. A Java or Kotlin file
    pasted under another name, and a copied one, get the package line of their folder too (no
    references are updated). The package-line rule applies to F6 and to MCP `move_file` as well,
    and to `copy_file` and the plain `move_file`, which work as Paste does. A folder of classes
    moves plainly (behaviour 22). [DOC, STD, GEN]
27. **MCP with several paths (v3):** `copy_file`, `move_file` and `delete_file` take `paths` (a
    list) instead of `path`. All items are checked before anything is written; if one fails, the
    error names it and nothing changes. `new_name` needs a single path. The answer lists the result
    per item; `delete_file`'s question names the count as behaviour 25 does. `move_file` with
    reference updates moves the files one after the other, each planned after the previous one was
    written (a dry run plans each against the current files). [ADR-0008]
28. **Popups close (v4):** every popup that is not a dialog closes without a choice, as Escape
    would, when the mouse is pressed anywhere outside it, when the window stops being the active
    window, and — for the menu of a ▶ run marker and the other short choice popups — when the
    keyboard focus moves out of it. These are: the menu of a ▶ marker in the gutter (Run, Debug),
    the other choice popups (Python interpreter, comparison target), Run… (⌃⌥R), Go to Line,
    Recent Files, the Rename name popup, Search Everywhere, File Structure, Show Usages, the branch,
    project, VCS operations, stash and theme popups, context actions (⌥↩) and quick documentation.
    A click outside goes on to what it hit (the editor places its caret, a tab opens), unlike the
    context menu of behaviour 2, whose click goes no further. When the click hit nothing that takes
    the focus, the editor gets it back. Dialogs (Settings, connection, commit, push, rename preview,
    …) stay open. One mechanism serves all of them, so a new popup gets this by being registered
    there. [GEN]
28a. **Focus after an inactive close (v7):** popups closed because the window stopped being
    active took the keyboard focus with them; when the window becomes active again and no element
    holds the focus, the active editor (or the project tree) gets it, so keys work without a click.
    The same holds when a popup asked for in the background (the chooser of ⌥⌘B) opened while the
    window was inactive and was closed by it at once. Plausible cause
    of OBS-0016 F034-8 (the window took no keys after ⌥⌘B while Computer Use and the accessibility
    interface acted on it), found by reading the code, not reproduced. [GEN, OBS-0016]
28b. **Clicks inside a popup stay there (v14):** a press or release of a mouse button and the
    wheel over a popup of behaviour 28 act on the popup only and never reach what lies under it:
    a click on a row chooses the row and does nothing else — the editor under it keeps its caret,
    selection and scroll position — and a click on the popup beside its rows does nothing. The
    mechanism of behaviour 28 sees to this for every registered popup, so a popup's own view need
    not; the short choice popup does it itself as well, because the SQL editor shows it without
    that mechanism. Before v14 the popups whose views did not see to it themselves let the press
    through: Quick Switch Scheme, the recent projects, and every short choice popup (the menu of
    a ▶ run marker and the other Run, Debug and Generate choices, the Python interpreter, the
    comparison target, and the SQL editor's run choice, confirmation and data source). [GEN,
    window check 2026-11-14]
29. **v9 – Submenus stay in the window:** a submenu that would end below the window's bottom (or past its right edge) is moved up (or to the left of its entry) so all its entries are shown, 4 px from the edge. Before v9 the editor's Git submenu, opened near the bottom, showed two of its entries. [OBS-0022 Nachprüfung im Fenster 2026-11-06, L26]

30. **Compacted packages (v10):** inside a source or test folder of the project model (SPEC-0048 v3
    behaviour 10: `src/main/…` and `src/test/…` with `java`, `kotlin`, `groovy` or `scala` of a
    Maven or Gradle module, and a module's own `sourceDirectory` / `testSourceDirectory`; v11: also
    a source or test folder recognised from the files where no build file declares one, SPEC-0048
    v6 behaviour 14 — next to the reference a project without a build file showed `empty` › `a` ›
    `b` and `util` › `deep` folder by folder where the reference showed `empty.a.b` and
    `util.deep`, OBS-0031 finding T1) a package
    — a folder below such a folder — that holds nothing but one sub-folder shares its row with that
    sub-folder. The row is named with dots (`lab.shop.core`) and stands one level below the source
    folder. The reference showed `lab.shop.core` as one row below the source folder where IDECraft
    showed three nested folders. [OBS-0024 item 1, L3]
    - **Where a row ends:** at a package that holds a file, more than one sub-folder, or nothing
      (an empty innermost package is part of the row). So `lab/shop/core` and `lab/shop/app` give
      the row `lab.shop` with `app` and `core` below it. [GEN: how IDE project views compact
      packages; OBS-0024 recorded only the single chain, the split is checked in the next
      comparison]
    - **What is never compacted:** the source folder itself, any folder with a role of its own (a
      module, a source, test or resource folder, excluded output), folders above or outside source
      folders, folders in resource folders, and projects without any source folder, declared or
      derived (among them remote projects). OBS-0024 records no compacted rows there, so they stay
      folders. [OBS-0024 item 1]
    - **Derived source folders (v11):** a derived folder has the icon and, for tests, the tint of
      a declared one, and everything in this behaviour holds below it. The window learns of them
      shortly after it opens and when they change (SPEC-0048 v6 behaviour 14); rows then join or
      split in place, the expanded folders and the selection stay, and a selection on a folder
      that is now inside a row moves to the row. [OBS-0031 T1]
    - **The row is its innermost folder:** its path is that folder's. Expanding shows that
      folder's content; the name is complete as soon as the row appears, before it is expanded.
      New creates in the innermost folder. Rename… opens the name field with the innermost
      folder's name and renames that folder only — never several folders at once. Delete… asks
      with the innermost folder's name and its counts and deletes that folder only; the packages
      above it stay, as a shorter row. Cut, Copy and Paste take the innermost folder; Paste goes
      into it. In a selection of several rows it counts once, with that path. [GEN; decision of
      this specification, the reference's Rename and Delete on such a row were not observed]
    - **Keys and mouse** (behaviours 8–10) treat it as one row: → expands it and then goes to its
      first child, ← collapses it and then goes to the source folder above. "Select Opened File"
      shows a file below it; showing a folder in the middle of a row selects the row. A file with
      an error below it underlines the row (SPEC-0059 v4 behaviour 10). [GEN]
    - **The rows follow the disk:** when the tree is read again after New, Paste, Delete or a change
      from outside, a file created in a package in the middle splits the row there and both parts
      stay expanded as far as the row was; a first sub-folder in an empty innermost package
      lengthens the row. [GEN]
    - **Option:** the Project tool window's ⋮ menu starts with "Compact Middle Packages", ticked
      while on; on by default, as in the reference (its default settings showed the compacted
      row). It is kept in the IDE settings as `project_view.compact_middle_packages` and applies to
      every window at once. Switching never hides a row: switching it off shows the folders of
      every visible compacted row expanded down to the innermost one, switching it on moves a
      selection on a folder inside a row to the row. Both views of the chooser (Project, Project
      Files) follow it. [OBS-0024 L3 for the default; GEN]
31. **MCP for the tree (v10):** `project_tree`, served by the window: `list` (default) returns
    `compact_middle_packages`, the `view` and the visible `rows` in display order, each with
    `name`, `depth`, `kind`, `selected`, for files and folders `path` relative to the workspace,
    for folders `expanded`, the `role` where the project model gives one or the folder is a derived
    source or test folder (v11; `sources`, `tests`), and `folders` — every
    folder the row stands for, outermost first, several for a compacted row. `expand`, `collapse`
    and `select` take `path`; any folder of a compacted row means that row. `compact` with `on`
    switches the option at once and saves it. `set_ide_settings` takes `compact_middle_packages`
    and `ide_settings` shows it under `settings.project_view`; like every IDE setting written to
    the file, open windows read it at their next start. [ADR-0008]
32. **v13 – The tree keys are keymap actions:** expanding and collapsing with the numeric keypad
    (behaviour 10) are the actions `ExpandTreeNode` (numpad +) and `CollapseTreeNode` (numpad −) of
    the bundled keymaps, and `FullyExpandTreeNode` (numpad \*) joins them. In the trees that share
    the keys of behaviour 10 (Project, Database, the test tree, Build Tools) every key the keymap
    in use binds to one of them does that, so a shortcut changed in Settings › Keymap works and a
    key taken away there no longer does it. The toolkit does not tell the keypad from the main
    block, so `+`, `-` and `*` typed there count as the keypad's keys, as `+` and `-` did before.
    `FullyExpandTreeNode` expands the selected node and every node below it, at most 200 nodes;
    in the Project tree it leaves out the folders Expand All leaves out (SPEC-0061 v6 behaviour
    26); a node whose children are read only when it opens (Database, the folders of a remote
    project) opens one level further with each use. On a leaf it does nothing. The other keys of
    behaviour 10 stay fixed. Chosen in Find Action the three act on the selected row of the Project
    tree. With their keys they act only while one of these trees has the focus (v19.1);
    elsewhere — the terminal, the editor, a field — no action takes the keys and they type
    their character. MCP: `project_tree` has `expand_all` (with `path`: that folder and every folder below
    it, as the key; without: as the header button) and `collapse_all` (as the header button).
    [OSS: ids and shortcuts of the bundled keymaps; GEN; ADR-0008]
33. **v13 – Expand All and Collapse All by action:** `ExpandAll` (⌘+ and ⌘= on macOS, Ctrl++ and
    Ctrl+= elsewhere) and `CollapseAll` (⌘−, Ctrl+−) are the Expand All and Collapse All buttons of
    the tree that has the keyboard focus: Project (SPEC-0061 v6 behaviour 26), Commit (SPEC-0063),
    the Find tool window (SPEC-0293), the test tree of the Run tool window (SPEC-0088) and Build
    Tools (SPEC-0100 behaviour 7); the Database tree has Collapse All only. With their keys they
    run only while one of these trees has the focus; in the editor the same keys stay
    `ExpandRegion` and `CollapseRegion` (SPEC-0083), also where the menu bar takes the keys for
    its folding items: with a tree in focus the tree gets them. Chosen in Find Action without such
    a focus they act on the Project tree. They are view state and get no MCP tool of their own
    (`project_tree` has both for the Project tree, behaviour 32). [OSS: ids and shortcuts of the
    bundled keymaps; GEN]
34. **v13 – File Path:** `ShowFilePath` (⌥⌘F12 on macOS, Ctrl+Alt+F12 elsewhere; Find Action lists
    it as "File Path") does Reveal in Finder (Open Containing Folder elsewhere) of behaviour 2 for
    the file of the active editor, or, while the Project tree has the focus, for its selected item.
    Without either the status line says that there is no file to show. A file of a remote project
    is answered as the menu item answers it. It starts the system's file manager, so for UI
    automation it is an action that starts a program: an external client needs the right `run` to
    press its key (SPEC-0319 behaviour 35). [OSS: id and shortcut of the bundled keymaps; GEN;
    CLAUDE.md rule 12]
35. **v13 – New in the same place:** `NewElementSamePlace` (⌃⌥N on macOS, Ctrl+Alt+Insert
    elsewhere; "New in This Directory…" in Find Action) opens the New menu of behaviour 3 for the
    folder of the active editor's file, from the editor and without selecting anything in the
    tree; what is created behaves as in behaviour 4. Without an open file it is `NewElement` (the
    tree's selection). [OSS: id and shortcut of the bundled keymaps; GEN]
36. **v13 – Open in the right split from the tree:** `OpenInRightSplit` (⇧↩) on a file selected in
    the Project tree opens it as Enter does and puts it into the right split, as ⇧↩ does in Search
    Everywhere (SPEC-0067 behaviour 11, v8 behaviour 19); on a folder it opens nothing and the
    status line says that a file has to be selected. Its key acts only while the Project tree has
    the focus; chosen in Find Action it acts on the tree's selected item in the same way. [OSS: id
    and shortcut of the bundled keymaps; GEN]

37. **v16 – The history of file operations:** every project window keeps the file operations
    made in its Project tree, newest last: New (a file of any kind, a folder, also several
    folders on the way of `a/b/c.txt`, behaviours 3–4), Rename (behaviour 5), Paste of copied
    items (behaviour 19), Paste of cut items (a move, behaviours 20–21, with the package line
    of behaviour 26) and Delete (behaviours 6 and 25). An operation on several selected items
    is one step. A new operation drops the steps that were undone and not redone. The history
    holds the last 100 steps, lives in memory and ends with the project window.
    Not in the history: a cut Java or Kotlin file that moves as the Move refactoring (behaviour
    26; its text edits are steps of the editors, SPEC-0193), operations in a remote project
    (SPEC-0317), operations made by the agents' file tools (`create_file`, `rename_file`,
    `move_file`, `copy_file`, `delete_file`: they do not go through the window) and anything
    done outside IDECraft. [OBS-0042, GEN]
38. **v16 – Undo and redo from the tree:** while the Project tree has the keyboard focus,
    `$Undo` (⌘Z) and `$Redo` (⇧⌘Z) — by key or from the Edit menu — mean the
    newest step of this history that is not undone, and the step undone last (SPEC-0057 v8
    behaviour 29). Nothing happens before the user has answered a question that names the step
    and the paths it touches, relative to the project folder, and says what will happen to them
    ("Undo New File "notes/a.txt"? The file is taken out of the project and its tab closes.").
    Its buttons are the action (Undo or Redo; Return) and Cancel (Escape); Cancel changes
    nothing and the step stays where it is. While the question is shown no other key reaches
    the window. [OBS-0042; the wording is IDECraft's own]
39. **v16 – What undo and redo do:**
    - **New:** undo takes the created file or folder out of the project and removes the folders
      the same step created on its way, if they are empty; redo puts it back with the content it
      had. A created folder is only taken back while it is empty.
    - **Rename, move:** undo gives the item its old name or place back, redo the new one. Where
      a move changed the package line of the file (behaviour 26), undo puts the old line back
      and redo the new one.
    - **Copy:** undo takes the copy out of the project, redo puts it back.
    - **Delete:** undo puts the item back where it was — a file with its bytes, a folder with
      everything in it — and redo deletes it again.
    Afterwards the tree is read again and selects what came back or moved; tabs of a file that
    went are closed, tabs of a renamed or moved file follow it (as in behaviours 5 and 21);
    bookmarks follow as they do for the operation itself (SPEC-0331 behaviour 9). A file that
    came back is not opened by itself. The status bar says what was done. [OBS-0042, GEN]
40. **v16 – Nothing is overwritten and nothing changed is lost:** before anything is touched,
    every item of the step is checked; if one check fails the whole step is refused, nothing
    on disk changes, the step stays in the history and the status bar (for an agent: the
    error) names the item and the reason.
    - **Changed since:** the item must be what the operation left: the same entries with the
      same names and kinds, every file with the same bytes, every link with the same target.
      (For an item of more than 64 MiB the sizes and modification times of its files are
      compared instead of their bytes.) A file edited and saved in IDECraft, or written by
      another program, since the operation is therefore never taken out of the project, moved
      back or deleted again by undo or redo.
    - **Unsaved changes:** an open editor of an item of the step with unsaved changes refuses
      the step, as it refuses Rename (behaviour 5).
    - **Occupied:** the place an item goes back to must be free; nothing existing is ever
      overwritten or merged.
    - **Gone:** an item that is no longer where the operation left it refuses the step.
    A step that was refused can be tried again after its reason is gone. [GEN; CLAUDE.md
    rule 12]
41. **v16 – Where a deleted item is kept:** Delete from the tree moves the item into the undo
    store of the project, `.idecraft/history/undo/` inside the project folder (machine-local
    state beside the Local History, ignored by Git through `.idecraft/.gitignore`, SPEC-0069
    behaviour 3), instead of removing it; the same store takes what undo of New and of a copy
    takes out of the project. Every file is recorded in the Local History before, as before v16
    (IMP-0275), so `restore_local_revision` still brings a text file back when the store no
    longer has it.
    - **Only from the store, only into the project:** undo restores an item from this store and
      from nowhere else — never from the system's trash, a temporary folder or a path given by
      anyone — and only to the path inside the project it was deleted from. The store's folders
      must be real folders inside the project folder; if `.idecraft`, `history` or `undo` is a
      link or leads out of the project, nothing is kept there and nothing is restored from
      there. Links inside a kept item are kept as links and never followed.
    - **Checked:** a kept item is restored only if it is still what was put into the store
      (the check of behaviour 40); a copy that was changed or removed in the store refuses the
      step.
    - **Size limit:** an item of more than 64 MiB (the sum of its files) is not kept. Delete
      then removes it as before v16 and the status bar says so, naming the limit: the deletion
      cannot be undone from the tree (text files up to 2 MiB each are still in the Local
      History). The same holds when the store cannot be used.
    - **Bounded:** what belongs to a step is removed from the store when the step leaves the
      history (the 100-step limit, a new operation after an undo, the end of the project
      window). When a project window opens, what an earlier session left in the store is
      removed. A second IDECraft process on the same project folder thereby empties the first
      one's store; the first one's undo of a delete is then refused (the item is gone from the
      store), never wrong.
    [GEN; SPEC-0044; CLAUDE.md rule 12]
42. **v16 – The Edit menu names the step:** while the Project tree has the focus, the Undo and
    Redo items of the Edit menu carry the step they would take ("Undo New File a.txt", "Redo
    Delete report.sql", "Undo Delete 3 items"); with no step they are plain "Undo" and "Redo",
    as with the focus anywhere else. [GEN]
43. **v16 – Agents:** the MCP tool `file_operation_undo` works on the history of the project
    window of the client's workspace. `list` (default) returns `undo` and `redo`, each the
    steps the next call would take first, with `label`, `kind` (`create`, `rename`, `move`,
    `copy`, `delete`) and `paths` relative to the project folder; it needs no right. `undo` and
    `redo` take the next step without the question of behaviour 38 — the user-granted `write`
    right is the consent — under every check of behaviours 40 and 41; a refusal is an error
    with the reason and changes nothing. Both are logged in `.idecraft/agent-log.jsonl` (time,
    tool, action, whether it was done, and the status line or the reason). The reply is the
    history after the call and `done`, the status line. The tool takes no path and no content:
    an agent can only take back or repeat what was done in the tree. It is served by the window
    over HTTP; a server without a window says so. [ADR-0008, SPEC-0053]
44. **v17 – New popup with a choice (window check 2026-11-14):** the New popup of the Project tree
    (⌘N) opens with its first entry chosen, so Enter creates it; the other context menus open with
    none until ↑ or ↓. [GEN]
45. **v18 – Show Source:** `ViewSource` (⌘↩ in the bundled macOS keymap, Ctrl+Enter in the
    default one) opens the file selected in the Project tree in the editor, as Jump to Source
    (`EditSource`) does, but the focus stays in the tree, so the next arrow key moves on in it.
    In the Find tool window it opens the selected usage, or the first usage of the selected
    group, and the window keeps the focus. On a folder, the searched symbol or the group row
    the status bar says that there is nothing to show. Elsewhere, and in the editor, the action
    does nothing and its key goes on to the next action. [OSS: id, title, keys, and "like Jump
    to Source without taking the focus, not offered in an editor" from `ViewSourceAction.java`,
    logic template]
46. **v18 – Copy…:** `CopyElement` (F5) opens the name field of Paste (behaviour 19) for a
    copy of the one item selected in the Project tree — or, from the editor, of the active
    file — in the item's own folder, with a free name proposed and its stem selected; Enter
    copies, an existing name is never overwritten, the copy is shown and selected in the tree
    and the step can be undone (behaviour 37). With several items selected, on the project
    folder and in a remote project the status bar says that one file or folder has to be
    selected. Refactor › Copy… runs it (SPEC-0124 v13 behaviour 15). [OSS: id, title and key;
    GEN]
47. **v19 – First and last row of a list:** a list that has the keyboard selects its first
    row on Home and its last row on End, and on the chords the keymap in use binds to
    `List-selectFirstRow` and `List-selectLastRow` (⌥↑ and ⌥↓ in the bundled macOS keymap; the
    default keymap binds none, there Home and End are the keys). The rule is decided in one
    place (`list_nav`) and every list below asks it, so a rebound chord reaches all of them.
    - **Rows.** The first and the last row that can be selected: a heading, a separator or an
      action that is switched off is passed over. Nothing wraps, an empty list takes no key,
      and the list scrolls to the row where it scrolls to its selection at all.
    - **Beside a text field.** In a popup whose text field has the keyboard, Home and End
      move the caret in the field while it holds text, and go to the list while it is empty;
      the chords of the keymap always go to the list. Find in Files, whose fields always have
      the keyboard, takes the chords only.
    - **Where ⌥↑ and ⌥↓ move the row.** The commit list of the interactive rebase keeps ⌥↑ and
      ⌥↓ for moving the selected commit (SPEC-0145) and takes Home and End only.
    - **The lists.** Popups and choosers: the list popup of named choices (New Scratch File,
      Go to Mnemonic, the list of a tool window's tabs, Custom Folding, Stop Background
      Processes, the targets of a comparison, the members to override, …), Attach to Process,
      the list of all bookmarks, the branch popup, Find in Files, Recent Files with its tool
      window column and Recent Locations, the context actions, the project popup, the rename
      preview, the list of the run widget, the stash popup, File Structure, the list of a
      group's tabs and the Switcher (⌃, held while the Switcher is used, does not count), the
      theme popup, Show Usages, the VCS operations popup, the commits of the Push dialog, the
      scope dialog of Inspect Code and the popup of the highlighting level. Tool windows:
      Bookmarks, Containers, Endpoints, Find, the commits of the Git log and the changed files
      of the selected commit, Problems and the results of Inspect Code, the Profiler (process
      list, call tree, hot spots), the installed packages of Python Packages, the hierarchies,
      Local History, the interactive rebase, Spring and TODO.
    - **Not among them.** The trees that follow behaviour 10 already have Home and End
      (Project, Database, Build Tools, the test tree). The completion list and the
      completions of Evaluate leave Home, End, ⌥↑ and ⌥↓ to the editor, whose caret and
      selection they move. Search Everywhere and the path list of Remote Development leave
      them to their text field. The Settings dialog, the grid of query results, the branch
      tree and the filter menus of the Git log, the picker of the connection dialog, menus,
      and the lists of suggestions under a name field keep their own keys. The list of the
      run configuration editor is left as it is: an open change rewrites that dialog, and ⌥↑
      and ⌥↓ move a configuration there.
    [OSS: the four ids, their titles ("Select First Row", "Select Last Row", "Extend Selection
    to First Row", "Extend Selection to Last Row") and the chords of the macOS keymap from
    the bundled keymap files and `ActionsBundle.properties`; GEN: Home and End in a list. In
    which of its lists the reference takes the chords was not observed.]
48. **v19 – Extending the selection to the first and the last row:**
    `List-selectFirstRowExtendSelection` and `List-selectLastRowExtendSelection` (⌥⇧↑ and ⌥⇧↓ in
    the bundled macOS keymap), and ⇧Home and ⇧End, act where several rows can be selected:
    in the Project tree (behaviour 24). The selection then reaches from its anchor — the row
    ⇧-ranges start from — to the first or the last visible row, that row becomes the lead,
    and the anchor stays, so the other end can be chosen right after and replaces the first
    range. In the Project tree the two actions without "extend" select the first and the
    last row alone, as Home and End do there. In a list with one selected row there is
    nothing to extend: the selection that reaches to the first row is the first row, so the
    two extending actions and ⇧Home and ⇧End select the first and the last row there, with
    the limits of behaviour 47 (beside a text field with text, ⇧Home and ⇧End select text in
    the field). [OSS: ids and chords; GEN: a range from the anchor, and one row where a list
    holds one selection. What the reference's lists do with these chords was not observed.]

## Acceptance criteria
- **AK-1** Given the menu models, when built for a file, a folder, the project folder, the editor
  with and without selection and a runnable file, and every kind of database row, then the items and
  their enabled state are as in behaviours 1, 14 and 15. (`context_menu::tests`)
- **AK-2** Given a Control-click on macOS, then it is a secondary click; not on other systems; the
  right button always is. (`context_menu::tests::control_click_is_secondary_on_macos_only`)
- **AK-3** Given a folder, when files of each kind, nested files and folders are created, then the
  extension is added once, folders on the way exist, a Java class has its package and class, and an
  existing name, an invalid name, a missing folder and a path outside the workspace are refused.
  (`file_ops::tests`)
- **AK-4** Given a file and a folder, when renamed and deleted, then a collision and the project
  folder are refused, Delete counts what goes, and the deleted file has a Local History revision.
  (`file_ops::tests`)
- **AK-5** Given any tree, when the arrow is clicked once, a row clicked once or twice, or the keys of
  behaviour 10 pressed, then the commands are those of behaviours 8–10. (`tree_nav::tests`,
  `project_tree::tests::shared_tree_behaviour_and_reveal`)
- **AK-6** Given the window, when real right clicks and Control-clicks reach a tab, a stripe button,
  a project tree row, the editor text and a database row, then each opens its menu; New › SQL File
  with a name creates and opens the file, the same name again is refused, Rename and Delete work, ⌘N
  opens New, the arrow toggles, a click selects and a double click opens. (`idecraft ui --selftest`:
  `mouse_steps`)
- **AK-7** Given MCP, when `create_file`, `rename_file` and `delete_file` run, then they behave as
  behaviours 3, 5, 6 and 7. (`crates/mcp/tests/it/protocol.rs`: `creates_renames_and_deletes_files`)
- **AK-8** Given the hand test (round 5, Nr. 193), then the menus, New, Rename, Delete and the trees
  behave as above on the project owner's Mac with mouse and trackpad.
- **AK-9** Given a file and a folder with nested files and a link, when copied and moved into another
  folder and into their own folder, then the copy is recursive, the link stays a link, a clash
  proposes a free name and an existing name is refused, a folder into itself and the project
  folder are refused, and Local History has the old paths and the label.
  (`file_ops::tests`: `copies_files_and_folders_recursively`, `moves_and_records_local_history`,
  `refuses_into_itself_root_and_clashes`, `free_names_never_overwrite`)
- **AK-10** Given the project tree menu, then Cut, Copy and Paste are there with `$Cut`, `$Copy`,
  `$Paste`, off as behaviour 17 says; given the file clipboard, then it is stale once the system
  clipboard text differs, Paste targets the folder of behaviour 19 and open tabs map to the moved
  paths. (`context_menu::tests::project_tree_menu_has_cut_copy_paste`, `file_clipboard::tests`)
- **AK-11** Given MCP, when `copy_file` and `move_file` (folder, `update_references: false`) run with
  and without `dry_run`, then they behave as behaviour 23. (`crates/mcp/tests/it/protocol.rs`:
  `copies_and_moves_files_and_folders`)
- **AK-12** Given the hand test (round 5), then ⌘X/⌘C/⌘V and the menu copy and move files and
  folders on the project owner's Mac, open tabs follow and a folder into itself is refused.
- **AK-13** Given rows of a tree and a selection, when ⇧-click, ⌘-click, ⇧↑/⇧↓, ⌘A, a plain click
  and a plain arrow happen, then the selected rows, the anchor and the lead are as behaviour 24
  says; the project tree keeps them by path across a reload and drops rows hidden by collapsing.
  (`tree_nav::tests::several_rows_are_selected_with_shift_and_command`,
  `project_tree::tests::several_selected_rows_survive_a_reload`)
- **AK-14** Given several selected paths, when cut or copied, then the file clipboard holds all of
  them with one path per line; nested items count once; the delete question names the count; the
  menu has Rename off with several selected. (`file_clipboard::tests::several_items_on_the_clipboard`,
  `file_ops::tests::outermost_items_and_the_question_for_several`,
  `context_menu::tests::rename_is_for_one_item`)
- **AK-15** Given a Java and a Kotlin file moved into another package folder, then the package line
  follows (replaced, added, removed) unless the server's edits already changed it or the line did
  not match the old folder; MCP `move_file` and `copy_file` give a Kotlin file its new package.
  (`move_file::tests::the_package_line_follows_the_folder`; `crates/mcp/tests/it/protocol.rs`:
  `moving_a_kotlin_file_updates_its_package`)
- **AK-16** Given MCP, when `copy_file`, `move_file` and `delete_file` run with `paths`, then all
  items are checked first, a failing item changes nothing, and the results are per item.
  (`crates/mcp/tests/it/protocol.rs`: `copies_moves_and_deletes_several_files`)
- **AK-17** Given the hand tests (round 5, Nr. 200 and 201), then selecting several rows, Cut, Copy,
  Paste, Delete and Copy Path on them, and pasting a cut Java class into another package behave as
  above on the project owner's Mac.
- **AK-18 (v4)** Given the menu of a ▶ marker, when a real click lands on the editor text, then the
  menu is closed (`idecraft ui --selftest`: the Java steps); given the registry of popups, then
  every popup of behaviour 28 is in it once (`popups::tests::every_popup_is_registered_once`).
- **AK-19 (v4)** Given the hand test (round 5, Nr. 8 and Nr. 208), then a click outside the ▶ menu,
  outside ⌃⌥R, Go to Line and Recent Files closes them on the project owner's Mac, and switching to another
  application closes them too.
- **AK-20 (v5)** Given a Java file with `main`, a test class with the caret outside and inside a
  test method, a Kotlin top-level `main` and a Python file, then the run and debug names are as in
  behaviour 14a; given the macOS keymap, then the labels beside Run and Debug are ⌃⇧R and ⌃⇧D.
  (`context_menu::tests::run_items_name_class_and_method`,
  `context_menu::tests::shortcut_labels_come_from_the_keymap`)
- **AK-21 (v6)** Given the editor menu without and with a selection, then Cut and Copy are absent
  and present; column selection, the folding, analysis, more-run and open-in submenus, Generate…
  and Compare with Clipboard carry the action ids of behaviour 14b.
  (`context_menu::tests::editor_menu_without_selection_hides_cut_and_copy`); given a clipboard text
  and a file, then the diff rows compare them, and an empty clipboard gives no diff
  (`context_menu::tests::clipboard_comparison`); given the window, then a right click without a
  selection has no Cut and has Compare with Clipboard (`idecraft ui --selftest`: `mouse_steps`).

- **AK-22 (v7)** Given a window whose popups closed while inactive and no focused element, when it
  becomes active again, then the editor holds the focus (`WorkbenchView::render`, refocus on
  activation; not testable headless, hand check of F034-8 open).
- **AK-23 (v8)** Given the editor menu and a file's tree menu in a repository, then the Git
  submenu lists the items of behaviour 14c in order with their action ids, the file items greyed
  outside a repository, a folder without them (unit test
  `context_menu::tests::git_submenu_has_the_file_items`);
  window self-test step (written, run later): Git › Show History filters the log to the file.
- **AK-24 (v9)** By hand in the window: the Git submenu opened from the bottom of the editor shows all entries.

- **AK-25 (v10)** Given a Maven project with `lab/shop/core` (a class and an empty `util`) in the
  sources, `lab/shop/{app,core}` in the tests, a chain in the resources and one outside, then the
  unit tests of the tree model show: (a) the rows `lab.shop.core` and `lab.shop` › `app`, `core`
  one level below the source folders, plain folders everywhere else, the row's path, outermost
  folder and folder list; (b) the full name before the row is expanded, an empty innermost package
  included; (c) the option off gives folders, switching keeps the visible rows and the selection, a
  reload keeps the option; (d) →, ←, ↓, Enter, a double click, the arrow, Open and Collapse All on
  the row; (e) showing a file below it and a folder in its middle; (f) New, a new sub-folder,
  Rename and Delete of the innermost folder and a file in the middle, each followed by reading the
  tree again; (g) ranges, ⌘-click, ⇧↓, ⌘A and a secondary click with compacted rows; (h) a folder
  with a role is never inside a row and an error below a row marks it; (i) no project model, a plain
  tree and a remote tree are not compacted
  (`project_tree::compact_tests::*`, nine tests).
- **AK-26 (v10)** Given the tree model, then its MCP view names the folders of every row and its
  actions take any folder of a row (`project_tree::compact_tests::the_state_names_the_folders_of_a_row`);
  given MCP, then `project_tree` passes `list`, `expand`, `collapse`, `select` and `compact` to the
  window, refuses an unknown action before it and explains itself without a window, and
  `compact_middle_packages` is on by default and round-trips through `ide_settings` /
  `set_ide_settings` (protocol tests); the settings file accepts `[project_view]` (drift guard of
  the settings shape).
- **AK-27 (v10)** Given the window next to the reference with the Maven project of OBS-0024, then
  `lab.shop.core` is one row in both, the ⋮ menu's option switches it, and New, Rename, Delete, Cut,
  Copy and Paste on the row act on the innermost folder (comparison in the window; not run yet).

- **AK-28 (v11)** Given a project without a build file with `src/main/java/lab/inventory`,
  `…/util/deep`, an empty chain `lab/empty/a/b`, `lab/two/x` and `lab/two/y`, tests in
  `src/test/java` and `docs/one/two` outside the sources, when the derived folders are read, then
  the rows are `lab`, `empty.a.b`, `inventory`, `util.deep`, `two`, `x`, `y` as the reference
  showed them, `docs/one/two` stays folder by folder, `java` has the role `sources` and `tests`
  in the MCP view, and before they are read the folders stand one by one; when a file is added
  and the folders are read again, a new source folder appears and reading the tree again keeps
  it; when a `pom.xml` appears, the declared roles replace them
  (`project_tree::compact_tests::{derived_source_folders_compact_packages,
  derived_folders_follow_the_files}`). A new class beside a file with a package gets that
  package, also outside a `src` folder
  (`source_roots::tests::package_of_a_new_file_from_its_neighbours`,
  `file_ops::tests::java_classes_get_their_package_from_the_files_around`). In the window next to
  the reference: not run yet.
- **AK-29 (v12)** Given a project whose only build file is `app/pom.xml`, with
  `app/src/main/java/lab/shop/core/Core.java` and `app/src/test/java`, when the tree is built,
  then the folders stand one by one without roles (nothing is walked on the UI thread); when the
  window's background reading has delivered the roles, then `app` has the role `module`,
  `app/src/main/java` `sources`, `app/src/test/java` `tests` in the MCP view, and the packages are
  one row `lab.shop.core` (`project_tree::compact_tests::a_module_in_a_sub_folder_gets_its_roles`).
- **AK-30 (v13, behaviour 32)** Given the bundled macOS and default keymaps, then `+`, `-` and `*`
  mean expand, collapse and expand fully in a tree, a rebound key does it with its modifiers and a
  key without the action does nothing; expanding fully opens every node below the selected one and
  nothing outside it, stops at 200 and does nothing on a leaf (unit tests
  `tree_nav::tests::{expand_and_collapse_follow_the_keymap, fully_expanding_opens_what_is_below}`);
  in the Project tree it opens the folders below the chosen one except the ignored ones
  (`project_tree::tests::expanding_a_folder_with_everything_below`); MCP `project_tree` with
  `expand_all` and `collapse_all` (`protocol.rs`: `project_tree_expands_and_collapses_all`).
- **AK-31 (v13, behaviour 33)** Given the actions of a chord, then `ExpandAll` and `CollapseAll` are
  tried only while a tree has the focus and then before the editor's folding, and the menu bar's
  folding items go to the focused tree (unit tests
  `keymap_actions::tests::{expand_all_and_collapse_all_need_a_focused_tree, the_menu_bars_folding_keys_reach_a_focused_tree}`);
  in a window of the test harness `ExpandAll` opens the folders of the Project tree and
  `CollapseAll` closes them (`tree_and_popup_actions_keymap.rs::expand_all_and_collapse_all_act_on_the_project_tree`).
- **AK-33 (v14, behaviour 28b)** Given a window of the test harness with a file open and Quick
  Switch Scheme shown over its text, when the column through the popup's middle is clicked from
  top to bottom, then every click on a row chooses its theme and leaves the editor's caret where
  it was, every click on the popup beside its rows changes nothing, and a click on the text
  without a popup does move the caret
  (`crates/workbench/tests/popup_clicks.rs::a_click_in_the_theme_popup_does_not_reach_the_editor`).
  The wheel and the other popups are covered by the shared mechanism, not by a test of their own.
- **AK-32 (v13, behaviours 34–36)** Given an active file and a tree selection, then File Path shows
  the active file, or the tree's selection while the tree has the focus, and New in the same place
  takes the folder of the active file, else the tree's (unit tests
  `keymap_actions::tests::{file_path_shows_the_active_file_or_the_trees_selection, new_in_the_same_place_takes_the_folder_of_the_active_file}`);
  in a window of the test harness New in the same place opens the New menu for the folder of the
  active file, and Open in Right Split puts the tree's selected file into a second editor group
  (`tree_and_popup_actions_keymap.rs::{new_in_the_same_place_opens_the_new_menu, the_trees_file_opens_in_the_right_split}`).
  The titles in Find Action (`everywhere::tests::fourth_package_actions_have_titles`) and the
  effects for UI automation (`security::tests::fourth_package_actions_declare_their_effects`).

- **AK-34 (v16, behaviours 37, 39)** Given a workspace on disk, when a file is created (with
  folders on its way), renamed, moved (with a changed text), copied and deleted through the
  history, then each undo puts the disk back as it was before the step, byte for byte, and each
  redo as it was after it; a folder with files and an empty folder come back after Delete;
  several items are one step; a new operation drops the undone steps and the store holds
  nothing of dropped steps (unit tests `idecraft_core::file_undo::tests`:
  `create_is_undone_and_redone`, `rename_and_move_are_undone_and_redone`,
  `a_deleted_file_and_folder_come_back`, `several_items_are_one_step`,
  `a_new_operation_drops_the_undone_steps`).
- **AK-35 (v16, behaviours 40, 41)** Given a step in the history, when its file was written
  with other bytes, when something else took the place it goes back to, when it is gone, or
  when its kept copy in the store was changed or removed, then undo is refused with the path
  and the reason, the disk is unchanged and the step is still the next one; when the reason is
  gone the same undo works (`a_changed_file_refuses_undo`, `an_occupied_place_refuses_undo`,
  `a_changed_store_refuses_undo`). Given an item larger than the limit, Delete removes it,
  keeps nothing and reports that (`too_large_a_delete_is_not_kept`). Given `.idecraft/history`
  is a link to a folder outside the workspace, Delete keeps nothing there and reports that
  (`the_store_never_leaves_the_workspace`).
- **AK-36 (v16, behaviours 38, 39, 42)** In a window of the test harness
  (`crates/workbench/tests/it/undo_file_operations.rs`): after New File from the tree and
  typing in another file, ⌘Z with the focus in the tree asks about the new file, not the
  typing; Cancel leaves file, tab and step; the action removes the file and closes its tab;
  ⇧⌘Z asks and puts it back; the Edit menu's titles name the step while the tree has the focus
  and are plain with the focus in the editor (`a_new_file_is_undone_from_the_tree`). Rename, a
  cut and pasted file, and Delete of a file and of two selected items are taken back and made
  again, tabs follow and the deleted content is back
  (`rename_move_and_delete_are_undone_from_the_tree`). A new file edited and saved refuses the
  undo with a status that names it, and an open editor with unsaved changes does too
  (`a_changed_file_is_not_taken_back`).
- **AK-37 (v16, behaviour 43)** `crates/mcp/tests/it/protocol.rs::file_operation_undo_lists_and_acts_through_the_window`:
  `list` reaches the window without a right; `undo` and `redo` are refused without `write`
  and never reach the window then; with it they reach the window and the call is in the agent
  log; a refusal of the window is the tool's error and is logged as not done; an unknown
  action never reaches the window; without a window the tool says so. In a window of the test
  harness the call takes back a New File without a question and lists the step under `redo`
  (`undo_file_operations.rs::the_agent_tool_takes_a_step_back_without_a_question`).
- **AK-38 (v16)** Hand test, open: the question and the Edit menu's titles in the
  application's window, next to the reference (OBS-0042 point 3).
- **AK-39 (v17, behaviour 44)** `workbench_menus::tests::the_new_popup_opens_with_its_first_entry_chosen`.
- **AK-40 (v18, behaviour 45)** In a window of the test harness: Show Source on a file of the
  Project tree opens it in a tab and the tree keeps the focus; on a folder the status bar
  says so; with the editor in focus the action is none
  (`sixth_package_keymap.rs::show_source_keeps_the_focus_in_the_tree`); in the Find tool
  window the selected usage opens and the window keeps the focus
  (`sixth_package_keymap.rs::find_window_filters_groups_and_excludes`). Not compared next to
  the reference.
- **AK-41 (v18, behaviour 46)** In a window of the test harness: Copy… on a file of the
  Project tree opens the name field with a free name, Enter creates the copy beside the
  original with the same text; with the project folder selected nothing opens
  (`sixth_package_keymap.rs::copy_makes_a_copy_beside_the_original`). By hand, open: F5 from
  the editor and on a folder.
- **AK-42 (v19, behaviour 47)** Home and End mean the first and the last row with or without
  a keymap chord, the chords of the bundled macOS keymap mean the same and the default keymap
  has none; a list beside a text field with text leaves Home and End, a list whose rows ⌥↑
  and ⌥↓ move leaves the chords
  (`list_nav::tests::home_end_and_the_keymap_chords_mean_first_and_last`); the row reached is
  the first or last that can be selected, none in an empty list
  (`list_nav::tests::keys_go_to_the_first_and_last_row`); every view named in behaviour 47
  asks the shared rule (`list_nav::tests::every_listed_view_uses_the_shared_navigation`, which
  reads the source of each). In a window of the test harness: End, Home, ⌥↓ and ⌥↑ in the
  list popup, the extending keys selecting the same rows there (behaviour 48), and End, Home
  and the chords in the Find tool window (`seventh_package_keymap.rs::lists_go_to_their_first_and_last_row`); Home and End in
  the popups of Custom Folding and Stop Background Processes
  (`custom_regions_are_chosen_from_a_list`, `background_tasks_are_listed_and_stopped_by_row`).
  The other lists were not each driven in a window; nothing was compared next to the
  reference.
- **AK-43 (v19, behaviour 48)** A selection extended to a row reaches from the anchor, keeps
  the anchor and stops at the ends (`tree_nav::tests::a_selection_extends_to_the_first_and_the_last_row`).
  In a window of the test harness: in the Project tree ⇧End, ⇧Home, ⌥⇧↓ and ⌥⇧↑ select the rows
  from the anchor to the end named, ⌥↓ and ⌥↑ the last and the first row alone, and the tree
  keeps the focus
  (`seventh_package_keymap.rs::the_project_tree_extends_its_selection_to_the_ends`).
- **AK-44 (v20, behaviour 14b)** `editor_view::tests::a_secondary_click_inside_a_selection_keeps_it`
  (inside, on the first and the last character and right after it: kept; outside, and with no
  selection: the caret moves; several selections) and
  `mouse::tests::a_control_click_is_no_primary_click_on_the_mac` (the editor's primary handler leaves
  such a click to the menu).

## Out of scope
- (v19) Page Up and Page Down in the lists of behaviour 47; the first and last row in the
  lists named there as not among them; several selected rows in a list other than the
  Project tree, and with them the two extending actions there.
- (v16) Undo of file operations in a remote project, of the Move refactoring of Java and
  Kotlin files from the tree's Paste (its text edits are undone in the editors), of the
  agents' file tools, and of operations in other trees; a history of file operations that
  survives closing the project window; a warning in the Delete question before an item larger
  than the store's limit is removed (the status bar says it afterwards).
- Run with coverage, diagrams and the remaining run kinds of the reference's editor menu (IDECraft
  has no such action yet).
- Drag and drop, pasting files copied in Finder into the tree.
- Moving a folder of Java or Kotlin classes with their package lines and references (Move Package).
- Several selected rows in the other trees (Database, test tree, Build Tools, Call Hierarchy).
- Safe Delete with usage search, refactoring Rename of a class with its file (use `rename_symbol`,
  `move_file`).
- File templates beyond the class and package line; the reference's template editor.
- (v8: Annotate and the file's history are in the Git submenu, behaviour 14c.) The reference's
  further Git entries (Compare with Revision…, Show History for Selection, Add to .gitignore,
  Repository › …).
- Drag and drop in trees.
- (v13) A list of the file's parent folders to choose one to show (`ShowFilePath` shows the file
  itself); `OpenInRightSplit` by its mouse shortcut (⌥ with a double click); `ExpandAll` in the
  Database tree, and both actions in the trees not named in behaviour 33 (hierarchies, File
  Structure, Bookmarks, Debug); `FullyExpandTreeNode` in the trees outside behaviour 32.
- (v10) The reference's other package options of the tree (flattening all packages into one level,
  hiding empty middle packages) and a separate Packages view.
- (v10) Compacting chains of folders outside source folders or in resource folders: OBS-0024
  records none in the reference.

## Known deviations from the reference
- (v16, OBS-0042) **Undo of a file operation is refused when the item changed since**, and a
  deleted item comes back only from IDECraft's own store inside the project, up to 64 MiB
  (behaviours 40 and 41). What the reference does with a file that changed, where it keeps
  deleted content and up to which size was not observed. The question before undo and redo
  uses IDECraft's own words and names what will happen to the file; its buttons are named
  after the action instead of a generic confirmation.
- (v16) Undo and redo of file operations are offered only while the Project tree has the
  focus; whether the reference's editor undo reaches them was not observed. The history ends
  with the project window; whether the reference's survives a restart was not observed.
- **Delete keeps a Local History revision of every deleted file and names how many files and
  folders a folder takes with it** (IMP-0275, IMP-0276), instead of a generic confirmation.
- The New menu is one flat list of the kinds IDECraft supports, not the reference's grouping by
  plugin.
- A click outside an open context menu closes it and goes no further (also on macOS, where the
  reference passes it on in some places).
- **Paste never overwrites** and proposes a free name on a clash instead of offering to replace the
  existing file (IMP-0279); **copies and moves are marked in Local History** with the old paths
  restorable (IMP-0280).
- **Paste of several items checks all of them first** and writes nothing if one cannot be pasted
  (IMP-0281), instead of stopping half-way; it asks no names (each item keeps its name or gets a
  free one), where the reference opens a dialog with the target folder.
- For Kotlin files the package line follows a move but references in other files do not, because
  the Kotlin language servers in use offer no edits for a move (behaviour 26).
- Paste asks for the name in the small name field, not in a dialog with a folder chooser; the target
  folder is the tree's selection.
- (v10) A compacted row keeps the folder icon; whether the reference draws packages with an icon
  of their own was not recorded in OBS-0024. Rename and Delete on a compacted row act on the
  innermost folder only (behaviour 30); what the reference offers there was not observed.
- v18: Copy… keeps the copy in the item's own folder; the reference's dialog also asks for the
  target folder. Another folder is Copy and Paste (behaviours 17–19). Show Source and Copy…
  were not looked at in the reference.
