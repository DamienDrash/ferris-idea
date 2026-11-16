# SPEC-0041: Quick fixes and refactorings through the language server (code actions)

- **Version:** 3 · **Date:** 2026-11-13 (v3: requests say that they were invoked; the MCP tool lists
  applicable actions first and leaves disabled ones out unless asked; v2: lazily computed actions are
  resolved before they are previewed or applied)
- **Sources:** GEN (IDEs offer context actions at the caret or selection — fixes for problems and
  refactorings such as extract variable/function — and show what an action changes; F-073), STD (LSP
  `textDocument/codeAction` with `CodeActionContext` diagnostics, `CodeAction` with `edit` and/or `command`,
  `workspace/executeCommand`, server request `workspace/applyEdit`, `codeAction/resolve` with the client
  capabilities `resolveSupport` and `dataSupport`, `CodeActionContext.triggerKind` and `CodeAction.disabled`
  of LSP 3.16/3.17), SPEC-0040 (edits, preview, apply)
- **Feature matrix rows:** F-033, F-073

## Purpose
Offer the language server's fixes and refactorings for a position or selection and apply the chosen one
after a preview.

## Behaviour
1. The client announces code-action literal support with all standard kinds, `isPreferred` and `disabled`,
   that it keeps an action's `data` (`dataSupport`) and can resolve its `edit` and `command` later
   (`resolveSupport`), and that it accepts `workspace/applyEdit` with document changes. [STD]
2. For a range of a file the diagnostics the server publishes for the file (waiting briefly for them) that
   overlap the range are sent as context, so quick fixes for those problems are offered. The answer lists
   each action with title, kind, whether it is preferred, the reason it is disabled (if any) and the
   diagnostics it fixes. **v3:** every request is made for an explicit call (⌥↩, a refactoring shortcut, an
   MCP tool) and says so with `triggerKind: Invoked`. typescript-language-server offers some refactorings
   only then: without it "Inline variable" with the caret on a use of the variable came back as disabled
   ("Could not find variable to inline.") although it can be applied. [STD]
3. Applying an action picks it by title from a fresh request. When the action has neither `edit` nor
   `command` and the server offers `resolveProvider`, it is first sent back unchanged with
   `codeAction/resolve` and the answer takes its place, so servers that compute edits lazily (for example
   rust-analyzer's assists) work; a server without `resolveProvider` keeps the action as it is. A failed
   resolve is reported with the server's message. Its `edit` is used directly; its `command`
   (or a bare `Command`) is executed with `workspace/executeCommand`, and the edits the server then sends
   through `workspace/applyEdit` are collected instead of being written at once. Both are combined into one
   plan of file edits as in SPEC-0040 (resource operations refused). A disabled action is refused with its
   reason; an unknown title lists the available ones. [STD, improvement IMP-0018]
4. The plan is previewed or applied with the SPEC-0040 rules (old text checked, all files or none). [GEN]
5. MCP tools `code_actions` (path, range) and `apply_code_action` (path, range, title, `dry_run` default
   true). [ADR-0008]
6. **v3 – What an agent can apply:** a server that knows `disabledSupport` may answer with actions that do
   not apply at the place, each with a reason; typescript-language-server sends about a dozen refactorings
   that way for every position of a TypeScript file (Convert default export to named export, Convert
   namespace import to named imports, Extract to typedef, Inline variable, …). `code_actions` therefore
   lists only the applicable actions by default (`applicable_only`, default true). With
   `applicable_only: false` the disabled ones follow the applicable ones, each with the server's reason in
   `disabled`; within both parts the server's order is kept. An action without a reason is one the server
   offers as applicable (for example "Move to a new file" at any position of a TypeScript file).
   `apply_code_action` refuses a disabled action with its reason (behaviour 3). The window's list (SPEC-0076
   behaviour 2) keeps showing the disabled ones greyed after the applicable ones. [STD, GEN]

## Acceptance criteria
- **AK-1** With `clangd`: a missing semicolon yields a preferred quick fix whose application makes the file
  compile; selecting an expression offers "extract subexpression to variable", whose application (through
  `executeCommand` and `applyEdit`) introduces a variable and still compiles; an unknown title is refused
  with the available titles. (Skipped without `clangd`.)
- **AK-2** Workspace edits from several `applyEdit` requests and an action's own edit are merged per file.
- **AK-3** The MCP tools list and apply an action.
- **AK-4** With `rust-analyzer` (which answers with unresolved actions when the client supports
  resolving): an assist on a selected expression is resolved, its preview (`dry_run`) lists the edits and
  applying it changes the file. (Skipped without `rust-analyzer`.) The decision when to resolve is a unit
  test without a server.

- **AK-5 (v3)** Applicable actions come first, disabled ones keep their reason and are left out with
  `applicable_only` (unit test). The MCP tool lists `applicable_only` in its schema and names it in its
  description; with typescript-language-server (skipped without it) the default answer has no disabled
  action, `applicable_only: false` appends them with their reasons, and `apply_code_action` refuses one of
  them with "is disabled".
- **AK-6 (v3)** With typescript-language-server (skipped without it): with the caret on a use of a variable
  "Inline variable" is offered without a reason and applies; "Convert default export to named export" comes
  with the server's reason.

## Out of scope
Actions on save, organise imports as
a separate command, snippet edits, the UI light bulb (M3.7).

## Known deviations from the reference
Actions come from each language's server; the reference's own intentions for Java/Kotlin are not part of
this. Not yet compared side by side.
