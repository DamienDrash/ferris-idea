# SPEC-0036: Git — merge a branch into the current one

- **Version:** 2 · **Date:** 2026-11-03 (v2: merge that stops on conflicts, smart merge)
- **Sources:** GEN (IDEs merge a selected branch into the current one; fast-forward when possible, a merge
  commit otherwise, conflicts are reported for resolution; F-074), STD (Git merge semantics: merge base,
  fast-forward, three-way tree merge via `gix-merge`, merge commit with two parents, default message
  `Merge branch '<name>'`), SPEC-0034/0035
- **Feature matrix rows:** F-074

## Purpose
Bring another branch's work into the current branch without calling the `git` program.

## Behaviour
1. The source is a revision (usually a branch name). HEAD must point at a commit. [STD]
2. If the source is already contained in HEAD, the result is *up to date* and nothing changes. [STD]
3. If HEAD is contained in the source, the merge is a *fast-forward*: the current branch (or detached HEAD)
   moves to the source commit with a reflog entry, and working tree and index follow (as in SPEC-0035,
   including the refusal on local changes in the way). [STD]
4. Otherwise the trees are merged three-way from the merge base. If the merge resolves cleanly, a merge
   commit with parents HEAD and source and Git's default message `Merge branch '<source>'` (`Merge commit
   '<source>'` for other revisions) plus ` into <current>` unless the current branch is `main` or `master` is
   written. Working tree and index are updated before the reference moves. [STD]
5. If the three-way merge has unresolved conflicts, nothing is changed and the paths in conflict are
   reported. A preview (dry run, the MCP default) reports the kind (up to date, fast-forward, merge,
   conflict), the paths that would change, local changes in the way and merge conflicts, without writing
   objects or touching the repository. [Improvement IMP-0016: conflicts are known before the merge starts]
6. MCP tool `git_merge` (source, `dry_run` default true). [ADR-0008]
7. **v2 – Merge that stops:** a merge started from the branch popup (SPEC-0068 v3 behaviour 12), or
   through `git_merge` with `allow_conflicts` and `dry_run` false, runs the `git` program (`git merge
   --no-edit`, so the repository's merge hooks run) and on conflicts leaves the repository merging:
   `MERGE_HEAD`, conflict markers in the files and the three index stages, for the merge view
   (SPEC-0101 v3) and Git: Continue / Abort (SPEC-0106). Local changes in the way are still refused first
   and can be put aside with a smart merge (SPEC-0231). [STD, OBS-0009]

## Acceptance criteria
- **AK-1** Up to date: merging an ancestor changes nothing.
- **AK-2** Fast-forward: branch, working tree and index follow; `git status` clean; reflog entry.
- **AK-3** Clean three-way merge of changes to different files and to different parts of one file: merge
  commit with both parents and the default message; content as `git merge` produces; `git fsck` passes.
- **AK-4** Conflicting changes to the same lines: reported paths, HEAD/working tree unchanged, no
  `MERGE_HEAD`; dry run reports the same without writing.
- **AK-5** A local change on a path the merge changes refuses the merge.
- **AK-6** The MCP tool previews and merges.
- **AK-7 (v2)** A conflicting merge with `allow_conflicts` stops with `MERGE_HEAD` and the conflicted
  path; continuing after resolving writes a merge commit with both parents (integration test, protocol
  test).

## Out of scope
Merge
strategies and options (`--no-ff`, squash, octopus), rebase, cherry-pick, pull.

## Known deviations from the reference
The domain function and the MCP default refuse conflicting merges up front (IMP-0016); the branch popup
and `allow_conflicts` stop in the merging state like the reference (v2).
