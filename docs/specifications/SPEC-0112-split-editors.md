# SPEC-0112: Split editors

- **Version:** 5.1 · **Date:** 2026-11-10 (v5.1: implementation decisions, behaviour 12; v5: any number of nested groups, behaviour 10, OBS-0031 L7; v1: 2026-11-01; v3: menu titles in Find Action, changed mark and menu in the second group; hand tests 39, 199;
  v4: the second view shows the same decorations, behaviour 9; OBS-0022 L25)
- **Sources:** GEN (IDEs split the editor area into groups with their own tabs; the same file in two
  groups is one document with two carets and scroll positions, and an edit, undo or save in one shows
  in the other; F-020), OSS (action ids `SplitVertically`, `SplitHorizontally`, `Unsplit`,
  `UnsplitAll`, `NextSplitter`, `PrevSplitter`, `MoveEditorToOppositeTabGroup` and the macOS shortcuts ⌥⇥ / ⌥⇧⇥ for the last two from
  the bundled keymap), SPEC-0059 (tabs), SPEC-0058 (saving)
- **Feature matrix rows:** F-020

## Purpose
Look at two places of one file, or at two files, side by side while editing.

## Behaviour
1. **Split:** `SplitVertically` (Split Right) and `SplitHorizontally` (Split Down), from Search
   Everywhere, open the active file in a second editor group right of or below the main group and focus
   it there, with the caret where it was. Splitting again adds the then active file as a tab of the
   second group. (v3) Find Action shows them under their menu titles "Split Right" and "Split
   Down" (SPEC-0031 v3), so typing the title does not run Split and Move instead. [GEN, OSS]
2. **Groups:** each group has its own tab bar and active tab; the group with the focus marks its active
   tab. A click into a group gives it the focus. Opening a file from the tree, Search Everywhere or a
   jump opens it in the main group. [GEN]
3. **One document:** both views of a file share the text, the undo history and the saved state: an
   edit in one appears in the other at once, its carets move with the edit, undo in either view undoes
   the last change of the document, and saving in one clears the modified mark in both. (v3) The
   second group's tab carries the modified mark (` *`) like the main group's, and a secondary
   click on its text opens the editor's context menu for that view. [GEN]
4. **Focus:** `NextSplitter` and `PrevSplitter` (⌥⇥, ⌥⇧⇥) move the focus to the other group; editor
   actions (find, completion, refactorings, run SQL …) work on the view with the focus. [OSS, GEN]
5. **Closing:** closing the last tab of the second group, or `Unsplit`/`UnsplitAll`, removes the group;
   the files stay open in the main group. [GEN]
6. **Move to the other group:** `MoveEditorToOppositeTabGroup` (View › Move Tab to Opposite Group)
   moves the active tab of the group with the focus into the other group and focuses it there. The
   document is not reloaded: text, unsaved changes, undo history and caret stay. A tab that leaves
   the second group is added to the main group; a tab that leaves the main group keeps its document
   alive for the second group and comes back when the file is opened again. Without a split the
   action only says so in the status bar. [OSS, GEN]
7. **Split and move:** `SplitAndMoveRight` and `SplitAndMoveDown` (View menu, Search Everywhere)
   split as in behaviour 1 and take the tab out of the main group. With a single tab in the main
   group nothing moves and the status bar says so. [GEN]
8. **Emptied group:** when moving empties the second group it closes; when it empties the main
   group, the second group's tabs become the main group's tabs and the split closes. [GEN]
9. **v4 – Same decorations:** the second view of a file shows what the main view shows of it: the
   change markers and their bar (SPEC-0064), the semantic colours (SPEC-0290), the usage counts and
   authors (SPEC-0292) and the parameter hints (SPEC-0291). A view opened by a split takes them from
   the main view at once; later answers (after a save, a new HEAD) reach every view of the file.
   Problems already come with the shared document. Before v4 the second view had no markers and
   only syntax colours. [GEN, OBS-0022 L25]
10. **v5 – Nested groups:** a split divides the focused group, not the whole area, so groups nest:
    Split Right then Split Down gives a left group and a right column of two. Any number of groups;
    each divider can be dragged, and a double click on it makes both sides equal. Behaviours 1–9
    apply per group: "the other group" of behaviour 4, 6 and 8 is the next group in reading order
    (`NextSplitter`/`PrevSplitter` cycle through all), Move Tab to Opposite Group moves to the
    group next to it, and when a group empties it closes and its neighbour takes its space.
    `UnsplitAll` gathers every tab into one group, keeping the focused group's active tab active.
    The layout (tree of splits, ratios, tabs per group) is remembered per project. [OBS-0031 L7, GEN]
11. **v5 – MCP:** `editor_tabs` (SPEC-0059 behaviour 21) lists every group with its place in the
    layout and runs `split_right`, `split_down`, `move_to_group` and `unsplit_all`. [ADR-0008]
12. **v5.1 – Decisions (implementation):** every split divides one group into two (a binary tree);
    the new group lies right of or below the divided one, takes half of its space and gets the
    focus. A split always makes a new group, also when one already exists (this replaces "splitting
    again adds the then active file as a tab of the second group" of behaviour 1). The main group
    is the one files open in (behaviour 2); when it empties while other groups exist, the next
    group in reading order takes its place and its tabs become the main group's. A divider's ratio
    stays between 10 % and 90 %. The layout is kept with the tabs in `.ferris-idea/editor-tabs.json`
    (SPEC-0059 v9.1 behaviour 22e).

## Acceptance criteria
- **AK-1** Carets of the other view move with an edit before them, stay before it and land after the
  new text when their text was replaced (unit tests `split_model::tests`).
- **AK-2** Window self-test: Split Right shows the file in a focused second group; typing there changes
  the main view; ⌥⇥ focuses the main group; undo there reverts the text in both; Unsplit closes the
  group.
- **AK-3** Hand test (round 4): two places of one file side by side, typing, undo and save in either,
  next to the reference.
- **AK-4** Window self-test: Split and Move Right takes the second file out of the main group into a
  focused second group; typing there and Move Tab to Opposite Group bring it back with the typed
  text, and the emptied second group closes. Hand test (round 4): the same, and moving the main
  group's only tab leaves one unsplit group with the second group's files.

- **AK-5 (v4)** Window self-test step (written, run later): Split Right on the changed file of the
  Git fixture shows its marker in the second view too (`EditorView::decorations` and
  `set_decorations` hand over base, markers, semantic colours, hints, usage counts and authors).

- **AK-6 (v5)** Unit tests on the layout tree: nested splits, closing a middle group, moving a tab
  to the next group, unsplit-all keeps the focused tab, ratios survive a save and reload. Window
  self-test: Split Right, then Split Down in the right group shows three editors.

## Out of scope
Dragging tabs between groups, breakpoints set in the second view (they are set in the main view).
(v5.1: saving the layout across restarts is in scope since v5 behaviour 10.)

## Known deviations from the reference
Compared side by side in OBS-0031: v5 lifts the limit of two groups. Files opened from the tree or by a jump always go to the main group, not to the
group with the focus. The find bar shows above the main group. Not compared side by side yet.
