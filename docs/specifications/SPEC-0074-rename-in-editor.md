# SPEC-0074: Rename from the editor

- **Version:** 2 · **Date:** 2026-11-18 (v2: no rename on a keyword, literal or built-in type, and
  where the language server's `prepareRename` finds no symbol; window check A12 R1)
- **Sources:** GEN (rename a symbol and all its usages from the editor; F-072), OSS (action id
  `RenameElement` with ⇧F6 in the ported keymaps), SPEC-0040 (rename plan from the language server),
  SPEC-0072 (requests on unsaved text), STD (LSP `textDocument/prepareRename`: `null` or an error
  means no rename is valid at the position)
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
5. **Nothing to rename (v2, window check A12 R1):** ⇧F6 with the caret on a keyword, a literal
   (`true`, `null`) or a built-in type (`int`) — as the file's syntax tree highlights it — opens no
   popup; the status bar says "Cannot rename this element: the caret should be on an identifier".
   A contextual keyword used as a name (Java `var record = …`) is a name in the tree and is offered.
   For the other places the popup opens at once and the language server is asked
   `textDocument/prepareRename` meanwhile (when it offers it): when it answers `null` or with an
   error (not "content modified" or "cancelled"), the popup closes with the same message. No answer
   within 10 s, no server or no `prepareProvider` leave the popup open; a server without rename
   leaves the place to Ferris IDEA's own text rename. Before v2 ⇧F6 on `record` of a Java record
   offered "Rename 'record' and its usages to". [GEN, STD]

## Acceptance criteria
- **AK-1** A rename plan on unsaved text (integration test with clangd: old name and edit positions).
- **AK-2** In a real window (`ferris-idea ui --selftest`, with clangd): ⇧F6 on `helper`, typing `calc` and
  Enter renames the definition in the editor.
- **AK-3** Use by hand: usages in other open and closed files, undo.
- **AK-4 (v2)** Keywords (`record`, `public`, `int`, `true` in Java; `fn` in Rust; `return` in
  Python) are refused, names (the record's name, a parameter, `var record`) are not, a file without
  a grammar says nothing (unit test `rename::tests::keywords_are_not_renameable`); in a window of
  the test harness ⇧F6 on `record` opens no popup and the status bar has the message, ⇧F6 on the
  record's name opens it (`crates/workbench/tests/it/move_undo_a12.rs::rename_is_refused_on_a_keyword`).

## Out of scope
In-place rename with live editing of all occurrences, renaming files and classes together, preview
dialog, renaming in comments and strings, conflicts dialog.

## Known deviations from the reference
The reference renames in place (all occurrences edited live) and offers a preview for wide changes;
Ferris IDEA uses a popup and applies directly (undoable per open file). Not compared side by side yet.
