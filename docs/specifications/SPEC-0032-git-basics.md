# SPEC-0032: Git — status, history and changes of a file

- **Version:** 3 · **Date:** 2026-11-16 (v3: the repositories of the workspace with a summary, behaviours 6–8, AK-6, AK-7, OBS-0071 O3; deviation IMP-0456); 2026-11-08 (v2: the IDE's local history is not listed)
- **Sources:** GEN (IDEs show changed and untracked files, the commit history and per-line change markers
  against the last commit; F-074), STD (Git repository format read with `gix`, MIT/Apache; line diff with
  `similar`, Apache-2.0, Myers algorithm), ADR-0005/ARCHITECTURE (`gix` for Git); v3: STD (a working
  tree is marked by its `.git` folder or file), OBS-0071 (O3: the reference lists the repositories of
  a project with branch and counts; IDECraft had no such list and `git_status` no summary), GEN (an
  agent asks "where is the repository and is it clean" before it asks for paths); decision of the
  coordinator 2026-11-16 under the general authority: additive only, existing answers keep their
  shape, a new read-only tool without a right
- **Feature matrix rows:** F-074

## Purpose
Answer "what did I change", "what happened before" and "which lines differ from the last commit" without
calling the `git` program.

## Behaviour
1. The repository is found from the workspace root upwards. A workspace outside a repository is an error
   that says so. [STD]
2. Status: per path (relative to the repository root, `/`-separated, sorted) the staged change (HEAD →
   index: added, deleted, modified, renamed) and the unstaged change (index → working tree: modified,
   deleted, type change, conflict; untracked files as added). Ignored files are not listed, and neither are untracked files
   below `.idecraft/history/` in any folder: the IDE's local history is its own state and is never
   offered for committing, also in a repository without an ignore rule for it. [STD, GEN]
3. History: commits reachable from HEAD, newest first, up to a limit: full and short id, summary line,
   author name and e-mail, commit time (seconds since the epoch). An unborn HEAD gives an empty history.
   [STD]
4. Changes of a file: the file at HEAD against the working tree, as line changes for gutter markers
   (added, modified, deleted, with 1-based line ranges in the new and the old text) and as unified hunks
   with three lines of context. A file not in HEAD is entirely added; a deleted file entirely deleted.
   Binary content (NUL byte) is reported as binary without hunks. [GEN, STD]
5. MCP tools `git_status`, `git_log` (limit), `git_diff` (path). [ADR-0008]
6. **(v3) Repositories below a folder:** a folder below the workspace with a `.git` entry (folder or
   file) is the working tree of a repository of its own. The search goes at most 4 folders deep,
   lists at most 50 in path order, follows no symbolic link, does not enter `.git` folders and skips
   `node_modules` and `target` (dependencies and build output); it reads at most 20 000 folders.
   Stopping at a bound is said. [STD, GEN]
7. **(v3) Summary of a repository:** the branch HEAD points at (none while detached), whether HEAD
   is detached, the short id of HEAD's commit (none before the first commit) and, from the status of
   behaviour 2, the number of paths staged, changed (tracked, not staged), untracked and in conflict
   — a path staged and changed again counts in both, ignored files in none — and whether it is
   clean (all four zero). [STD, GEN]
8. **(v3) MCP tool `git_repositories`:** the repository of behaviour 1, marked `workspace: true` (the
   one every other Git tool works on), then the repositories of behaviour 6, each with its summary
   and its `root`: workspace-relative with `/` (`.` for the workspace itself), absolute when it lies
   above the workspace, never a path climbing out with `..`. A folder whose `.git` does not open as
   that folder's repository has an `error` and is never replaced by the repository above it. The
   answer also has `truncated` and the `limits`. Without any repository the list is empty, not an
   error. It only reads: no file and no index is written, no `git` program runs, no right is
   needed. Additive only: `git_status` and every other tool keep the shape of their answers.
   [ADR-0008, GEN; OBS-0071 O3]

## Acceptance criteria
- **AK-1** Line changes and hunks for insertions, deletions, a modification, an empty old text and
  identical texts.
- **AK-2** In a scratch repository (created with the `git` program for the test): staged addition,
  staged modification, unstaged modification, unstaged deletion and an untracked file are reported;
  ignored files are not.
- **AK-3** History of three commits newest first with summaries and authors; limit; unborn HEAD.
- **AK-4** Changes of a committed and modified file, of a new file and of a binary file.
- **AK-5** The MCP tools report status, history and a diff, and say when there is no repository.
- **AK-6 (v3)** `crates/vcs/tests/it/git.rs::summary_of_a_repository_and_the_repositories_below`:
  unborn HEAD with branch `main`, no commit and clean; after a commit a 7-character head; one path
  staged and changed again, one changed, one untracked and one ignored count 1 staged, 2 changed,
  1 untracked; a detached HEAD has no branch; working trees `lib/a`, `lib/a/inner` and
  `tools/deep/b/c` are found and one in `node_modules` is not, depth 3 leaves out the fourth level,
  a limit of 1 keeps the first and says it stopped.
- **AK-7 (v3)** `protocol.rs::git_repositories_lists_the_workspace_repository_and_those_below`:
  without a repository the list is empty with the limits; after `git init` and a commit the
  workspace repository is first with root `.`, `workspace: true`, branch `main`, a head and the
  counts (1 staged, 1 changed, untracked, not clean); `broken/.git` gives an error entry,
  `vendor/lib` its own branch `trunk` without a head; `node_modules` is not searched; a second call
  gives the same answer; `git_status` is still a list.

## Out of scope
Committing, branches, merging, rebasing, remote operations, blame, stashes (later M6.3 steps); diffs
against the index or between commits.

## Known deviations from the reference
Not yet compared (the reference's VCS integration is open source but not ported; behaviour per GEN).

v3 (OBS-0071 O3): the reference lists the Git roots of a project with branch and counts, and gives a
root above the project as a relative path climbing out with `..` (`../../..`). `git_repositories`
gives the same facts read-only and without a right, a root above the workspace as an absolute path,
the working trees below the workspace with their own summary, and names a `.git` that does not open
instead of passing over it (IMP-0456). The reference's limit and filters for untracked and ignored
files are not taken over: `git_status` lists the paths, and ignored files are not listed or counted.
Several repositories as roots of one project (each Git tool choosing one) stay out of scope.
