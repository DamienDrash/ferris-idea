# SPEC-0033: Git — blame and branches

- **Version:** 1 · **Date:** 2026-10-31
- **Sources:** GEN (IDEs annotate each line with the commit, author and date that last changed it and list
  local and remote branches with the current one; F-074), STD (Git blame semantics via `gix-blame`,
  references `refs/heads/*`, `refs/remotes/*`), SPEC-0032 (repository access, line diff)
- **Feature matrix rows:** F-074

## Purpose
Show who changed each line and when, and which branches exist, including for files with unsaved or
uncommitted changes.

## Behaviour
1. Blame of a file runs on its content at HEAD and gives, per HEAD line, the commit that introduced it
   (id, short id, summary, author, time). [STD]
2. The result is mapped onto the working-tree content line by line with the SPEC-0032 line diff: unchanged
   lines keep their blame; added or modified lines are reported as *uncommitted* (no commit). A file not in
   HEAD is entirely uncommitted. [GEN, improvement: no stale attribution for edited lines]
3. Consecutive lines with the same commit are grouped into ranges (1-based, inclusive) for compact
   display. [GEN]
4. Branches: local branches (`refs/heads/*`) and remote-tracking branches (`refs/remotes/*`, without
   `HEAD` aliases), each with short name and target commit; the current branch is marked; a detached HEAD
   is reported as such. Sorted: current first, then local by name, then remote by name. [STD]
5. MCP tools `git_blame` (path) and `git_branches`. [ADR-0008]

## Acceptance criteria
- **AK-1** Mapping blame through a diff: unchanged, inserted, modified and deleted lines.
- **AK-2** In a scratch repository with two commits by different authors, blame attributes each line to the
  right commit and author; an edited line is uncommitted; a new file is fully uncommitted.
- **AK-3** Branches: current, other local, a remote-tracking branch, and a detached HEAD.
- **AK-4** The MCP tools return blame ranges and branches.

## Out of scope
Blame across renames beyond what `gix-blame` tracks by default, ignoring whitespace, creating/switching/
deleting branches, commit (later M6.3 steps).

## Known deviations from the reference
Not yet compared; uncommitted lines are reported explicitly.
