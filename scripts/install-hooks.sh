#!/bin/sh
# Points Git at the hooks in scripts/git-hooks/ for this clone and all its worktrees (one setting,
# core.hooksPath, kept in the clone's config). A worktree on a commit without that folder runs no
# hooks, as before.
#
#   scripts/install-hooks.sh            install
#   scripts/install-hooks.sh --remove   back to Git's own .git/hooks
#
# The hooks (AGENTS.md rules 6 and 14):
#   pre-merge-commit  refuses a merge of main into a branch that has no conflict
#   pre-push          refuses the first push of a new code branch while the work-in-progress limit
#                     of open pull requests is reached
set -eu
cd "$(dirname "$0")/.."
if [ "${1:-}" = --remove ]; then
  git config --unset core.hooksPath || true
  echo "core.hooksPath removed"; exit 0
fi
chmod +x scripts/git-hooks/*
git config core.hooksPath scripts/git-hooks
echo "core.hooksPath = scripts/git-hooks ($(ls scripts/git-hooks | tr '\n' ' '))"
