# SPEC-0044: Local history

- **Version:** 2 · **Date:** 2026-11-05 (v2: bounded size and stable revision ids, behaviours 8 and 9;
  found by the M8.2 soak, where an unbounded log made `idecraft mcp` grow from 89 to 309 MiB in two hours)
- **Sources:** GEN (IDEs keep earlier versions of files independent of version control, with labels, and
  let the user view and restore them; F-019), DOC (the reference keeps local history for a limited number
  of working days in its caches), STD (SHA-256 content addressing, JSON Lines), SPEC-0001 (workspace),
  SPEC-0026 (file changes)
- **Feature matrix rows:** F-019

## Purpose
Get back any earlier state of a file — also one never committed — without depending on Git.

## Behaviour
1. The history lives in the workspace at `.idecraft/history/` (not in an IDE cache): file contents are
   stored once per distinct content under their SHA-256 (`objects/`), revisions are appended to
   `revisions.jsonl` (path, time, content hash or deletion, label, kind). [STD, improvement IMP-0021]
2. Recording a file stores a revision only if its content differs from the file's latest revision; a
   deleted file is recorded as a deletion. Files larger than 2 MiB are not recorded. Changes reported by
   the file watcher (SPEC-0026) can be recorded in one call. [GEN]
3. A label can be put on a file or on the whole workspace (e.g. "before refactoring"). [GEN]
4. The revisions of a file are listed newest first with id, time, size, label and whether it is a
   deletion; workspace labels appear in every file's list at their time. The content of a revision can be
   read. [GEN]
5. Restoring a revision first records the current content (so the restore can itself be undone), then
   writes the revision's content (or deletes the file for a deletion) and records a `restore` revision.
   [GEN, improvement IMP-0021]
6. Pruning removes revisions older than the retention period (default 30 days) except each file's latest
   one, and deletes contents no revision refers to any more. A damaged line in `revisions.jsonl` is skipped
   instead of losing the whole history. [GEN, improvement IMP-0021]
7. MCP tools `local_history` (path; records the current state, then lists revisions; optional revision id
   to return its content) and `restore_local_revision` (path, revision, `dry_run` default true). [ADR-0008]
8. The history is bounded (v2): a file keeps at most its newest 1,000 entries and the workspace at most
   its newest 1,000 workspace labels. When recording pushes a file (or the workspace labels) past 1,250
   entries, the log is compacted in one atomic rewrite to the newest 1,000 per file, applying the
   retention of behaviour 6 at the same time, and contents no entry refers to any more are deleted. So
   the log, the memory needed to read it and the time of every history request stay bounded by the
   number of recorded files, not by how long the workspace has been worked on. Opening reads the log line
   by line instead of copying it whole. [GEN, ADR-0015]
9. Revision ids are sequence numbers written into the log; they never change, not even when pruning or
   compaction removes older entries. An id that was removed answers "no revision" instead of naming a
   different revision. Logs written before v2 (without ids) number their lines from 0 as before. [GEN]

## Acceptance criteria
- **AK-1** Recording: unchanged content adds nothing; changes and deletion add revisions; identical
  contents share one object; large files are skipped.
- **AK-2** Labels on a file and on the workspace appear in the list; list order and content reading.
- **AK-3** Restore writes the old content, keeps the pre-restore state as a revision and can be undone;
  restoring a deletion deletes the file.
- **AK-4** Pruning by age keeps the latest revision per file and removes unreferenced objects; a damaged
  line is skipped; watcher changes are recorded.
- **AK-5** The MCP tools list, show and restore.
- **AK-6** (v2) Recording one file 3,000 times leaves at most 1,250 of its entries in the log and in
  memory, keeps the newest ones, keeps other files' entries, and removes unreferenced contents; ids
  listed before a compaction still return the same content afterwards, removed ids return an error.
- **AK-7** (v2) The soak test (M8.2, `docs/testing/HARDENING.md`) keeps the resident memory of
  `idecraft mcp` within its growth budget for at least 20 minutes.

## Out of scope
The history UI with diff view (M3.7), recording directory moves as such, history of non-workspace files,
searching across history.

## Known deviations from the reference
Stored in the project instead of the IDE caches and kept 30 days by default instead of a few working days;
restores are recorded and undoable (IMP-0021). Bounded to 1,000 entries per file (behaviour 8).
