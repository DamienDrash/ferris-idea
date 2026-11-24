# SPEC-0059: Project tree and editor tabs

- **Version:** 9.11 · **Date:** 2026-11-18 (v9.11: the Project tree follows files and folders other programs create, delete or rename within a second, behaviour 31, AK-20; window check OBS-0096 FW1); 2026-11-15 (v9.10: Iterate Recent Files in the Switcher, behaviour 27h, AK-19; tenth package of docs/research/KEYMAP_COVERAGE.md); 2026-11-15 (v9.9: `ReopenClosedTab` has no shortcut in the default keymap, behaviour 22b; Ctrl+Alt+Shift+T is Refactor This there); 2026-11-14 (v9.8: `ForceRefresh` reads the project and its Git state again without trusting what Ferris IDEA remembers, behaviour 30, AK-18; ninth package of docs/research/KEYMAP_COVERAGE.md; v9.7: `NextEditorTab` and `PreviousEditorTab` step through the editor tabs, behaviour 29, AK-17; eighth package of docs/research/KEYMAP_COVERAGE.md; v9.6: `NewScratchFile` creates a scratch file of a chosen kind in the user's scratches folder, behaviour 28, AK-16; sixth package of docs/research/KEYMAP_COVERAGE.md; v9.5: the tab menu's copy entries also have the path with the line number and Copy Reference, behaviour 18, SPEC-0332 v2; v9.4 2026-11-13: the Switcher in detail — two columns, shortcut characters, Recent Locations, the folder line, open in a split — and the tab menu's first item named Close, behaviour 27, OBS-0031 run of 2026-11-13; v9.3: Select In, Reload All from Disk and Save As, behaviours 24–26; docs/research/KEYMAP_COVERAGE.md; v9.2 2026-11-10: Reopen Closed Tab restores the former place, behaviour 23; v9.1: implementation decisions, behaviour 22; v9: overflow, limit 30, Editor Tabs settings, tab menu and tab actions, pinned tabs, OBS-0031; v2: file status colours; v3: tab menu; v4: problem marks;
  v5: the empty editor area, behaviour 11; v6: navigation bar line and popup, behaviours 12–13; v7:
  popup placed at the navigation path with icons, behaviour 14, OBS-0018; v8: nodes without a folder
  on disk are never marked, OBS-0027)
- **Sources:** OSS (default ignored names of `FileTypeManagerImpl`, logic template; action ids of the
  ported keymaps, SPEC-0028; file status colour keys `FILESTATUS_*` of the bundled schemes), GEN (project tree and editor tabs of IDEs; F-011), SPEC-0058 (editor view),
  ADR-0009, GEN (IDEs underline the names of files with errors in the tabs and the project tree; F-032),
  SPEC-0070 (editor diagnostics), SPEC-0090 (diagnostics while typing), hand test 2026-11-04; v5:
  OBS-0011 item 1 (with no file open the reference's editor area lists the main ways to open
  something with their shortcuts and says that files can be dropped there), STD (macOS drag and
  drop of files from Finder), SPEC-0028 (shortcut display); v6: OBS-0013 item 3 (while the project
  tree is shown, the reference lists the navigation bar instead of the project view), OSS (action id
  `ShowNavBar` with ⌘↑ / Alt+Home in the ported keymaps), GEN (a navigation bar lets one walk the
  folders of the current file from the keyboard)
- **Feature matrix rows:** F-020, F-074, F-032

## Purpose
The workbench window around the editor: a project tree to find and open files, and tabs to switch
between open files.

## Behaviour
1. **Tree content:** the workspace folder is the root, expanded at start. Children are read when a folder
   is first expanded (a large repository opens at once), folders before files, each group sorted by name
   ignoring case. Names on the default ignore list are hidden: `.git`, `.hg`, `.svn`, `CVS`, `_svn`,
   `.DS_Store`, `__pycache__`, `.mypy_cache`, `.pytest_cache`, `.ruff_cache`, `vssver.scc`,
   `vssver2.scc` and names ending in `.pyc`, `.pyo`, `.rbc`, `.yarb` or `~`. [OSS]
2. **Tree use:** a click selects a row; a click on a folder's arrow or a double click on a folder
   expands or collapses it; a double click on a file, or Enter on a selected file, opens it; Up/Down move
   the selection, Right expands, Left collapses or goes to the parent. [GEN]
