# SPEC-0193: One undo step for a refactoring across files

- **Version:** 6 · **Date:** 2026-11-17 (v6: MCP `redo_refactoring`, and redo takes the group undone last; found by driving undo and redo headless for F-011, OBS-0075; v5: an agent's refactoring over MCP is an undo group of the window, SPEC-0220 v4; v4, 2026-11-10: ⌘Z of a refactoring in one editor says what it took back, window run of hand test Nr. 210; v3: a refused group stays locked instead of a single-file undo, OBS-0030, IMP-0393; v2: the refactoring is named in each file's local history, open files keep their saved state there; hand test Nr. 123, D46)
- **Sources:** GEN (IDEs undo a refactoring that changed several files with one Undo, in every file it
  touched, and refuse when one of those files was changed afterwards; refactorings leave a named entry in
  the local history; F-072, F-073), SPEC-0003 (grouped undo in a buffer), SPEC-0040, SPEC-0041 v2,
  SPEC-0044 (local history and labels), SPEC-0074, SPEC-0076 v3, SPEC-0113, SPEC-0148, ADR-0008,
  SPEC-0220 v4
- **Feature matrix rows:** F-072, F-073

## Purpose
A rename or a code action that changed many files is taken back with one ⌘Z, without hunting through
the files it changed, and without silently losing edits made afterwards.

