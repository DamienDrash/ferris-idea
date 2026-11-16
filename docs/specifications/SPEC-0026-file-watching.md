# SPEC-0026: Watching workspace files

- **Version:** 3 · **Date:** 2026-11-12 (v3: a file still being written is reported once it stands
  still; v2: a reload replaces only the changed lines)
- **Sources:** GEN (IDEs notice files created, changed, deleted or renamed outside the editor and reload
  unmodified documents, asking when the open document has unsaved changes), STD (file metadata: length,
  modification time, inode on Unix), SPEC-0001 (workspace file list), SPEC-0003 (buffer)
- **Feature matrix rows:** F-001 (workspace), F-010 (editor)

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

## Acceptance criteria
- **AK-1** Diff: created, modified, deleted, renamed (inode), ambiguous renames stay delete + create.
- **AK-2** A real directory: the watcher reports a created file, a modification, a rename and a deletion
  within two seconds each, and stops when dropped.
- **AK-3** Reconcile: unchanged / reload / conflict; reload is one undo step and leaves the buffer saved.
- **AK-4** The MCP tool reports nothing on the first call and the changes made between two calls.
- **AK-5** (v3) A file truncated and then written in two ticks is not reported until a tick shows
  the same state again; then it is reported once, in its final state; a rename and a new file
  settle the same way.

## Out of scope
Native event APIs (FSEvents, inotify, ReadDirectoryChangesW) — a later optimisation behind the same
interface; files outside the workspace; changes within one modification-time tick that keep the length.

## Known deviations from the reference
The reference uses a native helper for file events; behaviour (reload/conflict) to be compared in the
guided session.
