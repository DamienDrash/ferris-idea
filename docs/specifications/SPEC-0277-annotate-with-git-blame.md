# SPEC-0277: Annotate with Git Blame

- **Version:** 3 · **Date:** 2026-11-06 (v2: annotations per editor view, behaviour 13; v3: regional short date and DST-correct times; OBS-0022)
- **Sources:** GEN (IDEs show "who changed this line, when" as a column beside the line numbers of
  the editor, switched on from the editor's and the tab's menus, the line-number gutter and the
  action search; hovering a line's annotation shows the commit, clicking it leads to the commit in
  the history; edited lines have no commit yet), STD (Git blame semantics, SPEC-0033), OSS (action
  id `Annotate` of `JetBrains/intellij-community` and the colour scheme keys `ANNOTATIONS_COLOR`,
  `ANNOTATIONS_LAST_COMMIT_COLOR`, `VCS_ANNOTATIONS_COLOR_1`…`_5` of its bundled schemes, already in
  `crates/ui/themes`), DOC (jetbrains.com/help: Annotate with Git Blame is in the editor's and the
  gutter's context menus and under Git in the main menu; the annotation shows date and author; a
  click shows the commit), SPEC-0033 (blame, line mapping), SPEC-0064 (HEAD text of open files),
  SPEC-0086 (Git log tool window), SPEC-0275 (context menus), ADR-0008 (MCP), ADR-0009 (look and
  feel), SPEC-0112 (split editors; v2: DOC — jetbrains.com/help describes annotations as a
  property of the editor they are shown in). No observation of the reference: `docs/research/observations` has no notes on annotations,
  and the reference is not operated for this spec.
- **Feature matrix rows:** F-074 (Git), F-002 (editor)

## Purpose
In the editor the user switches on a column that shows, for every line, when it last changed and
who changed it; hovering shows the commit, a click opens it in the Git log. The column follows
edits without being recomputed on the window's thread and closes from its own menu. Agents read
the same blame, for a line range, over MCP.

## Behaviour
1. **Opening:** "Annotate with Git Blame" (action `Annotate`) is in the editor text's menu under
   Git ›, in the editor tab's menu under Git ›, in the menu of a secondary click on the line-number
   gutter, in the main menu under Git › Current File, in the project tree's Git › for a file (opens
   it, then annotates), and in Find Action / Search Everywhere under its title, group Git. It acts
   on the active editor's file. While the column is shown, the same entries read "Close Annotations"
   and close it. [GEN, DOC, OSS]
2. **Column:** a column at the left of the gutter, before the line numbers, shows per line the
   author date (`YYYY-MM-DD`, local time) and the author's name (at most 16 characters, longer
   names end in `…`). Its width is that of the longest label of the file. Lines changed since HEAD
   show "not committed" in the dim annotation colour. [GEN, DOC]
