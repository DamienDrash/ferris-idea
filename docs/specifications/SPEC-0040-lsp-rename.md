# SPEC-0040: Rename refactoring through the language server

- **Version:** 1 · **Date:** 2026-10-31
- **Sources:** GEN (IDEs rename a symbol with all its usages after showing the affected places; F-072),
  STD (LSP `textDocument/prepareRename`, `textDocument/rename`, `WorkspaceEdit` with `changes` or
  `documentChanges`, position encodings), SPEC-0023/0024 (LSP client, positions)
- **Feature matrix rows:** F-072

## Purpose
Rename functions, variables, types and other symbols in every language that has a language server, with a
preview before files change.

## Behaviour
1. Positions are 0-based lines and characters counted in Unicode scalar values (as in SPEC-0024) and are
   converted to and from the server's encoding. [STD]
2. If the server supports `prepareRename`, it is asked first; a position without a renameable symbol is
   refused with "nothing to rename here", and the current name (placeholder or range text) is reported.
   [STD]
3. `rename` returns a workspace edit; edits from `changes` and from `documentChanges` text edits are
   collected per file (paths, edits sorted by position). Resource operations (create, rename, delete file)
   are refused, since the preview could not show them faithfully. A server error (e.g. an invalid new name)
   is passed on with its message. [STD]
4. The preview lists each file with its edits (range, old text, new text). Applying writes every file once
   with all its edits from the end backwards; before writing, each edit's old text is checked against the
   file on disk, and if any file changed in the meantime nothing is written. [GEN, improvement IMP-0017]
5. MCP tool `rename_symbol` (path, line, character, new name, `dry_run` default true). [ADR-0008]

## Acceptance criteria
- **AK-1** Applying edits: several edits per line and across lines, multi-byte characters, descending
  order; a stale file refuses the whole apply.
- **AK-2** With `clangd`: renaming a C function changes its definition, declaration and calls; the preview
  shows old and new text; the applied file compiles; a position on whitespace is refused; an invalid name
  is refused with the server's message. (Skipped without `clangd`.)
- **AK-3** The MCP tool previews and applies a rename.

## Out of scope
Renaming files together with symbols, rename in comments and strings, conflict detection beyond what the
server reports, undo (the editor's undo covers open files later), other LSP refactorings (code actions).

## Known deviations from the reference
Java and Kotlin renames in the reference come from its own analysis; IDECraft relies on the language
server of each language. Not yet compared side by side.
