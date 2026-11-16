# SPEC-0084: Diff viewer

- **Version:** 6 · **Date:** 2026-11-14 (v6: a file that comes to the front by any way closes the viewer, not only a click on its tab, behaviour 6, AK-18; second window check of 2026-11-14); 2026-11-13 (v5: taking a change over in both directions with the rules for what may be written, the current pane, the next and previous file, the settings popup, `compare_files` lists and takes changes, behaviours 17–24; v4: the viewer's actions under their keymap ids, comparing two files of the Project tree, MCP `compare_files`, behaviours 14–16; docs/research/KEYMAP_COVERAGE.md; v3 2026-11-06: changed words stand out in the unified view, behaviour 13; v1: 2026-10-31; v2: the viewer as a tab of the editor area,
  syntax colours, changed words, rollback arrows, the unified view and comparing with a branch,
  behaviours 6–12; OBS-0022 item 8, L22)
- **Sources:** GEN (two versions of a file side by side with changed lines coloured, next/previous
  difference; F-074), OSS (action ids `Diff.ShowDiff` ⌘D / Ctrl+D, `NextDiff` F7, `PreviousDiff` ⇧F7 in
  the ported keymaps; scheme keys `DIFF_INSERTED`, `DIFF_DELETED`, `DIFF_MODIFIED`), SPEC-0032 (Git status
  and diff), SPEC-0063 (Commit tool window); v2: OBS-0022 item 8 (the reference opens the diff as an
  editor tab with syntax colours, marks the changed words, offers arrows that take over a change and
  switches between side by side and unified), SPEC-0027 (syntax highlighting), SPEC-0064 v3 (rolling
  back one change); v5: OSS — `JetBrains/intellij-community` at the pinned commit
  `d4018fb57c367e7b5d895bd8a0c84c382efd0e17`, read as a template for the rules, no code taken:
  `platform/platform-resources-en/src/messages/ActionsBundle.properties` (titles of `Diff.NextChange`,
  `Diff.PrevChange`, `Diff.ApplyLeftSide`, `Diff.ApplyRightSide`, `Diff.FocusOppositePane`,
  `Diff.ShowSettingsPopup`), `platform/diff-impl/src/com/intellij/diff/actions/impl/`
  (`DiffNextFileAction.kt`, `DiffPreviousFileAction.kt`, `DiffFileNavigationAction.kt`,
  `DiffDifferenceNavigationAction.kt`, `DiffNextDifferenceAction.kt`, `FocusOppositePaneAction.java`),
  `platform/diff-impl/src/com/intellij/diff/impl/DiffRequestProcessor.java` (the navigator between
  files) and `DiffSettingsHolder.kt` (going on to the next file is on by default),
  `platform/diff-impl/src/com/intellij/diff/tools/simple/SimpleDiffViewer.java` (which side a change
  is taken into and when), `platform/diff-impl/src/com/intellij/diff/tools/util/side/TwosideTextDiffViewer.java`
  (the current side), `platform/diff-impl/src/com/intellij/diff/util/DiffUtil.java` (a side is
  written only when it can be made writable; one command per taking) and
  `platform/diff-impl/src/com/intellij/diff/tools/util/PrevNextDifferenceIterableBase.java`
  (no step past the first and last difference); shortcuts from the bundled keymaps
  (`crates/ui/keymaps/`)
- **Feature matrix rows:** F-074

## Purpose
See exactly what changed in a file before committing it.

## Behaviour
1. **Open:** a double-click on a file in the Commit tool window, or ⌘D (Ctrl+D) on its selected row,
   opens the diff of that file. [GEN, OSS]
2. **View:** the viewer replaces the editor area until it is closed. A header names the file and the
   number of differences; columns "HEAD" and "Working copy" show the two versions with line numbers in
   the editor font. The working copy is the open editor's text when the file is open (unsaved changes
   count), else the file on disk; a new file has an empty HEAD side. Rows align: unchanged lines side by
   side, replaced lines paired in order with the shorter side left empty, added lines against an empty
   left side, deleted lines against an empty right side; colours come from `DIFF_INSERTED`,
   `DIFF_DELETED` and `DIFF_MODIFIED`. The viewer opens at the first difference. [GEN, OSS]
3. **Navigate:** F7 goes to the next difference and ⇧F7 to the previous one, stopping at the ends; the
   current difference is marked with a line above it and scrolled into view. [GEN, OSS]
