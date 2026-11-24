#!/bin/sh
# Shared by pre-merge-commit and pre-commit: refuses to merge main into a branch when the merge has
# no conflict (AGENTS.md rule 6).
#
# The server CI checks every branch as it would be on main, merged into main's tip, so a branch
# never needs main merged in to be checked or merged. Every such merge is a new head, which makes
# the server CI drop the running check (`superseded`) and start again: 1,101 merges of main into
# branches between 2026-10-30 and 2026-11-23 cost hours of queue time.
#
# A merge counts as conflict-free when `git merge-tree` merges the branch and the merged commit
# cleanly. Resolving a conflict the server CI reported ("does not merge into main") stays possible.
# pre-merge-commit runs for a merge that completes by itself; pre-commit catches the `git commit`
# that would conclude a refused one. FERRIS_IDEA_ALLOW_MAIN_MERGE=1 lets a merge through (a branch
# that needs a change from main to build).
#
#   no-main-merge.sh SHA…   the commits being merged into HEAD
[ "${FERRIS_IDEA_ALLOW_MAIN_MERGE:-0}" = 1 ] && exit 0
[ $# -gt 0 ] || exit 0
branch=$(git symbolic-ref --quiet --short HEAD 2>/dev/null) || exit 0
case "$branch" in main|queue/*|schleuse/probe/*) exit 0 ;; esac
main=$(git rev-parse --quiet --verify refs/remotes/origin/main 2>/dev/null) || exit 0
for head in "$@"; do
  # Main's tip or an older commit of main.
  git merge-base --is-ancestor "$head" "$main" 2>/dev/null || continue
  # With a conflict the merge is needed: let it through.
  git merge-tree --write-tree HEAD "$head" >/dev/null 2>&1 || continue
  cat >&2 <<MSG
Not merging main into $branch: the merge has no conflict, so the branch does not need it.

The server CI checks the branch merged into main's tip anyway, and a new head only starts its
check again (AGENTS.md rule 6).

  git merge --abort        leave the branch as it was

Merge main only to resolve a conflict the server CI reported ("does not merge into main"), or set
FERRIS_IDEA_ALLOW_MAIN_MERGE=1 when the branch needs a change from main to build.
MSG
  exit 1
done
exit 0
