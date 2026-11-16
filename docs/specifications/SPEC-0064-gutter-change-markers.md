# SPEC-0064: Change markers in the editor gutter

- **Version:** 4 · **Date:** 2026-11-06 (v4: the old lines in the bar are coloured, the marker opens on lines with a run icon and on folded lines, behaviour 12; v2: lines compared as the editor shows them; from hand test 9;
  v3: a click on a marker opens the change bar, behaviours 6–11; OBS-0022 item 6, L7)
- **Sources:** GEN (change markers in the gutter of IDEs; F-074), OSS (editor colour keys
  `ADDED_LINES_COLOR`, `MODIFIED_LINES_COLOR`, `DELETED_LINES_COLOR` of the bundled schemes), SPEC-0032
  (line changes against HEAD), SPEC-0058 (editor view); v3: OBS-0022 item 6 (a click on a marker
  opens a bar with previous and next change, rollback, diff, copy and a field to commit only this
  change), OSS (action ids `VcsShowPrevChangeMarker`, `VcsShowNextChangeMarker`,
  `Vcs.RollbackChangedLines` of the bundled keymaps), SPEC-0084 (diff viewer), SPEC-0034 (commit)
- **Feature matrix rows:** F-074

## Purpose
See in the editor which lines differ from the last commit while editing, as with the reference's change
markers.

## Behaviour
1. **Base:** when a file of a Git repository opens, its text at HEAD is read in the background and is the
   base. A file added to the index but not in HEAD has an empty base (all lines added); an unversioned
   file, a binary file and a file outside Git have no markers. When the repository status changes (commit,
   checkout, changes from outside, checked every 2 seconds as in SPEC-0059 behaviour 7), the bases of the
   open files are read again. [GEN]
2. **Markers:** the line changes of the buffer against the base (SPEC-0032 behaviour 4) are drawn as a
   3 px bar at the right edge of the line numbers: added lines in `ADDED_LINES_COLOR`, modified lines in
   `MODIFIED_LINES_COLOR`; deleted lines as a short mark across the boundary where they were, in
   `DELETED_LINES_COLOR`. [GEN, OSS]
3. **Live:** the markers follow the buffer, not the saved file; they are computed on a background thread
   after each change, so typing never waits for the diff (a change during a diff starts one more diff
   after it). [GEN]
4. The MCP counterpart is `git_diff` (SPEC-0032), which returns the same line changes for the saved
   file. [ADR-0008]
5. **v2 – Lines as shown:** lines are compared without their separators, numbered as the editor shows
   them: a final newline is an empty last line. Removing it marks a deletion after the last line instead of
   a modified last line; `\r\n` and `\n` endings of otherwise equal lines are equal. Changes that touch
   (an inserted line right next to a changed one) form one modified block, as in the reference. [GEN]
6. **v3 – Change bar:** a click on a marker (its bar, or 3 px around it; a deletion mark on the line
   below or above its boundary) opens the change bar of that change just below the change's last
   line, at the gutter's right edge; the caret does not move and no breakpoint is set. The bar is a
   popup whose message field (behaviour 8) has the focus: Escape (back to the editor) or a click
   outside closes it, as does opening a context menu. It shows a row of buttons and,
   below them, the change's text at HEAD (for a modified or deleted change, at most 12 lines, in the
   editor font with the deleted-lines background; an added change shows none). [OBS-0022 item 6]
7. **v3 – Buttons:** *Previous change* and *Next change* (↑ ↓; also `VcsShowPrevChangeMarker` /
   `VcsShowNextChangeMarker` in the editor) move the caret to the first line of the neighbouring
   change, scroll it into view and show the bar there; they stop at the first and last change.
   *Rollback* (`Vcs.RollbackChangedLines` with the caret in a change) replaces the change's lines by
   its HEAD text in the buffer, one undo step, nothing saved. *Show Diff* opens the diff viewer of the
   file (SPEC-0084) at this change. *Copy* puts the HEAD text of the change on the clipboard (an added
   change copies nothing and the button is disabled). [OBS-0022 item 6, OSS]
