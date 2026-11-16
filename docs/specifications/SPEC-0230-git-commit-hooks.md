# SPEC-0230: Git commit hooks

- **Version:** 1 · **Date:** 2026-11-03
- **Sources:** STD (Git hooks: `pre-commit`, `prepare-commit-msg`, `commit-msg`, `post-commit`, their
  arguments, `core.hooksPath`, hooks of linked worktrees in the common directory, a non-zero exit of
  `pre-commit`/`commit-msg` aborts the commit, `--no-verify` skips those two; `git commit --only <paths>`
  runs the hooks against a temporary index holding exactly the commit), GEN (IDE commit dialogs offer
  "Run Git hooks" and show a failing hook's output), SPEC-0034 (commit of selected files), SPEC-0063
  (commit tool window), ADR-0008, ADR-0012
- **Feature matrix rows:** F-074

## Purpose
Projects that check code before each commit (formatters, linters, ticket ids in messages) keep working
when the user commits from IDECraft: the same hooks run as on the command line, and a failing check stops
the commit with its output in view.

## Behaviour
1. **Which hooks:** a commit from the commit window or from `git_commit` runs `pre-commit`,
   `prepare-commit-msg` (arguments: message file, `message`), `commit-msg` (argument: message file) and,
   after the commit, `post-commit`. A hook is found where Git finds it (`git rev-parse --git-path
   hooks/<name>`, so `core.hooksPath` and linked worktrees are honoured) and runs only when the file is
   executable. [STD]
2. **What the hooks see:** they run in the working-tree root with `GIT_INDEX_FILE` pointing to a temporary
   index that holds exactly the tree to be committed (HEAD plus the selected files), as Git does for a
   partial commit; files a `pre-commit` hook stages there (a formatter that runs `git add`) are part of the
   commit, the user's own index keeps its other entries. `GIT_EDITOR` is `:` and stdin is empty, so a hook
   never waits for input. [STD, ADR-0012]
3. **Blocked commit:** a `pre-commit`, `prepare-commit-msg` or `commit-msg` hook that exits non-zero, cannot be started or runs
   longer than 5 minutes (it is then killed) blocks the commit: no commit object is referenced, HEAD and
   the user's index stay as they were. The commit window keeps the message and the ticked files and shows
   "Commit blocked by <hook> hook (exit <n>)" with the hook's output (stdout and stderr interleaved as
   written, the last 200 lines). [STD, GEN]
4. **Message:** `prepare-commit-msg` and `commit-msg` may rewrite the message file; the commit uses the
   message as the hooks leave it, with comment lines (`#`) removed. A message that is empty afterwards
   refuses the commit. [STD]
5. **After the commit:** `post-commit` runs once the commit is written; its exit code does not undo the
   commit. Its output, if any, is shown in the commit window as for the other hooks. [STD]
6. **Run Git hooks:** the commit window has a checkbox "Run Git hooks", shown only when the repository has
   at least one of the hooks above and ticked by default. Unticked, `pre-commit` and `commit-msg` are
   skipped, as with `git commit --no-verify`; `prepare-commit-msg` and `post-commit` still run, as in Git.
   [GEN, STD]
7. **Feedback:** with hooks, the commit runs in the background; the Commit button reads "Committing…" and
   is disabled until it ends. The status bar reports the commit or the blocking hook. [GEN]
8. **MCP:** `git_commit` takes `no_verify` (default false) and returns the hooks that ran with exit code
   and output; a blocked commit is a `hook_failed` error naming the hook with its output. A dry run runs no
   hook and lists the hooks a commit would run. Committing through MCP runs the repository's hooks like the
   window does; no extra right is needed because the user's own hooks are run as Git would run them.
   [ADR-0008]

## Acceptance criteria
- **AK-1** On a scratch repository: a `pre-commit` hook that exits 1 blocks the commit (HEAD, index and
  files unchanged, output returned); with `no_verify` the commit goes through (integration test).
- **AK-2** A `pre-commit` hook that formats a selected file and stages it: the commit contains the
  formatted text; another file staged by the user before stays staged and out of the commit
  (integration test).
- **AK-3** `commit-msg` appends a line: the commit message has it; a `commit-msg` that empties the message
  refuses the commit; `post-commit` output is returned and its failure keeps the commit (integration test).
- **AK-4** `core.hooksPath` and a non-executable hook file (ignored) (integration test).
- **AK-5** `git_commit` with a failing hook returns `hook_failed`; the dry run lists the hooks (protocol
  test).
- **AK-6** In a real window (`idecraft ui --selftest`): a failing `pre-commit` hook leaves the message and
  shows the hook's output; unticking "Run Git hooks" commits.
- **AK-7** Hand test with a real hook framework (pre-commit or husky) next to the reference.

## Out of scope
`pre-push` (the push already runs the `git` program, which runs it), `pre-merge-commit` for merge commits
written by IDECraft itself (SPEC-0036; merges that stop with conflicts and are finished through the `git`
program run it), `pre-rebase` and other hooks the `git` program runs itself, amending commits, signing.

## Known deviations from the reference
Not compared with the reference yet (hand test AK-7).