3. **Tabs:** opening a file that is already open activates its tab; otherwise a tab is added right of the
   active one and activated. Each tab shows the file name, with the parent folder added when two open
   files share a name, and a modified marker while the buffer has unsaved changes. [GEN]
4. **Tab limit:** at most 10 tabs; opening an eleventh closes the least recently activated tab without
   unsaved changes (none is closed if all have changes). [GEN]
5. **Closing:** the tab's close button, a middle click or `CloseContent` closes a tab; the tab activated
   before it becomes active again. A tab with unsaved changes is not closed by `CloseContent` until saved
   or closed a second time (no silent loss). [GEN]
6. **Switching:** `NextTab`/`PreviousTab` activate the neighbour tab, wrapping around;
   `ActivateProjectToolWindow` shows or hides the tree. Keys go to the editor first; actions it does not
   handle reach the workbench. [OSS, GEN]
7. **File status colours:** in a Git repository, the names of changed files in the tree and in the tabs
   take the colour of their status as in the Commit tool window (SPEC-0063 behaviour 1); unchanged files
   and folders keep the normal text colour. The status is read in the background when the window opens
   and every 2 seconds after, so saves, commits and changes from outside appear without action. [GEN, OSS]
8. Commands: `ferris-idea ui [folder]` opens the workbench on a folder (the current folder by default).
   The MCP counterparts are `list_files`/`read_file` (SPEC-0006) for the tree content. [ADR-0008]
9. **Tab menu (v3):** a right click on a tab opens its menu with "Close Tab" and "Local History ›
   Show History"; an item activates that tab and runs its action on it (SPEC-0085). [GEN, DOC]
