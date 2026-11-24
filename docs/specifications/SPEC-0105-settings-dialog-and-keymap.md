# SPEC-0105: Settings dialog and keymap settings

- **Version:** 12 · **Date:** 2026-11-23 (v12: the keymap page marks a shortcut that macOS takes
  itself, behaviour 18, AK-20; OBS-0119 K5, IMP-0544); 2026-11-19 (v11: the options of every page switch by a click on their label as on their box and select their row, the page then takes Space, Enter and ⌫, the search only the keys it has; a search never starts with a space; behaviour 17, window check of 2026-11-18 G1; v10: a page's note goes when another page is shown; on the keymap page a click on a row or Tab from the search gives the keys to the list, so ⌫ removes and Escape goes back to the search; behaviours 15, 16, window check of 2026-11-18 S8 and K1; v9: each page says whether its settings belong to the IDE or to the project and where they are kept, project pages are marked in the list; File › Manage IDE Settings imports and exports a settings file; behaviours 13, 14; v8: Select All, Copy, Cut, Paste, Undo and Redo act on
  the dialog's focused field, also when the menu bar takes the key; the page list rolls, with a
  scroll bar, keeps the page shown in view and takes ↑/↓ after a click; the dialog takes the
  window's height and a page taller than its area rolls; behaviours 10–12, OBS-0062 S6, S7; v7: a visible search field and the found label marked on the page, Revert changes and Restore defaults per page, a dot after changed pages, behaviours 7–9, OBS-0062 S1–S3; v6: the page list has the reference's outline with the group Appearance & Behavior first and Advanced Settings last, SPEC-0337 behaviour 1, OBS-0048; v5: the search finds a page by the label of every setting on it; v4: ⇧⌫ restores on a chosen keymap row also with a search typed, changes apply without OK/Apply; v3: the search takes committed text through the input handler; v2: typing goes on in the search, ⌫ on Mac keyboards, action titles; hand test 30)
