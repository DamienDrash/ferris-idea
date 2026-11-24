# SPEC-0067: Search Everywhere popup

- **Version:** 13 · **Date:** 2026-11-17 (v13: the rows item of v5 had the number of Close, 5; it is behaviour 23 now, no change of behaviour); 2026-11-14 (v12: the focus goes back to the element that had it — a list or tree of the tool window, not only the window —, and a tool window counts as focused wherever in it the focus is, behaviour 21, AK-13; second window check of 2026-11-14; v11: `SearchEverywhere.NavigateToNextGroup` and `SearchEverywhere.NavigateToPrevGroup` go from group to group of the results, behaviour 22, AK-12; sixth package of docs/research/KEYMAP_COVERAGE.md; v10: the focus goes back to the tool window that had it when the popup closes or runs an action, behaviour 21); 2026-11-13 (v9: an Endpoints tab that searches the rows of the Endpoints tool window by path, HTTP method and handler, opened by `GotoUrlAction`, behaviour 20; v8: `TextSearchAction` opens the Text tab and ⇧Enter is the keymap action `OpenInRightSplit`, behaviour 19; v7: Tab, ⇧Tab and Enter of the popup are the keymap actions `SearchEverywhere.NextTab`, `SearchEverywhere.PrevTab` and `SearchEverywhere.SelectItem`, behaviour 18); 2026-11-12 (v6: the files are the project's own sources and the popup says what it left out, behaviour 13, SPEC-0031 v5); 2026-11-06 (v2: readable rows, raised popup; v3: action titles; v4: only
  Shift tapped alone counts, OBS-0021 item 12; v5: side-by-side comparison OBS-0023 — Classes and Text
  tabs, recent files, marks, footer, right split, preview, non-project items, filter, calculator,
  settings pages, Find window)
- **Sources:** GEN (one popup for files, symbols and actions, opened by pressing Shift twice; F-016), OSS
  (action ids `SearchEverywhere` with the double-Shift gesture, `GotoFile`, `GotoClass`, `GotoSymbol`,
  `GotoAction` of the ported keymaps), SPEC-0031 (matching, ranking, categories), SPEC-0059 (workbench
  actions), SPEC-0058 (editor actions); v9: SPEC-0103 (the rows of the Endpoints tool window), GEN
  (IDEs with an endpoints list let the user jump to an endpoint by typing a part of its URL), DOC
  (the names of the HTTP request methods as the frameworks of SPEC-0103 write them), task of the
  Project Owner's run of 2026-11-13 (the action id `GotoUrlAction`; the bundled keymaps do not
  hold the id)
- **Feature matrix rows:** F-016

## Purpose
Reach any file, symbol or action from the keyboard in one place, as with the reference's Search
Everywhere.

## Behaviour
1. **Open:** pressing Shift twice within 400 ms (no other key in between) opens the popup on the "All"
   tab; `GotoFile` (⇧⌘O / Ctrl+Shift+N) on "Files", `GotoClass` and `GotoSymbol` (⌘O, ⌥⌘O / Ctrl+N,
   Ctrl+Shift+Alt+N) on "Symbols", `GotoAction` (⇧⌘A / Ctrl+Shift+A) on "Actions". The popup floats
   above the window under the toolbar; the query keeps the last text, selected. Tab switches to the next
   tab, a click on a tab selects it. [GEN, OSS] (v4) A press counts only as a tap: Shift pressed while
   no other modifier is held and released before any key or other modifier. The Shift left over
   when ⌘ of ⌘⇧← is released first, or the Shift of a capital letter, is no tap, so ⌘⇧← followed
   quickly by ⇧P types `P`. [GEN; OBS-0021 item 12: Ferris IDEA opened the popup there, the reference
   did not]
2. **Search while typing:** each change searches in the background with SPEC-0031's matching and ranking,
   at most 8 results per category on "All" and 50 on a single tab. A newer query cancels the running
   search (symbol scanning stops between files). The file list is read once each time the popup opens.
   [GEN]
