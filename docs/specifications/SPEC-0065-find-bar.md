# SPEC-0065: Find and replace bar in the editor

- **Version:** 6 · **Date:** 2026-11-14 (v6: the filter menu opens by the keymap action `ShowFilterPopup`, behaviour 20, AK-8; sixth package of docs/research/KEYMAP_COVERAGE.md); 2026-11-13 (v5: the recent searches open by the keymap action `ShowSearchHistory`, behaviour 19); 2026-11-06 (v2: replace; v3: framed fields with placeholders; v4:
  side-by-side comparison OBS-0023 — stripe marks, replace toggle, clear, history, line breaks, in
  selection, comments and strings, more menu, preserve case, exclude, replacement preview)
- **Sources:** GEN (find/replace bar above the editor of IDEs; F-015), OSS (action ids `Find`, `FindNext`,
  `FindPrevious`, `Replace` of the ported keymaps; find results drawn with the scheme key
  `TEXT_SEARCH_RESULT_ATTRIBUTES`, as in the open `EditorLivePreviewPresentation`), SPEC-0029 (query,
  options, matches), SPEC-0058 (editor view)
- **Feature matrix rows:** F-015

## Purpose
Find text in the open file without leaving the editor, as with the reference's find bar.

## Behaviour
1. **Open:** `Find` (⌘F on macOS, Ctrl+F elsewhere) opens the bar between the tabs and the text of the
   active editor. The query field starts with the editor's selection when it lies within one line, else
   with the last query; its text is selected and it has focus. Toggles: match case (`Cc`), whole words
   (`W`), regular expression (`.*`), all off at first (SPEC-0029 behaviour 1). [GEN, OSS]
2. **Search while typing:** every change of the query or an option searches the active editor's text; the
   current match is the first one at or after the caret (else the first) and is selected and scrolled into
   view. Edits of the text, and switching tabs, search again without moving the selection. [GEN]
3. **Step:** Enter, `FindNext` (⌘G / F3) and the down arrow button go to the next match; Shift+Enter,
   `FindPrevious` (⇧⌘G / Shift+F3) and the up arrow button to the previous one; both wrap around. `FindNext`
   and `FindPrevious` work from the editor while the bar is open and open it when it is closed. [GEN, OSS]
4. **Counter:** `current/total` (e.g. `2/5`); `0 results` in red when nothing matches; the reason in red
   for an invalid regular expression; empty for an empty query. [GEN]
5. **Highlights:** all matches have the background of `TEXT_SEARCH_RESULT_ATTRIBUTES`; the current match
   is the editor's selection. [OSS]
6. **Close:** Escape in the bar or its close button closes it, removes the highlights and returns focus
   to the editor with the current match still selected. [GEN]
7. The MCP counterparts are `search_in_files` and `replace_in_files_preview` (SPEC-0029) with the same
   options. [ADR-0008]
8. **Replace:** `Replace` (⌘R on macOS, Ctrl+R elsewhere) opens the bar with a second row: replacement
   field, "Replace" and "Replace All"; focus stays in the query, Tab moves between the two fields. Enter
   in the replacement field or "Replace" replaces the current match and goes to the next match after it;
   "Replace All" replaces every match and reports the count in the status bar. With a regex, `$1` and
   `${name}` insert captures (SPEC-0029 behaviour 4). Each replacement — also "Replace All" — is one undo
   step. [GEN, OSS]

9. **Fields (v3):** the search and the replace field are framed boxes on the bar's own background,
   separated from the code by a line; the search field starts with a magnifier, and while empty the
   fields read "Search" and "Replace with" in grey. [GEN; hand test 2026-11-01: the fields were hard to
   tell apart]

**v4 (OBS-0023 L24–L30).**
9a. **Stripe marks:** each line with a match gets a mark in the editor's error stripe
   (`TEXT_SEARCH_RESULT_ATTRIBUTES` stripe or background colour); a click puts the caret on the
   line's first match. Matches stay readable on field names: scheme colours follow SPEC-0060 v9.
   [GEN, OSS; OBS-0023 L24, L25]
10. **Replace toggle, clear, history:** an arrow at the start of the bar shows or hides the replace
   row; a clear button (×) appears in a field with text; the magnifier with its arrow, and ⌥↓ in the
   field, open the recent searches (the last 20 of the window, newest first, recorded on Enter and when
   the bar closes); choosing one puts it into the field. [GEN; OBS-0023 L26]
