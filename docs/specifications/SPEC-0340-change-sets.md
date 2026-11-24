# SPEC-0340: Change sets as the unit of an agent's work

- **Version:** 1 · **Date:** 2026-11-17
- **Sources:** `GEN` (patches, hunks and partial staging as version control tools offer them;
  content hashes); ADR-0023 rule 6 (change sets are the unit of work) and rule 7 (safety rules
  for MCP tools); ADR-0014 rule 3 (isolation by default: one worktree per agent session); ADR-0008
  (every feature has an MCP tool); SPEC-0044 (local history); SPEC-0053 (agent rights, files that
  commonly hold secrets, the agent log); SPEC-0220 (tool edits through the open editor);
  SPEC-0064 and SPEC-0084 (line changes of a file and taking one change); SPEC-0006 v56
  behaviour 8 (`replace_text_in_file`). No observation of the reference: the reference has no
  change sets for agents.
- **Feature matrix rows:** none (ROADMAP M19.4, extends M11.5; harness phase after 1.0).

## Purpose
An agent's edits should not land in the project file by file as they are made. They form a change
set: a named unit that holds the proposed text of every file it touches, the text each file had
when the edit was proposed, and who proposed it with which model and at what cost. A person or an
agent with the right to write looks at the set as a diff, takes it in whole, per file or per hunk,
rejects the rest, and can take an applied set back through Ferris IDEA's local history. Proposing
needs no right to write: an agent that may only read can still hand over a finished change.

## Behaviour

### The set
1. A change set has an `id` (`cs-` followed by lowercase letters, digits and `-`), a `name`, the
   time it was begun, the workspace folder it belongs to, its metadata, its files and its events.
   It is stored as one JSON file `.ferris-idea/change-sets/<id>.json` in the workspace; the folder is
   machine-local state and stays out of version control (SPEC-0069 behaviour 3). [GEN]
2. The metadata are `task`, `agent`, `model` and `cost`. `agent` defaults to the name and version
   the MCP client gave. `cost` holds `input_tokens`, `output_tokens`, `amount` and `currency`, all
   optional. Every text of the metadata is at most 200 characters and is stored with known secret
   patterns masked; a set never stores a credential. [ADR-0023 rules 6 and 7]
3. When the workspace lies in a Git repository, the set records its `worktree`: the root of the
   working tree, the current branch and whether it is a linked worktree (its `.git` is a file) or
   the repository's main working tree. When the main working tree is used, the answers of
   `change_set_begin` and `change_set_status` carry a notice that agents with the right to write
   are meant to work in their own worktree. [ADR-0014 rule 3, ADR-0023 rule 6]
4. At most 200 sets are kept. When a new set would exceed this, the oldest sets with nothing
   pending are removed first; when every kept set still has pending files, the new set is refused.
   [GEN]

### Proposing edits
5. A file of a set has its workspace-relative `path`, its `base` (the text the file had when its
   first edit was proposed, as the window shows it, saved or not; none when the file did not
   exist), the SHA-256 hash of the base, the `proposed` text (none when the file is to be deleted)
   and a status: `pending`, `applied`, `rejected` or `undone`. [SPEC-0220, GEN]
6. An edit gives the file's whole new text, or `old_text` and `new_text` (with `replace_all` and
   `case_sensitive` as in `replace_text_in_file`: the text must occur exactly once unless
   `replace_all`), or `delete`. A replacement works on the text already proposed in the set for
   that file, else on the base. An edit of a file that is no longer pending is refused. An edit
   that makes the proposed text equal to the base removes the file from the set. Proposing writes
   nothing but the set's own file. [SPEC-0006 v56 behaviour 8]
