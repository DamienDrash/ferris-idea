# SPEC-0073: Find Usages popup

- **Version:** 6 · **Date:** 2026-11-13 (v6: the usages marked in the editor are SPEC-0295; v2: below the caret, full-width selection; hand test; v3: the
  popup is Show Usages only, ⌥F7 opens the Find tool window of SPEC-0293; bounded search and clear
  outcome, OBS-0012; v4: every row one line, long code ends in …, OBS-0012; v5: without the
  declaration, a single usage as a hint, OBS-0016 defect F034-6)
- **Sources:** GEN (list every use of a symbol and jump to one; F-034), OSS (action ids `FindUsages` ⌥F7
  and `ShowUsages` ⌥⌘F7 / Ctrl+Alt+F7 of the ported keymaps), SPEC-0024 (references from the language
  server), SPEC-0071 (navigation history), SPEC-0072 (requests on unsaved text), OBS-0012 (side by side 2026-11-05:
  the reference opens a tool window on ⌥F7 and a popup on ⌥⌘F7; on the JDK type `String` IDECraft showed
  nothing for more than 25 s), SPEC-0293 (Find tool window and its bounded search)
- **Feature matrix rows:** F-034

## Purpose
See where a symbol is used and go there.

## Behaviour
1. **Start:** `ShowUsages` (⌥⌘F7) on the symbol at the caret asks the file's language server in the
   background for its references including the declaration, on the editor's current text; the status
   bar says it is looking, or that nothing was found. `FindUsages` (⌥F7) opens the Find tool window
   instead (SPEC-0293). (v3) [GEN, OSS, STD, OBS-0012]
2. **List:** a popup titled `Usages of <symbol> (<count>)` opens just below the caret's line (moved to
   stay inside the window; at the top of the window when that line is not visible) and lists the usages
   sorted by path, line and column, each with file icon, the trimmed text of its line (from the open
   editor when the file is open, else from disk) and `path:line` (relative to the workspace). The
   selection covers the whole row and keeps `path:line` readable.
   **v4:** every row is one line of fixed height: the line's text is shown with its line breaks, tabs
   and runs of spaces as single spaces, and when it is longer than the room left beside `path:line` it
   ends in `…`; nothing is drawn over the next row. [GEN, OBS-0012]
3. **Choose:** Up/Down select with wrap-around; Enter or a click opens the file at the usage and records
   the place left for Back (SPEC-0071); Escape closes and keeps the caret. [GEN]
4. The MCP counterparts are `find_symbol_locations` with references (SPEC-0024) and `find_usages`
   (SPEC-0293). [ADR-0008]
5. **Bounded (v3):** the popup's search uses the deadline, cancellation and project-files filter of
   SPEC-0293 behaviours 2 and 3. When the server does not answer in time the popup lists the text
   occurrences of SPEC-0293 behaviour 10 under the title `Text occurrences of <symbol> (<count>)`, and
   the status bar says the language server did not answer in time; with nothing to list the status bar
   says `No usages of <symbol> in project files` or gives the reason the search failed. The status
   never stays at "Finding usages" after the deadline. [GEN, OBS-0012]

6. **Without the declaration (v5):** the popup lists the usages of SPEC-0293 behaviour 2 without
   the declaration, and its count does not include it; ⌘B on a declaration (SPEC-0071 v7
   behaviour 9) shows the same list. This replaces "including the declaration" in behaviour 1.
   [OBS-0016 Nr. 8: the reference counts usages only]
7. **One usage (v5):** with exactly one usage no popup opens. When the caret is on that usage, a
   hint below the caret says `<symbol> has no other usage in project files; Find Usages (⌥F7)
   shows it in the Find tool window`. Otherwise the caret jumps to the usage (the place left is
   recorded for Back) and the hint below it says `Only usage of <symbol> in project files`. The
   hint closes like the documentation popup (SPEC-0077 behaviour 3). The reference shows such a
   hint with a link to its search options (OBS-0016 Nr. 8); IDECraft has no options dialog and
   names ⌥F7 instead. [OBS-0016]

## Acceptance criteria
- **AK-1** Sorting, de-duplication, relative paths, line text, identifier at a column (unit test).
- **AK-2** In a real window (`idecraft ui --selftest`, with clangd): ⌥⌘F7 (v3; ⌥F7 before) on `helper` lists
  lines 1 and 2; Escape closes with the caret unchanged. (v5: replaced by AK-6, the declaration is
  no longer listed.)
- **AK-3** Use by hand: many usages across files, mouse.
- **AK-4** (v3) In a real window: ⌥⌘F7 on `helper` opens the popup, ⌥F7 does not (it opens the Find
  tool window); a search without an answer ends with a status message (SPEC-0293/AK-3).
- **AK-5** (v4) A usage's text is one line whatever its source line holds (unit test); in a real
  window long lines end in … without overlapping the next row (hand test).

- **AK-6 (v5)** The declaration is not listed; one usage at the caret gives the hint, one elsewhere
  a jump and the hint (unit test `usages_model::tests::one_usage_is_a_hint`); in a real window ⌥⌘F7
  on the call of `helper` opens no popup but the hint, and on its definition jumps to the call
  (window self-test, replacing the step that listed the definition and the call).

## Out of scope
The Find tool window (SPEC-0293); grouping by usage type (read/write, imports), usages in non-code
files, search scopes. (v6) Marking the usages of the symbol at the caret in the editor, by itself and
with `HighlightUsagesInFile`, is SPEC-0295.

## Known deviations from the reference
Results come from the language server, not from an own index; when it is slower than the deadline the
popup lists text occurrences, marked as such. Compared side by side in OBS-0012 (2026-11-05).
