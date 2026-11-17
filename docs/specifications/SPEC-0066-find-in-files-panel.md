# SPEC-0066: Find in Files panel

- **Version:** 7 · **Date:** 2026-11-17 (v7: the list shows the matches of unsaved editors, behaviour 19, AK-9, beta finding B1 of hand test 6); 2026-11-12 (v2: Replace in Files; v3: side-by-side comparison OBS-0023 —
  scope, preview, pinning, comments and strings, line breaks; v4: only the project's own sources,
  decision of the Project Owner of 2026-11-12; v5: a replacement changes what the list shows; v6: the
  toggle and the line for a remote project too, same decision for the remaining places)
- **Sources:** GEN (find in files of IDEs; F-015), OSS (action id `FindInPath` of the ported keymaps; match
  background `TEXT_SEARCH_RESULT_ATTRIBUTES`), SPEC-0029 (workspace search, file mask, limits), SPEC-0065
  (options as in the find bar)
- **Feature matrix rows:** F-015

## Purpose
Find text in all files of the project and jump to a match, as with the reference's Find in Files.

## Behaviour
1. **Open:** `FindInPath` (⇧⌘F on macOS, Ctrl+Shift+F elsewhere) shows the panel under the editor and
   focuses its query field, which starts with the editor's selection when it lies within one line, else
   with the last query (selected). [GEN, OSS]
2. **Search while typing:** each change of the query, the options (match case, whole words, regular
   expression) or the file mask searches the workspace files in the background (SPEC-0029 behaviour 5:
   exclusions, binary and large files skipped, at most 1 000 matches). A change during a search starts one
   more search after it; the window never waits. The first match is selected. [GEN]