4. **Close:** Escape closes the viewer and returns the focus to the Commit tool window (or the editor).
   [GEN]
5. **MCP:** `git_diff` with `side_by_side=true` also returns the rows (kind, left and right line with
   number). [ADR-0008]
6. **v2 – As a tab:** the editor area keeps its tab row while the viewer is shown; the viewer has a tab
   of its own at the end of the row (a diff symbol and the title, with a close button), which is the
   active tab. A click on another tab, or on the diff tab's close button, closes the viewer and shows
   that tab (or the editor's last tab). One viewer at a time: opening another diff replaces it.
   [OBS-0022 item 8]
   **v6 (second window check 2026-11-14):** the diff tab is in front exactly as long as the
   viewer is shown; it is never an open tab behind a file. So every way that brings a file tab of
   the main editor group to the front closes the viewer, as the click on a tab does: opening a
   file (Go to File and the other tabs of Search Everywhere, the Project tree, a navigation such
   as Go to Declaration or a usage, Recent Files, an agent's `open_file`), the next and previous
   tab, the tab switcher and the tab list, and an agent's activation of a tab. Until v6 only the
   click closed it: after Go to File the file was opened and named in the navigation bar while
   the viewer stayed over it and its tab stayed the selected one. That the diff tab goes when
   another tab is chosen is this behaviour, not a defect; a diff that stays open as a tab among
   the file tabs is out of scope (several diff tabs). [GEN]
7. **v2 – Syntax colours:** both sides are coloured by the file's language as the editor colours it
   (SPEC-0027), each side parsed as a whole text; files without a grammar stay plain. The change
   backgrounds of behaviour 2 lie under the colours. [OBS-0022 item 8, GEN]
8. **v2 – Changed words:** in a modified row with both sides, the words that differ (letters, digits
   and `_` form a word; every other character is one) get a stronger background on their side:
   `DIFF_DELETED` on the left and `DIFF_INSERTED` on the right, over the row's `DIFF_MODIFIED`.
   When more than half of a line differs, no words are marked (the whole line is the change).
   [OBS-0022 item 8, GEN]
9. **v2 – Taking over a change:** when the right side is the working copy (behaviour 1, the editor's
   Show Diff and the change bar of SPEC-0064), each change shows `»` at the start of its first row
   between the columns; a click replaces the change's lines in the working copy by the HEAD side:
   in the open editor's buffer as one undo step (nothing saved), or in the file on disk when it is
   not open. The viewer reads both sides again and stays at the same row. The other diffs (stash,
   local history, clipboard, a branch) are read-only and show no arrows. [OBS-0022 item 8]
10. **v2 – Unified view:** a switch in the header (and `U` while the viewer has the focus) shows
   one column instead: unchanged lines with both line numbers, each change's HEAD lines (with their
   left number, `DIFF_DELETED` background, `-` sign) followed by its working copy lines (right
   number, `DIFF_INSERTED`, `+`). F7 / ⇧F7, the changed words and `»` work in both views. The choice
   is kept for the next viewer of the window. [OBS-0022 item 8]
11. **v2 – Compare with a branch:** the editor menu's Git › Compare with Branch… (SPEC-0275 v8) lists
   the local and remote branches and shows the file at the chosen branch on the left (titled with the
   branch) and the working copy on the right, read-only; a file missing on the branch has an empty
   left side. [GEN]
12. **v2 – MCP:** `git_diff` gets `unified=true` (the unified rows: kind, old and new number,
   text) and `revision` (compare the working copy with that branch, tag or commit instead of HEAD);
   the side-by-side rows carry `left_words` / `right_words`, char ranges of the changed words
   (behaviour 8). Taking over a change is MCP `git_line_change` with `action=rollback` (SPEC-0064 v3).
   [ADR-0008]
13. **v3 – Changed words in the unified view:** a row of the unified view already has the colour of its side, so its changed words get a stronger shade of that colour (lighter on a dark theme, darker on a light one); in the side-by-side view they keep the colours of behaviour 8. Before v3 the words were drawn in the row's own colour and could not be seen. [OBS-0022 Nachprüfung im Fenster 2026-11-06, L22]
14. **v4 – Actions by id:** `NextDiff` and `PreviousDiff` (F7 and ⇧F7 in the bundled keymaps) carry out behaviour 3 whenever a viewer is open, so Find Action, the keymap page and a changed shortcut reach them; without an open viewer they are not available. `Diff.ApplyLeftSide` (⌃⌘→ on macOS) takes over the current difference exactly as a click on its `»` does (behaviour 9); it is available only in a viewer whose right side is the working copy and while a difference is current. Until v5 `Diff.ApplyRightSide`, `Diff.NextChange`, `Diff.PrevChange`, `Diff.ShowSettingsPopup` and `Diff.FocusOppositePane` were not carried out; behaviours 17–24 define them. [OSS: ids and shortcuts of the bundled keymaps; GEN]
15. **v4 – Compare two files:** `CompareTwoFiles` (⌘D, Ctrl+D elsewhere) with exactly two files selected in the Project tree shows them in the viewer: the upper row of the tree on the left, the lower one on the right, each column titled with its file name (with the path relative to the project when both names are equal), read-only without `»`, coloured by the language of the right file. With its key the action runs only while the Project tree has the focus (elsewhere the key keeps its other actions); from Find Action it uses the tree's selection. Anything else than two files selected, a folder among them, or a file that cannot be read as text is refused with the reason in the status line. [OSS: id and shortcut; GEN]
16. **v4 – MCP:** `compare_files` with `left` and `right` (workspace paths) returns `differences` and the `rows` of `git_diff` with `side_by_side` (kind, both lines with numbers, word ranges), with `unified=true` also the unified rows. It reads only (until v5, behaviour 23). [ADR-0008]
17. **v5 – What a side is and whether it may be written:** each side of a viewer is either a *file* of the project (the open editor's text when the file is open, else the file on disk) or *fixed text*. Files are: the working copy on the right of Show Diff (behaviour 1) and of a comparison with a branch or commit in a local project (behaviour 11), and both sides of Compare Two Files (behaviour 15). Everything else is fixed text and is never written: HEAD, a branch or commit, a stash, a local history revision, the clipboard, and every side in the window of a remote project. A file side is not written either when the file no longer exists, when the file system marks it read-only, or when its editor is read-only (a library source, SPEC-0149 v4). There is no question that would lift one of these: the refusal names the reason in the status line and nothing changes. [OSS: a side takes a change only when it is editable and its document can be made writable; GEN]
18. **v5 – Taking a change over:** `Diff.ApplyLeftSide` ("Accept Left Side"; ⌃⌘→ on macOS, Ctrl+Alt+R elsewhere) puts the left lines of the current difference in place of its right lines; `Diff.ApplyRightSide` ("Accept Right Side"; ⌃⌘←, Ctrl+Alt+A) puts the right lines in place of the left ones. An added block is inserted, a deleted block removed, a changed block replaced; where the target's lines are already equal nothing is written and the status line says so. The side written is the other one and must be writable by behaviour 17, so in Show Diff only "Accept Left Side" can run (the left side is HEAD) and in Compare Two Files both can. `»` of behaviour 9 is the same operation for one difference; it is now also shown for a comparison with a branch or commit in a local project. The lines taken get the line break of the file they go into (the first one found there, else the source's); a last line without a line break stays without one when it ends the file, and gets one when lines follow it. Before anything is written both sides are read again: when they no longer give the rows shown, nothing is written, the viewer shows the new rows and the status line says so. In an open editor the change is one undo step and nothing is saved; a file that is not open is written on disk, as with `»`. Afterwards the viewer reads both sides again and stays at the same place. While the viewer has the focus the two keys do nothing else, also when the action is refused. [OSS: direction, editability, one command per taking, the key is used up; GEN: line breaks, reading again]
19. **v5 – The current pane:** the side-by-side view has a current pane, the right one when a viewer opens; its column title is drawn in the text colour with a line under it, the other one muted. `Diff.FocusOppositePane` ("Select Opposite Diff Pane"; ⌃⇧Tab) makes the other pane current. The place does not move: both panes show the same rows, so the current difference and the scroll position stay, and the status line names the pane and its line at the current difference (where that pane has no line there, the line above it). The unified view has one pane; there the action says so. The reference keeps a caret per pane and has a second action that carries the position over (`Diff.FocusOppositePaneAndScroll`, without a key in the bundled keymaps); IDECraft's panes share their rows, so the one action is enough. [OSS: the action and its key; GEN]
20. **v5 – Next and previous file:** in the open platform `Diff.NextChange` and `Diff.PrevChange` are "Compare Next File" and "Compare Previous File" (⌃⇧→ and ⌃⇧← on macOS, Alt+Shift+Right and Alt+Shift+Left elsewhere): a *change* there is a changed file of the list the viewer was opened from, not a difference within a file (that is `NextDiff`, behaviour 3). A viewer of Show Diff has such a list: the files of the Commit tool window in the order of its list, or, while that window was never opened, the changed files in the order Git reports them (sorted by path; files Git does not know yet are among them, as in the Commit tool window). The actions show the diff of the next or previous file of the list, opened at its first difference. They do not wrap: at the last (first) file nothing changes and the status line says that there is no further file; the key is used up all the same. The other viewers have no list; there the status line says so. [OSS]
21. **v5 – F7 across files:** with a file list (behaviour 20), F7 on the last difference of a file — or in a file without differences — does not stop silently: the first press says in the status line that another press goes to the next file, the second press shows the next file at its first difference. ⇧F7 on the first difference does the same towards the previous file and shows it at its last difference. A step that moves within the file, a step in the other direction and any change of the viewer forget the first press. At the last file (first file) and in viewers without a list F7 (⇧F7) stops as in behaviour 3. [OSS: two presses, first and last change; on by default]
22. **v5 – Settings popup:** `Diff.ShowSettingsPopup` ("Show Diff Settings Popup…"; ⇧⌘D on macOS, Ctrl+Shift+D elsewhere) lists the settings the viewer has: the view, side by side or unified, with the current one marked; choosing the other one switches as the header's switch does (behaviour 10); afterwards the viewer has the focus again. Settings IDECraft's viewer does not have (ignoring whitespace, the highlighting mode, soft wraps and the others of the reference's popup) are not listed. [OSS: id, title and key; GEN]
23. **v5 – MCP and automation:** `compare_files` also returns `changes`: for each difference its 1-based `index`, `kind`, the line ranges `left_start`/`left_end` and `right_start`/`right_end` (1-based, inclusive; an empty range has end = start − 1) and `left_text`/`right_text`. With `apply` (an index) and `take` (`left` or `right`, the side whose lines are taken) it returns `target` (the path that would change), `text` (its new content) and `changed`; `dry_run` defaults to true. With `dry_run=false` it needs the `write` right and writes the target, unless the file system marks it read-only or the window holds unsaved changes of it. For the working copy against HEAD the tool stays `git_line_change` (SPEC-0064 v3). For a client driving the window (`crates/uidriver/src/security.rs`) the two taking actions are declared as *write*, the others as without effect. [ADR-0008]
24. **v5 – Where the actions are:** none of them has a menu bar item; like the other actions of the viewer they are actions of a focused part (SPEC-0124 v4 behaviour 7): their keys work while the viewer has the focus, Find Action lists them in the group "Diff", and the Keymap settings page shows and changes their keys. From Find Action they act on the open viewer.

## Acceptance criteria
- **AK-1** Alignment of unchanged, modified (uneven), added and deleted lines; difference starts (unit
  test `side_by_side_aligns_changes`).
- **AK-2** MCP `git_diff` with `side_by_side` returns a modified row with its old line number
  (protocol test).
- **AK-3** In a real window (`idecraft ui --selftest`): a double-click on `bad.c` in the Commit tool
  window opens its diff with at least one difference selected; F7 keeps one selected; Escape closes.
- **AK-4** Use by hand: colours, long files, F7 / ⇧F7 scrolling, ⌘D.
- **AK-5 (v2)** Changed words of paired lines, nothing marked when most of the line differs; unified
  rows with numbers and signs; the change of a row and its reverted text (unit tests
  `diff::tests::changed_words_of_paired_lines`, `diff::tests::unified_rows_follow_the_changes`,
  `diff_model::tests`).
- **AK-6 (v2)** Syntax colours per side map onto the rows' lines (unit test
  `diff_model::tests::syntax_runs_follow_their_lines`).
- **AK-7 (v2)** MCP `git_diff` with `unified`, `revision` and the word ranges (protocol test
  `git_diff_unified_words_and_revision`).
- **AK-8 (v2)** Window self-test steps (written, run later): the diff opens with the tab row and its
  own tab, `U` switches to the unified view and back, `»` on the first change restores the HEAD line
  in the editor without saving, a click on the file's tab closes the viewer.
- **AK-9 (v3)** The stronger shade differs from the row colour on dark and light themes (unit test `diff_view::tests::changed_words_get_a_stronger_shade_of_the_row`).
- **AK-10 (v4)** Which difference `Diff.ApplyLeftSide` acts on and when the viewer's actions are available; the pair `CompareTwoFiles` compares and its refusals (unit tests `keymap_actions::tests::compare_pair_is_two_files_in_tree_order`, `keymap_actions::tests::scoped_actions_run_first_and_only_in_their_focus`). In the window (open): F7 from Find Action steps, ⌘D on two files of the tree opens the viewer.
- **AK-11 (v4)** MCP `compare_files` returns the rows of two workspace files and refuses a folder (protocol test `compare_files_returns_the_rows_of_two_files`).
- **AK-12 (v5)** Stepping: within a file, at the first and last difference, in a diff without differences, the two presses towards another file and what forgets the first one; the next and previous file of a list without wrapping (unit tests `diff_actions::tests::steps_stop_at_the_ends_or_ask_for_a_second_press`, `diff_actions::tests::the_neighbour_file_does_not_wrap`).
- **AK-13 (v5)** Taking over an added, a deleted and a changed block in both directions; CRLF files keep their line breaks and a taken LF line gets the target's; a last line without a line break; lines that are already equal (unit tests `diff::tests::a_change_is_taken_in_both_directions`, `diff::tests::taken_lines_get_the_line_break_of_their_target`, `diff::tests::the_last_line_without_a_line_break`).
- **AK-14 (v5)** What may be written: fixed text, a remote project, a missing file, a read-only file and a read-only editor are refused with their reason (unit test `diff_actions::tests::only_a_writable_file_side_is_written`).
- **AK-15 (v5)** In a window of the test harness (`crates/workbench/tests/it/diff_viewer_actions.rs`): Accept Left Side changes the working copy in the editor as one undo step and Accept Right Side is refused against HEAD; both directions between two files, and a read-only file stays as it is; the current pane switches; the next and previous file with F7's two presses; the settings popup switches the view.
- **AK-16 (v5)** MCP `compare_files` lists the changes, previews one by default, writes it with `dry_run=false` and the `write` right, and refuses a read-only target (protocol tests `compare_files_lists_and_takes_changes`, `compare_files_writes_only_with_the_write_right`).
- **AK-17 (v5)** The actions have their titles, group and keys in both keymaps, and their declared effects (unit tests `everywhere::tests::diff_viewer_actions_have_titles_and_keys`, `security::tests::diff_viewer_actions_declare_their_effects`).
- **AK-18 (v6, behaviour 6)** In a window of the test harness
  (`window_check_1114b.rs::a_file_opened_over_the_diff_tab_becomes_the_tab_in_front`): with the diff
  tab in front, the file chosen in Go to File is the active tab with the focus in its editor and the
  viewer is closed; the next tab and opening a file that is open already close it as well.

## Out of scope
Editing the working copy in the viewer, comparing files outside the Project tree or more than two, comparing folders, ignoring whitespace,
synchronised scrolling of two independent editors, (v2) several diff tabs at once, moving a diff tab,
the reference's per-change checkboxes (partial commit from the diff), taking a working copy change
over to HEAD (the HEAD side is fixed text, behaviour 17), (v5) typing in a pane, a caret or selection in
a pane and taking several selected differences at once, appending instead of replacing
(`Diff.AppendLeftSide`, `Diff.AppendRightSide`), the settings of the reference's popup that IDECraft's
viewer does not have, a file list for viewers other than Show Diff, lifting a file's read-only mark
from the viewer.

## Known deviations from the reference
v1: the reference opens the diff in an editor tab and lets you edit the right side; IDECraft shows a
read-only view over the editor area. v2: the viewer has a tab in the tab row but is not one of the
editor's file tabs (it is not kept across restarts and not counted towards the tab limit); the right
side stays read-only except for `»`. Changed words are found by IDECraft's own rule (behaviour 8), not
the reference's. v5 (against the open sources; the reference's window was not observed for v5): the
open platform takes the differences under the caret or selection of the focused editor; IDECraft takes
the current difference (the one F7 marked). A side that cannot be written is refused without a question
whether to clear the read-only mark (the more careful choice, CLAUDE.md rule 12). Before writing,
IDECraft reads both sides again and refuses when they changed (behaviour 18). Files that differ only in
their line breaks still show every line as changed (behaviour 2); taking such a block writes nothing.