10. **Problem marks (v4):** a file with at least one error has its name underlined with a wavy line in
    the error colour (effect colour of `ERRORS_ATTRIBUTES`) on its tab and in the tree; in the tree
    every folder above it, up to the root, is underlined the same way. The text keeps its colour (the
    file status colour of behaviour 7 or the normal one): the mark is only the underline. Only errors
    count; warnings and weaker problems do not mark. [GEN, OSS] Nodes without a folder on disk
    ("External Libraries", "Scratches and Consoles") are never marked (v8, OBS-0027 L13).
    - **Source of errors:** for an open file, the problems its editor shows (language server and
      Ferris IDEA's own inspections, SPEC-0070); for a file that is not open, the errors the language
      server pool last published for it (SPEC-0090 behaviour 3; servers that analyse the whole project
      publish them). [GEN, SPEC-0090]
    - **Live:** the marks follow the diagnostics as they change while typing (within the pool's
      300 ms plus the server's delay) and disappear as soon as the file has no error left. Closing a
      file whose server forgets its document (`didClose`) drops its mark with it. [GEN]
    - **Agents:** no MCP tool lists the window's tabs or tree rows; agents read the same errors per
      file through `file_diagnostics` (SPEC-0023). [ADR-0008]
11. **Empty editor area (v5):** with no tab open, the editor area shows a short, centred list of the
    main ways to open something, each with the shortcut of the keymap in use beside it (written as
    SPEC-0028 behaviour 5 says, a double-modifier gesture as "Double ⇧"): Search Everywhere, Go to
    File, Recent Files and the project view; below it a line saying that files can be dropped here.
    Files dropped from the file manager onto the editor area (empty or not) open in tabs, the last
    one active; dropped folders are ignored. An action without a shortcut is listed without one.
    [OBS-0011, STD]
12. **Project view or navigation bar (v6):** the fourth line of the empty editor area depends on
    the project tree: while it is hidden the line names the project view with the shortcut of
    `ActivateProjectToolWindow`; while it is shown that line would only hide it again, so the line
    names the navigation bar instead, with the shortcut of `ShowNavBar` (⌘↑ in the macOS keymap,
    Alt+Home in the default one). The list follows the tree as it is shown and hidden. [OBS-0013,
    OSS]
13. **Navigation bar popup (v6):** `ShowNavBar` opens a popup listing the folders and then the files
    of the active file's folder (the project folder when no file is open), sorted by name, hidden
    names left out as in the tree (behaviour 2); its heading is the folder's path from the project
    folder on. Choosing a folder lists that folder in the same way; choosing a file opens it; Escape
    closes. Folders end in `/`. A folder without entries shows one line saying it is empty. [GEN,
    OSS]
14. **Navigation bar popup placement (v7):** the popup of behaviour 13 opens just above the status
    line at its left end, where the navigation path starts, not in the middle of the editor area;
    each row shows the folder icon or the file type icon of the project tree before the name.
    [OBS-0018 item 13]
15. **Overflow (v9):** the tab bar is one row. When the tabs do not fit, it scrolls so that the
    active tab is always fully visible (also right after opening, activating by any means, or
    closing); a wheel or trackpad scroll over the bar scrolls it sideways. At the right end a
    "Show hidden tabs" button (chevron) opens a list of all tabs of that group, hidden ones included,
    each with icon, name and modified mark; choosing one activates it and scrolls it into view;
    typing filters the list. [OBS-0031 L1]
16. **Limit (v9):** the limit of behaviour 4 is a setting, 30 by default, range 1–100. Over the limit
    one tab is closed by the chosen rule: "Close unchanged" (default: the least recently activated
    tab without unsaved changes) or "Close unused" (the least recently activated tab; a tab with
    unsaved changes keeps its text in memory and its file stays modified until saved — Ferris IDEA
    saves it first, never loses it). Pinned tabs are never closed by the limit. [OBS-0031 L2]
17. **Settings page (v9):** Settings › Editor › Editor Tabs holds: tab placement (Top, None),
    show tabs in one row with scrolling (default) or squeezed, or multiple rows; pinned tabs in a
    separate row; show file icon; show file extension; show the folder for equal names (behaviour
    3); mark modified with ` *` (on by default in Ferris IDEA, as before; the reference has it off by
    default — a deliberate deviation, unsaved work stays visible); show the full path on hover (tooltip); close
    button position (Right, Left, None) and whether it shows on every tab or only on the active
    and hovered one (default: active and hovered, as the reference); sort alphabetically; open new
    tabs at the end; preview tab (a single click in the tree opens a provisional, italic tab that
    the next single click replaces; editing or a double click keeps it); the limit and its rule
    (behaviour 16); and which tab becomes active when the current one closes: the one on the left,
    the one on the right, or the most recently opened (default: most recently opened, as
    behaviour 5 already does). Every setting applies at once to every group. The MCP tool
    `settings` reads and changes them under `editor.tabs.*`. [OBS-0031 L3, L4; ADR-0008]
18. **Tab menu (v9):** the right-click menu of a tab, carried out on that tab: Close; Close Other
    Tabs; Close All Tabs; Close Tabs to the Left; Close Tabs to the Right; Close Unmodified Tabs;
    Close All but Pinned (while a tab is pinned); separator; Copy Path/Reference › (the entries of
    SPEC-0275's tree menu, then Path With Line Number and Copy Reference, SPEC-0332 v2
    behaviour 13e); separator; Split Right; Split and Move Right; Split Down; Split and Move
    Down; Move Tab to Opposite Group (while split); separator; Pin Tab / Unpin Tab; Open Tab in New
    Window (disabled with a tooltip until a second editor window exists, see Out of scope);
    Configure Editor Tabs… (opens the page of behaviour 17); separator; Run and Debug of the file
    when a run configuration applies (SPEC-0087); Open In › Finder / Terminal; Local History ›
    Show History; Git › Annotate (as before); Rename File… (SPEC-0275 rename). Closing several
    tabs follows behaviour 5 for each: a tab with unsaved changes is asked about once in one
    dialog listing them (Save All, Don't Save, Cancel), never closed silently. [OBS-0031 L5]
19. **Tab actions (v9):** with their keymap shortcuts (SPEC-0028): `ReopenClosedTab` (⇧⌘T) reopens
    the most recently closed file at its last caret, up to 30 remembered; `PinActiveEditorTab`;
    `CloseAllEditorsButActive`; `CloseAllEditors`; `CloseAllToTheLeft`; `CloseAllToTheRight`;
    `CloseAllUnmodifiedEditors`; `CloseAllUnpinnedEditors`; `Switcher` (⌃⇥, ⌃⇧⇥: a popup of the
    recently used files and tool windows, held ⌃ cycles, releasing ⌃ opens the selection);
    `TabList` (the list of behaviour 15 from the keyboard); `MaximizeEditorInSplit` (⇧⌘'
    toggles hiding the tool windows and the other group). All are in Find Action and the View or
    Window menu as in the reference's menu bar. [OBS-0031 L6]
20. **Pinned tabs (v9):** a pinned tab shows a pin icon instead of the close button, moves to the
    left end after the other pinned tabs and stays there; Close Other/All and the limit leave it
    open; closing it directly still works. With "pinned tabs in a separate row" the pinned tabs
    form a row above the others. Pins are remembered per project with the open tabs. [GEN]
21. **MCP (v9):** the tool `editor_tabs` lists the groups with their tabs (path, pinned, modified,
    active) and runs `activate`, `close`, `close_others`, `close_all`, `pin`, `unpin`,
    `reopen_closed` on them, so agents can tidy the editor area. [ADR-0008]
22. **Decisions (v9.1, implementation):**
    a. The settings of behaviour 17 are kept as `[editor.tabs]` in the user's `settings.toml`
       (SPEC-0129). The MCP counterpart of "`settings`" is the existing pair `ide_settings` (reads
       them under `settings.editor.tabs`) and `set_ide_settings` with an `editor_tabs` object of
       the keys to change; an unknown key or value is refused and nothing is changed. The window
       applies a change made in Settings at once; a change made through MCP reaches open windows at
       their next start, as every IDE setting (SPEC-0129).
    b. `ReopenClosedTab` gets ⇧⌘T in the macOS keymap even though the bundled keymap also gives it
       to `GotoTest`/`ServiceView.ShowServices` (`ServiceView.ShowServices` is not carried out;
       since SPEC-0071 v10 `GotoTest` exists and yields this chord, behaviour 16 there). In the
       default keymap (Linux) it has no shortcut, as in the reference: Ctrl+Shift+T is Go to Test and
       Ctrl+Alt+Shift+T, meant for it until v9.8, is Refactor This (`Refactorings.QuickListPopupAction`)
       there; the Window menu and Find Action reach it (v9.9, found by the Linux window self-test).
       `MaximizeEditorInSplit` shares ⇧⌘' (Ctrl+Shift+' by default) with `MaximizeToolWindow`:
       with the focus in an editor it maximizes the editor, otherwise the tool window.
    c. The question of behaviour 18 is a native alert listing the files; Save All saves them and
       closes, Don't Save closes them and drops the changes, Cancel closes nothing.
    d. `editor_tabs` needs no right: it changes only what the window shows, and it never closes a
       tab with unsaved changes — such tabs stay open and are named in `kept_modified`.
    e. Open tabs, pins, the active tab per group and the split layout (SPEC-0112 behaviour 10) are
       written to `.ferris-idea/editor-tabs.json` when the window closes or the app quits and are read
       when the project opens; files that no longer exist are left out. The file is state of
       one user and is listed in the generated `.ferris-idea/.gitignore` (SPEC-0069 behaviour 3a).
    f. The preview tab of behaviour 17 is off by default.
    g. "Close unused" over the limit saves a modified tab before closing it (behaviour 16); if the
       save fails, the tab stays open.
23. **Reopen at the former place (v9.2):** `ReopenClosedTab` (behaviour 19) and `reopen_closed`
    (behaviour 21) put the tab back where it was, not at the end of the row: in the group it was
    closed in if that group still exists (otherwise in the main group), at the index it had there,
    clamped to the current number of tabs. A tab that was pinned when it closed comes back pinned,
    at its index within the pinned tabs; one that was not pinned never lands among the pinned tabs
    (at least right after them). It becomes the active tab of that group, with the focus, and the row
    scrolls it into view (behaviour 15). Settings "sort alphabetically" still wins: with sorting
    on, the tab goes to its sorted place. [GEN: IDEs reopen a closed tab at its former position;
    finding of the F-020 side-by-side check]
24. **Select In (v9.3):** `SelectIn` (⌥F1) selects the file of the active editor tab in the Project tree, expanding down to it, shows the Project tool window and gives it the focus. The Project tree is the only target Ferris IDEA has, so no list of targets is shown first. Without a file in the active tab, or for a file the tree does not hold, the status line says so. [OSS: id and shortcut of the bundled keymaps; GEN]
25. **Reload All from Disk (v9.3):** `Synchronize` (⌥⌘Y on macOS) reads from disk at once what Ferris IDEA otherwise notices on its own (behaviour 7 and the editor's disk check): the Project tree is read again keeping its selection and expanded folders, every open editor is compared with its file (an unmodified one takes the file's content, a modified one keeps its text and says so), and file status and branch are read again. Nothing is saved or discarded. [OSS: id and shortcut; GEN]
26. **Save As (v9.3):** `SaveAs` (⇧⌘S on macOS) asks in the system's save dialog, starting in the file's folder with its name, where to write the text of the active editor (unsaved changes included), writes it there and opens that file in a tab. The original file and its tab stay as they are, unsaved changes included. Without an active file editor, and for a document of a remote project, the action is not available. [OSS: id and shortcut; GEN]

27. **The Switcher in detail (v9.4):** next to the reference (OBS-0031, run of 2026-11-13) the Switcher of behaviour 19 differed in what it shows: the reference's popup has two columns — on the left the tool windows, each with a shortcut character, and below them an entry for Recent Locations with its shortcut; on the right the recently used files with their icons — the folder of the selected file at the bottom, a split control at the right edge of the selected file's row, and no preview of the file's content. Ferris IDEA's was one list (the files under a heading, then the tool windows under a heading), the files written with their path from the project folder, without characters, folder line or split control. Since v9.4:
    a. **Two columns.** Left: the tool windows in their default order (SPEC-0061 v4 behaviour 13), then, set apart, Recent Locations with the shortcut of `RecentLocations` in the keymap in use. Right: the recently used files, most recent first (the files of behaviour 19, at most 30), each with the icon its tab has and its file name; two files of the same name also show their folder, as tabs do (behaviour 3). A file with unsaved changes carries the modified mark. No headings, no preview. [OBS-0031]
    b. **Shortcut characters.** A tool window whose activation action has a shortcut of one keystroke with a digit in the keymap in use shows that digit (Project `1` with the macOS keymap); every other tool window shows the first letter of its title that no tool window before it took, in upper case, or nothing when all its letters are taken. Pressing the character while ⌃ is held — the way the Switcher is used — closes the Switcher and shows that tool window. Without ⌃ (the Switcher opened from the menu) typed characters filter the rows as before. [OBS-0031 for the characters being shown, digits and letters; GEN for pressing them; which letter the reference gives a tool window was not recorded, so the rule is this specification's]
    c. **Selection and keys.** As before, the Switcher opens with the file used before the current one selected, ⌃⇥ / ⌃⇧⇥ (and ↓ / ↑) walk through the files and then the tool windows and Recent Locations, wrapping, releasing ⌃ or Enter opens the selection, Escape closes. New: ← and → change the column, to the row at the same height or the column's last row. [GEN; the reference's order of walking was not recorded]
    d. **Folder line.** The bottom line of the popup names the folder of the selected file: from the project folder (the project folder's own name for a file directly in it), with the home folder written `~` for a file outside the project. It is empty while a tool window or Recent Locations is selected. [OBS-0031]
    e. **Open in a split.** The selected file's row shows a split control at its right edge. A click on it, or the shortcut of `OpenInRightSplit` in the keymap in use (⇧Enter in the bundled keymaps, as in Search Everywhere, SPEC-0067 v8 behaviour 19, and the Project tree, SPEC-0275 v13 behaviour 36; ⌃ still held does not count), opens the file in a new group right of the main group (SPEC-0112 behaviour 1) and focuses it there; the main group gets no tab for a file it did not show before. [OBS-0031 for the control; DOC: the reference's help describes opening the selected file of such a list in a split on the right with Shift+Enter; the effect was not triggered in the comparison]
    f. **Recent Locations.** Choosing the entry closes the Switcher and opens Recent Locations (SPEC-0069 v2 behaviour 8). [OBS-0031 for the entry]
    g. **Tab menu (correction to behaviour 18).** The first item of a tab's menu is labelled "Close", as behaviour 18 lists it and the reference shows it (OBS-0031 item 5); the implementation still said "Close Tab" from behaviour 9. The menu bar keeps "Close Tab". [OBS-0031 item 5]
    h. **Iterate Recent Files (v9.10).** `SwitcherIterateItems` (⌘E in the bundled macOS keymap, Ctrl+E in the default one) moves the Switcher's selection down as ⌃⇥ does; ⌃ held for the Switcher does not count. In Recent Files the same chord keeps switching "Show edited only" (SPEC-0069 v3 behaviour 9), the second action the reference binds there. [OSS: id and key of the bundled keymaps, "moves the selection in an open popup" from `recent-files.spec.md` of the open platform]
    No MCP tool: the Switcher is window state, like the lists of SPEC-0069; agents switch with `editor_tabs` and `tool_windows`.
28. **New Scratch File (v9.6):** `NewScratchFile` (⇧⌘N, Ctrl+Alt+Shift+Insert in the default
    keymap; File › New Scratch File…) lists file kinds — plain text first, then CSS, Go, HTML,
    HTTP request, Java, JavaScript, JSON, Kotlin, Markdown, Python, Rust, Shell script, SQL,
    TypeScript, XML, YAML — with the kind of the active file selected, else plain text. The
    chosen kind gets an empty file in the user's scratches folder (`scratches` in Ferris IDEA's
    settings folder, the folder of the Scratches node of the Project tree, SPEC-0061 v4
    behaviour 16), named `scratch.<extension>`, then `scratch_1.<extension>`, `scratch_2…`: the
    first name that is free; nothing is ever overwritten. The folder is created when missing,
    the file opens in the editor with the focus, and the status bar names it. A scratch file
    belongs to no project and is in no repository. In a remote project the action says that
    scratch files are not available there. [OSS: id, title, keys, the name `scratch` with a
    running number and "a list of languages, the current file's first" from
    `ScratchFileActions.java`, logic template. The open platform starts the file with the
    editor's selection and offers every language it knows; Ferris IDEA starts empty and offers
    the kinds above]
29. **Next and previous editor tab (v9.7):** `NextEditorTab` (⌃⇧→ in the bundled macOS keymap,
    ⌥⇧→ in the default one) selects the editor tab right of the active one in the focused tab
    group and `PreviousEditorTab` (⌃⇧←, ⌥⇧←) the one left of it, wrapping at the ends, and the
    focus goes into its editor — what `NextTab` and `PreviousTab` do, but never for the tabs of a
    tool window. A group with one tab keeps it. While the diff viewer has the focus the chords
    stay its next and previous change (SPEC-0084 v5 behaviour 22). Find Action names them "Next
    Editor Tab" and "Previous Editor Tab". No MCP tool: the active tab is window state, as for
    `NextTab`. [OSS: ids and keys of the bundled keymaps; GEN]

30. **Force Refresh (v9.8):** `ForceRefresh` (⌥⇧⌘R in the bundled macOS keymap, Ctrl+Shift+F5
    in the default one; File › Force Refresh after Reload All from Disk) does what Reload All
    from Disk does (behaviour 25) without relying on what Ferris IDEA remembers about the files:
    a. every open editor reads its file's content and compares it with what it holds, also
       when the file's modification time and size are what they were when it was last read
       (a checkout or a tool that keeps the time stamp; behaviour 25 trusts the time stamp
       and size). An unmodified editor takes the new content, a modified one keeps its text
       and says so, as in behaviour 25;
    b. the HEAD text every open editor's change markers compare against is read again from
       Git (SPEC-0064), and with it the annotations and authors that depend on HEAD
       (SPEC-0277 behaviour 8, SPEC-0292 v4 behaviour 7d) — after a commit, amend or reset
       made outside Ferris IDEA the markers follow at once;
    c. the Project tree, the file status colours, the branch with ahead and behind, the
       Commit tool window and its changelists and the Git log are read again (behaviour 25);
    d. the run configurations are read again from `.ferris-idea/run.toml` (SPEC-0087 behaviour
       1).
    Nothing is saved, discarded, fetched or pushed, and no language server is restarted. The
    status bar says what was done: "Force Refresh: N open files compared with the disk, Git
    state and run configurations read again". In a remote project the action is behaviour 25
    for the host's files (the host's Git state comes through the session) and says so. Find
    Action names it "Force Refresh". No MCP tool: every MCP tool reads the disk and Git when
    it is called and keeps nothing to refresh. [OSS: id, keys and the title "Force Refresh"
    of the open platform; what it does is this specification's: GEN]

31. **The tree follows the disk (v9.11):** every half second, with the window in front or not,
    the window lists again, off the UI thread, every folder of this machine the Project tree has
    read (expanded, or read and collapsed since; one directory listing per folder, nothing read
    below a folder the tree has not read) and compares the names and whether each is a folder
    with what the tree holds (hidden names of behaviour 1 left out; the files of a resource
    bundle count under their own names). When they differ, the tree is read again as Reload All
    from Disk reads it (behaviour 25): expanded folders stay expanded and the selection stays
    where its path still exists. A file or folder another program creates, deletes or renames in
    a shown folder — also in the project folder itself, in a new subfolder, and for a project
    opened through a symbolic link such as `/tmp` for `/private/tmp` — therefore shows or goes
    within about a second, without user action. A look that overlaps a reading of the tree by
    Ferris IDEA itself is dropped. A remote project's tree (SPEC-0317) is not looked at. Before
    v9.11 the tree was read again only after Ferris IDEA's own operations and Reload All from Disk:
    a README created in a shell did not show after 25 s with the window in front (OBS-0096 FW1).
    Content changes of open files are behaviour 10 of SPEC-0058, the language servers'
    notifications SPEC-0042 v11 behaviour 20. [GEN]

## Acceptance criteria
- **AK-1** Tree: lazy children, ordering, hidden names, expand/collapse, visible rows with depth,
  keyboard selection and parent navigation (unit tests on a temporary folder).
- **AK-2** Tabs: open/activate, insertion right of the active tab, duplicate names with parent folder,
  limit with least-recently-activated eviction sparing modified tabs, close returns to the previous tab,
  next/previous wrap (unit tests).
- **AK-3** In a real window (`ferris-idea ui --selftest`, keys dispatched through the toolkit): Down/Enter
  in the tree opens a file in a tab, typed text reaches the editor, the keymap deletes a line, a modified
  tab survives the first `CloseContent` and closes on the second. Mouse use is checked by hand.
- **AK-4** Colour key per changed file by absolute path (unit test); in the window self-test the changed
  file of the Git fixture is shown with `FILESTATUS_MODIFIED` and a clean file without colour.
- **AK-5 (v3)** The tab menu offers the local history and the workbench runs every item (unit test
  `tabs::tests::the_tab_menu_offers_the_local_history_and_every_item_runs`); Replace in Files keeps
  the old text in the local history (unit test
  `find_in_files_model::tests::replace_on_disk_keeps_the_old_text_in_the_local_history`).
- **AK-6 (v4)** Problem marks: only errors mark a file, a folder is marked when a file below it has an
  error and loses the mark when the error is fixed, the tab flags follow the open paths (unit tests
  `problem_marks::tests`, `tabs::tests::tab_problem_flags_follow_the_errors`,
  `project_tree::tests::tree_rows_mark_problem_files_and_their_folders`).
- **AK-7 (v4)** By hand (hand test 207): an error in an open Java file underlines its tab and its
  tree row with its folders, keeps the file status colour, and fixing it removes the marks live.
- **AK-8 (v5)** The hint list of the empty editor area names the four ways with the macOS and the
  default keymap's shortcuts, and the files to open from a drop leave out folders (unit tests
  `chrome::tests::empty_editor_hints_follow_the_keymap`, `chrome::tests::dropped_files_open_without_folders`);
  the window self-test sees the hint list while no tab is open (`ferris-idea ui --selftest`). Dropping
  a file from Finder is checked by hand.
