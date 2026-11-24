# SPEC-0101: Resolving merge conflicts

- **Version:** 8 · **Date:** 2026-11-18 (v8: file tabs keep their width and roll instead of shrinking to a sliver, the note of Resolve Simple stays on one line in the header; behaviour 28, window check of 2026-11-18 M1; v7: the result column is an editable text with undo, sides are taken and ignored in the gutters, changed words are marked, F7 / ⇧F7 step through changes, Close asks before unapplied work is dropped; window check of 2026-11-14 and the points OBS-0009 lists for the reference's merge view; v6: binary files and files one side deleted take a whole side, left-over markers and a missing base are refused; found by driving real conflicts headless; v2: sides of a rebase, OBS-0009; v3: simple conflicts, opening by itself, finishing the operation; v4: the view closes when an operation leaves no conflict, hand tests 34 and 71, D15; v5: header never hides its buttons, an unmodified editor takes the result from disk; OBS-0022)
- **Sources:** GEN (after a merge, rebase or cherry-pick stops with conflicts, the IDE lists the
  conflicted files and opens a three-column merge editor — your version, the result, their version —
  where each conflict takes one side or both, "Accept Left"/"Accept Right" take a side for all, and
  Apply marks the file resolved; F-075), STD (index stages 1 base, 2 ours, 3 theirs of an unmerged
  path; `git add` marks it resolved), OSS (action id `Git.ResolveConflicts`), SPEC-0037 (three-way
  merge model), OBS-0009 (the reference's conflict dialog and merge view for a merge and a rebase;
  for v7 its result 4: an editable result column that starts with the base text, accept and ignore
  controls between the columns, stepping to the previous and next change, a counter of changes and
  conflicts, one step for the changes that do not conflict), OBS-0022 and OBS-0038 (side by side),
  SPEC-0003 and SPEC-0025 (text buffer, caret and selection the result column reuses), SPEC-0084 v2
  behaviour 8 (changed words of a line pair); v8: GEN (tabs that do not fit roll; a status text in
  a toolbar is cut with an ellipsis), window check of 2026-11-18 (M1: with data.bin and notes.txt
  in a view about 1000 points wide the second tab showed only "n", and "1 simple conflict
  resolved" ran over three lines into the column headers)
- **Feature matrix rows:** F-075

## Purpose
Finish a stopped merge without leaving the IDE or editing conflict markers by hand.

## Behaviour
1. **Open:** `Git.ResolveConflicts` (Search Everywhere) reads the unmerged paths of the repository's
   index with their base, ours and theirs text and opens the merge view over the editor area on the
   first file; the file names are tabs at the top. Without conflicts the status bar says "No merge
   conflicts". Binary sides count as missing. [STD, GEN]
2. **Columns:** "Yours (HEAD)", "Result" and "Theirs" show the file region by region (the merge of
   SPEC-0037): unchanged regions plain, changes of one side in the scheme's modified (ours) or
   inserted (theirs) colour and already in the result, conflicts in the conflict colour. An open
   conflict shows "← Left", "Both" and "Right →" in the result column; a resolved one shows its text.
   In a stopped rebase HEAD is the target branch, so the sides are swapped for the user: the left
   column is the branch being rebased ("Yours (branch)", Git's stage 3) and the right one the target
   ("Theirs (rebase target)", stage 2), as the reference names them. [GEN, STD, OBS]
3. **Accept Left / Accept Right** take that side for every open conflict of the file; the header
   counts the open conflicts ("1 of 2 conflicts open", "All conflicts resolved"). [GEN]
4. **Apply** (only with no open conflict) writes the result to the file, marks it resolved (`git add`)
   and goes on with the next file, or closes the view after the last. An editor showing the file
   takes the new text. Errors appear above the columns. **Close** or Esc leaves the conflicts as they
   are. [GEN, STD]
5. Agents list conflicts with `git_conflicts` (sides, automatic merge with markers, open conflicts)
   and resolve one file with `git_resolve_conflict` (`take` ours/theirs/base or `text`; dry run by
   default). [ADR-0008]

6. **v3 – Resolve Simple:** the toolbar button "Resolve Simple" resolves every simple conflict of the
   shown file (SPEC-0037 v2 behaviour 8); the status says how many; the rest stay open. [GEN, OBS-0009]
7. **v3 – Opens by itself:** when a merge, rebase, cherry-pick, pull, stash apply or Git: Continue started
   in Ferris IDEA stops with conflicts, the merge view opens on them. [OBS-0009]
8. **v3 – Finishing:** Apply on the last conflicted file of a waiting merge, rebase or cherry-pick runs Git:
   Continue — the merge commit gets Git's message (OBS-0009: no further question); a rebase replays the
   next commits and, if one stops again, the view shows the new conflicts. Without a waiting operation (a
   stash that did not apply cleanly) the view just closes. [OBS-0009, GEN]
   **v4:** whenever a Git operation started in Ferris IDEA ends — Continue, Abort or Skip from the branch
   popup or Search Everywhere, a merge, rebase or cherry-pick — and no conflicted file is left, an open
   merge view closes and the keys go back to the editor; if conflicts are left, it shows them anew.
   [GEN]
9. **v3 – MCP:** `git_resolve_conflict` takes `take: "simple"`, which resolves the simple conflicts and is
   refused when conflicts would stay open (naming how many). [ADR-0008]

10. **v5 – Header:** the file tabs of the header give way (v8: as a row that rolls, behaviour 28) so that the conflict count
    and the buttons Resolve Simple, Accept Left, Accept Right, Apply and Close are always visible, also
    with a side tool window open; before v5 Apply and Close were pushed out of the window. [GEN,
    OBS-0022 L3]
11. **v5 – Editor after Apply:** an open editor of the resolved file without unsaved edits takes the
    file from disk and stays unmodified (no `*` on its tab); one with unsaved edits gets the result as
    an undoable change. [GEN, OBS-0022 L4]
12. **v6 – Whole sides:** a conflicted file is resolved *as a whole* instead of region by region when
    one of its sides is not text — it holds a NUL byte, is not valid UTF-8, or is no regular file (a
    symbolic link, a submodule) — or when ours or theirs has no such file (one side deleted it, the
    other changed it). Such a file is never merged as text and never written from text the IDE built:
    - it counts as one open conflict; the view shows each side's text, "Binary file" or "No file on
      this side" in its column, and offers only the two sides ("← Left", "Right →", Accept Left,
      Accept Right); Both and Resolve Simple do not apply;
    - taking a side that has the file writes exactly what the index holds for it (stage 2 for ours,
      3 for theirs, 1 for the base), byte for byte, and marks the path resolved;
    - taking a side that has no file removes the file from the working tree and stages the removal;
    - an open editor of the file without unsaved edits takes the file from disk; one with unsaved
      edits is left alone.
    Before v6 every missing or binary side was read as empty text: a binary file listed no conflict
    at all, and Apply or `take` wrote an empty file over it; taking the side that deleted a file left an
    empty file instead of removing it. A file whose sides are all text and that only lacks a base (both
    sides added it) is merged as text as before. [STD, GEN]
13. **v6 – No markers as a resolution:** `git_resolve_conflict` with `text` refuses a text that has
    conflict markers (SPEC-0037 v3 behaviour 10) and changes nothing, so a file is not marked resolved
    with its conflict still in it; `allow_markers: true` writes it all the same, for the rare file
    that holds such lines on purpose. [GEN]
14. **v6 – MCP for whole sides:** `git_conflicts` reports per file `binary`, `has_base`, `has_ours`
    and `has_theirs`; a binary file has `merged: null`, and every whole-side file `open_conflicts: 1`.
    `git_resolve_conflict` with `take` ours or theirs follows behaviour 12 for such a file and answers
    with `whole_side` and `deleted` (`text` is the side's text, `null` when binary or removed); `take:
    "base"` is refused when the file has no base, instead of writing only what both sides share;
    `take: "simple"` is refused for a whole-side file. In a stopped rebase `ours` and `theirs` stay
    Git's stages 2 and 3 (the target and the replayed commit), as `git_conflicts` lists them. [ADR-0008]

15. **v7 – The result is a text:** the middle column holds the result of the shown file as one
    text that the user edits like a file: unchanged regions as they are, changes of one side or
    of both sides alike already applied (behaviour 2), and for an open conflict the base text of
    its region, in the conflict colour, until it is resolved. The side columns stay read-only. In
    the result column a click places the caret (⇧-click and dragging select); typing, Enter (the
    file's line end), Tab, Backspace and Delete change the text; the arrow keys, word moves, line start
    and end, document start and end move the caret and with Shift select; Select All, Copy, Cut,
    Paste, Undo and Redo have their usual keys (⌘A ⌘C ⌘X ⌘V ⌘Z ⇧⌘Z, with Ctrl instead of ⌘
    outside macOS; Ctrl+Y also redoes there). Keys the result column does not use stay with the
    workbench. The rows of the three columns stay level: a region is as high as its longest
    column, and a line of the result belongs to the region its first character lies in. A file tab
    whose session holds choices or edits not applied carries a `*`. [GEN, OBS-0009]
16. **v7 – Taking and ignoring a side:** between the columns, next to every change and conflict,
    stand the controls of the side beside them: `»` on the left and `«` on the right take that
    side, `×` ignores it.
    - An open conflict: taking a side replaces the region's base text with that side and resolves
      the conflict in this one step (IMP-0039). Ignoring one side leaves the conflict open for the
      other; ignoring both resolves it with the base text.
    - A resolved conflict keeps the take control of a side not yet taken: it appends that side
      below what the region holds (left then right, or right then left — "both"), and never
      replaces text.
    - A change that does not conflict is applied from the start; `×` puts the base text back
      (the change is *ignored*), `»`/`«` then applies it again.
    Accept Left, Accept Right and Resolve Simple of the header work as before, on the open
    conflicts. What a control inserts is ordinary text of the result and can be edited further;
    the caret goes to the start of the region a control changed.
    The buttons "← Left", "Both" and "Right →" inside the result column are gone. [GEN, OBS-0009]
17. **v7 – Resolved by hand:** an edit that changes text inside the region of an open conflict
    marks that conflict resolved (by hand); one that only touches other regions does not. An edit
    that spans several regions counts for each it changes. A conflict whose base region is empty
    takes text typed at its place. [GEN]
18. **v7 – Undo and redo** belong to the merge session of the file: every edit, taken or ignored
    side, Accept Left/Right and Resolve Simple is one step that Undo takes back with the text, the
    state of every conflict and the caret; typing in a row is one step. The steps are gone when the
    file is applied or the view closes; they never reach the undo history of an editor. [GEN]
19. **v7 – Sessions per file:** each conflicted file of the view has its own session; changing the
    file tab keeps the choices and edits of the other files (before v7 they were dropped). [GEN]
20. **v7 – Apply writes the result text:** Apply writes exactly the text of the result column, with
    its line ends as they are, and marks the file resolved. It is refused while a conflict is open.
    A result that has conflict markers (SPEC-0037 v3 behaviour 10) — typed or pasted by hand — is
    not written at once: the view asks, naming the markers, and writes only on "Apply Anyway" (`A`);
    Escape, Enter or "Keep Resolving" go back. This is the view's form of `allow_markers`
    (behaviour 13). [GEN]
21. **v7 – Nothing on disk before Apply:** choices and edits live in memory only. The view writes
    no file, no index entry, no swap or recovery file and no local-history revision until Apply; a
    crash or a closed view leaves the repository as the stopped operation left it. [GEN,
    security]
22. **v7 – Whole-side files stay whole-side:** a binary file, a file that is not UTF-8 and a file
    one side deleted (behaviour 12) have no editable result: typing, pasting and `×` do nothing
    there, and only `»` and `«` (or Accept Left/Right) choose the side. [GEN, security]
23. **v7 – Changed words:** in the side columns the words of a line that differ are marked with a
    stronger shade of the region's colour (as the diff viewer does, SPEC-0084 v2 behaviour 8 and v3
    behaviour 13). A side's line is compared with the base line at the same place when the region
    has as many lines on that side as in the base; otherwise, in a conflict whose sides have
    equally many lines, with the other side's line. Lines that differ in more than half are not
    marked. [GEN, OBS-0009 (the reference marks changes inside lines)]
24. **v7 – Next and previous change:** F7 moves the caret to the start of the next change or
    conflict of the result after the caret's region, ⇧F7 to the previous one, and scrolls it into
    view; at the last or first one the caret stays and the header says so. The header counts what
    is left: the changes that do not conflict (with the number ignored, if any) and the open
    conflicts of the file. [GEN, SPEC-0084 behaviour 3]
25. **v7 – Close asks:** Close and Escape ask before they drop work — when a session of the view
    holds a taken or ignored side or an edit that is not applied. The question names the number of
    files; "Discard and Close" (`D`) closes, Escape, Enter or "Keep Resolving" go back. A view in
    which nothing was done closes at once: nothing is lost, the conflicts stay as they are and the
    branch widget goes on naming the stopped operation. A view that closes because the operation
    ended (behaviour 8, v4) does not ask. [GEN]
26. **v7 – MCP:** `git_resolve_conflict` answers for a file merged as text with the state of the
    resolution as the view would show it: `result` (the result text; an open conflict contributes
    its base text, never markers), `open_conflicts`, and `conflicts` — for every conflict `index`,
    `open`, `simple`, `start_line` and `lines` (its place in `result`, lines counted from 1) and
    the `base`, `ours` and `theirs` text of the region. `choices` resolves single conflicts:
    entries `{conflict, take}` with `take` ours, theirs, both, base or simple, or `{conflict,
    text}`; conflicts without an entry stay open. Called with neither `take`, `text` nor `choices`
    it is a read of that state (dry run only). `dry_run: false` is refused while a conflict would
    stay open, and a `choices` text with conflict markers is refused like `text` (behaviour 13).
    `text` in the answer is the text that is (or would be) written and `null` while a conflict is
    open. A whole-side file answers the read with its state (`binary`, `has_base`, `has_ours`,
    `has_theirs`, `open_conflicts: 1`) and refuses `choices`. A file whose name commonly holds
    secrets is refused as before unless the agent may read secrets (SPEC-0053 v6): neither the
    read nor `choices` gives any of its text. The tool works on the index; choices made in
    an open merge view and not applied are not visible to it. [ADR-0008, SPEC-0053 v6]
27. **v7 – Non-conflicting changes in one step (stated):** Ferris IDEA applies every change of the
    left side, of the right side and of both sides alike when the file opens (behaviour 2, SPEC-0037
    behaviour 2) — the user needs no step for it; behaviour 16 takes a single one back. [GEN,
    OBS-0009]
28. **v8 – Header row:** the header is one row of 36 points. Each file tab keeps the width of its
    name and is never squeezed; when the tabs do not fit, their row is cut at its right edge and
    rolls sideways (wheel or trackpad), and choosing a file rolls its tab into view. The note of
    Resolve Simple or a step ("1 simple conflict resolved") stands on one line between the tabs
    and the count, takes only the room the tabs leave and ends with "…" when it is cut; it never
    wraps and never covers the column headers. The count and the buttons keep their full width
    (behaviour 10). The tabs and the note are nodes of the semantic tree (`merge-file-<n>`,
    selected for the file shown, and `merge-note`). Until v7 the tabs shrank to slivers and the
    note wrapped below the header. [GEN]

## Acceptance criteria
- **AK-1** A real stopped merge: the three sides come from the index; resolving writes the file and
  clears the conflict; a file without conflict is refused (`crates/vcs/tests/it/git.rs`:
  `conflicts_and_resolving_them`).
- **AK-2** MCP: listing, dry run (file unchanged) and writing a hand-written result
  (`crates/mcp/tests/it/protocol.rs`: `conflict_tools_resolve_a_stopped_merge`).
- **AK-3** In a real window (`ferris-idea ui --selftest`): a merge that stops with a conflict opens in the
  view with one open conflict; taking the right side and Apply writes it and clears the conflict.
- **AK-5 (v2)** A stopped rebase names the branch being rebased, and its stage 2 is the target
  (`crates/vcs/tests/it/ops.rs`: `rebase_replays_and_continues_after_resolving`).
- **AK-6 (v3)** `take: "simple"` on a file with only simple conflicts writes the word-merged text; a file
  with a real conflict is refused (protocol test).
- **AK-7 (v3)** In a real window: a merge from the branch popup stops, the view opens by itself,
  Resolve Simple resolves the simple conflict, Accept Right and Apply finish the merge with a merge
  commit (`--selftest`).
- **AK-8 (v4)** In a real window: Abort of a stopped rebase from the branch popup, with the merge view
  still open behind it, closes the view (`git_m63_steps`).
- **AK-4** By hand: a merge and a rebase with several conflicted files next to the reference.

- **AK-9 (v5)** In the release bundle (1400 × 954 pt, Commit tool window open): Apply and Close are
  visible; after Apply the tab of the open file carries no `*` (OBS-0022 Nachprüfung).
- **AK-10 (v6)** On the repositories of `crates/vcs/tests/fixtures/conflict-repos.sh`, each stopped
  by a merge, a rebase and a cherry-pick: the listed sides equal the index stages; a binary file and a
  file that is not UTF-8 take either side byte for byte; taking the deleting side removes the file,
  taking the other keeps its text; afterwards `git status` shows no unmerged path and continuing
  finishes the operation, while continuing with a file still unresolved is refused and leaves the
  operation waiting (`crates/vcs/tests/it/conflicts.rs`).
- **AK-11 (v6)** The model of one file: a whole-side file is open until a side is taken, takes no Both
  and no simple resolution, and its outcome is that side, never a text; in a rebase left and right are
  stages 3 and 2 (unit tests in `crates/vcs/src/resolve.rs`).
- **AK-12 (v6)** MCP: the fields of behaviour 14; a text with markers is refused and the path stays
  unmerged, `allow_markers` writes it; `take: "base"` without a base and `take: "simple"` on a binary
  file are refused; taking the deleting side removes the file (protocol test).

- **AK-13 (v7)** The session of one file: the result starts with the changes applied and the base
  text in an open conflict; typing, deleting, pasting and cutting change the result; an edit inside
  an open conflict resolves it by hand, an edit elsewhere does not, an edit across regions counts
  for each; what Apply writes is exactly the result text, CRLF line ends included (unit tests in
  `crates/workbench/src/merge_session.rs`).
- **AK-14 (v7)** Taking a side resolves a conflict in one step; taking the other side afterwards
  appends it; ignoring one side leaves it open, ignoring both leaves the base text; ignoring a
  change that does not conflict puts the base back and taking it applies it again (unit tests).
- **AK-15 (v7)** Undo and redo restore text, conflict states and caret across typing, taken sides,
  Accept Left and Resolve Simple; typing in a row is one step (unit tests).
- **AK-16 (v7)** Changed words of the side columns: against the base with equal line counts,
  against the other side otherwise, none for lines that differ too much (unit test).
- **AK-17 (v7)** Next and previous change move the caret region by region and stop at the ends;
  the counter names changes, ignored changes and open conflicts (unit tests).
- **AK-18 (v7)** A whole-side file takes no edit and no ignore; a session without choices and edits
  is not dirty, one with them is; a result with conflict markers is recognised (unit tests).
- **AK-19 (v7)** MCP: reading the state lists the result, the conflicts with their lines and
  sides; `choices` resolves single conflicts; writing with a conflict left open is refused and
  changes nothing; a secret-named file is refused without any of its text (protocol test
  `conflict_tool_reads_the_result_and_takes_choices`).
- **AK-20 (v7)** In a real window (`ferris-idea ui --selftest`, `merge_edit_steps`): in a stopped
  merge F7 reaches the conflict, typed keys change the result and resolve the conflict by hand, ⌘Z
  opens it again, ⇧F7 goes back, a click on `»` takes the left side and typing edits it, Escape
  asks and Escape again keeps the view, Apply writes exactly the edited text and finishes the
  merge; before Apply the file on disk still holds Git's markers. The clicks on `»` and Apply go by the
  controls' test ids through the UI driver (`FERRIS_IDEA_SELFTEST_DRIVER=1`, SPEC-0319 behaviour 44);
  without it the group checks the keys and the question, closes the view and aborts its merge.
- **AK-21 (v8)** In a window of the test harness (`crates/workbench/tests/it/merge_header_layout.rs`:
  `header_keeps_tabs_whole_and_the_note_on_one_line`), with data.bin (whole side) and notes.txt
  (one simple conflict), 1000 and 1200 points wide: both tabs are wider than 50 points and stand in
  the header, the shown tab and Close are wholly in view; where the second tab is beyond the row's
  edge the wheel rolls the row until it is wholly in view; after a click on it and on Resolve
  Simple the note is one line high (a tab without its padding) and ends inside the header, the
  tabs keep their width and Apply and Close stay wholly in view.

## Out of scope
A conflicts list in the commit window, three-way merge of binary files, showing a binary side's
content, files in an encoding other than UTF-8 as text (they take a whole side, v6). In the result
column: several carets, syntax colours, line numbers, code completion, find and replace, input
methods that compose characters, a blinking caret, and selecting or copying text in the side
columns. A question about an unapplied merge session when the
project window closes or the IDE quits (nothing on disk is lost). Reading the unapplied state of an
open merge view through MCP (behaviour 26 works on the index).

## Known deviations from the reference
A side is taken or ignored per conflict in one step, which resolves it — the reference keeps a
conflict open until both sides are handled and asks before saving one that is still open; Ferris IDEA
offers Apply only when nothing is open (IMP-0039). "Both" is taking one side and then the other
(v7), as in the reference, but the conflict is already resolved after the first. Changes that do not
conflict are applied when the file opens instead of by a toolbar step (behaviour 27). Close asks
only when choices or edits would be dropped (behaviour 25). The view opens by itself or from Search
Everywhere instead of through a conflicts dialog (v3). Compared with the reference in OBS-0009; the
editable result, the gutter controls, changed words and stepping (v7) follow what OBS-0009 records
and were not placed next to the reference again; the hand test (AK-4) is still open.
