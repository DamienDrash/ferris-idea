# SPEC-0085: Local History view

- **Version:** 2 · **Date:** 2026-11-15 (v2: title with the file, entries as changes with date and time showing the state before them, search, patch, stepping through the differences, both sides marked; OBS-0057 H1–H5)
- **Sources:** GEN (the saved versions of a file listed by time, each compared with the current text,
  one of them put back; F-019), OSS (action id `LocalHistory.ShowHistory`, which has no default shortcut
  and is found through Search Everywhere), SPEC-0044 (local history), SPEC-0084 (diff viewer); v2: OBS
  (OBS-0057: the reference's history of a file is titled with the file, lists changes with their kind
  and date and time, shows for each the state before it against the current text, offers a search,
  a patch and stepping through the differences, and marks both sides of a difference)
- **Feature matrix rows:** F-019

## Purpose
Get back a version of a file that was never committed.

## Behaviour
1. **Open:** `LocalHistory.ShowHistory` (e.g. from Search Everywhere, which also lists IDECraft actions
   without a shortcut) opens the history of the active file over the editor area. Without saved versions
   the status bar says "No local history for this file yet." [GEN, OSS]
2. **List:** on the left, the file's revisions newest first, each as its age ("5 min ago", "yesterday")
   and what happened (Saved, Reverted, Deleted, or its label); workspace labels are not listed. The
   newest is selected. [GEN]
3. **Compare and keys:** on the right, the chosen revision ("<age> · <what>") and "Current" (the editor's
   text, unsaved changes included) side by side as in the diff viewer (SPEC-0084). Up/Down or a click
   choose a revision, F7 / ⇧F7 step through its differences, Escape closes. [GEN]
4. **Revert:** the Revert button replaces the editor's text with the chosen revision as one undo step and
   closes the view; saving then records it as usual. [GEN]
5. Saves are recorded also when the workspace folder is reached through a symbolic link (on macOS
   `/var` → `/private/var`). [GEN]
6. Agents use `local_history` and `restore_local_revision` (SPEC-0044). [ADR-0008]

7. **Title (v2, OBS-0057 H1):** the view is headed "Local History: <file name>". It still opens over
   the editor area (Known deviations). [OBS]
8. **Entries (v2, OBS-0057 H2):** each row names the change and when it was made, in local time:
   `Change · 2026-11-15 13:56`; a revision with a label shows the label instead of "Change" (a
   refactoring's name such as "Rename runde to aufrunden", SPEC-0193 v2 behaviour 8), a restore
   "Revert", a deletion "Deleted". The age ("5 min ago") is gone from the row. Since v1 a row
   said "<age> · Saved". [OBS]
9. **The state before (v2, OBS-0057 H3):** an entry stands for its change; the left side shows the
   file as it was **before** that change ("Before 2026-11-15 13:56", read only), the right side the
   current text. The newest entry therefore shows the last change. The oldest entry has nothing
   older kept and shows the oldest state kept. Revert puts back that left side. Until v1 an entry
   showed the state after it, so the newest entry showed no difference. [OBS]
10. **Search (v2, OBS-0057 H4):** typed text (committed text, SPEC-0004 v6 behaviour 17) narrows the
   list to the entries whose state before contains it, ignoring case; ⌫ shortens it, Escape first
   clears it. The list says "No entry contains <text>" when none does. [OBS]
11. **Patch (v2, OBS-0057 H4):** Create Patch copies a unified patch from the left side to the
   current text, headed `--- a/<path>` and `+++ b/<path>`, to the clipboard; the status bar says
   "Patch copied to the clipboard". The reference writes a patch file through a dialog; IDECraft
   copies it (Known deviations). [OBS, GEN]
12. **Stepping (v2, OBS-0057 H4):** the comparison's header has Previous Difference and Next
   Difference (also F7 / ⇧F7 as before) and the number of differences. [OBS]
13. **Both sides marked (v2, OBS-0057 H5):** a line added since the state before is marked on the
   right and its empty row on the left in a lighter shade of the same colour; a deleted line the
   other way round; modified lines on both sides as before. Only this view does so; the diff viewer
   of SPEC-0084 keeps its look. [OBS]

## Acceptance criteria
- **AK-1** Ages, row texts and the filter of workspace labels (unit tests of `history_model`).
- **AK-2** In a real window (`idecraft ui --selftest`, workspace in the temp folder behind `/var`):
  after a save the history of `bad.c` lists at least two revisions; Escape closes it.
- **AK-3** Use by hand: Revert and ⌘Z, older revisions, Search Everywhere finds the action.

- **AK-4 (v2)** Unit tests of `history_model`: the row texts with kind and local date and time; the
  state before an entry (the next older one, the oldest its own, a deletion an empty text); the search
  over the states before; the patch's header and hunks.
- **AK-5 (v2)** In a window of the test harness, by keys and clicks found through the semantic tree
  (`crates/workbench/tests/it/local_history_view.rs`: `history_shows_changes_before_and_searches`):
  after two saves the view is titled with the file, the newest entry shows the last change, its left
  side is the state before it; typed text narrows the list; Create Patch puts the patch on the
  clipboard; Next Difference steps; Revert puts the state before back as one undo step.

## Out of scope
History of a directory or the whole project, a selection's history, putting labels from the view,
recent changes across files, the history of deleted files.

## Known deviations from the reference
The reference shows the history in a separate window and offers Revert for single changes; IDECraft
shows it over the editor area and reverts the whole revision. **(v2)** The window stays the editor
area's view (behaviour 7): a separate window brings its own focus and window handling for one list
and one comparison. IDECraft's history does not record whether a change came from the editor or
from outside, so every change is "Change" and none "External change" (behaviour 8). The patch goes
to the clipboard instead of a file chosen in a dialog (behaviour 11). The comparison has no editing,
no collapsing of unchanged parts and no settings of its own; Side-by-side/Unified stays.
Connections between the sides in the middle are not drawn; the empty row on the other side carries
the mark instead (behaviour 13).