3. **Results:** grouped under the headings Files, Symbols, Actions (empty groups hidden). Files show icon,
   name and folder; symbols their kind, name and `file:line`; actions their name in words and shortcut.
   Up and Down move over results (skipping headings) and wrap around; the first result is selected. [GEN]
4. **Choose:** Enter or a double click: a file opens in a tab; a symbol opens its file with the caret at
   the start of its line; an action runs as if its shortcut had been pressed (editor actions on the
   active editor first, then workbench actions); an action Ferris IDEA does not have yet reports so in the
   status bar. The popup closes. [GEN]
5. **Close:** Escape closes the popup and returns focus to the editor. [GEN]
6. The MCP counterpart is `search_everywhere` (SPEC-0031); both use the same function
   `ferris_idea_languages::everywhere::search_everywhere`. [ADR-0008, ADR-0012]

9. **Look (v2):** the popup uses the theme's popup background and border (`Popup.background`,
   `Popup.borderColor`); rows span the whole width, symbols and files start with their icon, locations,
   folders and shortcuts are right-aligned and stay readable on the selection; group headers are
   emphasised. [GEN, OSS; hand test 2026-11-01: hard to read]
10. **Titles (v3):** an action whose title in the reference differs from its id as words is shown and
   matched by that title, with its menu group muted next to it: `LocalHistory.ShowHistory` is "Show
   History" in "Local History" and comes first for "Show History"; also "Run…", "Debug…", "Resume
   Program", "Add Selection for Next Occurrence", "Unselect Occurrence", "Select All Occurrences",
   "Problems". The id still matches. When a chosen action cannot run, the status names its title
   and shortcut ("Resume Program (⌥⌘R) is not available here"). [DOC: titles of the reference's
   menus; hand tests 6 and 8 of round 4]

**v5 (OBS-0023).** Matching, categories and ranking follow SPEC-0031 v4.
1b. **Tabs:** All, Classes, Files, Symbols, Actions, Text; Tab and ⇧Tab go round. `GotoClass` (⌘O)
   opens Classes, `GotoSymbol` (⌥⌘O) Symbols, `GotoFile` (⇧⌘O) Files. Single tabs have no group
   headings; the All tab keeps them (see deviations). [GEN, OSS; OBS-0023 L1]
2a. **Empty query:** the recently opened files without the one in the editor (All, Classes, Files);
   without results the popup shows only its head and field. [GEN; OBS-0023 L3, L17]
23. **Rows (v5; numbered 5 until v13):** symbols show their parameters and, for Java packages and Python modules, `of <container>`,
   with `file:line` on the right; text rows the line with the match marked and `file line`; actions and
   settings pages their group and shortcut. The selected row marks the typed characters with the find
   result background. [GEN; OBS-0023 L6, L12]
7. **Line:** `name:line[:column]` opens the chosen file or symbol at that place. [GEN; OBS-0023 L4]
8. **Field:** a framed box with a magnifier and a grey hint while empty; on the Text tab the toggles
   Cc, W and .* sit in the field. A query with line breaks grows the field. [GEN; OBS-0023 L17]
11. **Footer and right split:** under the list the selected file's path (`~` for the home folder), the
   action id, or a note for the calculator; ⇧Enter or "Open in Right Split" opens the file in the right
   editor group. [GEN; OBS-0023 L7]
12. **Preview:** a toggle in the head shows below the list the lines around the selected file, symbol
   or text hit with syntax colours, line numbers and the match marked; it stays on until turned off.
   [GEN; OBS-0023 L8]
13. **Non-project items:** on All, Classes, Files, Symbols and Text a checkbox "Include non-project
   items" also searches the files of excluded folders (`node_modules`, `target`, `__pycache__`, …;
   only `.git`, `.hg`, `.svn` and `.ferris-idea` stay out); pressing the tab's shortcut again while it is
   shown toggles it. Each opening starts with the project scope again (IMP-0333). (v6) Without it
   the files of All and Files are the project's own sources (SPEC-0031 v5 behaviour 12), the files
   open in editors and the recently opened ones; a line below the results says what was left out,
   `2 folders not listed (libraries and ignored files)`. In a remote project the host lists and
   counts them. [GEN; OBS-0023 L9; decision of the Project Owner of 2026-11-12]