7. A path must be workspace-relative, without `..`, not absolute, and must not lie in `.ferris-idea/`
   or `.git/`; an existing file must resolve inside the workspace after resolving links, and for a
   new file the nearest existing folder must. A file whose name commonly holds secrets can be
   proposed only with the `secret-read` right, as every tool that is handed such a path
   (SPEC-0053 v6 behaviour 10). A proposed text is at most 2 MiB (the local history's limit), a
   set holds at most 500 files. [ADR-0023 rule 7, SPEC-0044]

### Diff and hunks
8. The hunks of a file are its line changes from the base to the proposed text, numbered from 1
   in file order, each with its kind (`added`, `modified`, `deleted`), its old and new line
   ranges (1-based, inclusive) and its old and new lines. A new or deleted file has exactly one
   hunk, the whole file. Every file of the diff also has a unified diff with three lines of
   context. [SPEC-0064, SPEC-0084, SPEC-0032 behaviour 4]
9. The diff of a file whose name commonly holds secrets is withheld from an agent without the
   `secret-read` right: the file is listed with its path and hunk count, its text is left out
   with the usual note. Diff text is project and agent content: the tool description marks it as
   untrusted data, not instructions. [SPEC-0053 v6 behaviour 10, ADR-0023 rule 7]

### Applying
10. Applying takes the pending files of the set, or the files named, or for named files only the
    hunks named. Hunks that were rejected are never applied. A new or deleted file is applied
    whole. The text written is the base with the chosen hunks of the proposed text applied, every
    other line of the base kept as it is. [SPEC-0064 v3 behaviour 8]
11. Applying is a dry run unless `dry_run = false`. The dry run returns per file the hunks that
    would be applied, whether the file still matches its base, and the conflicts; nothing is
    written. [SPEC-0006]
12. `dry_run = false` needs the agent right `write` (SPEC-0053), and for a file whose name
    commonly holds secrets also `secret-read`. Before anything is written every chosen file is
    checked: its current text (as the window shows it) must have the base's hash, and a new file
    must still not exist. If one file fails the check, the call is refused as a conflict naming
    each file, and nothing is written at all. A file open in an editor with unsaved changes is
    refused as for every other tool (SPEC-0220 behaviour 9). [ADR-0023 rule 7]
13. Each file is written as other tools write: an open file through its editor as one undo step,
    otherwise on disk only while it still holds the checked text (SPEC-0220 behaviours 7–10). A
    set never writes outside the workspace folder: a new file's folder is resolved after creating
    it and must lie inside the workspace. [SPEC-0220, ADR-0023 rule 7]
14. Before a file is written, its current state is recorded in the local history; after it is
    written, its new state is recorded with the label `Change set <id>: <name>` (SPEC-0044
    behaviours 2 and 3). The set keeps for each applied file the chosen hunks, the hash of the text
    written (none for a deletion) and the local-history revision of the state before. [SPEC-0044]
15. A file applied is `applied`; the rest of its hunks are dropped. To take more, the file is
    undone and applied again with the larger choice.

### Rejecting and undoing
16. Rejecting marks the named files `rejected`, or for named files only the named hunks, or with
    no names every pending file. Rejecting writes nothing but the set's own file and needs no
    right. A file all of whose hunks are rejected is `rejected`. [GEN]
17. Undoing takes the applied files of the set, or the files named. It is a dry run unless
    `dry_run = false`, which needs the right `write` (and `secret-read` for a file whose name
    commonly holds secrets). A file can be undone only while its current text still has the hash
    of the text the set wrote (for a deletion: while the file still does not exist); otherwise the
    call is refused as a conflict and nothing is written. [ADR-0023 rule 6]
18. An undone file gets back the state before the set from the local history: a file not open in
    an editor is restored from the recorded revision (the restore records the current state first,
    so the undo can itself be undone, SPEC-0044 behaviour 5); an open file gets the text through
    its editor; a file the set created is deleted after its state was recorded. The file's status
    becomes `undone`. [SPEC-0044]

### Log
19. Every call that changes a set or the project leaves its one line in `.ferris-idea/agent-log.jsonl`
    (SPEC-0053 v7 behaviour 11). Applying and undoing for real also log a line with the set, the
    files and each file's outcome (written, conflict), never file text. The set itself keeps its
    events: time, action (`begin`, `edit`, `apply`, `reject`, `undo`), the agent and the files.
    [ADR-0023 rule 7, SPEC-0053]

### MCP tools (ADR-0008)
20. `change_set_begin` (`name`, optional `task`, `agent`, `model`, `cost`) begins a set and returns
    its `id`, metadata and `worktree`. No right is needed.
21. `change_set_edit` (`id`, and `path` with `text`, `old_text`/`new_text` or `delete`; and/or
    `model` and `cost` to update the metadata) proposes an edit; it returns the file's status and
    hunk count. No right is needed.
22. `change_set_status` (optional `id`) lists the sets (id, name, agent, time, counts per status)
    or returns one set with its metadata, worktree, events and files (status, hunk count, whether
    the file still matches its base now). Read only.
23. `change_set_diff` (`id`, optional `path`) returns the files with their hunks and unified diff
    (behaviours 8 and 9). Read only.
24. `change_set_apply` (`id`, optional `files`, `hunks` as a map from path to hunk numbers,
    `dry_run` default true) applies (behaviours 10–15).
25. `change_set_reject` (`id`, optional `files`, `hunks`) rejects (behaviour 16).
26. `change_set_undo` (`id`, optional `files`, `dry_run` default true) undoes (behaviours 17, 18).

## Acceptance criteria
- **AK-1** Given a base and a proposed text with three separate changes, when the hunks are read
  and two of them are applied, then the hunks are numbered in file order with their kinds and
  ranges, and the result is the base with exactly those two changes; a new file and a deletion
  each have one hunk; a replacement edit works on the text already proposed; an edit back to the
  base removes the file; ids and paths that leave the folder are refused (behaviours 1, 5–8, 10).
- **AK-2** Given a set with metadata holding a token-like secret, when it is stored and read back,
  then the secret is masked, texts are cut to 200 characters, events are kept, and with more than
  200 sets the oldest without pending files go first (behaviours 1, 2, 4, 19).
- **AK-3** Given an MCP client without the `write` right, when it begins a set, proposes edits,
  reads the status and the diff and dry-runs the apply, then all of it works and no project file
  changes; `dry_run = false` is refused naming the right (behaviours 11, 12, 20–24).
- **AK-4** Given a set over a changed file, a new file and a deleted file, when it is applied with
  one hunk rejected, then the files hold the base with the other hunks, the new file exists, the
  deleted one is gone, the local history holds the labelled states, and the call is in the agent
  log; when it is undone, the files are back as before and the undo needs the file to hold what the
  set wrote (behaviours 13–18, 25, 26).
- **AK-5** Given a set whose file changed on disk after the edit was proposed, when it is applied,
  then the call is refused as a conflict and no file of the set is written; given a path outside
  the workspace, in `.ferris-idea/` or `.git/`, the edit is refused; given a file whose name commonly
  holds secrets, proposing, diffing and applying need `secret-read` (behaviours 7, 9, 12).
- **AK-6** Given a workspace that is the main working tree of a Git repository, when a set is
  begun, then its worktree is recorded with branch and `linked: false` and the answer carries the
  notice (behaviour 3).

## Out of scope
- A window UI for change sets (a review tool window with accept and reject per hunk). The tools
  return everything such a view needs; it follows with the agent host's views (ROADMAP M19.5,
  M19.10).
- Creating a worktree for an agent session and starting the agent in it. That belongs to the agent
  host, which starts the agent's process and its MCP session in that folder (ADR-0014 rule 3,
  ROADMAP M19.5); a set records which worktree it was made in (behaviour 3) and is applied there.
  Ferris IDEA has no mechanism yet to create linked worktrees.
- Integration of an agent's worktree through the merge queue (ADR-0023 rule 6, ROADMAP M12).
- Collecting a set automatically from other editing tools (`replace_text_in_file`,
  `apply_code_action`, refactorings); in this version an agent proposes through `change_set_edit`.
- Binary files: a set holds text only.

## Known deviations from the reference
The reference has no change sets for agents; nothing is copied. Improvements over editing tools
that write at once:
- **D1** Proposing needs no right to write (behaviours 20, 21): a read-only agent can deliver a
  complete change, and a person or another agent with the right decides what lands.
- **D2** All or nothing against a base hash (behaviour 12): a set never overwrites a change made
  after it was proposed, and a conflict writes no file of the set.
