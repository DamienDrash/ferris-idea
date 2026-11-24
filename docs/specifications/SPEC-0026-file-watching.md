# SPEC-0026: Watching workspace files

- **Version:** 5 · **Date:** 2026-11-18 (v5: one watcher for the Project tree — behaviour 6 is carried out by the watcher of SPEC-0059 v9.11 behaviour 31, every half second; the two-second look of v4 is gone, both had come in separately); 2026-11-18 (v4: the Project tree shows files created, deleted or
  renamed outside Ferris IDEA within two seconds while the window stays in front, behaviour 6, AK-6;
  OBS-0088 U2); 2026-11-12 (v3: a file still being written is reported once it stands
  still; v2: a reload replaces only the changed lines)
- **Sources:** GEN (IDEs notice files created, changed, deleted or renamed outside the editor and reload
  unmodified documents, asking when the open document has unsaved changes), STD (file metadata: length,
  modification time, inode on Unix), SPEC-0001 (workspace file list), SPEC-0003 (buffer)
- **Feature matrix rows:** F-001 (workspace), F-010 (editor), F-011 (project tree)

## Purpose
Keep the IDE's view of the workspace and of open documents in step with the disk without losing unsaved
work.

## Behaviour
1. A snapshot records, for every workspace file (SPEC-0001 rules, excluded directories skipped), its
   length, modification time and — on Unix — inode. [STD]
2. Comparing two snapshots gives changes sorted by path: created, modified (length, time or inode
   differs), deleted, and renamed — a deleted and a created file with the same inode (Unix), or with the
   same length and time where no inode exists, when that pairing is unique. [GEN]
3. A watcher compares a fresh snapshot with the previous one at a fixed interval (default 500 ms) on a
   background thread and delivers non-empty change batches in order; it stops when its handle or the
   receiver is dropped. It needs no native file-system API and no extra dependency. (v3) A file is
   reported only in a state it already had one interval earlier: a file another program is still
   writing (truncated, half written) is reported once it has stood still for a whole interval, so
   no reader sees it in between. This delays a report by at most one interval. [GEN, improvement:
   a change is never read half written by the watcher's users]
4. Open documents: when the file of a document changes on disk, the result is *unchanged* if the disk text
   equals the buffer text, *reload* if the buffer has no unsaved changes, and *conflict* otherwise (the
   user decides; nothing is overwritten). Reloading replaces the content as one undo step and counts as
   saved, so a reload can be undone. Only the lines that differ are replaced (a line diff), so
   breakpoints, folds and other line marks stay on their lines and move with inserted or removed
   ones (v2, hand test 106, D40: every breakpoint jumped to line 1). [GEN, improvement: reload is
   undoable]
5. MCP tool `file_changes`: changes since the previous call of the tool; the first call records the
   baseline and reports nothing. [ADR-0008]
6. **v4 – The Project tree follows the disk:** (v5: carried out by the one watcher of SPEC-0059
   v9.11 behaviour 31, which looks every half second; the rest of this behaviour holds as written.)
   Every two seconds (v4), with the window in front or
   not, the project window lists the folders the Project tree shows expanded (names and whether
   each entry is a folder; off the UI thread, one directory listing per expanded folder, nothing
   read below a collapsed folder). When a listing differs from the previous look, the tree is
   read again as after one of its own operations: expanded folders stay expanded and the
   selection stays where its path still exists. A file another program creates, deletes or
   renames in a shown folder therefore appears or goes within about two seconds. A look that
   overlaps a reading of the tree by Ferris IDEA itself only sets the new baseline. Remote projects
   (SPEC-0317) are not looked at. Before v4 the tree was read again only after Ferris IDEA's own
   operations and Synchronize; a file created outside did not show while the window stayed in
   front (OBS-0088 U2). [GEN]

## Acceptance criteria
- **AK-1** Diff: created, modified, deleted, renamed (inode), ambiguous renames stay delete + create.
- **AK-2** A real directory: the watcher reports a created file, a modification, a rename and a deletion
  within two seconds each, and stops when dropped.
- **AK-3** Reconcile: unchanged / reload / conflict; reload is one undo step and leaves the buffer saved.
- **AK-4** The MCP tool reports nothing on the first call and the changes made between two calls.
- **AK-5** (v3) A file truncated and then written in two ticks is not reported until a tick shows
  the same state again; then it is reported once, in its final state; a rename and a new file
  settle the same way.

- **AK-6 (v4, behaviour 6)** `project_tree::tests::the_listing_of_shown_folders_changes_with_the_disk`
  (a new file in an expanded folder changes it, one in a collapsed folder does not); in a window of
  the test harness (`crates/workbench/tests/it/window_check_gaps_1118.rs::a_file_created_outside_shows_in_the_tree`)
  a file written by the test appears among the tree's rows after the next look, without
  activating the window, and goes again after it is deleted.

## Out of scope
Native event APIs (FSEvents, inotify, ReadDirectoryChangesW) — a later optimisation behind the same
interface; files outside the workspace; changes within one modification-time tick that keep the length.

## Known deviations from the reference
The reference uses a native helper for file events; behaviour (reload/conflict) to be compared in the
guided session.