3. **Colours:** the label uses the scheme's annotation colour, the newest commit of the file its
   "last commit" colour; the cell background goes from `VCS_ANNOTATIONS_COLOR_1` (newest fifth of
   the file's commits by time) to `_5` (oldest fifth). Without these keys the cells stay plain.
   [OSS, GEN]
4. **Hover:** resting the pointer on a line's annotation shows a card with the short commit id,
   author with e-mail, date and time, and the whole commit message; on an uncommitted line it says
   that the line changed since the last commit. Leaving the column hides the card. [GEN, DOC]
5. **Click:** a click on a committed line's annotation opens the Git log tool window (SPEC-0086)
   with that commit selected and its details read; a commit older than the loaded history is found
   through the filter by its id. A click on an uncommitted line does nothing. [GEN, DOC]
6. **Column menu:** a secondary click on the column opens a menu with Close Annotations, Show in
   Git Log and Copy Revision Number (both off on an uncommitted line). Close Annotations removes the
   column; the gutter returns to its normal width. [GEN, DOC]
7. **Edits:** the blame is computed once for the HEAD text; the shown lines are mapped onto the
   current buffer with the line diff of SPEC-0033: unchanged lines keep their commit, inserted and
   modified lines are uncommitted, deleted lines vanish. Typing, undo and paste remap; undoing back
   to the HEAD text shows every line's commit again. [GEN, STD]
8. **Refresh:** after a commit made in IDECraft and whenever the HEAD text of the file changes
   (commit elsewhere, checkout, pull, noticed by the status poll of SPEC-0064), the blame of an
   annotated editor is computed again. [GEN]
9. **Cannot annotate:** the action is always offered, and says why it cannot run in the status bar
   instead of being greyed out (IMP-0278): a file outside every Git repository ("… is not in a Git
   repository"), an untracked file ("… is not tracked by Git"), a file added but never committed
   ("… has no committed version yet"), no open editor ("Open a file to annotate it"). [GEN]
10. **Off the window's thread:** the blame (history walk) runs on a background thread; the status
    bar says "Annotating <file>…" meanwhile, and the editor stays usable. Remapping after an edit
    runs on a background thread too; the window only paints the visible rows. A second request
    while one runs is dropped. [GEN]
11. **Budget** (release build, Apple M-series or comparable; recorded here, measured by the test of
    AK-6 with a tolerance for debug builds): blame of a 20 000-line file with 50 commits touching it
    ≤ 2 s; remapping that file after an edit ≤ 50 ms; the window's thread spends no time on either
    (at most one frame's layout for the column). [GEN]
12. **MCP:** `git_blame` takes `path` and optional `start_line`/`end_line` (1-based, inclusive);
    it blames the text an editor shows when the file is open (SPEC-0220), else the file on disk; it
    returns the ranges of SPEC-0033 clipped to the requested lines; an untracked file is an error
    with the message of behaviour 9; a file added but not committed is one uncommitted range.
    [ADR-0008, SPEC-0033]
13. **Per editor view (v2):** annotations belong to one editor view, not to the file. In a split
    view (SPEC-0112) the editor text's menu, the gutter's menu, the column's menu, the main menu
    and Find Action act on the editor view that has the focus (the menus focus the view they were
    opened on); the tab menu of the main group acts on the main group's view: Annotate shows the column in that view only, Close Annotations closes it there only, and
    the label of the menus (Annotate / Close Annotations) follows that view. Both views of one file
    can be annotated independently; each maps the shared buffer's edits onto its own column
    (behaviour 7), each is refreshed when HEAD changes (behaviour 8), and a click or the column menu
    in the second view opens the Git log as in the first (behaviours 5, 6). One background blame
    serves every view of the file that asked for it while it runs (behaviour 10). Closing the
    second group drops its annotations; the first group's stay. [DOC, GEN, SPEC-0112]

14. **v3 – Date format:** the column shows the author date as the region's short date, like the Git log
    (SPEC-0086 v3 behaviour 2: `01.10.26` in Germany, `10/1/26` in the US), and the hover card date
    and time in the same format; both use the zone's offset at the commit's time (SPEC-0086 v6
    behaviour 11). Replaces the fixed `YYYY-MM-DD` of behaviour 2. The author stays the full name
    (IMP-0319). [OBS-0022 L1, L18]

## Acceptance criteria
- **AK-1** Given a scratch repository with commits by two authors, when a file is blamed at HEAD,
  then each HEAD line carries the right commit with author, time and full message; an untracked
  file, an added but uncommitted file and a path outside every repository give the errors of
  behaviour 9. (`crates/vcs/tests/it/git.rs`: `blame_head_with_messages_and_reasons`)
- **AK-2** Given a HEAD blame, when the buffer is edited (insert, modify, delete lines, undo back
  to HEAD), then unchanged lines keep their commit and changed lines are uncommitted; labels, the
  age buckets, the column width and the hover text are as in behaviours 2–4. (`blame_model::tests`)
- **AK-3** Given the menu models, then the editor menu, the tab menu, the gutter menu and the
  project tree's file menu offer Annotate with Git Blame (Close Annotations while shown), the column
  menu offers Close Annotations, Show in Git Log and Copy Revision Number with the enabled states of
  behaviour 6, the main menu has Git › Current File › Annotate with Git Blame, and Find Action finds
  it by its title. (`context_menu::tests`, `tabs::tests`, `menu_bar::tests`,
  `idecraft_languages::everywhere` tests)
- **AK-4** Given the error kinds, then the status texts are those of behaviour 9.
  (`blame_model::tests::reasons_name_the_file`)
- **AK-5** Given MCP, when `git_blame` runs with and without a line range, on an open edited
  buffer and on an untracked file, then it behaves as behaviour 12.
  (`crates/mcp/tests/it/protocol.rs`: `git_blame_ranges_and_reasons`)
- **AK-6** Given a 20 000-line file with 50 commits, when it is blamed and remapped after an edit,
  then both stay within the budget of behaviour 11 (debug builds: ×10).
  (`crates/vcs/tests/it/git.rs`: `blame_of_a_large_file_within_budget`)
- **AK-7** Given the hand test (round 5), then opening from the editor menu, the tab menu, the
  gutter and Find Action, hover, click into the Git log, editing, Close Annotations and the error
  messages behave as above on the project owner's Mac.
- **AK-8** Given a file open in both groups of a split view, when the second view is annotated,
  then only it shows the column; when the first is annotated too and the second closed, only the
  first keeps it; a blame finished while both waited reaches both, a view that closed meanwhile is
  skipped; refresh after a HEAD change covers every annotated view. (`blame_model::tests::
  annotations_belong_to_editor_views`; hand test round 5, Nr. 199)

- **AK-9 (v3)** `de_DE`: `01.11.25`; ISO style: `2025-11-01` and `2025-11-01 03:05`
  (`blame_model::tests::labels_ages_width_and_cells`).

## Out of scope
- Annotating an older revision ("annotate previous revision"), blame ignoring whitespace or moved
  lines, options for the column (name format, date format, show commit number), the per-line
  inline blame after the text.

## Known deviations from the reference
- The action is never greyed out; it says why it cannot annotate (IMP-0278).
- The date is always `YYYY-MM-DD`, independent of the system's short date format, so the column
  sorts and reads the same everywhere.
- v3: the column shows the full author name where the reference shows only the last name
  (IMP-0319); lines changed since HEAD read "not committed" where the reference leaves the cell
  empty, so a changed line is not mistaken for a line without history. The fixed `YYYY-MM-DD` of
  v1/v2 is replaced by the regional short date.
