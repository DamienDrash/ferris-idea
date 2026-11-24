#!/bin/bash
# Tidies the clone after merged work (AGENTS.md rule 15): worktrees, local and remote branches
# whose pull request is merged or whose change main holds already. Without --apply it only reports.
#
#   scripts/janitor.sh                   report what would go, and the disk's free space
#   scripts/janitor.sh --apply           remove those worktrees (with their build folders) and
#                                        delete those local branches
#   scripts/janitor.sh --apply --remote  also delete those branches on origin
#   scripts/janitor.sh --sizes           also measure each worktree (slow with many build folders)
#
# Never touched: the main checkout, a worktree with uncommitted or untracked files (its build
# folders aside), a branch checked out somewhere, `main`, `queue/*`, `schleuse/probe/*` (the
# Schleuse removes those itself), a branch whose pull request is open, a branch whose pull
# request was closed without merging (listed, for a person to decide).
# A branch counts as merged when merging it into main would leave main as it is, or when its pull
# request is merged with the branch's tip as its head (no commit after the merge). Between 2026-10-30 and 2026-11-23 the clone grew to 39 worktrees
# (152 GB) and 1,890 branches; a full disk stopped the server CI for 4.7 hours on 2026-11-17.
set -euo pipefail
cd "$(dirname "$0")/.."

apply=0; remote=0; sizes=0
for a in "$@"; do
  case "$a" in
    --apply) apply=1 ;;
    --remote) remote=1 ;;
    --sizes) sizes=1 ;;
    *) sed -n '2,10p' "$0"; exit 2 ;;
  esac
done
[ "$remote" = 0 ] || [ "$apply" = 1 ] || { echo "--remote needs --apply"; exit 2; }

git fetch -q --prune origin
main_tree=$(git rev-parse 'origin/main^{tree}')
top=$(git rev-parse --show-toplevel)

tmp=$(mktemp -d "${TMPDIR:-/tmp}/janitor.XXXXXX"); trap 'rm -rf "$tmp"' EXIT
gh pr list --state all --limit 5000 --json headRefName,headRefOid,state \
  -q '.[] | .state + " " + .headRefName + " " + .headRefOid' > "$tmp/prs" ||
  { echo "could not read the pull requests (gh)"; exit 1; }
# The newest pull request of a branch decides (a name can be used again).
awk '!seen[$2]++' "$tmp/prs" > "$tmp/state"
state_of() { awk -v b="$1" '$2 == b {print $1; exit}' "$tmp/state"; }
head_of() { awk -v b="$1" '$2 == b {print $3; exit}' "$tmp/state"; }

# Whether merging `sha` into main leaves main as it is.
in_main() { [ "$(git merge-tree --write-tree origin/main "$1" 2>/dev/null | head -1)" = "$main_tree" ]; }

# Whether `branch` (with tip `sha`) is merged: main holds its change, or its pull request is merged
# with exactly this tip (a commit made after the merge keeps the branch).
merged() { # branch sha
  in_main "$2" && return 0
  [ "$(state_of "$1")" = MERGED ] && [ "$(head_of "$1")" = "$2" ]
}

say() { printf '%s\n' "$*"; }
act() { if [ "$apply" = 1 ]; then "$@"; fi; }

# --- Worktrees ---
say "== worktrees"
checked_out=" "
gone_wt=0; kept_wt=0; dirty_wt=0
while IFS=$'\t' read -r path head branch; do
  [ -n "$path" ] || continue
  [ -z "$branch" ] || checked_out="$checked_out$branch "
  [ "$path" != "$top" ] || continue
  [ -d "$path" ] || continue
  size=""
  if [ "$sizes" = 1 ]; then size=" $(du -sh "$path" 2>/dev/null | cut -f1)"; fi
  name=${branch:-detached}
  # Changes other than build folders (ignored files never count).
  if [ -n "$(git -C "$path" status --porcelain --untracked-files=normal 2>/dev/null | head -1)" ]; then
    say "  keep (changes)   $path [$name]$size"; dirty_wt=$((dirty_wt + 1)); continue
  fi
  if { [ -n "$branch" ] && merged "$branch" "$head"; } || { [ -z "$branch" ] && in_main "$head"; }; then
    say "  remove (merged)  $path [$name]$size"; gone_wt=$((gone_wt + 1))
    if [ "$apply" = 1 ]; then
      # Build folders first: `worktree remove` would refuse nothing for them, but they are most of
      # the space and an interrupted removal should free them first.
      for t in "$path"/target "$path"/target2; do [ -d "$t" ] && rm -rf -- "$t"; done
      git worktree remove "$path" || say "    could not remove $path"
      checked_out=${checked_out/ $branch / }
    fi
  else
    say "  keep (open)      $path [$name]$size"; kept_wt=$((kept_wt + 1))
  fi
done < <(git worktree list --porcelain | awk '
  # The first entry is the main checkout: never listed.
  /^worktree / {n++; p = (n == 1 ? "" : substr($0, 10)); h = ""; b = ""}
  /^HEAD / {h = $2}
  /^branch / {b = $2; sub("^refs/heads/", "", b)}
  /^$/ {if (p != "") print p "\t" h "\t" b; p = ""}
  END {if (p != "") print p "\t" h "\t" b}')
act git worktree prune
say "  $gone_wt to remove, $kept_wt kept, $dirty_wt with changes"

# --- Local branches ---
say "== local branches"
gone_lb=0; closed_lb=0; total_lb=0
while read -r sha branch; do
  total_lb=$((total_lb + 1))
  case "$branch" in main|queue/*|schleuse/probe/*) continue ;; esac
  case "$checked_out" in *" $branch "*) continue ;; esac
  if merged "$branch" "$sha"; then
    gone_lb=$((gone_lb + 1)); act git branch -q -D "$branch"
  elif [ "$(state_of "$branch")" = CLOSED ]; then
    closed_lb=$((closed_lb + 1)); say "  closed without merge, kept: $branch"
  fi
done < <(git for-each-ref --format='%(objectname) %(refname:short)' refs/heads)
say "  $gone_lb of $total_lb merged$([ "$apply" = 1 ] && echo ", deleted" || echo ", would be deleted"); $closed_lb closed without merge (kept)"

# --- Remote branches ---
say "== branches on origin"
: > "$tmp/remote"
while read -r sha ref; do
  branch=${ref#origin/}
  case "$branch" in main|HEAD|queue/*|schleuse/probe/*|origin) continue ;; esac
  merged "$branch" "$sha" && echo "$branch" >> "$tmp/remote"
done < <(git for-each-ref --format='%(objectname) %(refname:short)' refs/remotes/origin)
n=$(wc -l < "$tmp/remote" | tr -d ' ')
if [ "$remote" = 1 ]; then
  # In groups, so one refused name does not stop the rest.
  xargs -n 50 git push -q origin --delete < "$tmp/remote" || say "  some could not be deleted"
  say "  $n merged, deleted on origin"
else
  say "  $n merged$([ "$n" = 0 ] || echo " (delete with --apply --remote)")"
fi

say "== disk"
df -h "$top" | tail -1 | awk '{print "  " $4 " free of " $2 " (" $5 " used)"}'
[ "$apply" = 1 ] || say "(report only; --apply to tidy)"