## Behaviour
1. **Group:** applying a text refactoring — rename (also in comments, strings and text files), Change
   Signature, a code action (quick fix, intention, refactoring shortcut) — makes one undo group named
   after it ("Rename helper to calc", the action's title). Files open in editors change through their
   editors as one undo step each, with the caret kept in place; other files are checked and then written
   on disk. This replaces the "one undo step each" of SPEC-0074 behaviour 3, SPEC-0076 behaviour 5,
   SPEC-0113 and SPEC-0148 for those refactorings. [GEN]
2. **Local history:** files written on disk are recorded in the local history before and after the
   write, and the workspace gets a label with the group's name, so the state before the refactoring can
   be found and restored there (SPEC-0044). [GEN]
3. **Undo:** `$Undo` (⌘Z / Ctrl+Z) in an editor whose newest undo step belongs to a group undoes the
   whole group: the step in every editor of the group and every file written on disk, which gets its
   earlier text back. First every part is checked: an editor's newest step must still be the group's
   step (an editor closed since counts as a file on disk); a file on disk must still hold the text the
   refactoring wrote. If one part changed, nothing is undone and the status bar says "Cannot undo <name>:
   <file> changed since. Undo that change first, or restore from Local History." (v3) The group is
   kept, not forgotten: a further ⌘Z in any of its editors refuses the same way, without a dialog, and
   never takes the refactoring back in one file alone, because that would leave the other files using
   the new name (OBS-0030 L1, IMP-0393). Once the later change is undone, ⌘Z undoes the whole group.
   The status bar reports "Undone: <name> (n files)". [GEN, OBS-0030]
4. **Redo:** `$Redo` (⇧⌘Z / Ctrl+Shift+Z) in an editor whose next redo step belongs to an undone group
   redoes the whole group with the same checks in reverse (files on disk must hold the earlier text).
   [GEN]
5. Undoing or redoing a group records the files written on disk in the local history again and labels
   the workspace "Undo <name>" or "Redo <name>". [GEN]
6. A group that changes only one open file needs no grouping: plain undo already takes it back; it
   still gets its local-history label. The 20 newest groups are kept. [GEN]
7. **MCP:** applying `rename_symbol` or `apply_code_action` (not as a preview) remembers its group in
   the MCP server, labels the local history the same way, and the tool `undo_refactoring` (no arguments)
   takes back the newest one that has not been undone yet, with the same check: every file must still
   hold the text the refactoring wrote, else nothing changes and the changed files are named. [ADR-0008]
8. **Named in each file's history (v2, D46):** the label of behaviours 2 and 5 is also put on each
   written file, and that file's Local History view shows it on the revision it names ("Rename runde
   to aufrunden", "Undo Rename …") instead of "Saved". A file the refactoring changes only in its
   editor (open and without unsaved changes) gets its saved state recorded and labelled "Before
   <name>", so its history has the state before the refactoring; the changed text is recorded when it
   is saved. [GEN]
9. **Status after undo in one editor (v4):** ⌘Z (⇧⌘Z) in an editor on the undo step of a
   refactoring that changed only that editor (behaviour 6) replaces the status bar's message with
   "Undone: <name>" ("Redone: <name>"), e.g. "Undone: Rename placeholder to flaeche", so the message
   of the refactoring ("Renamed placeholder to flaeche in 1 file(s)") does not stay as if it still
   held. Undoing other typing leaves the status bar alone. [GEN]
10. **An agent's refactoring is a group of the window (v5).** A refactoring an agent applies over the
    window's MCP server — `rename_symbol`, `apply_code_action` and the class refactorings
    (Extract Interface, Extract Superclass, Pull Members Up), `change_signature` — that changes files
    open in that window's editors is kept by the window like one the user applied (behaviour 1),
    under the same name ("Rename add to sum", the action's title, "Change Signature"): each open
    file's editor step is a part of the group, each file the change wrote to disk is a part with its
    text before and after (SPEC-0220 v4 behaviour 15). ⌘Z (⇧⌘Z) in any of its editors then undoes
    (redoes) the whole group with the checks, refusals and messages of behaviours 3 to 5, and a
    group of one editor step with no file on disk names its step as in behaviour 9. The group the
    MCP server keeps for `undo_refactoring` (behaviour 7) is a second record of the same change:
    after ⌘Z undid it, the tool's check finds the files changed and refuses (behaviour 7). What
    `undo_refactoring` changes through the editors is itself kept as a window group named
    "Undo <name>", so one ⌘Z in any of its editors takes the tool's undo back in all of them, and
    the change is never taken back twice nor in one file alone. Changes that rename
    files, or whose files are shown in more than one project window or only in a split, are not
    kept as a window group (each editor undoes its own step). [GEN, ADR-0008]
11. **Redo over MCP (v6).** The tool `redo_refactoring` (no arguments) applies again the
    refactoring that `undo_refactoring` took back last, in every file at once, with the checks of
    behaviour 4: every file must still hold the text the undo left, else nothing changes, the
    changed file is named ("cannot redo <name>: <file> changed since") and the group is dropped.
    Open files change through their editors as one step each, which the window keeps as a group
    named "Redo <name>" (behaviour 10); files on disk are recorded in the local history under the
    same label (behaviour 5). A refactoring applied after an undo ends redo (as typing ends redo
    in an editor). With several groups undone, redo takes them in the order they were made: the
    one undone last first. Before v6 an agent could undo but not redo, and the redo of files on
    disk took the newest undone group, which fails its check when an older undone group touched
    the same file and then dropped it. The groups live as long as the MCP server: a one-shot
    `idecraft call` keeps none, so its "nothing to undo" ("nothing to redo") says so and points to
    `local_history` and `restore_local_revision`, where each refactoring is labelled (behaviour 2).
    [GEN, ADR-0008]

## Acceptance criteria
- **AK-1** Group bookkeeping (unit tests): the group of an editor step is found for undo, an undone
  group for redo, a changed part refuses the whole group and names the file, at most 20 groups are kept.
- **AK-2** Files on disk: undo restores the earlier text of every written file and refuses when one of
  them changed since; redo writes the new text again (unit tests on a temporary folder); the local
  history has the group's label.
- **AK-3** MCP (protocol test with clangd): after `apply_code_action`, `undo_refactoring` restores the
  file; the next call refuses the rename before it because Change Signature changed `calc.c` since and
  names it; then nothing is left to undo.
- **AK-4** In a real window (`idecraft ui --selftest`, with clangd): renaming a function used in an open
  and a closed file, then one ⌘Z in the open file, brings both files back.
- **AK-6 (v2)** With the real local history: a written file lists "<name>" then "Saved", an open
  file "Before <name>"; a label on a file names the revision before it (unit tests
  `history_model::tests::refactorings_show_in_a_files_history`,
  `history_model::tests::refactoring_labels_name_their_revision`).
- **AK-8 (v4)** Message (unit test `actions::tests::status_messages_follow_the_last_action`); in a
  real window (`idecraft ui --selftest`, `refactor_name_steps`): ⌘Z after an extraction and its
  rename says "Undone: Rename placeholder to d", then "Undone: Extract …". Not run yet.
- **AK-9 (v5)** In a window of the test harness: an agent's change of two open files and one file
  on disk, applied as the window applies MCP's request (`WorkbenchView::edit_documents_in`) under the
  name "Rename add to sum", is undone by one ⌘Z in either editor in both editors and on disk, with
  "Undone: Rename add to sum (3 files)"; ⇧⌘Z redoes it; after typing in the other editor ⌘Z refuses
  with "Cannot undo Rename add to sum: … changed since" and changes nothing; a change of one editor
  says "Undone: <name>" (`crates/workbench/tests/it/undo_redo.rs`).
- **AK-5** Use by hand: rename across files, ⌘Z, ⇧⌘Z, refusal after editing one of the files.
- **AK-7 (v3)** A refused group is kept and refused again on a second undo; once the other file holds
  the refactoring's text again, the whole group passes the check (unit test
  `undo_group::tests::a_refused_group_stays_until_it_can_be_undone`); by hand: a second ⌘Z leaves
  every file with the new name.
- **AK-10 (v6)** MCP (protocol test with clangd): after `undo_refactoring`, `redo_refactoring`
  applies the code action again and the local history has "Redo <name>"; a second redo has nothing
  to redo; a file changed after an undo refuses the redo and stays as it is. Redo order (unit test
  `undo_group::tests::redo_takes_the_group_undone_last`): two groups of one file, both undone, are
  redone oldest first.

## Out of scope
Undoing Move (it renames a file; SPEC-0114 undoes nothing yet), SQL refactorings applied to a database
(they run DDL, which local undo cannot take back), undo groups for Replace in Files (SPEC-0066), a
confirmation dialog before a global undo, undo across IDE restarts.

## Known deviations from the reference
Compared side by side in OBS-0030. The reference asks in a modal dialog before undoing or redoing a
refactoring in other files and shows a modal error on each refused attempt; IDECraft undoes and redoes
directly and refuses in the status bar (IMP-0393). Both keep the refactoring locked after a refusal.