3. **Results:** a row per file (file-type icon, name, folder dimmed, number of matches) followed by a row
   per match (line number, the line's text with the match on `TEXT_SEARCH_RESULT_ATTRIBUTES`). Up and Down
   move the selection through the matches, skipping file rows; a click selects a match. [GEN, OSS]
4. **Summary:** `N matches in M files` in the header, `N+` when the limit cut the search short; the reason
   in red for an invalid regular expression. [GEN]
5. **Open a match:** Enter or a double click opens the file in a tab, selects the match and closes the
   panel; Escape or the close button closes the panel and returns to the editor. [GEN]
6. The MCP counterpart is `search_in_files` (SPEC-0029). [ADR-0008]
7. **v2 – Replace in Files:** `ReplaceInPath` (⇧⌘R on macOS, Ctrl+Shift+R elsewhere) opens the same panel
   titled "Replace in Files" with a "Replace with:" field and the buttons Replace (the selected match)
   and Replace All (every match of the query and file mask; regular expressions expand `$1` groups).
   A file open in an editor changes there, as one undo step and unsaved, like typing; any other file is
   written with a local history revision before and after (SPEC-0044), so it can be restored. The
   status bar reports "Replaced N occurrences in M files" (or the files that could not be written),
   and the results are searched again. `FindInPath` hides the field. The MCP counterpart is
   `replace_in_files_preview` (SPEC-0029). [GEN, OSS]

**v3 (OBS-0023 L31–L34).**
8. **Scope:** a row "In" offers Project, Directory and Open Files. Directory searches below a
   workspace-relative folder (or file) typed in its field, which starts with the folder of the file in
   the editor; Open Files searches the files of the editor tabs. The summary counts only what the
   scope keeps. [GEN; OBS-0023 L31]
9. **Preview:** right of the results the selected match's file around its line, with syntax colours,
   line numbers and the match marked (on by default, a toggle in the head). [GEN; OBS-0023 L32]
10. **Pinning:** a pin in the head keeps the panel open when a match is opened, so the results stay
   for the next match; ⌘↩ (Ctrl+Enter) pins and opens, the counterpart of "Open in Find Window".
   Search Everywhere's Text tab and the find bar's "Open Results in Find Window" open the panel
   pinned. [GEN; OBS-0023 L33]
11. **Comments and strings:** a funnel in the head chooses anywhere, in comments, in string literals,
   except comments, except string literals or except both (SPEC-0065 v4 behaviour 13); the row "In"
   names the choice. [GEN; OBS-0023 L34]
12. **Line breaks:** a button next to the query inserts a line break; the field grows. [GEN]
13. The MCP tool `search_in_files` takes `directory` and `context` (`anywhere`, `in_comments`,
   `in_strings`, `except_comments`, `except_strings`, `except_comments_and_strings`). [ADR-0008]

**v4 (decision of the Project Owner, 2026-11-12; SPEC-0029 v2 behaviours 7–10).**
14. **Own sources:** the scope Project searches only the project's own sources. The scope Directory
   searches a folder that is not the project's own (`.venv/lib`) completely, because it was named;
   Open Files searches the open files whatever they are. [GEN; decision]
15. **Libraries and ignored files:** a toggle of that name in the row "In" (off in a new window; it
   stays as set, visibly marked, until it is turned off) searches everything SPEC-0029 v2
   behaviour 8 allows. Replace and Replace All change a file that
   is not the project's own only while the toggle is on; otherwise the status bar names how many
   files were left unchanged and why, and nothing in them is written. [GEN; decision]
16. **Not searched:** when the search left folders or files out, the line of SPEC-0029 v2 behaviour
   10 follows the summary in the head: `3 matches in 2 files · 2 folders and 1 file not searched
   (libraries and ignored files)`; it is shown even when nothing was found. [GEN; decision]

**v5 (fault found with v4: Replace All replaced by query and file mask in the whole project, whatever
scope and filter the list showed; security first, CLAUDE.md rule 12).**
17. **A replacement changes what the list shows, nothing else:** Replace and Replace All take their
   matches from the same search as the result list — the query with its options, the file mask, the
   scope (Project, Directory, Open Files), the comments/strings filter and the toggle of behaviour
   15. This replaces "every match of the query and file mask" of behaviour 7. In detail:
   - **One search for both.** The replacement runs the search of the list again at the moment of
     replacing, with the same request, and changes only the matches that search returns. There is
     no second rule for scope or filter. A file outside the Directory, a file that is not open (Open
     Files) and a match the filter leaves out (a comment under "except comments") stay as they are.
   - **A file that changed since the list was built** is therefore searched again: what no longer
     matches inside the scope and the filter is not changed. Replace (one match) changes the
     selected match only if the new search still has a match at the same place with the same
     length; otherwise nothing is written and the status bar says that the match is no longer there.
   - **A file open in an editor** is replaced in the editor's text: the file must be in the list
     (the list reads the disk), and its matches are found again in the editor's text with the same
     filter, so unsaved changes are respected.
   - **A file on disk** is written only if it still holds the text the replacement was worked out
     for; if it changed in between, it is left and named in the status bar.
   - **The limit of the list does not limit Replace All:** when the list stopped at 1 000 matches
     (`1000+`), Replace All changes every match of the same search, also those not listed.
   - Behaviour 15 holds unchanged: a file that is not the project's own changes only with the
     toggle; version control data, IDECraft's data, files behind a symbolic link and files outside
     the project never.
   - **Status bar:** `Replaced N occurrences in M files`, followed where it applies by the files
     left unchanged with the reason (behaviour 15), the files that could not be written or changed
     in between, and for Replace the note that the match is no longer there. [GEN; decision
     "security first"]

**v6 (decision of the Project Owner, 2026-11-12: the remaining places follow the rule; SPEC-0029 v5
behaviour 13, SPEC-0318 v8 behaviour 13).**
18. **Remote projects:** the panel of a remote project behaves as behaviours 14 to 16 say. The
   toggle "Libraries and ignored files" is shown and sent to the host as `include_non_project`; the
   Directory scope is sent as `folder` and wins when it names a folder that is not the project's
   own; the Open Files scope sends the open files as `files`, which the host searches whatever they
   are; the host's line on what was not searched follows the summary in the head. Replace in Files
   stays unavailable for a remote project. [GEN; decision]

**v7 (beta screen run of workflow (a), hand test 6).**
19. **The list shows what the editors hold:** a listed file that is open in an editor with unsaved
   changes shows the matches of the editor's text — lines, places and counts in the summary — found
   with the same query, options and comments/strings filter; a file whose editor text no longer
   matches leaves the list. So after Replace All, which changes an open file in its editor without
   saving (behaviour 7), the list searched again no longer shows the replaced text. A file whose
   matches are only in unsaved text is still not listed (the files to search are those on disk).
   Before v7 the list kept showing "2 matches in 1 file" with the old text of the open file after
   Replace All. [GEN]

## Acceptance criteria
- **AK-1** Rows per file and match, match position after trimming the line, selection skipping file rows
  without wrapping, summary texts (unit test).
- **AK-2** In a real window (`idecraft ui --selftest`): ⇧⌘F and typing list `3 matches in 2 files` of the
  Git fixture; Down and Enter open the second match's file.
- **AK-3** Use by hand: options, file mask, large project, scrolling, preview of the reference compared.
- **AK-4 (v2)** Replacements: all, one, with regex groups, char offsets after non-ASCII text (unit test
  `replacements_all_one_and_with_groups`); in a real window ⇧⌘R and Replace All change a closed file on
  disk and an open file in its editor, unsaved, with the count in the status bar (`--selftest`).

- **AK-5 (v3)** Scope and comment filter narrow a search (unit test
  `find_in_files_model::tests::scope_and_context_narrow_the_results`); MCP `directory`/`context`
  (protocol test `search_and_replace_preview_tools`); in the window (OBS-0023 Nachprüfung): Directory
  scope, preview, pinned panel stays open.

- **AK-6 (v4)** The result list carries the line on what was not searched, also without a match; the
  Directory scope and the open files are searched whatever they are; a replacement leaves files
  outside the own sources unless the toggle is on, and the status names them (unit test
  `not_searched_line_and_replacements_in_the_own_sources`); in the window: the toggle, the line in
  the head and the status after Replace All (hand test).

- **AK-7 (v5)** With the text searched in two folders and the scope Directory on one, Replace All
  changes only the files of that folder; with Open Files only the open file; with "except comments"
  the comment keeps its text; a file that changed after the list was built is searched again (a
  match that is gone is not replaced, Replace of a match that moved writes nothing and says so); a
  file open in an editor is replaced in the editor's text; the status line names matches, files and
  what was left (unit tests `replace_all_changes_only_what_the_scope_lists`,
  `replace_all_keeps_what_the_filter_leaves_out`, `replacing_searches_a_changed_file_again`).

- **AK-8 (v6)** The request to the host carries the toggle, the folder of the Directory scope and the
  files of the Open Files scope, and the result list carries the host's line (unit test
  `find_in_files_view::tests::the_host_request_carries_scope_and_toggle`; the host's side: SPEC-0029
  AK-11); in the window of a remote project: the toggle and the line in the head (hand test).

- **AK-9 (v7, behaviour 19)** A listed file with unsaved changes shows the editor's matches and
  leaves the list once its editor no longer matches
  (`find_in_files_model::tests::the_list_shows_the_matches_of_unsaved_editors`); in the harness
  window, after Replace All over an open and a closed file, the list shows no match
  (`workflow_a_beta_handtests.rs::hand_test_6_replace_in_files`).

## Out of scope
Replace in Files for a remote project (since v6 its search follows behaviours 14 to 16). Files whose matches are only in unsaved editor text (the files to search are read on disk; since v7 the matches of a listed file come from its editor), a replace preview per match,
modules and named custom scopes (one module per workspace), search history of the panel.

## Known deviations from the reference
A panel under the editor instead of a floating popup, with the results grouped by file and the preview
beside them (OBS-0023 L35, equivalent); pinning keeps this panel instead of moving the results into a
separate tool window. The search runs on every change without a delay (searches that are overtaken
are not started).