- **AK-9 (v6)** The fourth hint names the project view while the tree is hidden and the navigation
  bar with ⌘↑ (macOS) / Alt+Home (default) while it is shown; the popup's entries list folders
  before files, sorted, without hidden names, and its heading is the path from the project folder
  (unit tests in `chrome`); the window self-test sees the navigation bar line while the tree is
  shown. The shortcut and walking the popup are checked by hand.
- **AK-11 (v9)** Unit tests on the tab model: overflow offset keeps the active tab visible for any
  bar width; the hidden-tabs list holds every tab; limit 30 with both rules and pinned tabs spared;
  every tab menu item runs on its own tab; close-several asks once for modified tabs; reopen-closed
  restores the last file and caret; pinned tabs move left and survive Close Others; the settings
  round-trip through `editor.tabs.*`; `editor_tabs` lists and acts (protocol test).
- **AK-12 (v9)** Window self-test: 14 files opened leave all 14 tabs open with the active one visible;
  the chevron list activates a hidden tab; ⇧⌘T reopens a closed tab; Close Other Tabs from the tab
  menu leaves the pinned and the active tab. Hand test: the same next to the reference, plus the
  Switcher with ⌃⇥.
- **AK-13 (v9.2)** Unit tests on the tab model: a closed tab reopens at its former index (clamped),
  a formerly pinned tab comes back pinned, an unpinned one never among the pinned tabs, sorting
  wins; the window self-test reopens the third of 14 tabs as the third tab, so the first tab stays
  in view and can be clicked.