- **Sources:** GEN (common settings dialogs of desktop IDEs: search field, page list, page);
  OSS (`JetBrains/intellij-community`: the user keymap format of `KeymapImpl` — a keymap file names its
  parent and lists only the changed actions, an action listed without shortcuts has none; the
  `ShowSettings` and keymap action ids of the bundled keymaps), SPEC-0028, SPEC-0060, ADR-0008; v7:
  OBS (OBS-0062: the reference's search field above the page tree, the matching control framed on
  the page, Revert changes and Restore defaults on a changed page, a dot after pages with changed
  settings); v8: GEN (text fields take Select All and the clipboard keys, lists that do not fit
  roll), OBS (OBS-0062 S6, S7: Ferris IDEA's own search ignored ⌘A, and in a window 901 points high
  its page list was cut below Tools › AI Assistant); v9: GEN (IDE-wide and project-wide settings,
  importing and exporting settings), OSS (`JetBrains/intellij-community`: the action ids
  `ImportSettings` and `ExportSettings`), SPEC-0303 (the `settings` exchange file), SPEC-0315
  (the project's inspection profile), SPEC-0150 (EditorConfig before Code Style); v10: GEN (a
  status line belongs to the page it speaks about; a list chosen with a click or Tab takes the
  keys of its rows), window check of 2026-11-18 (S8: "Font: defaults restored" still shown on
  Tools › Terminal; K1: with "Reformat" typed, a click on a keymap row left ⌫ shortening the
  search and Escape closing the dialog); v11: GEN (a check box's label belongs to it: a click on
  either switches it, and a focused option takes Space), window check of 2026-11-18 (G1: on Editor ›
  General a click on an option's label switched nothing, and Space or Enter after it went to the
  empty search instead of switching), ADR-0025 and SPEC-0319 (options that weaken a protection are
  the user's own)
- **Feature matrix rows:** F-002, F-017

## Purpose
The user opens Settings, switches the theme and changes keyboard shortcuts; the changed shortcuts
apply at once in every window and are kept in the reference's keymap format.

## Behaviour
1. **Opening:** `ShowSettings` (⌘, on macOS, Ctrl+Alt+S elsewhere) opens a dialog with a search
   field, a page list (`Appearance`, `Keymap`) and the selected page; Escape closes it. [GEN, OSS]
2. **Pages:** clicking a page or Tab shows it. (v6) The page list follows the outline of
   SPEC-0337 behaviour 1: Appearance stands under the group Appearance & Behavior with System
   Settings, Notifications and Trusted Locations, Tab goes through the pages in the list's order,
   and Advanced Settings is the last page. [OBS-0048] `Appearance` lists the bundled themes with the chosen
   one marked; Enter or a click switches as Quick Switch Scheme does (SPEC-0060 v5 behaviour 10).
   [GEN]
3. **Search:** typing goes into the search; the page list keeps the pages whose name or keywords
   match, and the keymap page always stays because it searches the actions. The keymap list keeps the
   actions whose id or shortcut text contains the search (case, spaces, dots and underscores
   ignored). (v2) While the search holds text, every printable key, Space included, goes on with
   it, also on pages where Space would switch a setting. [GEN]
3a. **v3 – Text, not keys:** the search receives committed text through the platform's text input, as
   every other input does, so dead keys, input methods and synthetic input with a Unicode string type
   what they mean. Keys that act on the list keep their meaning. [GEN]
3b. **v5 – Labels:** the page list also keeps a page when the search is part of the label of a
   setting the page shows, whatever the case: the texts beside its switches, values and fields
   ("Sync with OS", "Line spacing", "Hard wrap at", "Protected branches", "Shell path"), its group
   headings, and the rows that come from a list (theme names, the Editor Tabs rows, the Code Style
   languages). The labels the search knows are the texts the page draws. Until v4 a page was found
   by its name and a hand-written word list only, so "sync", "compact" or "ligatures" found
   nothing. [GEN]
4. **Keymap page:** every action of the keymap and the actions Ferris IDEA runs without a bundled
   shortcut, sorted by id, with their shortcuts as shown on this platform. (v2) Rows and notes show
   the action's title as Find Action does (`Save All`, not `SaveAll`), and the search also matches
   the title. Enter waits for a
   keystroke (Escape cancels) and adds it to the selected action; a note names other actions that
   use the same keystroke. Delete removes the action's shortcuts; Shift+Delete restores the bundled
   ones. (v2) With the search empty, ⌫ and ⇧⌫ act as Delete and Shift+Delete, since Mac
   keyboards have no Delete key without fn; this holds for every list of the dialog. (v4) On the
   keymap page, once a row was chosen with a click, ↑/↓ or Enter and until the search text changes,
   ⇧⌫ restores also while the search holds text (hand test 30 types "save all" first and then
   restores). ⌫ alone keeps editing the search, so that correcting the search never removes
   shortcuts; removing with a search typed takes forward Delete (fn⌫ on Mac keyboards). [GEN]
4a. **v4 – No confirmation buttons:** every change in the dialog applies and is saved at once;
   there is no OK, Apply or Cancel, and Escape closes the dialog without undoing anything. A
   changed shortcut is undone with Shift+Delete, a setting by setting it back. [GEN]
5. **User keymap:** changes are saved to `keymaps/FerrisIDEA.xml` in the user's settings folder in the
   reference's format: a keymap `Ferris IDEA` whose parent is the platform's bundled keymap, with only
   the changed actions. Every window uses the bundled keymap with these changes from start-up on and
   right after a change, also for Search Everywhere. [OSS]
6. **MCP:** `set_shortcut` gives an action exactly the given shortcuts, removes them (empty list) or
   restores the bundled ones (`reset`), and reports actions that share a first keystroke;
   `keymap_lookup` with `keymap: user` and `search_everywhere` use the user's keymap. [ADR-0008]

7. **Search field (v7, OBS-0062 S1):** the search stands as a field above the page list, with a
   placeholder "Search settings" while it is empty and the text typed (behaviour 3a) otherwise; a
   click on it puts the focus back into the dialog's search. On the page shown, every label of a
   setting that contains the search text (case ignored) is framed in the accent colour; the frame
   goes with the search. The field and each framed label are nodes of the semantic tree
   (`settings-search`, `settings-hit`). Until v6 the search text stood in the dialog's title bar
   and nothing on the page was marked. [OBS]
8. **Revert changes and Restore defaults (v7, OBS-0062 S2):** a bar above the page names it and,
   for a page with settings of its own (Appearance, System Settings, Notifications, Editor ›
   General, Font, Editor Tabs, Code Style, Natural Languages, Version Control, Terminal, AI
   Assistant, Advanced Settings), offers "Revert changes" while the page's settings differ from
   what they were when the dialog opened, and "Restore defaults" while they differ from the
   defaults. Each acts on that page's settings only, applies and saves at once (behaviour 4a) and
   puts the page's fields back to the values; the other pages keep theirs. Keymap, Plugins,
   Inspections, Build Tools, Languages and Trusted Locations keep their own ways (restoring an
   action's shortcuts, the inspection profile). [OBS, GEN]
9. **Changed pages (v7, OBS-0062 S3):** the page list puts a dot after each page that holds a
   setting changed from its default, for the pages of behaviour 8; the dot is a node of the
   semantic tree (`settings-page-modified-<title>`). [OBS]
10. **Edit actions in the fields (v8, OBS-0062 S6):** Select All, Copy, Cut, Paste, Undo and Redo
    act on the field of the dialog that has the focus, whether they come from their keys (as the
    keymap binds them: ⌘A, ⌘C, ⌘X, ⌘V, ⌘Z, ⇧⌘Z on macOS) or from the Edit menu of the menu bar,
    which on macOS takes these keys first. The fields are the search, the Size and Line spacing
    fields and the preview of Font, the shell path, the protected branches, the fields of AI
    Assistant and the search and value field of Advanced Settings; a password field keeps copy
    and cut refused (SPEC-0108). While Settings is open these actions never reach the editor
    behind it, and the status bar does not call them unavailable. In the search, Select All
    marks its whole text, drawn in the editor's selection colour; typed or pasted text then
    replaces it, ⌫ or ⌦ empty the search, Cut empties it onto the clipboard, Copy copies it, and
    any other key or a click on the field ends the mark. Without a mark, Paste adds the first line
    of the clipboard at the end and Copy copies nothing. Undo and Redo change nothing in the
    search. The search field's node in the semantic tree is selected while its text is marked.
    Until v7 ⌘A in the search went on to the menu bar, which found no Select All there, and the
    search could only be emptied character by character. [OBS, GEN]
11. **The page list rolls (v8, OBS-0062 S7):** when the pages do not fit, the list rolls below the
    search field, which stays where it is: with the mouse wheel or the trackpad and with a scroll
    bar at its right edge, whose thumb is the part shown; dragging the thumb rolls with the
    pointer, a click beside it rolls by the list's height. Whenever the page shown changes (a
    click, Tab, the search, the keys below), the list rolls just far enough to show that page's
    row. A click on a page in the list gives the list the keys: ↑ and ↓ go to the previous and
    the next page and show it (stopping at the first and the last), and the page's row has the
    colour of a focused selection, otherwise that of a selection without the focus. Enter, → or
    a click on the page give the keys back to the page; Escape, Tab and typing keep their
    meaning; the other keys of the page's rows (Space, ←, −, +, Delete) do nothing while the
    list has the keys. Until v7 the list was cut at the dialog's lower edge and ↑/↓ moved only
    the page's rows, so Advanced Settings could only be reached by the search. [OBS, GEN]
12. **Size and page area (v8, OBS-0062 S7):** the dialog takes its size from the window: as wide
    as the window less 40 points on each side, between 560 and 1000 points, and as high as the
    room from its top (40 points below the toolbar) to 40 points above the window's lower edge,
    between 320 and 900 points. Until v7 it had a fixed 760 × 520 points and used about two
    thirds of the height of a window 901 points high. A page taller than the area below its
    header rolls as a whole, with the wheel and a scroll bar as in behaviour 11, and starts at
    its top each time it is shown; Keymap and Inspections roll their own lists between their
    fixed lines. The scroll bars are nodes of the semantic tree (`settings-pages-scrollbar`,
    `settings-page-scrollbar`). [OBS, GEN]

13. **Where the settings are kept (v9):** the bar above a page (behaviour 8) says, after the
    page's title, where its settings belong, in the muted text colour:
    - "IDE · settings.toml, the same for every project" for the pages of behaviour 8 except
      Code Style; Code Style adds "; a project's .editorconfig comes first" (SPEC-0150);
    - "IDE · keymaps/FerrisIDEA.toml" (Keymap), "IDE · plugins folder" (Plugins), "IDE ·
      trusted-projects.toml" (Trusted Locations);
    - "Project · .idea/inspectionProfiles/Project_Default.xml, shared with the project's files"
      (Inspections, SPEC-0315);
    - "Read from this project and this machine; nothing is stored here" (Build Tools,
      Languages & Frameworks, which only show what Ferris IDEA finds).
    The page list puts a muted "project" after each page whose settings belong to the project.
    The text and the marks are nodes of the semantic tree (`settings-page-scope`,
    `settings-page-project-<title>`). Until v8 nothing in the dialog said which settings applied
    to every project and which went with the project. [GEN]
14. **Import and export (v9):** File › Manage IDE Settings holds "Import Settings…"
    (`ImportSettings`) and "Export Settings…" (`ExportSettings`); Find Action finds both under
    that group. Export Settings asks in the system's save dialog, starting in the project's
    folder with the name `settings.idcsettings`, and writes the `settings` exchange file of
    SPEC-0303 behaviour 13 (Editor, Appearance with the theme, Version Control and Code Style,
    and the user keymap's changes; never the AI Assistant, the terminal, MCP or other private
    settings, never a secret); the status bar names the file written and the sections left
    out. Import Settings asks in the system's open dialog for a file and shows its import
    preview (SPEC-0303 behaviour 11): nothing changes before the user confirms there, and what
    was imported applies at once in every window. A file that cannot be written or read is
    named with the reason in the status bar and nothing changes. Both are the user's own:
    UI automation by an external client cannot run them (SPEC-0319, undeclared effects). [OSS,
    GEN, SPEC-0303]
15. **v10 – A page's note:** the status line under a page ("Font: defaults restored", "Editor
    reverted", the result of applying a field) belongs to that page. Showing another page — by a
    click in the list, Tab, ↑/↓ on the list or the search moving to another page — removes it, and
    it does not come back when the first page is shown again. Until v9 the note of Restore
    defaults on Editor › Font stayed under the Shell path field of Tools › Terminal. [GEN]
16. **v10 – Keys of the keymap list:** on the keymap page a click on a row, or Tab while the
    search holds text, gives the keys to the list: ⌫ and Delete remove the selected action's
    shortcuts, ⇧⌫ and Shift+Delete restore the bundled ones, ↑/↓ move through the rows and Enter
    records a shortcut; the search field loses its accent frame meanwhile. Escape gives the keys
    back to the search instead of closing the dialog; a second Escape closes it. Typing goes into
    the search again and, as it changes the search, gives the keys back to the search; so does a
    click on the search field or showing another page. ↑/↓ from the search leave the keys with the
    search, so that ⌫ still corrects the search there (behaviour 4, v4). A chosen row whose
    shortcuts were removed stays listed and chosen, so that ⇧⌫ restores the same action. Until v9 a click on a
    filtered row left ⌫ shortening the search, the hint "Delete removes · Shift+Delete restores"
    could not be followed with ⌫, and Escape closed the whole dialog. [GEN]
17. **v11 – Options of a page:** every option of a settings page — a check box or a choice of two
    shown by its value (Appearance, System Settings, Notifications, Editor › General, Editor Tabs,
    the ligatures of Font, Code Style, Version Control, AI Assistant, the switches of Advanced
    Settings) — is one row of the same kind: the box (or the choice's mark) and the option's label.
    A click anywhere on the row, on the label as on the box, switches the option, selects its row
    and gives the keys to the page, as a click on a keymap row does (behaviour 16): Space and Enter
    then switch the selected option, ↑/↓ move the selection, and ⌫ never shortens the search. The
    search keeps its text and loses its accent frame meanwhile; it takes the keys again by a click
    on it, by typed text other than a space, or by showing another page. With a search typed,
    Escape gives the keys back to the search; with the search empty Escape closes the dialog as
    before. The search never starts with a space: Space on a row that switches nothing (a theme)
    types nothing. Each option is a check box of the semantic tree (`settings-<page>-<n>`, checked
    and selected as shown, its label `settings-<page>-<n>-label`); an option that weakens a
    protection (a classical SSH key exchange or an own ProxyCommand, ADR-0025; MCP over HTTP;
    agents operating the window, SPEC-0319) declares the effect reserved for the user, so UI
    automation cannot switch it. Until v10 only a click on the box switched on Editor › General,
    and Space or Enter after a click on the label went to the empty search, so the page's hint
    "Enter or Space switches the selected option" could not be followed. [GEN]
18. **v12 – Chords macOS takes:** in the macOS keymap a row whose shortcut macOS takes for itself
    before any application sees it shows after the shortcut, dimmed, which chord and what macOS
    takes it for ("⌃Space is taken by macOS (the previous input source)"); recording such a chord
    adds it all the same and the note says it will not arrive. The chords are macOS's own default
    ones: ⌃Space and ⌃⌥Space (input sources), ⌘Space (Spotlight), ⌥⌘Space (Finder search), ⌘Tab,
    ⇧⌘3/4/5 (screenshots), ⌃↑, ⌃↓, ⌃← and ⌃→ (Mission Control and spaces). The bundled macOS
    keymap puts basic completion on ⌃Space, so its row is marked, and the user can give it a free
    chord there. Whether the user switched a system shortcut off is not read (it would need the
    system's settings); the mark says what macOS does by default. The default keymap marks
    nothing. The reference says only in a general notification that some shortcuts conflict with
    macOS (IMP-0544). [OBS-0119 K5, GEN]

## Acceptance criteria
- **AK-1** Keystrokes and user keymaps round-trip through text and XML, with keyboard, two-stroke,
  mouse and removed entries; a user keymap overrides and removes bundled shortcuts (unit test).
- **AK-2** The search keeps the right pages and finds actions by id and by shortcut text, with
  actions that have no shortcut (unit test).
- **AK-3** `set_shortcut` sets, removes and restores shortcuts in the user keymap file, rejects
  invalid keystrokes and calls without `shortcuts` or `reset`, and `keymap_lookup` with `user` sees
  the change (protocol test).
- **AK-4** Window self-test: the shortcut opens Settings on Appearance; Tab and typing find Save All
  on the keymap page; Enter and a keystroke add the shortcut, which the keymap uses at once and the
  file keeps; Shift+Delete restores the bundled shortcuts; Escape closes.
- **AK-5** Hand test (round 4): the dialog's look next to the reference, and a recorded shortcut
  triggering its action in the editor.

- **AK-6 (v3)** `settings_view` unit test `committed_text_goes_into_the_search`; in the window:
  synthetic input of "external" shows that word.

- **AK-7 (v4)** `settings_view` unit test `search_typing_and_mac_delete`: ⇧⌫ on a chosen keymap row
  counts as Shift+Delete with a search typed, ⌫ alone and other keys do not. Window self-test:
  after recording with "saveall" typed, ⇧⌫ restores the bundled shortcuts and the search stays.

- **AK-8 (v5, v6)** Every page the specs list exists under its group (v6: Appearance under
  Appearance & Behavior), and the search finds each page by
  every label of a setting on it, in any case and by a word of it (`settings_model::tests::
  every_setting_is_found_by_its_label`); the labels the search knows are string literals of the view
  (`settings_view::tests::the_searched_labels_are_the_drawn_ones`). Window self-test
  (`settings_steps`): "sync" leaves Appearance and Keymap in the page list, the emptied search
  lists every page again; (v6) Tab from Appearance goes through the pages of its group to Keymap.

- **AK-11 (v7)** Unit test `settings_model::page_settings_tests::revert_restore_and_marks_act_on_one_page`:
  a page's copy reverts it and leaves the other pages; a page without settings of its own has no
  copy; the dot follows the defaults; labels match the search text in any case, none for an empty
  one.
- **AK-12 (v7)** In a window of the test harness, by typed text and by clicks on controls found
  through the semantic tree (`crates/workbench/tests/settings_window_checks.rs`:
  `search_revert_defaults_font_and_advanced`): the search field shows "spacing" as it is typed,
  the Font page comes up with "Line spacing" framed, the mark goes with the search; two clicks on
  + offer Revert changes and Restore defaults and put the dot after Font; Revert changes brings
  the size back and saves it and leaves nothing to revert; Restore defaults takes a typed size
  back and the dot goes.

- **AK-13 (v8)** Unit tests: the search's edits (`settings_view::edit_actions::tests::
  the_search_takes_select_all_paste_and_cut`): Select All marks a text and nothing in an empty
  search, typed and pasted text replace the mark, Cut empties onto the clipboard, Copy copies
  only a mark, Undo changes nothing, other ids are not edit actions; the dialog's size
  (`settings_model::tests::the_dialog_takes_the_windows_room`): 1300 × 901 gives more than 85 %
  of the height below the dialog's top, the limits hold for very small and very large windows;
  ↑/↓ on the list (`settings_model::tests::arrows_on_the_list_go_through_the_pages`) stop at the
  ends and follow the search's list; the scroll bar
  (`scroll_bar::tests::the_thumb_shows_the_part_shown_and_follows_a_drag`).
- **AK-14 (v8)** In a window of the test harness, by real keys, the wheel, a drag and clicks on
  controls found through the semantic tree (`crates/workbench/tests/settings_window_checks.rs`:
  `select_all_and_rolling_lists`): ⌘A marks the search "external", ⌫ empties it and every page is
  listed again; text typed after ⌘A replaces the mark; Select All from the menu bar (the
  workbench's menu action) marks the search too and leaves no "not available" in the status
  bar, and in the Size field selects its text, so "20" and Enter set 20 while the editor behind
  keeps its caret without a selection. In a window of 1300 × 901 points Advanced Settings is in
  view without rolling; in one of 1300 × 460 it is not, the wheel over the list brings it into
  view, dragging the list's thumb back to the top brings Appearance back; a click on Appearance
  and ↓ go page by page to Advanced Settings, which is then in view; on Font the preview, cut at
  first, is in view after the wheel over the page.
- **AK-15 (v9)** Unit tests: `settings_model::tests::every_page_says_where_its_settings_are_kept`:
  every page has a scope, only Inspections belongs to the project, the pages of behaviour 8
  name `settings.toml` and Code Style names `.editorconfig`; `menus::tests::
  manage_ide_settings_imports_and_exports` and `everywhere::tests::
  reference_comparison_items_have_titles_and_groups`: the submenu, its two items and their
  Find Action titles.
- **AK-16 (v9)** In a window of the test harness with a settings folder and a project of its own
  (`crates/workbench/tests/it/settings_scope_and_exchange.rs`:
  `pages_name_their_scope_and_settings_go_out_and_come_back`): the scope text of Font and of
  Inspections in the semantic tree, the "project" mark after Inspections only; Export Settings
  from the menu action, with the path chosen in the save dialog that starts in the project's
  folder, writes a `settings` file holding the changed protected branches and not the terminal's
  shell, and the status bar names the file and the terminal as left out; after the branches are
  changed again, the import preview of that file changes nothing until Import is confirmed with
  Enter, which brings the exported branches back into the settings file and the window and leaves
  the shell alone; a file that cannot be written leaves a reason in the status bar.
- **AK-17 (v10)** In a window of the test harness (`crates/workbench/tests/settings_window_checks.rs`:
  `search_revert_defaults_font_and_advanced`): after Restore defaults on Font the note says
  "Font: defaults restored"; a click on Terminal in the page list leaves no note, and neither does
  a click back on Font.
- **AK-18 (v10)** In a window of the test harness with a keymap file of its own, by real keys and
  clicks on controls found through the semantic tree (`crates/workbench/tests/settings_window_checks.rs`:
  `keymap_rows_take_the_keys_after_a_click_or_tab`): with "reformat" typed on the keymap page, a
  click on a row with shortcuts (`settings-keymap-row-<n>`) and ⌫ remove that action's shortcuts
  and leave the search as typed; the row stays chosen without shortcuts and ⇧⌫ restores them;
  ↓ and ↑ move the selection; Escape keeps the dialog open and gives the keys back to the search,
  where ⌫ takes a letter off; Tab from the search gives the keys to the list again and ⌫ removes
  the chosen action's shortcuts, ⇧⌫ restores them; then Escape goes back to the search and a
  second Escape closes the dialog.

- **AK-19 (v11)** Unit tests (`settings_view::options::tests`): Space typed after a click on an
  option is no search text, also with a search typed; the options that weaken a protection are the
  user's; the marks of the box. In a window of the test harness with a settings file and a project
  of its own, an editor open behind the dialog, by real keys and clicks on controls found through
  the semantic tree (`crates/workbench/tests/settings_window_checks.rs`:
  `options_switch_by_a_click_on_the_label_and_then_by_keys`): on Editor › General a click on the
  label of "Remove trailing spaces on save" switches it, selects its row (checked and selected in
  the tree), Space and Enter switch it again and the search stays empty, the value is in the
  settings file; a click on another label moves the selection and Space switches only that one;
  on System Settings a click on a label and Space switch "Reopen the last project"; with
  "trailing" typed, a click on the label and Space switch and the search stays "trailing", Escape
  keeps the dialog and ⌫ shortens the search again; the editor behind keeps its text.

- **AK-20 (v12, behaviour 18)** Unit test (`settings_model::tests::chords_macos_takes_are_marked`):
  ⌃Space and ⌘Space are known with what for, ⌃⇧Space and ⇧⌘Space are not; the note names the
  first taken shortcut; in the bundled macOS keymap the row of basic completion is marked and that
  of Insert Live Template is not; the default keymap marks nothing. The mark on the page and the
  recording note: to be seen in the window.

## Out of scope
Further pages (editor font, code style, version control, plugins …), (v9) project-level
values of the IDE's own settings (a project overriding the font, the code style or the version
control settings in a shared file; the project's code style is shared through `.editorconfig`),
"Restore Default Settings…" for every page at once and the reference's settings synchronisation,
choosing among several keymaps, mouse and two-stroke recording in the dialog (two-strokes are possible
through `set_shortcut`), the reference's shortcut-conflict dialog, importing keymaps of other IDEs.

## Known deviations from the reference
(v4) The reference collects changes until OK or Apply and drops them on Cancel; Ferris IDEA applies
each change at once and has no such buttons (behaviour 4a). (v7) Revert changes (behaviour 8)
takes back a page's changes since the dialog opened, as Cancel would for that page; there is no
Cancel for all pages at once, and the reference's preview of a change before Apply is the change
itself in Ferris IDEA. (v8) The reference's Settings is a window of its own; Ferris IDEA's dialog lies
over the IDE window and takes its size from it (behaviour 12). (v9) Export Settings writes one
text file of Ferris IDEA's exchange format with the shared sections only (behaviour 14), without a
choice of parts and without private settings or secrets; Import Settings always lists each
change before anything is written (SPEC-0303). Settings shows two pages for now. Recording adds a one-keystroke shortcut directly and reports
conflicts in a note instead of a dialog that asks whether to remove the other assignments. A keymap
change applies without first copying the bundled keymap under a new name; the copy is always called
`Ferris IDEA`. Observed in OBS-0008: the reference shows the actions as a tree of groups with display
names and keycaps, offers a find-by-shortcut popup, shows conflicts already while recording and then
asks Cancel / Keep / Remove; it opens Settings on the page shown last. Ferris IDEA lists action ids
flat and sorted; the tree with display names is open.

**Window check 2026-11-09** (`main` @ 3b3cc1e0, agent run with real input, OBS-0028): the Settings search takes synthetic text ("external" filtered Keymap).
