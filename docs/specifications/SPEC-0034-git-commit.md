# SPEC-0034: Git — commit selected files and create branches

- **Version:** 1 · **Date:** 2026-10-31
- **Sources:** GEN (IDEs commit the files ticked in a commit dialog with a message, regardless of what is
  staged, and create branches from the current commit; F-074), STD (Git object model: blobs, trees,
  commits, references and reflog via `gix`; index format via `gix-index`), SPEC-0032/0033
- **Feature matrix rows:** F-074

## Purpose
Record chosen changes as a commit and start new branches without calling the `git` program.

## Behaviour
1. A commit takes a message and a list of repository-relative file paths. The new tree is HEAD's tree
   (empty for an unborn HEAD) with each path set to its working-tree content (regular, executable or
   symlink) or removed if the file no longer exists. Other changes, staged or not, stay out of the commit
   and keep their state. [GEN]
2. Rejected without writing anything: an empty or whitespace-only message; no paths; a path that is
   absolute, contains `..`, is a directory, or neither exists nor is in HEAD; a resulting tree equal to
   HEAD's ("nothing to commit"). [GEN, STD]
3. The commit's parent is HEAD's commit (none for an unborn HEAD); author and committer come from the Git
   configuration (`user.name`, `user.email`); a missing identity is an error. The current branch (or HEAD
   when detached) moves to the new commit with a reflog entry; if HEAD moved meanwhile the commit is refused
   (no lost work). [STD]
4. Afterwards the index matches the commit for the committed paths (entry updated with file stat, or
   removed); index entries of other paths are unchanged, so `git status` shows the committed files as clean.
   [STD]
5. A preview (dry run) lists the paths with their change kind (added, modified, deleted) and validates as in
   2 without writing. [Improvement IMP-0015: an agent sees exactly what would be committed before it is]
6. Creating a branch takes a name and an optional start revision (default HEAD); invalid names and
   existing branches are rejected; HEAD is not switched. [STD]
7. MCP tools `git_commit` (message, paths, `dry_run` default true) and `git_create_branch` (name, optional
   start). [ADR-0008]

## Acceptance criteria
- **AK-1** Committing a modified, a new and a deleted file: the history has the new commit with the right
  parent, message and author; the tree contains exactly the expected content; status is clean for those
  paths while an unselected modified file and a staged file keep their state.
- **AK-2** First commit on an unborn HEAD creates the branch.
- **AK-3** Rejections: empty message, no paths, `..` path, unknown path, nothing to commit; nothing is
  written in those cases (HEAD unchanged).
- **AK-4** Dry run lists kinds and leaves HEAD unchanged.
- **AK-5** Branch creation from HEAD and from a revision; duplicate and invalid names rejected.
- **AK-6** The MCP tools preview, commit and create a branch.

## Out of scope
Partial (hunk-level) commits, amend, signing, hooks (`pre-commit`, `commit-msg`), switching branches,
merge, rebase, push.

## Known deviations from the reference
Commit hooks are not run yet (later step); commits via MCP default to a dry run (IMP-0015).
