# SPEC-0092: Problems tool window

- **Version:** 4 · **Date:** 2026-11-06 (v2: the errors also mark tabs and the tree; v3: file heading,
  counts on the tab, errors first, toolbar, project errors of all files — OBS-0019 item 5; v4: the
  editor preview — defect F032-3)
- **Sources:** GEN (a Problems tool window lists the errors and warnings of the current file and of the
  project, each with its message and line; choosing one jumps to it; F-032), OSS (action id
  `ActivateProblemsViewToolWindow` with ⌘6 / Alt+6 in the ported keymaps; platform icons
  `expui/status/{error,warning,info}`, `expui/toolwindows/problems`), SPEC-0070 (editor diagnostics),
  SPEC-0090 (diagnostics while typing); v3: OBS-0019 item 5; v4: OBS-0019 item 5 (the File tab's
  toolbar has a preview button that shows the code of the selected problem inside the tool window),
  SPEC-0070 v6 (highlighting level)
- **Feature matrix rows:** F-032

## Purpose
See all problems of a file, or of everything open, in one list and jump to each.

## Behaviour
1. **Open:** `ActivateProblemsViewToolWindow` (⌘6 on macOS, Alt+6 elsewhere) or the stripe button
   shows the tool window under the editor with the keyboard in its list. Pressing it again while the list
   has the focus hides it, and so do the close button and `HideActiveWindow`. [GEN, OSS]
2. **Rows:**
   - **File tab** (default): the problems of the active editor by line, errors before warnings on the
     same line. Each row has the severity icon, the message and the line.
   - **Project tab:** all open files with problems. Each file has a heading (file-type icon, name,
     folder, counts). Files with errors come first, then the others by path.
   - **Header:** the counts, e.g. "2 errors, 1 warning" or "No problems".
   - **Empty list:** says "No problems in this file" or "No problems in the open files".
   - The list follows the diagnostics as they change while typing (SPEC-0090). [GEN]
3. **Navigate:** Up/Down move over the problems (skipping headings, wrapping around). Enter or a double
   click opens the file with the caret at the problem's start. Escape returns to the editor. [GEN]
4. Agents read the same diagnostics through `file_diagnostics` (SPEC-0023). [ADR-0008]
5. **Marks outside the list (v2):** the files whose problems include an error are also marked on their
   editor tabs and in the project tree, folders included (SPEC-0059 v4 behaviour 10). [GEN]
6. **File tab (v3, replaces the file tab of behaviour 2):** the tab's label carries the number of
   problems ("File 6"). The list starts with a heading for the file (file-type icon, name, folder
   relative to the project, "6 problems"), then the problems sorted by severity — errors, warnings,
   weak warnings — and within a severity by line and column. [OBS-0019]
7. **Project Errors tab (v3, replaces the project tab of behaviour 2):** named "Project Errors", it lists
   only errors: those of the open editors and those the language servers published for files that
   are not open (SPEC-0090), under file headings as before. Empty, it says that no errors were found
   and offers a link that runs Inspect Code (SPEC-0098). [OBS-0019]
8. **Toolbar (v3):** a narrow column of toggle buttons left of the File tab's list: **Group by
   inspection** (problems of Ferris IDEA's inspections under a heading per inspection with its count,
   headings sorted by name, their problems listed below them; language-server problems stay
   ungrouped at the top),
   **Sort by name** (by message instead of severity), **Show warnings** and **Show weak warnings**
   (filters; errors always show). The choices are kept for the session. Each button has a tooltip and
   shows its state. A fifth button, **Show quick fixes**, opens the selected problem in the editor
   and shows the ⌥↩ list there (SPEC-0076). [OBS-0019]
9. **Editor preview (v4):** a sixth toolbar button, **Open Editor Preview**, splits the File tab: the
   list keeps the left half, the right half shows the code of the selected problem — a line with the
   file name and `:line`, then the lines around it (5 above and 5 below, fewer at the file's ends)
   in the editor font with line numbers; the problem's line has the caret-row background and the
   problem's text is underlined in its severity's effect colour (grey text for unused code). The
   preview follows the selection (also Up/Down) and the open editor's unsaved text; without a
   selection it says "Select a problem to preview". A click on a preview line opens the file with
   the caret at the start of that line (the problem's start on its own line). The button shows its
   state, the choice is kept for the session, and the File tab lists only what the file's
   highlighting level shows (SPEC-0070 v6). Agents get the same lines with the MCP tool
   `problem_preview` (path, 0-based line, optional column and `radius`, default 5): the first line
   number, the lines with their numbers, and which line holds the problem. [OBS-0019, ADR-0008]

## Acceptance criteria
- **AK-1** Ordering in both tabs, file headings with counts, stepping over headings with wrap-around,
  the summary texts (unit test `problems_model::tests`).
- **AK-2** In a real window (`ferris-idea ui --selftest`, with clangd): with a C file that has errors, ⌘6
  lists them, and Down and Enter put the caret on the first one.
- **AK-3** By hand: look next to the reference, several files, live updates while typing.
- **AK-4 (v3)** File heading and tab count, severity order, grouping by inspection, sorting by name,
  the two filters, project errors from open and unopened files (unit tests `problems_model::tests`);
  the window self-test sees the heading and the error first on the Java file of
  `java_inspection_steps`; side by side in OBS-0019 (re-check).

- **AK-5 (v4)** The preview lines around a problem at the start, middle and end of a file, with the
  problem's line and range (unit test `problems_model::tests::preview_lines_around_a_problem`); MCP
  `problem_preview` (`crates/mcp/tests/it/protocol.rs`: `problem_preview_tool`); window self-test
  (`java_inspection_steps`, written): the preview button splits the File tab and shows the selected
  problem's line; by hand next to the reference.

## Out of scope
Editing inside the preview, grouping by severity, the "Server-Side Analysis",
"Vulnerable Dependencies", "Qodana" and "Security Analysis" tabs.

## Known deviations from the reference
v3: compared side by side in OBS-0019. The reference starts the inspection groups folded; Ferris IDEA
lists their problems at once. Project Errors holds what the language servers published;
a server that only analyses open documents contributes nothing for closed files. v4: the reference's
preview is an editor; Ferris IDEA's preview is read-only and a click opens the real editor, so there is
one place to edit a file.