8. **v3 – Commit this change:** a message field in the bar; Enter (or the Commit button) commits
   only this change: the file's HEAD text with just this change applied becomes the committed
   content of the file, the repository's commit hooks run as for any commit (SPEC-0230), the working
   file and the editor keep every other change, which stays as change markers afterwards. An empty
   message, a file only in the index, a file with a merge conflict or another staged change of the
   same file refuse with the reason in the bar. [OBS-0022 item 6, GEN]
9. **v3 – After an action:** rollback and commit close the bar; the markers follow at once (behaviour
   3; after a commit the base is read again). The status bar names what happened ("Rolled back lines
   4–6", "Committed 1 change of Inventory.java as 3f2a1c9"). [GEN]
10. **v3 – Agents:** MCP `git_line_change` (path, optional 1-based `line`): without `action` it lists
   the file's changes (kind, new and old line range, HEAD text); `action` `rollback` rolls back the
   change containing `line` in the file on disk, `commit` with `message` commits only it; both are dry
   runs unless `dry_run=false`, which needs the `vcs-write` right. An open file with unsaved changes
   is refused for both (save first), so the window and the disk never disagree. [ADR-0008]
11. **v3 – In both views:** the second view of a split file (SPEC-0112) has the same markers and the
   same bar. [GEN]
12. **v4 – Bar details:** the HEAD lines in the bar are coloured by the file's language and their changed words against the current lines are marked as in the diff viewer (SPEC-0084 v2 behaviour 8). On a line that also has a run icon, a click on the marker itself (and the slack to its right) opens the bar; the run icon keeps the space to its left. A marker on a folded line stands for the hidden lines: a click opens the first change inside the fold, which is unfolded first, and the bar opens below it. Before v4 a folded `main` with a change inside could not open its bar, and the old lines were plain. [OBS-0022 Nachprüfung im Fenster 2026-11-06, L7]

## Acceptance criteria
- **AK-1** Visible markers clipped to the visible lines, deletion on the boundary (unit test);
  `Repo::head_text` (via the existing diff tests of SPEC-0032).
- **AK-2** In a real window (`idecraft ui --selftest`): the changed line of a committed file opened from
  the Git fixture shows one modified marker; after deleting a line by key, the editor shows a deletion
  marker without saving.
- **AK-3** Look by hand next to the reference.
- **AK-4 (v2)** Removing the final newline gives one deletion after the last line; CRLF lines compare by
  content (unit test `diff::tests::gutter_markers`).
- **AK-5 (v3)** The change at a gutter line (bars, deletion boundary), previous/next with their ends,
  the HEAD text of a change, the rolled-back text and the text with only one change applied, CRLF
  kept (unit tests `diff::tests::one_change_reverted_and_applied`, `change_bar_model::tests`).
- **AK-6 (v3)** Committing one change of a file with two changes: HEAD holds only that change, the
  working file both, the other change stays a marker; refused with an empty message and with a
  staged change of the file (integration test `crates/vcs/tests/it/git.rs`
  `commit_one_change_keeps_the_others`).
- **AK-7 (v3)** MCP `git_line_change`: lists, dry run and real rollback and commit, refusal without the
  right and for an open file with unsaved changes (protocol test `git_line_change_tool`).
- **AK-8 (v3)** Window self-test steps (written, run later): a click on the modified marker of the
  Git fixture's file opens the bar with the HEAD text; Rollback restores the line without saving;
  Escape closes the bar.
- **AK-9 (v4)** By hand in the window: the bar of a folded `main` opens below the unfolded change; the old line is coloured with the changed number marked.

## Out of scope
Markers in the scrollbar, whitespace-only changes in their own colour, markers against another base
than HEAD (changelists, staged state), (v3) the reference's per-change checkboxes of the commit
window (partial commit of several chosen changes), resizing the bar.

## Known deviations from the reference
Not compared side by side yet; the shape of the deletion mark is our own. (v3) The bar's buttons use
IDECraft's own icons and labels; the commit field commits at once instead of opening the commit
window with the change preselected.
