# SPEC-0035: Git — switch branches

- **Version:** 1 · **Date:** 2026-10-31
- **Sources:** GEN (IDEs check out a local branch from the branch list; local changes that do not touch the
  files the switch changes are carried over; F-074), STD (Git `switch` semantics: working tree, index and
  HEAD follow the target commit; a switch that would overwrite local changes is refused and names the
  files), SPEC-0032/0034
- **Feature matrix rows:** F-074

## Purpose
Move to another local branch while keeping unrelated local work.

## Behaviour
1. The target is a local branch name. The files that differ between HEAD's tree and the target's tree
   (added, changed, removed, mode changes) are determined first. [STD]
2. If any of those paths has a local change (staged, unstaged or an untracked file in the way), nothing is
   changed and the refusal lists every such path. [STD, improvement: all conflicting paths at once]
3. Otherwise the working tree is updated for exactly those paths (content and executable bit, symlinks;
   removed files are deleted together with directories that became empty), the index entries of those paths
   are set to the target (tree cache dropped), and HEAD becomes a symbolic reference to the branch with a
   reflog entry `checkout: moving from <old> to <new>`. Local changes on other paths stay as they are. [STD]
4. Switching to the current branch is a no-op; an unknown branch is rejected. [STD]
5. A preview (dry run) returns the paths that would change and any conflicts without touching anything.
   [IMP-0015 applies]
6. MCP tool `git_switch_branch` (name, `dry_run` default true). [ADR-0008]

## Acceptance criteria
- **AK-1** Switching between two branches that add, change, remove a file and make one executable: working
  tree, index (`git status` clean) and HEAD match the target; `git fsck` passes; reflog entry written.
- **AK-2** A local change on an unaffected file is carried over; a local change or untracked file on an
  affected path refuses the switch, lists the path(s) and leaves everything unchanged.
- **AK-3** Unknown branch rejected; switching to the current branch is a no-op; dry run changes nothing.
- **AK-4** The MCP tool previews and switches.

## Out of scope
Switching to remote-tracking branches or commits (detached), "smart checkout" (stash and re-apply local
changes), submodules, sparse checkout, line-ending and filter conversion (`.gitattributes`).

## Known deviations from the reference
No smart checkout yet: conflicting local changes refuse the switch (as `git switch` does) instead of being
shelved automatically.