- **AK-10 (v7)** The popup rows carry the tree's icons (unit test
  `chrome::tests::nav_entries_carry_icons`); the popup sits at the bottom left above the status line
  (checked in the window in the re-check of OBS-0018).
- **AK-14 (v9.3)** The three actions are found by Find Action under their menu titles and carried out by the workbench (`menu_bar::tests::every_menu_action_is_carried_out_somewhere`, `every_menu_action_is_found_by_search_everywhere`); the file Save As proposes (`keymap_actions::tests::save_as_starts_at_the_file`). In the window (open): ⌥F1 selects the file in the tree, ⌥⌘Y shows a file made in a shell, ⇧⌘S writes a copy and opens it.

- **AK-15 (v9.4)** Unit tests on the Switcher's model: the characters of the tool windows (digits from the keymap, first free letter otherwise), the names of files with equal names, the folder line inside and outside the project (the `switcher` tests in `tabs::tests`). In a window without a screen (the `the_switcher` tests in `crates/workbench/tests/it/tabs_and_tree_handtests.rs`): ⌃⇥ opens it with the previous file selected and releasing ⌃ opens that file; the columns hold the tool windows with characters and Recent Locations, and the files; ⌃ with a tool window's character shows the tool window; → and ← change the column; the folder line follows the selection; the shortcut of `OpenInRightSplit` (⇧Enter, also with ⌃ held) opens the selected file in a group on the right without a new tab in the main group; Recent Locations opens its popup. By hand next to the reference: how the two columns, the characters, the folder line and the split control are drawn.
- **AK-16 (v9.6, behaviour 28)** The names in a row, the kind selected first and that an
  existing file is kept (`keymap_actions::tests::scratch_files_get_the_next_free_name`); in a
  window of the test harness the action lists the kinds with the active file's kind selected,
  and a scratch file created in a folder of the test is there, empty, open in a tab, and a
  second one gets the next name (`sixth_package_keymap.rs::a_scratch_file_is_created_and_opened`).
  The user's own scratches folder is not written by a test. On screen: not checked.