14. **Filter:** a funnel opens a list with a checkbox per group of the All tab (Files, Symbols,
   Actions, Text, Calculator), and on the other tabs per file type of the hits; turned-off types stay
   off while the popup lives. [GEN; OBS-0023 L10]
15. **Calculator:** an arithmetic query shows `= value` first on All; Enter copies the value and the
   status bar says so. [GEN; OBS-0023 L14]
16. **Settings pages:** the pages of the Settings dialog (SPEC-0105, SPEC-0129) are found among the
   actions in the group "Settings"; Enter opens the dialog on that page. Only Ferris IDEA's own pages are
   offered, no options of languages or tools the project does not use (IMP-0332). [GEN; OBS-0023 L15]
17. **Find window:** on the Text tab a button shows the search in the Find in Files panel (SPEC-0066)
   with the same options, pinned so the results stay while matches are opened. [GEN; OBS-0023 L11]
18. **The popup's keys from the keymap (v7):** switching to the next tab (behaviour 1, Tab) is the
   keymap action `SearchEverywhere.NextTab`, to the previous tab (v5, ⇧Tab)
   `SearchEverywhere.PrevTab`, and choosing the selected result (behaviour 4, Enter)
   `SearchEverywhere.SelectItem`. While the popup has the focus, every key the keymap in use binds
   to one of them does that, with or without modifiers; with the bundled keymaps these are the
   keys as before. Settings › Keymap lists them as "Next Tab", "Previous Tab" and "Open Selected
   Item" (group Search Everywhere), and a key taken away there no longer does it. ⇧Enter (open in
   the right split, behaviour 11), Up, Down and Escape stay fixed keys. Chosen from the popup's
   own action list they act on nothing, since the popup closes first, and answer that they are
   not available here. [OSS (action ids and keys of the ported keymaps), GEN, SPEC-0028]
19. **Text tab by action, right split from the keymap (v8):** `TextSearchAction` (⌥⇧⌘E on macOS,
   Ctrl+Alt+Shift+E elsewhere; "Text…" in Find Action and in Navigate, SPEC-0124 v8) opens the popup on the Text tab, as
   `GotoClass`, `GotoFile` and `GotoSymbol` open theirs (behaviour 1); in an open popup it
   switches to that tab. Opening the selected result in the right split (behaviour 11) is the
   keymap action `OpenInRightSplit` (⇧Enter in the bundled keymaps), no longer a fixed key: every
   key the keymap in use binds to it does that in the popup, and ⇧Enter without it does nothing.
   Up, Down and Escape stay fixed keys. Outside the popup `OpenInRightSplit` belongs to the
   Project tree (SPEC-0275 v13 behaviour 36). [OSS (action ids and keys of the ported keymaps),
   GEN, SPEC-0028]

