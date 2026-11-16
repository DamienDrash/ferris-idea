# SPEC-0105: Settings dialog and keymap settings

- **Version:** 8 · **Date:** 2026-11-16 (v8: Select All, Copy, Cut, Paste, Undo and Redo act on
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
  roll), OBS (OBS-0062 S6, S7: IDECraft's own search ignored ⌘A, and in a window 901 points high
  its page list was cut below Tools › AI Assistant)
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
4. **Keymap page:** every action of the keymap and the actions IDECraft runs without a bundled
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
5. **User keymap:** changes are saved to `keymaps/IDECraft.xml` in the user's settings folder in the
   reference's format: a keymap `IDECraft` whose parent is the platform's bundled keymap, with only
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

## Out of scope
Further pages (editor font, code style, version control, plugins …), project-level settings,
choosing among several keymaps, mouse and two-stroke recording in the dialog (two-strokes are possible
through `set_shortcut`), the reference's shortcut-conflict dialog, importing keymaps of other IDEs.

## Known deviations from the reference
(v4) The reference collects changes until OK or Apply and drops them on Cancel; IDECraft applies
each change at once and has no such buttons (behaviour 4a). (v7) Revert changes (behaviour 8)
takes back a page's changes since the dialog opened, as Cancel would for that page; there is no
Cancel for all pages at once, and the reference's preview of a change before Apply is the change
itself in IDECraft. (v8) The reference's Settings is a window of its own; IDECraft's dialog lies
over the IDE window and takes its size from it (behaviour 12). Settings shows two pages for now. Recording adds a one-keystroke shortcut directly and reports
conflicts in a note instead of a dialog that asks whether to remove the other assignments. A keymap
change applies without first copying the bundled keymap under a new name; the copy is always called
`IDECraft`. Observed in OBS-0008: the reference shows the actions as a tree of groups with display
names and keycaps, offers a find-by-shortcut popup, shows conflicts already while recording and then
asks Cancel / Keep / Remove; it opens Settings on the page shown last. IDECraft lists action ids
flat and sorted; the tree with display names is open.

**Window check 2026-11-09** (`main` @ 3b3cc1e0, agent run with real input, OBS-0028): the Settings search takes synthetic text ("external" filtered Keymap).