11. **Line breaks:** a button in the query field and ⌘⇧↩ (Ctrl+Shift+Enter) insert a line break, so a
   search spans lines; the field grows up to four lines. [GEN; OBS-0023 L27]
12. **In selection:** opening the bar on a selection over several lines searches only in it; the
   filter menu's first entry and `ToggleInSelection` (⌃⌥E, the key of the reference's
   `ToggleFindInSelection`; SPEC-0028 v4 behaviour 8) switch it. [GEN, DOC; OBS-0023
   L28]
13. **Comments and strings:** the filter menu offers anywhere, in comments, in string literals, except
   comments, except string literals, except both; the file's grammar tells comments and strings apart
   (no grammar: no comments or strings). The funnel is highlighted while the bar is narrowed.
   [GEN; OBS-0023 L28]
14. **More menu (⋮):** "Open Results in Find Window" shows the search in the Find in Files panel
   (SPEC-0066) limited to this file and pinned; Add Selection for Next Occurrence, Unselect
   Occurrence and Select All Occurrences with their shortcuts; "Scroll to Results During Typing"
   (on by default; off keeps the editor where it is while typing). [GEN; OBS-0023 L29]
15. Replacing respects behaviours 12–13: only the matches the bar counts are replaced.
16. **Preserve case:** a toggle in the replace field keeps the case of each match (all upper → upper,
   first letter upper → capitalised, all lower → lower). [GEN; OBS-0023 L30]
17. **Exclude:** a button leaves the current match out of Replace All and goes to the next one.
   [GEN; OBS-0023 L30]
18. **Replacement preview:** with a regular expression, the replace row shows `→ <replacement>` of the
   current match next to its buttons, instead of a balloon over the code (IMP-0331). [GEN; OBS-0023
   L30]
19. **Recent searches by action (v5):** opening the recent searches from the field (behaviour 10,
   ⌥↓) is the keymap action `ShowSearchHistory` (⌥↓, Alt+Down in the bundled keymaps; "Search
   History" in Find Action): in the bar's fields every key the keymap in use binds to it opens
   the list, and ⌥↓ without it does not. Chosen in Find Action it opens the list of the open bar;
   without a bar it is not available. [OSS: id and shortcut of the bundled keymaps; GEN]
20. **Filter menu by action (v6):** `ShowFilterPopup` (⌃⌥F in the bundled macOS keymap,
    Ctrl+Alt+F in the default one) opens the filter menu of the find bar — search in selection
    and the comments and string literals filter of behaviour 13 — as its button does. Without
    a find bar the action does nothing and its key goes on to the next action. [OSS: id and
    keys of the ported keymaps; the menu's items are Ferris IDEA's, behaviours 12 and 13]

## Acceptance criteria
- **AK-1** Current match from the caret, wrapping both ways, counter texts, options, empty query, invalid
  regular expression (unit tests).
- **AK-2** In a real window (`ferris-idea ui --selftest`): ⌘F and typing count the matches (`1/2`), Enter
  and Shift+Enter step without changing the text, Escape closes the bar.
- **AK-3** Use by hand: highlights, toggles, buttons, search while editing, Replace All and its undo.
- **AK-4** Replacements with captures, literal `$1` without regex, invalid regex (unit test); in the
  window self-test ⌘R, query, Tab, replacement and Enter replace the match.

- **AK-6 (v4)** Preserve case, excluded matches, selection and comment narrowing (unit test
  `find_model::tests::preserve_case_exclude_and_narrowing`); search contexts (unit test
  `syntax::tests::search_contexts_tell_comments_and_strings_apart`); in the window (OBS-0023
  Nachprüfung): stripe marks, menus, a line break in the query, Exclude and Replace All.
- **AK-7 (v5, behaviour 19)** In a window of the test harness: without a find bar the action is
  not available; with the bar open the action, and ⌥↓ in the field, open the recent searches
  (`tree_and_popup_actions_keymap.rs::history_actions_need_their_place`); the title
  (`everywhere::tests::fourth_package_actions_have_titles`).
- **AK-8 (v6, behaviour 20)** In a window of the test harness: without a find bar the action is
  none; with one it opens the filter menu
  (`sixth_package_keymap.rs::the_find_bar_filter_menu_opens_by_action`).

## Out of scope
Keeping the search history across restarts.

## Known deviations from the reference
Toggles are shown as text labels instead of icons. The regex dialect is the `regex` crate's
(SPEC-0029). The replacement preview sits in the replace row (IMP-0331). "Open Results in Find
Window" uses the Find in Files panel with the file as its folder scope.
