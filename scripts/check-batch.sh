#!/bin/sh
# One window check for several branches: merges them onto a temporary local branch cut from
# origin/main, builds the app bundle once and names the self-test command. Saves building and
# self-testing every pull request by itself. Never pushes and never touches main: after the look in
# the window the coordinator merges the pull requests as usual.
#
#   check-batch.sh BRANCH [BRANCH…]      fetch, merge origin/BRANCH in the given order onto
#                                        batch/<date-time>, run scripts/bundle-macos.sh
#   check-batch.sh --dry-run BRANCH…     only try the merges: print the result, delete the branch
#   check-batch.sh --selftest BRANCH…    also run the window self-test (30-minute watchdog). It
#                                        opens windows and sends keys: nobody may work at the Mac.
#
# Refuses to run with uncommitted changes. Plain merges, never a rebase. CHANGELOG.md and
# TRACEABILITY.md merge by the union rule in .gitattributes; any other conflict stops the run,
# names the branch and the files, and removes the temporary branch. A union merge can put a line
# written under `## Unreleased` into a released section when its branch was cut before that
# release: the script lists the sections the added changelog lines ended up in.
#
# Without --dry-run the checkout stays on the batch branch (the bundle is built from it); the last
# lines say how to go back.
set -eu
cd "$(dirname "$0")/.."

usage() { echo "usage: $0 [--dry-run] [--selftest] BRANCH [BRANCH…]" >&2; exit 2; }
fail() { echo "error: $*" >&2; exit 1; }

# Everything runs inside main, so the shell has read the whole script before the checkout changes
# the files under it.
main() {
dry=0
selftest=0
branches=""
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) dry=1 ;;
    --selftest) selftest=1 ;;
    -*) usage ;;
    *) branches="$branches ${1#origin/}" ;;
  esac
  shift
done
[ -n "$branches" ] || usage
# shellcheck disable=SC2086  # branch names hold no spaces; one word each
set -- $branches

[ -z "$(git status --porcelain --untracked-files=no)" ] \
  || fail "uncommitted changes; commit them or set them aside first"
home=$(git symbolic-ref --quiet --short HEAD || git rev-parse HEAD)

echo "== fetch origin: main $*"
git fetch --quiet origin main "$@" || fail "cannot fetch main and every named branch from origin"

batch="batch/$(date +%Y%m%d-%H%M%S)"
git checkout --quiet --no-track -b "$batch" origin/main
created=1
# Goes back to where the run started and removes the temporary branch.
discard() {
  [ "$created" = 1 ] || return 0
  created=0
  git merge --abort 2>/dev/null || true
  git checkout --quiet --force "$home"
  git branch --quiet -D "$batch"
}
trap 'discard' EXIT
trap 'exit 130' INT TERM

merged=""
for b in "$@"; do
  tip=$(git rev-parse --short "origin/$b")
  printf '\n== merge origin/%s (%s)\n' "$b" "$tip"
  if ! git merge --no-ff --no-edit "origin/$b"; then
    echo "error: origin/$b does not merge onto origin/main${merged:+ after}$merged; conflicts in:" >&2
    git diff --name-only --diff-filter=U | sed 's/^/  /' >&2
    exit 1
  fi
  merged="$merged $b"
done

# Where the changelog lines added against origin/main ended up.
printf '\n== added changelog lines by section\n'
git diff -U0 origin/main -- CHANGELOG.md >"${TMPDIR:-/tmp}/idecraft-batch-changelog.$$"
python3 -I - "${TMPDIR:-/tmp}/idecraft-batch-changelog.$$" <<'PY' || true
import re, sys
added = []
for line in open(sys.argv[1], encoding='utf-8'):
    m = re.match(r'@@ -\S+ \+(\d+)(?:,(\d+))? @@', line)
    if m:
        start, count = int(m.group(1)), int(m.group(2) or 1)
        added += range(start, start + count)
text = open('CHANGELOG.md', encoding='utf-8').read().splitlines()
section, per, seen = '(above the first section)', {}, {}
for number, line in enumerate(text, 1):
    if line.startswith('## '):
        section = line[3:]
    if number in added and line.strip() and not line.startswith('#'):
        per.setdefault(section, []).append(line)
    if line.startswith('- '):
        seen.setdefault((section, line), []).append(number)
if not per:
    print('none')
for section, lines in per.items():
    warn = '' if section == 'Unreleased' else '   <-- not under Unreleased: move these lines by hand'
    print(f'{section}: {len(lines)} line(s){warn}')
    if warn:
        for l in lines:
            print(f'    {l[:110]}')
twice = [(s, l) for (s, l), n in seen.items() if len(n) > 1]
for s, l in twice:
    print(f'twice in {s}: {l[:110]}')
PY
rm -f "${TMPDIR:-/tmp}/idecraft-batch-changelog.$$"

# The merged branches and their tips, for the merges of the pull requests afterwards.
report() {
  printf '\n== %s: origin/main %s plus\n' "$batch" "$(git rev-parse --short origin/main)"
  for b in "$@"; do
    printf '  %s  %s\n' "$(git rev-parse --short "origin/$b")" "$b"
  done
}

if [ "$dry" = 1 ]; then
  printf '\n== result of the merges (files against origin/main)\n'
  git diff --stat origin/main HEAD | tail -n 1
  report "$@"
  echo "dry run: all branches merge; nothing built, $batch deleted"
  exit 0
fi

# From here on the batch branch stays, also when a step fails: the merges are worth keeping.
created=0
printf '\n== scripts/check-docs.sh\n'
scripts/check-docs.sh
printf '\n== scripts/bundle-macos.sh\n'
scripts/bundle-macos.sh
app="${CARGO_TARGET_DIR:-target}/bundle/macos/IDECraft.app"
binary="$app/Contents/MacOS/idecraft"
echo "bundle: $(pwd)/$app"

rc=0
if [ "$selftest" = 1 ]; then
  printf '\n== window self-test (watchdog 30 min)\n'
  log=$(mktemp "${TMPDIR:-/tmp}/idecraft-batch-selftest.XXXXXX")
  "$binary" ui --selftest >"$log" 2>&1 &
  pid=$!
  (sleep 1800 && kill "$pid" 2>/dev/null && echo "error: the self-test timed out after 30 min" >&2) &
  dog=$!
  wait "$pid" || rc=$?
  kill "$dog" 2>/dev/null || true
  last=$(grep '^{' "$log" | tail -n 1 || true)
  echo "self-test: ${last:-no result line; output in $log}"
  case "$last" in
    *'"failed":[]'* | *'"failed": []'*) [ "$rc" -ne 0 ] || rm -f "$log" ;;
    *) [ "$rc" -ne 0 ] || rc=1 ;;
  esac
  [ "$rc" -eq 0 ] || echo "error: the self-test failed (exit $rc)${last:+; output in $log}" >&2
else
  echo "self-test (takes over the screen, about 15 min):"
  echo "  $app/Contents/MacOS/idecraft ui --selftest"
fi

report "$@"
echo "nothing was pushed and main is untouched. Back to where you were:"
echo "  git checkout $home && git branch -D $batch"
exit "$rc"
}
main "$@"
