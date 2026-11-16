# SPEC-0074: Rename from the editor

- **Version:** 1 · **Date:** 2026-10-31
- **Sources:** GEN (rename a symbol and all its usages from the editor; F-072), OSS (action id
  `RenameElement` with ⇧F6 in the ported keymaps), SPEC-0040 (rename plan from the language server),
  SPEC-0072 (requests on unsaved text)
- **Feature matrix rows:** F-072

## Purpose
Rename a symbol everywhere without leaving the keyboard, safely for files with unsaved changes.

## Behaviour
1. **Start:** `RenameElement` (⇧F6) on an identifier opens a small popup "Rename '<name>' and its usages
   to" with the current name selected. [GEN, OSS]
2. **Validate:** Enter accepts a name of letters, digits and `_` not starting with a digit; the same name
   just closes; an invalid name does nothing; Escape closes. [GEN]
3. **Apply:** the language server plans the rename on the editor's current text (SPEC-0040 on unsaved
   text). Files open in tabs change through their editor as one undo step each, with the caret kept in
   place; other files are checked and then written on disk. The status bar reports the old and new name
   and the number of files, or the server's reason when nothing can be renamed. [GEN, STD]
4. The MCP counterpart is `rename_symbol` with a preview by default (SPEC-0040). [ADR-0008]

## Acceptance criteria
- **AK-1** A rename plan on unsaved text (integration test with clangd: old name and edit positions).
- **AK-2** In a real window (`idecraft ui --selftest`, with clangd): ⇧F6 on `helper`, typing `calc` and
  Enter renames the definition in the editor.
- **AK-3** Use by hand: usages in other open and closed files, undo.

## Out of scope
In-place rename with live editing of all occurrences, renaming files and classes together, preview
dialog, renaming in comments and strings, conflicts dialog.

## Known deviations from the reference
The reference renames in place (all occurrences edited live) and offers a preview for wide changes;
IDECraft uses a popup and applies directly (undoable per open file). Not compared side by side yet.
