# SPEC-0151: Column selection mode

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** GEN (editors have a column selection mode, switched with ⇧⌘8 on macOS and
  Alt+Shift+Insert elsewhere or from the Edit menu, in which ⇧-arrows and plain mouse drags select
  rectangles; F-012), OSS (keymap action `EditorToggleColumnMode`, `JetBrains/intellij-community`
  keymaps), SPEC-0130, ADR-0008
- **Feature matrix rows:** F-012

## Purpose
Select and edit rectangles with the keyboard, without holding ⌥ for every drag.

## Behaviour
1. **Switching:** `EditorToggleColumnMode` (⇧⌘8 on macOS, Alt+Shift+Insert in the default keymap,
   Edit › Column Selection Mode) switches the mode of the focused editor on and off; the carets stay
   as they are. While it is on, the status bar shows "Column" next to the caret position; a click on
   it switches the mode off. Single-line fields have no column mode. [GEN, OSS]
2. **Mouse:** in column mode a plain click and drag selects a rectangle as ⌥-drag does
   (SPEC-0130). [GEN, SPEC-0130]
3. **Keyboard:** in column mode ⇧←/⇧→/⇧↑/⇧↓ move the free corner of a rectangle by one column or
   line; it starts at the caret, and a rectangle made with the mouse goes on from its corner.
   Columns go on past the end of a line, so a rectangle reaches over short lines; these get a caret
   at their end. Once the carets change in another way, the next ⇧-arrow starts a new rectangle at
   the caret. [GEN]
4. **Editing:** typing, deleting and pasting work on every part of the rectangle as with multiple
   carets (SPEC-0057). [SPEC-0057]
5. **MCP:** `preview_column_edit` (SPEC-0130) previews the same rectangle edit for agents. [ADR-0008]

## Acceptance criteria
- **AK-1** Window self-test: ⇧⌘8 switches the mode on; two ⇧↓ and three ⇧→ from line 1 column 7
  select `one`, a caret at the end of `be` and `two`; typing `X` replaces all three; ⇧⌘8 switches it
  off.
- **AK-2** MCP rectangle edit (`column_edit_preview`, SPEC-0130).
- **AK-3** Hand test (round 4, no. 77): column mode with keyboard and mouse next to the reference.

## Out of scope
Virtual space after the line end (padding short lines with spaces when typing), keeping column
mode across editors and restarts.

## Known deviations from the reference
Typing into a short line puts the text at its end instead of padding it with spaces to the column.