- **AK-17 (v9.7, behaviour 29)** In a window of the test harness with three files open, the
  two actions step right and left through the tabs and wrap at both ends; the key reaches the
  action from the editor (`eighth_package_keymap.rs::editor_tabs_step_right_and_left`). On
  screen: not checked.
- **AK-18 (v9.8, behaviour 30)** In a window of the test harness: a file rewritten with the
  same length and modification time is not taken by Reload All from Disk and is taken by Force
  Refresh; after a commit made outside Ferris IDEA the change markers of the open file are gone
  after Force Refresh; the status line names the open files
  (`ninth_package_keymap.rs::force_refresh_*`). On screen: not checked.
- **AK-19 (v9.10, behaviour 27h)** In a window of the test harness with three files open, ⌃⇥
  selects the file used before and ⌘E (⌃ still held) the next one, which Enter opens
  (`tenth_package_keymap.rs::command_e_iterates_the_switcher`). On screen: not checked.

- **AK-20 (v9.11, behaviour 31)** The folders the tree has read and their comparison with the
  disk: a file created, renamed or deleted, a new folder and a file that became a folder make it
  differ, also with the project opened through a symbolic link; a folder never read, a hidden
  name and a resource bundle's grouping do not
  (`project_tree::compact_tests::the_disk_differs_from_the_read_folders`). In a window of the test
  harness, on a project opened through a symbolic link, with the window's clock advanced at most
  one second each: a README created in the project folder, a new folder below an expanded one,
  a Java file in a new expanded subfolder, a rename and a deletion show, and the expanded folders
  stay expanded (`file_watch_fw1.rs::the_tree_follows_files_and_folders_changed_outside`). On
  screen: not checked.
## Out of scope
Tool-window stripes and other tool windows, drag and drop of tabs, split editors (SPEC-0112), Open Tab in New Window (a second editor window; planned), the tab placements Left/Right/Bottom, file
operations in the tree (new, rename, delete), scroll-from-source, problem marks for files no language
server has published diagnostics for (a whole-project analysis).

## Known deviations from the reference
The Switcher (behaviour 27): which letter a tool window without a digit gets, the order of walking with ⌃⇥ and the effect of the split control follow this specification, not an observation; the reference lists its own tool windows, Ferris IDEA its own. Not compared in the window after v9.4.
The empty editor area and the navigation bar popup were compared side by side in OBS-0013 and
OBS-0018; the rest not yet. A modified tab is protected from `CloseContent` once (the reference closes
it and keeps the change only in memory or saves automatically, depending on settings). File status
colours follow changes made outside Ferris IDEA within 2 s, without a manual refresh (IMP-0038). Problem
marks of files that are not open depend on what their language server publishes; whether the reference
keeps the mark of a closed file is not compared yet.