20. **Endpoints tab (v9):** a seventh tab, Endpoints, after Text, to jump to an HTTP endpoint by
   typing. Until v9 the endpoints could only be read in their tool window (SPEC-0103). [GEN]
   - **Opening:** `GotoUrlAction` ("Endpoint…" in Find Action, group Navigate, and in Navigate
     after Text…, SPEC-0124 v10) opens the popup on the tab, as `TextSearchAction` opens Text;
     in an open popup it switches to it. Tab and ⇧Tab reach it like every tab. The action has no
     shortcut: the bundled keymaps do not bind the id. [task of 2026-11-13; OSS for the keymaps]
   - **Rows from the tool window's model:** the rows are those of SPEC-0103 — the same scan of
     the project's own sources (SPEC-0103 behaviours 1 and 5 to 7), so every framework the tool
     window knows, a mapping with several methods as one row and the rows of client interfaces.
     The scan runs in the background each time the popup opens, as the tool window scans when
     it opens; typing searches the rows already read. The All tab does not list endpoints.
     [SPEC-0103]
   - **Method:** when the query starts with the name of an HTTP method (`GET`, `POST`, `PUT`,
     `PATCH`, `DELETE`, `HEAD`, `OPTIONS`, `TRACE`; upper or lower case) followed by a space,
     only rows of that method are kept: rows whose mapping names it, and rows whose mapping
     names no method and so takes every method. The rest of the query is the text. A method
     name without the space after it is text like any other. [GEN, DOC]
   - **Text:** matched with SPEC-0031's rule for names (v4 behaviour 1) against the row's path
     and against its handler. Case is ignored. Every fragment of the query starts at a word
     start, which in a path is the start of each segment and of each part after `-`, `_`, `.`,
     `{` or `:`; `*` lets a fragment start anywhere; a `/` in the query needs a `/` of the
     path, so `api/ord` finds `/api/orders/{id}` and `users/id` finds `/users/{id}`. When the
     text as a whole matches neither the path nor the handler and has several words, every
     word must match the path or the handler, in any order (`orders create` finds `POST
     /api/orders` handled by `OrderApi.create`). `name:line` is not read on this tab: a path
     may hold a colon and a number. [SPEC-0031]
   - **Ranking:** rows whose path matches the text come first, then the rows found by their
     handler (alone, or together with the path for a query of several words); within each by
     the score of SPEC-0031, then the shorter path, then path, methods, file and line in
     ascending order. With no text — an empty query or
     only the method — the rows stay in the tool window's order (path, then methods). At most
     50 rows, as on every single tab. [GEN]
   - **Rows:** the methods as the tool window shows them (`POST|PUT`, nothing for a mapping
     without a method), the path, the handler, the word "client" for a row of a client
     interface, and on the right `file:line`. The selected row marks the matched characters of
     path and handler (behaviour 23). The footer shows the file's path. [GEN]
   - **Choose:** Enter or a double click opens the file with the caret on the row's line — the
     declaration the tool window opens (SPEC-0103 behaviour 2); `OpenInRightSplit` opens it in
     the right split (behaviour 19); the preview (behaviour 12) shows the lines around it.
     [GEN]
   - **What the tab leaves out:** no "Include non-project items" (the endpoints are those of
     the project's own sources, SPEC-0103 behaviour 5); the funnel filters by the file types
     of the rows as on the other single tabs. A project without endpoints, and a query that
     finds none, show one line that says so. [GEN]
   - **MCP:** `endpoints` takes an optional `query` with the same method prefix, matching and
     ranking (one function for both, `ferris_idea_languages::endpoints::search_endpoints`), without
     the limit of 50; without `query` it answers as before. [ADR-0008, ADR-0012]
21. **v10 – Focus back where it was (window check 2026-11-14):** when the popup closes, or runs an
    action from its Actions tab, the focus goes back to the tool window that had it when the popup
    opened, if that window is still shown, and the action acts on it: Stretch to Top from the popup
    changes that window, and an editor action is not run in the editor then. With no such tool window the focus goes to the editor as before. Until v10 the focus
    always went to the editor. [GEN]
    **v12 (second window check 2026-11-14):** the tool window that "had the focus" is the shown
    window in whose content the focus is, wherever in it: its tree, a list, a field or a button —
    not only the element the window focuses when it is activated. The focus goes back to that
    very element (the list of changes of the Commit window, a row's tree, the question field of AI
    Chat), and to the element the window focuses when activated only if the element is gone.
    Until v12 the Commit window counted as focused only while its message field had the focus, and
    AI Chat only in its question field: with the caret in the list of changes Find Action ran the
    action without a tool window, and the window's activation shortcut focused the message field
    instead of hiding the window. The same rule decides everywhere which tool window is the
    active one (the stretch and maximise actions, Hide Active Tool Window, the activation
    shortcuts of SPEC-0167 behaviour 11). [GEN]
22. **From group to group (v11):** `SearchEverywhere.NavigateToNextGroup` (Page Down and ⌘↓ in
    the bundled macOS keymap, Page Down and Ctrl+Down in the default one) selects the first
    result below the next heading of the list; in the last group it selects the last result.
    `SearchEverywhere.NavigateToPrevGroup` (Page Up, ⌘↑) selects the first result of the group
    the selection is in, and from there the first result of the group before; in the first
    group it stays on the first result. A list without headings (every tab but All) is one
    group: the actions go to its last and first result. Neither wraps around, unlike ↑ and ↓
    of behaviour 3. The keys are the keymap's, as those of behaviour 18; Find Action lists the
    two as "Next Group Element" and "Prev Group Element". [OSS: ids, titles and keys of the
    ported keymaps and `ActionsBundle.properties`; that the actions move the selection to
    another group of the list is from `SearchEverywhereUI.java`, the stepping rule itself is
    Ferris IDEA's, GEN]

## Acceptance criteria
- **AK-1** Rows grouped with empty groups hidden, selection skipping headings and wrapping, tab
  categories and limits (unit tests); search with cancellation stops early (unit test).
- **AK-2** In a real window (`ferris-idea ui --selftest`): Go to File and typing select `new.txt`, Enter opens
  it and closes the popup; Go to Action finds `CloseContent` and Enter runs it.
- **AK-3** Use by hand: double Shift, tabs, symbols with line, mouse.
- **AK-4 (v3)** "Show History" finds the local history first with either keymap (unit test
  `everywhere::tests::show_history_is_found_by_its_title`); status text of an unavailable action
  (unit test `menu_bar::tests::run_and_resume_keys_on_macos`).
- **AK-5 (v4)** Two taps within 400 ms open, a slow second tap does not, the Shift left from ⌘⇧← and
  that of capital letters are no taps (unit test
  `everywhere_model::tests::double_shift_counts_only_taps_alone`); OBS-0021 Nachprüfung in the window.

- **AK-6 (v5)** Six tabs and ⇧Tab, no headings on single tabs, calculator first, hidden file types
  (unit tests `everywhere_model::tests::filters`, `rows_skip_empty_groups_and_selection_wraps`);
  matching and categories per SPEC-0031 AK-7; the preview's lines and colours (unit test
  `code_preview::tests::excerpt_lines_spans_and_mark`); in the window (`--selftest`): Classes tab,
  `name:line`, empty query lists the recent file, preview lines, non-project toggle; OBS-0023
  Nachprüfung.
- **AK-7 (v6)** The files of behaviour 13 and the line on what was left out as SPEC-0031 AK-12; in the
  window by hand: a project with `.venv`, Files tab, the line, the checkbox lists the `.venv` file,
  a file opened from `.venv` is found again. Open (hand test).
- **AK-8 (v7, behaviour 18)** The popup's keys by the actions bound to the pressed key: Tab, ⇧Tab
  and Enter with the bundled macOS and default keymaps, a rebound key with a modifier, ⇧Enter, Up,
  Down and Escape as fixed keys, and an unbound key (unit test
  `everywhere_model::tests::popup_keys_follow_the_keymap`); the titles
  (`everywhere::tests::second_package_actions_have_titles`).
- **AK-9 (v8, behaviour 19)** ⇧Enter opens in the right split with both bundled keymaps, a key
  rebound to `OpenInRightSplit` does it, and ⇧Enter bound to something else does not (unit test
  `everywhere_model::tests::popup_keys_follow_the_keymap`); in a window of the test harness
  `TextSearchAction` opens the popup on the Text tab
  (`tree_and_popup_actions_keymap.rs::text_search_opens_the_text_tab`); the title
  (`everywhere::tests::fourth_package_actions_have_titles`).

- **AK-10 (v9, behaviour 20)** Matching and ranking on the rows of the model: path segments in
  any case, `/` in the query, a fragment inside a word only after `*`, the handler, several words
  over path and handler, a path match before a handler match and the shorter path first; the
  method prefix in either case with the rows without a method, a method name without a space as
  text, only the method and the empty query in the model's order; a project without endpoints;
  600 endpoints with the limit of 50 and the limit lifted (unit tests
  `endpoints::tests::{search_matches_path_segments_and_handlers, search_ranks_paths_before_handlers,
  search_narrows_by_a_leading_method, search_of_an_empty_project_and_of_many_endpoints}`). The
  tab's place, label, limit and rows without headings, the selection over them and the hidden
  file types (unit test `everywhere_model::tests::endpoints_tab_rows`). In a window of the test
  harness `GotoUrlAction` opens the popup on the Endpoints tab, typing selects the row and
  choosing it opens the handler's file with the caret on the mapping's line
  (`endpoint_search.rs::goto_url_opens_the_endpoints_tab_and_enter_opens_the_handler`). The
  title (`everywhere::tests::endpoint_search_has_its_title`), the menu item
  (`menus::tests::endpoint_search_is_in_navigate`), the declared effect
  (`security::tests::endpoint_search_declares_no_effect`). MCP: `query` with a path, a method
  prefix and a handler, and the list unchanged without it
  (`crates/mcp/tests/it/protocol.rs`: `endpoints_query`). On screen by hand: open (hand test).
- **AK-11 (v10, behaviour 21)** `everywhere_view::focus_back_tests`: a shown tool window gets the
  focus back, a hidden one or none leaves it to the editor. By hand: focus the Terminal, ⇧⌘A,
  Stretch to Top — the Terminal grows and keeps the focus.
- **AK-12 (v11, behaviour 22)** On a list with three groups: down from the middle of the first
  group to the first result of the second, from the last group to the last result; up from
  the middle of a group to its first result, from there to the first result of the group
  before, and no further than the first result; a list without headings as one group
  (`everywhere_model::tests::group_steps_go_to_the_first_result_of_a_group`). The popup takes
  the keys of the two actions from the keymap (`everywhere_model::tests`, the `popup_key`
  cases for Page Down and ⌘↑). In a window of the test harness the actions move the selection
  of the open popup and do nothing without one
  (`sixth_package_keymap.rs::search_everywhere_steps_by_group`). Not compared next to the
  reference.
- **AK-13 (v12, behaviour 21)** In a window of the test harness
  (`window_check_1114b.rs::find_action_acts_on_the_tool_window_that_had_the_focus`): with the focus
  in the Project tree, the Database tree, the list of changes of the Commit window and the Problems
  list, Find Action's Maximize Tool Window maximises that window, the window holds the focus
  again and the focused element is the one that had it; closing the popup without an action
  gives the focus back the same way.

## Out of scope
Jumping to the next or previous group of results and completing a command with Tab
(`SearchEverywhere.NavigateToNextGroup`, `SearchEverywhere.NavigateToPrevGroup`,
`SearchEverywhere.CompleteCommand`: the popup has neither operation).
Usage-based ranking beyond recent files, symbols from language servers, a persistent index for very
large workspaces, the reference's `/` commands (shortcuts to features of its plugins; OBS-0023 L18),
an "include disabled actions" switch (Ferris IDEA lists every action and says when one cannot run,
v3), descriptions of actions in the footer and assigning a shortcut from the list (Keymap settings,
SPEC-0105). Endpoints (v9): endpoints on the All tab, matching the query against a full URL with
host and port or against a path with values in place of its variables (`/users/42` for
`/users/{id}`), a shortcut for `GotoUrlAction` in the bundled keymaps, generating a request from
the popup (the tool window does that, SPEC-0103 behaviour 3), and everything SPEC-0103 leaves out
of its rows.

## Known deviations from the reference
Symbols are found by the grammars' tags queries (SPEC-0031), not by an index of the language; ranking
weights are our own. The All tab keeps group headings (Files, Symbols, Actions, Text) where the
reference shows one mixed list: the kind of each hit stays visible (OBS-0023 L19, equivalent). The
right column shows `file:line` instead of the project name (one project per window). Non-project
items are the excluded folders, not libraries and the JDK (SPEC-0031 v4). "Open in Find Window" is
offered on the Text tab: the other tabs list navigation targets that the popup itself opens. The
scope resets on each opening (IMP-0333); only own settings pages are offered (IMP-0332).
The Endpoints tab (v9) was not compared with the reference: its name, its place among the tabs,
the title "Endpoint…", the rows and the ranking are Ferris IDEA's own, and the action has no
shortcut. In a remote project the tab lists what the Endpoints tool window lists there.
