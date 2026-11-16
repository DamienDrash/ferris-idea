# SPEC-0130: Column selection with the mouse

- **Version:** 2 · **Date:** 2026-11-13 (v2: the two clicks are the keymap's mouse shortcuts of `EditorCreateRectangularSelectionOnMouseDrag` and `EditorAddOrRemoveCaret`, and a click on a caret removes it, behaviour 5; docs/research/KEYMAP_COVERAGE.md); 2026-11-02 (v1)
- **Sources:** GEN (editors select a rectangle of text with ⌥-drag on macOS and Alt-drag
  elsewhere; every line of it gets its own selection, and typing changes all of them; F-012),
  SPEC-0057/SPEC-0058 (multiple carets, block selection in the editor core), ADR-0008; v2: OSS
  (action ids and mouse shortcuts of the bundled keymaps of `JetBrains/intellij-community`),
  GEN (editors with several carets take one away again with the click that added it)
- **Feature matrix rows:** F-012

## Purpose
Edit the same columns of many lines at once, e.g. aligned values or a prefix in a block.

## Behaviour
1. **Mouse:** ⌥-click (Alt-click) in the text puts a caret at the clicked line and column; dragging
   with the button held stretches a rectangle from there to the pointer. Columns go on past the
   end of a line (one per character width of the editor font); dragging above or below the
   visible lines stops at the first or last one. Releasing the button ends it. [GEN]
2. **Selections:** every line of the rectangle gets a selection between the two columns; a line
   shorter than the left column gets a caret at its end. Typing, deleting, copying and pasting then
   act on all of them as with multiple carets (SPEC-0057). [GEN]
3. Other clicks are unchanged: ⌥⇧-click still adds a caret, ⌘-click (Ctrl-click) still goes to the
   declaration.
4. **MCP:** `preview_column_edit` (path, two corners, replacement) returns the parts selected and the
   text after typing the replacement into each; nothing is written. [ADR-0008]
5. **The clicks are mouse shortcuts of the keymap (v2):** the click that starts a column selection
   (behaviour 1) is the mouse shortcut of `EditorCreateRectangularSelectionOnMouseDrag` (⌥ with
   the primary button in the bundled keymaps), and the click that adds a caret (behaviour 3) the
   one of `EditorAddOrRemoveCaret` (⌥⇧ with the primary button). The editor asks the keymap in
   use which of the two the modifiers of a single click with the primary button mean, so a mouse
   shortcut changed in the keymap file works and one taken away no longer does it; in column
   selection mode (SPEC-0151) a plain click still starts a column selection. A click with
   `EditorAddOrRemoveCaret` on the place of a caret without selection removes that caret while
   another one remains; anywhere else it adds one, as before. Find Action lists both actions
   under the words of their ids, as it lists the other editor actions (SPEC-0067 v4 behaviour 4);
   chosen there they do nothing and answer that they are not available, since they need the place
   of a click. No MCP tool: `preview_column_edit` (behaviour 4) and the caret tools of SPEC-0057
   cover both. [OSS: ids and mouse shortcuts of the bundled keymaps; GEN]

## Acceptance criteria
- **AK-1** Block selections over lines of different lengths, in both directions (`selection::tests`
  of the editor core).
- **AK-2** MCP: parts and text of a column edit (`crates/mcp/tests/it/protocol.rs`:
  `column_edit_preview`).
- **AK-3** Window self-test (`column_select_steps`): the rectangle from line 0 column 6 to line 2
  column 9 gives three parts with a caret at the end of the short line; typing replaces them.
- **AK-4** Hand test (round 4): ⌥-drag over several lines, typing, ⌫, copy and paste next to the
  reference.
- **AK-5 (v2, behaviour 5)** Given the bundled macOS and default keymaps, then a single click with
  ⌥ means the column selection, with ⌥⇧ the caret, a plain click or a double click neither, and a
  mouse shortcut given by the user is followed (unit test
  `keymap::tests::mouse_shortcuts_name_their_actions`); a click on a caret removes it, on the only
  caret or on other text it does not (unit test `selection::tests::a_click_on_a_caret_removes_it`).
  In the window: by hand.

## Out of scope
Column selection mode (⇧⌘8) for the keyboard, virtual space past the line ends (spaces are not
inserted), tab characters counted as more than one column, middle-button drag (the keymap's second
mouse shortcut of `EditorCreateRectangularSelectionOnMouseDrag`), adding a rectangle to the
selections there are (`EditorAddRectangularSelectionOnMouseDrag`), a column selection up to a
middle click (`EditorCreateRectangularSelection`).

## Known deviations from the reference
Not compared side by side.
